//! Shared app state: what /events broadcasts and what start/stop change.

use crate::db::{self, Line, ProfileRef};
use crate::frames::Folder;
use crate::pipeline;
use crate::translate::Translator;
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
    /// Folder of frames to read until the OBS source exists.
    pub frames_dir: Option<PathBuf>,
    pub translator: Arc<dyn Translator>,
    /// Pause between frames; NFR-03 caps reading at 4 fps.
    pub frame_interval: Duration,
}

pub struct App {
    pub state: watch::Sender<AppState>,
    pub events: broadcast::Sender<Push>,
    /// Flips to true on Ctrl-C so open SSE streams end and shutdown can finish.
    pub shutdown: watch::Receiver<bool>,
    pub translator: Arc<dyn Translator>,
    pub frame_interval: Duration,
    frames_dir: Option<PathBuf>,
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
        let (state, _) = watch::channel(AppState {
            status: Status::Idle,
            profile,
            translator: "gemini",
            session_id: None,
        });
        let (events, _) = broadcast::channel(64);
        Ok(Arc::new(Self {
            state,
            events,
            shutdown,
            translator: cfg.translator,
            frame_interval: cfg.frame_interval,
            frames_dir: cfg.frames_dir,
            db: Mutex::new(cfg.conn),
            running: Mutex::new(None),
        }))
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Start reading frames in a new session (FR-APP-04). No-op if running.
    pub fn start(self: &Arc<Self>) -> Result<(), StartError> {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.is_some() {
            return Ok(());
        }
        let dir = self.frames_dir.as_ref().ok_or(StartError::NoFrameSource)?;
        let frames = Folder::open(dir).map_err(|e| StartError::FrameSource(e.to_string()))?;
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
        });
        let session = pipeline::Session {
            profile_id: profile.id,
            session_id,
        };
        *running = Some(tokio::spawn(pipeline::run(
            self.clone(),
            Box::new(frames),
            session,
        )));
        Ok(())
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
        self.state.send_modify(|s| s.status = Status::Idle);
    }
}
