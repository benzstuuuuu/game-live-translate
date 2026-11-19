//! Shared app state: what /events broadcasts and what start/stop change.

use crate::audio::{AudioSource, WavFile};
use crate::core::card_audio::Permission;
use crate::core::error::{AppError, ErrorCode};
use crate::core::fallback::{self, Fallback};
use crate::core::jobs::{self, Job, Pick};
use crate::core::obs::Region;
use crate::core::tokens::Usage;
use crate::db::{self, Line, ProfileRef};
use crate::frames::{Folder, FrameSource};
use crate::live;
use crate::obs;
use crate::pipeline;
use crate::recovery;
use crate::translate::{Translator, Unavailable};
use futures_util::future::BoxFuture;
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, oneshot, watch};
use tokio::task::JoinHandle;

/// AppState of SRS 8.4, trimmed to what exists so far.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppState {
    pub status: Status,
    pub profile: Option<ProfileRef>,
    pub translator: &'static str,
    /// The engine for "แปลใหม่" on /reader; None = the live one (FR-TRN-16).
    pub retranslator: Option<&'static str>,
    /// Current (or last) session; /reader resets its lines when it changes.
    pub session_id: Option<String>,
    /// On the backup translator for now, and why (FR-TRN-05).
    pub fallback: fallback::View,
    /// Why translating stopped, while status is `error` (FR-APP-08, FR-ERR-04).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
    /// Gemini tokens of this session, shown on /setup (FR-TRN-10).
    pub tokens: Usage,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Idle,
    Translating,
    Error,
}

/// Events other than `state` (SRS 8.3).
#[derive(Debug, Clone)]
pub enum Push {
    Line(Line),
    LineUpdated(Line),
    SubtitleGone,
    /// The stream bar moved on /setup; /overlay follows (FR-STR-03).
    Bar(crate::core::bar::Bar),
}

pub struct Config {
    pub conn: Connection,
    /// Folder of frames to read instead of OBS (`--frames`, dev and tests).
    pub frames_dir: Option<PathBuf>,
    /// OBS to read frames from when there is no `frames_dir`, and to set up.
    pub obs: Option<obs::Link>,
    /// Engines the user can switch between, for live lines and for
    /// "แปลใหม่" alike (FR-TRN-02, 16); the first is the default. A new
    /// engine only needs adding here, in `main`.
    pub translators: Vec<Arc<dyn Translator>>,
    /// Pause between frames; NFR-03 caps reading at 4 fps.
    pub frame_interval: Duration,
    /// Live Translate and the audio for profiles whose text comes from audio
    /// (FR-AUD); None = such profiles cannot start.
    pub live: Option<live::Link>,
}

pub struct App {
    pub state: watch::Sender<AppState>,
    pub events: broadcast::Sender<Push>,
    /// Flips to true on Ctrl-C so open SSE streams end and shutdown can finish.
    pub shutdown: watch::Receiver<bool>,
    translators: Vec<Arc<dyn Translator>>,
    pub frame_interval: Duration,
    frames_dir: Option<PathBuf>,
    obs: Option<obs::Link>,
    live: Option<live::Link>,
    db: Mutex<Connection>,
    running: Mutex<Option<JoinHandle<()>>>,
    /// Asks the running audio session to finish its last sentence and end
    /// (FR-AUD-07); None for a subtitle session.
    stop_audio: Mutex<Option<oneshot::Sender<()>>>,
    /// Last card still for the box picker on /setup, in memory only.
    pub still: crate::region::Still,
    /// Mirrored into `AppState.fallback` by `with_fallback`.
    fallback: Mutex<Fallback>,
    /// Engines' token totals when this session started (FR-TRN-10).
    tokens_start: Mutex<Usage>,
    /// Live Translate tokens since the app opened (FR-TRN-10).
    live_usage: Mutex<Usage>,
}

#[derive(Debug, PartialEq)]
pub enum StartError {
    NoFrameSource,
    FrameSource(String),
    NoProfile,
    /// The profile has no subtitle box; `AppState.error` says so (FR-ONB-06).
    NoRegion,
    /// An audio profile, but nothing to hear: no card and no `--audio` file.
    NoAudioSource,
    AudioSource(String),
    /// Microphone permission is off; `AppState.error` says how to turn it on.
    MicPermission,
}

