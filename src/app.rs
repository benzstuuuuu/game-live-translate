//! Shared app state: what /events broadcasts and what start/stop change.

use crate::core::error::{AppError, ErrorCode};
use crate::db::{self, Line, ProfileRef};
use crate::frames::{Folder, FrameSource};
use crate::obs;
use crate::pipeline;
use crate::translate::{Translator, Unavailable};
use futures_util::future::BoxFuture;
use rusqlite::Connection;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

/// AppState of SRS 8.4, trimmed to what exists so far.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppState {
    pub status: Status,
    pub profile: Option<ProfileRef>,
    pub translator: &'static str,
    /// Current (or last) session; /reader resets its lines when it changes.
    pub session_id: Option<String>,
    /// Why translating stopped, while status is `error` (FR-APP-08, FR-ERR-04).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
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
}

pub struct Config {
    pub conn: Connection,
    /// Folder of frames to read instead of OBS (`--frames`, dev and tests).
    pub frames_dir: Option<PathBuf>,
    /// OBS to read frames from when there is no `frames_dir`, and to set up.
    pub obs: Option<obs::Link>,
    /// Engines the user can switch between (FR-TRN-02); the first is the
    /// default. A new engine only needs adding here, in `main`.
    pub translators: Vec<Arc<dyn Translator>>,
    /// Pause between frames; NFR-03 caps reading at 4 fps.
    pub frame_interval: Duration,
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
    db: Mutex<Connection>,
    running: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Debug, PartialEq)]
pub enum StartError {
    NoFrameSource,
    FrameSource(String),
    NoProfile,
}

impl App {
    pub fn new(cfg: Config, shutdown: watch::Receiver<bool>) -> rusqlite::Result<Arc<Self>> {
        let profile = db::last_profile(&cfg.conn)?;
        let first = cfg.translators.first().expect("at least one translator");
        let (state, _) = watch::channel(AppState {
            status: Status::Idle,
            profile,
            translator: first.engine(),
            session_id: None,
            error: None,
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
            db: Mutex::new(cfg.conn),
            running: Mutex::new(None),
        }))
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn obs(&self) -> Option<&obs::Link> {
        self.obs.as_ref()
    }

    /// Start reading frames in a new session (FR-APP-04). No-op if running.
    pub fn start(self: &Arc<Self>) -> Result<(), StartError> {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.as_ref().is_some_and(|t| !t.is_finished()) {
            return Ok(());
        }
        let frames = self.open_frames()?;
        let profile = self
            .state
            .borrow()
            .profile
            .clone()
            .ok_or(StartError::NoProfile)?;
        let session_id = uuid::Uuid::new_v4().to_string();
        self.state.send_modify(|s| {
            s.status = Status::Translating;
            s.session_id = Some(session_id.clone());
            s.error = None;
        });
        let session = pipeline::Session {
            profile_id: profile.id,
            session_id,
        };
        *running = Some(tokio::spawn(pipeline::run(self.clone(), frames, session)));
        Ok(())
    }

    /// The folder if `--frames` was given, else OBS (opened in the session task).
    fn open_frames(&self) -> Result<FramesOpening, StartError> {
        if let Some(dir) = &self.frames_dir {
            let folder = Folder::open(dir).map_err(|e| StartError::FrameSource(e.to_string()))?;
            return Ok(Box::pin(async move {
                Ok(Box::new(folder) as Box<dyn FrameSource>)
            }));
        }
        let link = self.obs.clone().ok_or(StartError::NoFrameSource)?;
        Ok(Box::pin(async move { obs::open(&link).await }))
    }

    /// Stop translating because of `error` (FR-ERR-04). History stays.
    pub fn fail(&self, error: AppError) {
        self.state.send_modify(|s| {
            s.status = Status::Error;
            s.error = Some(error);
        });
    }

    /// Stop reading frames at once (NFR-03). History of the session stays.
    pub fn stop(&self) {
        if let Some(task) = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            task.abort();
        }
        self.state.send_modify(|s| {
            s.status = Status::Idle;
            s.error = None;
        });
    }

    /// The translator in use now: the one named in `AppState.translator`.
    pub fn translator(&self) -> Arc<dyn Translator> {
        let name = self.state.borrow().translator;
        self.find_translator(name)
            .unwrap_or_else(|| self.translators[0].clone())
    }

    fn find_translator(&self, name: &str) -> Option<Arc<dyn Translator>> {
        self.translators
            .iter()
            .find(|t| t.engine() == name)
            .cloned()
    }

    /// Switches the app-wide translator (FR-TRN-02); every page sees it in the
    /// next `state` event. False for an unknown engine.
    pub fn set_translator(&self, name: &str) -> bool {
        let Some(t) = self.find_translator(name) else {
            return false;
        };
        self.state.send_modify(|s| {
            s.translator = t.engine();
            clear_translator_error(s);
        });
        true
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