impl App {
    pub fn new(cfg: Config, shutdown: watch::Receiver<bool>) -> rusqlite::Result<Arc<Self>> {
        let profile = db::last_profile(&cfg.conn)?;
        let first = cfg.translators.first().expect("at least one translator");
        let (state, _) = watch::channel(AppState {
            status: Status::Idle,
            profile,
            translator: first.engine(),
            retranslator: None,
            session_id: None,
            fallback: Default::default(),
            error: None,
            tokens: Usage::default(),
        });
        let (events, _) = broadcast::channel(64);
        Ok(Arc::new(Self {
            state,
            events,
            shutdown,
            translators: cfg.translators,
            frame_interval: cfg.frame_interval,
            frames_dir: cfg.frames_dir,
            obs: cfg.obs,
            live: cfg.live,
            db: Mutex::new(cfg.conn),
            running: Mutex::new(None),
            stop_audio: Mutex::new(None),
            still: Default::default(),
            fallback: Default::default(),
            tokens_start: Default::default(),
            live_usage: Default::default(),
        }))
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn obs(&self) -> Option<&obs::Link> {
        self.obs.as_ref()
    }

    pub fn live(&self) -> Option<&live::Link> {
        self.live.as_ref()
    }

    /// Start a new session (FR-APP-04): frames for a subtitle profile, audio
    /// through Live Translate for an audio profile. No-op if running,
    /// unless the session is waiting out a problem: then "ลองอีกครั้ง" starts
    /// over now instead of at the next automatic try (FR-ERR-04).
    pub fn start(self: &Arc<Self>) -> Result<(), StartError> {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.as_ref().is_some_and(|t| !t.is_finished()) {
            if self.state.borrow().status != Status::Error {
                return Ok(());
            }
            if let Some(task) = running.take() {
                task.abort();
            }
        }
        let profile = self
            .state
            .borrow()
            .profile
            .clone()
            .ok_or(StartError::NoProfile)?;
        if profile.source == "audio" {
            let link = self.live.clone().ok_or(StartError::NoAudioSource)?;
            let audio: Box<dyn AudioSource> = match (&link.audio_file, &link.card) {
                (Some(path), _) => Box::new(WavFile::open(path).map_err(StartError::AudioSource)?),
                (None, Some(card)) => {
                    // Not even Live Translate is opened when nothing can be heard.
                    if card.permission() == Permission::Denied {
                        self.fail(AppError::mic_permission_denied());
                        return Err(StartError::MicPermission);
                    }
                    Box::new(recovery::Audio::new(self.clone(), card.clone()))
                }
                (None, None) => return Err(StartError::NoAudioSource),
            };
            let instruction = link.instruction(self, &profile.id);
            let session = self.begin_session(profile.id);
            let (stop, stopped) = oneshot::channel();
            *self.stop_audio.lock().unwrap_or_else(|e| e.into_inner()) = Some(stop);
            *running = Some(tokio::spawn(live::run(
                self.clone(),
                link,
                audio,
                session,
                instruction,
                stopped,
            )));
            return Ok(());
        }
        // Only check here; drawing the box is a /setup job (#38).
        let region = db::profile_region(&self.db(), &profile.id).ok().flatten();
        let Some(region) = region else {
            self.fail(AppError::no_region(&profile.name));
            return Err(StartError::NoRegion);
        };
        let frames = self.open_frames(region)?;
        let session = self.begin_session(profile.id);
        self.translator_for(Job::Live).warm();
        self.translator_for(Job::Retranslate).warm();
        *running = Some(tokio::spawn(pipeline::run(self.clone(), frames, session)));
        Ok(())
    }

    /// A new session id, token count from zero, and status translating.
    fn begin_session(&self, profile_id: String) -> pipeline::Session {
        let session_id = uuid::Uuid::new_v4().to_string();
        self.with_fallback(Fallback::reset);
        *self.tokens_start.lock().unwrap_or_else(|e| e.into_inner()) = self.usage();
        self.state.send_modify(|s| {
            s.status = Status::Translating;
            s.session_id = Some(session_id.clone());
            s.error = None;
            s.tokens = Usage::default();
        });
        pipeline::Session {
            profile_id,
            session_id,
        }
    }

    /// True when the profile in use takes its text from audio (FR-AUD-02).
    pub fn audio_profile(&self) -> bool {
        self.state
            .borrow()
            .profile
            .as_ref()
            .is_some_and(|p| p.source == "audio")
    }

    /// Can an audio session start (FR-AUD-04)? Needs Live Translate and a
    /// saved key; the translator picked plays no part.
    pub async fn check_live(&self) -> Result<(), Unavailable> {
        match &self.live {
            Some(link) => link.check().await,
            None => Err(Unavailable {
                message: "Yokoten นี้ไม่ได้ต่อ Live Translate".into(),
                fix: "เปลี่ยนแหล่งข้อความของโปรไฟล์เป็น subtitle".into(),
            }),
        }
    }

    /// The folder if `--frames` was given, else OBS (opened in the session
    /// task, and again after it drops out).
    fn open_frames(&self, region: Region) -> Result<Opener, StartError> {
        if let Some(dir) = &self.frames_dir {
            let folder = Folder::open(dir).map_err(|e| StartError::FrameSource(e.to_string()))?;
            // A folder never fails, so it is only opened once.
            let mut folder = Some(folder);
            return Ok(Box::new(move || {
                let folder = folder.take().unwrap_or_default();
                Box::pin(async move { Ok(Box::new(folder) as Box<dyn FrameSource>) })
            }));
        }
        let link = self.obs.clone().ok_or(StartError::NoFrameSource)?;
        Ok(Box::new(move || {
            let link = link.clone();
            Box::pin(async move { obs::open(&link, region).await })
        }))
    }

    /// True when frames come from `--frames`, not OBS.
    pub fn reads_folder(&self) -> bool {
        self.frames_dir.is_some()
    }

    /// Stop translating because of `error` (FR-ERR-04). History stays.
    pub fn fail(&self, error: AppError) {
        self.state.send_modify(|s| {
            s.status = Status::Error;
            s.error = Some(error);
        });
    }

    /// Back to translating in the same session once the problem fixed
    /// itself (FR-ERR-05, 06). Nothing happens unless status is `error`.
    pub fn resume(&self) {
        self.state.send_if_modified(|s| {
            if s.status != Status::Error {
                return false;
            }
            s.status = Status::Translating;
            s.error = None;
            true
        });
    }

    /// Stop reading frames at once (NFR-03). An audio session is asked to
    /// end instead and finishes its last sentence on its own (FR-AUD-07).
    /// History of the session stays.
    pub fn stop(&self) {
        let task = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let audio = self
            .stop_audio
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let ending = audio.is_some_and(|stop| stop.send(()).is_ok());
        if let Some(task) = task.filter(|_| !ending) {
            task.abort();
        }
        // Engines that keep processes open close them (FR-TRN-15).
        for t in &self.translators {
            t.stop();
        }
        self.with_fallback(Fallback::reset);
        self.state.send_modify(|s| {
            s.status = Status::Idle;
            s.error = None;
        });
    }

    /// The translator in use now: the one named in `AppState.translator`.
    pub fn translator(&self) -> Arc<dyn Translator> {
        self.translator_for(Job::Live)
    }

    /// The translator that does `job` (FR-TRN-16), with that engine's model
    /// for the job (FR-TRN-13). Both picks only ever hold engines this app
    /// has, so neither falls back to another engine.
    pub fn translator_for(&self, job: Job) -> Arc<dyn Translator> {
        let (live, retranslate) = {
            let s = self.state.borrow();
            (s.translator, s.retranslator)
        };
        let engine = jobs::engine_for(job, live, retranslate);
        let picked = self
            .find_translator(engine)
            .unwrap_or_else(|| self.translators[0].clone());
        match job {
            Job::Live => picked,
            Job::Retranslate => picked.retranslator().unwrap_or(picked),
        }
    }

    /// Picks the engine for "แปลใหม่" on /reader (FR-TRN-16): an engine name,
    /// or None or "" for the live one. False for an engine this app lacks.
    pub fn set_retranslator(&self, name: Option<&str>) -> bool {
        let engines: Vec<&'static str> = self.translators.iter().map(|t| t.engine()).collect();
        let pick = match jobs::pick(name, &engines) {
            Pick::SameAsLive => None,
            Pick::Engine(engine) => Some(engine),
            Pick::Unknown => return false,
        };
        self.state.send_if_modified(|s| {
            let changed = s.retranslator != pick;
            s.retranslator = pick;
            changed
        });
        self.follow_picks();
        true
    }

    /// The engine called `name`, if this build has it.
    pub fn find_translator(&self, name: &str) -> Option<Arc<dyn Translator>> {
        self.translators
            .iter()
            .find(|t| t.engine() == name)
            .cloned()
    }

    /// The translator that takes lines while the one in use is failing
    /// (FR-TRN-04); None if it has no backup or the backup is not built in.
    pub fn backup(&self) -> Option<Arc<dyn Translator>> {
        fallback::backup_for(self.state.borrow().translator).and_then(|b| self.find_translator(b))
    }

    /// Runs `f` on the fallback state and shows the result in
    /// `AppState.fallback`; pages get a `state` event only when it changed.
    pub fn with_fallback<T>(&self, f: impl FnOnce(&mut Fallback) -> T) -> T {
        let mut fb = self.fallback.lock().unwrap_or_else(|e| e.into_inner());
        let out = f(&mut fb);
        let view = fb.view();
        self.state.send_if_modified(|s| {
            let changed = s.fallback != view;
            s.fallback = view;
            changed
        });
        out
    }

    /// Tokens every engine and Live Translate have used since the app opened.
    fn usage(&self) -> Usage {
        let live = *self.live_usage.lock().unwrap_or_else(|e| e.into_inner());
        self.translators.iter().fold(live, |sum, t| sum + t.usage())
    }

    /// Adds the tokens one Live Translate message reported and shows the
    /// session's total (FR-TRN-10).
    pub fn add_live_usage(&self, used: Usage) {
        {
            let mut live = self.live_usage.lock().unwrap_or_else(|e| e.into_inner());
            *live = *live + used;
        }
        self.count_tokens();
    }

    /// Puts the tokens used since the session started in `AppState.tokens`
    /// (FR-TRN-10); pages get a `state` event only when they changed. Called
    /// after each translation.
    pub fn count_tokens(&self) {
        let start = *self.tokens_start.lock().unwrap_or_else(|e| e.into_inner());
        let tokens = self.usage().since(start);
        self.state.send_if_modified(|s| {
            let changed = s.tokens != tokens;
            s.tokens = tokens;
            changed
        });
    }

    /// Switches the app-wide translator (FR-TRN-02); every page sees it in the
    /// next `state` event. False for an unknown engine. Picking the one in use
    /// again is "ลอง Gemini อีกครั้ง": the next line tries it (FR-TRN-05).
    pub fn set_translator(&self, name: &str) -> bool {
        let Some(t) = self.find_translator(name) else {
            return false;
        };
        if self.state.borrow().translator == t.engine() {
            self.with_fallback(|f| f.retry_now(Instant::now()));
        } else {
            self.with_fallback(Fallback::reset);
        }
        self.state.send_modify(|s| {
            s.translator = t.engine();
            clear_translator_error(s);
        });
        self.follow_picks();
        true
    }

    /// After a pick changed: engines that now do neither job let go of what
    /// they keep open, and during a subtitle session the ones in use get
    /// ready, as `start` does (FR-TRN-15).
    fn follow_picks(&self) {
        let (live, retranslate, translating) = {
            let s = self.state.borrow();
            (
                s.translator,
                s.retranslator,
                s.status == Status::Translating,
            )
        };
        let used = [
            jobs::engine_for(Job::Live, live, retranslate),
            jobs::engine_for(Job::Retranslate, live, retranslate),
        ];
        let session = translating && !self.audio_profile();
        for t in &self.translators {
            if !used.contains(&t.engine()) {
                t.stop();
            } else if session {
                t.warm();
            }
        }
    }

    /// Asks the current translator whether it can work (FR-TRN-11). If not,
    /// stops any session and puts the reason in `AppState.error`.
    pub async fn check_translator(&self) -> Result<(), Unavailable> {
        let result = self.translator().check().await;
        match &result {
            Ok(()) => {
                self.state.send_if_modified(clear_translator_error);
            }
            Err(e) => {
                self.stop();
                self.state.send_modify(|s| {
                    s.status = Status::Error;
                    s.error = Some(AppError::translator_unavailable(e.message.clone(), &e.fix));
                });
            }
        }
        result
    }
}

/// Drops a `translator_unavailable` error; true if there was one.
fn clear_translator_error(s: &mut AppState) -> bool {
    if s.error
        .as_ref()
        .is_none_or(|e| e.code != ErrorCode::TranslatorUnavailable)
    {
        return false;
    }
    s.error = None;
    if s.status == Status::Error {
        s.status = Status::Idle;
    }
    true
}

/// A frame source still being opened (OBS connects asynchronously).
pub type FramesOpening = BoxFuture<'static, Result<Box<dyn FrameSource>, AppError>>;

/// Opens the session's frame source; called again after it drops out.
pub type Opener = Box<dyn FnMut() -> FramesOpening + Send>;
