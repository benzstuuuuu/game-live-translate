//! Frames → OCR → dedup → translate → SQLite + /events.

use crate::app::{App, FramesOpening, Push};
use crate::core::dedup::{Change, Dedup};
use crate::core::thai;
use crate::db::{self, Line};
use crate::ocr;
use std::sync::Arc;
use std::time::Instant;

pub struct Session {
    pub profile_id: String,
    pub session_id: String,
}

pub async fn run(app: Arc<App>, frames: FramesOpening, session: Session) {
    let mut frames = match frames.await {
        Ok(f) => f,
        Err(e) => return app.fail(e),
    };
    let mut dedup = Dedup::default();
    let mut current: Option<Line> = None;
    let mut tick = tokio::time::interval(app.frame_interval);
    loop {
        tick.tick().await;
        let frame = match frames.next_frame().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return app.fail(e),
        };
        let started = Instant::now();
        let lines = match tokio::task::spawn_blocking(move || ocr::read_frame(&frame)).await {
            Ok(Ok(lines)) => lines,
            Ok(Err(e)) => {
                eprintln!("{e}");
                continue;
            }
            Err(_) => continue,
        };
        let ocr_ms = started.elapsed().as_millis() as i64;
        match dedup.push(&lines) {
            Some(Change::New(text)) => {
                if let Some(line) = new_line(&app, &session, text, ocr_ms).await {
                    let _ = app.events.send(Push::Line(line.clone()));
                    current = Some(line);
                }
            }
            Some(Change::Update(text)) => {
                if let Some(line) = current.as_mut() {
                    if retranslate(&app, line, text).await {
                        let _ = app.events.send(Push::LineUpdated(line.clone()));
                    }
                }
            }
            Some(Change::Gone) => {
                let _ = app.events.send(Push::SubtitleGone);
            }
            None => {}
        }
    }
}

async fn new_line(app: &App, s: &Session, text: String, ocr_ms: i64) -> Option<Line> {
    let started = Instant::now();
    let (engine, target) = translate_or_log(app, &text).await?;
    let line = Line {
        id: uuid::Uuid::new_v4().to_string(),
        profile_id: s.profile_id.clone(),
        session_id: s.session_id.clone(),
        source: "subtitle".into(),
        source_text: text,
        target_text: target,
        engine: engine.into(),
        is_fallback: false,
        model_id: None,
        done: true,
        created_at: db::now_ms(),
        ocr_ms: Some(ocr_ms),
        translate_ms: Some(started.elapsed().as_millis() as i64),
    };
    if let Err(e) = db::insert_line(&app.db(), &line) {
        eprintln!("save line: {e}");
    }
    Some(line)
}

async fn retranslate(app: &App, line: &mut Line, text: String) -> bool {
    let started = Instant::now();
    let Some((engine, target)) = translate_or_log(app, &text).await else {
        return false;
    };
    line.source_text = text;
    line.target_text = target;
    line.engine = engine.into();
    line.translate_ms = Some(started.elapsed().as_millis() as i64);
    if let Err(e) = db::update_line(&app.db(), line) {
        eprintln!("update line: {e}");
    }
    true
}

/// Translates with the engine in use now; returns its name with the Thai text.
async fn translate_or_log(app: &App, text: &str) -> Option<(&'static str, String)> {
    let translator = app.translator();
    match translator.translate(text).await {
        Ok(t) => Some((translator.engine(), thai::fix_am(&t))),
        Err(e) => {
            eprintln!("translate: {e}");
            None
        }
    }
}
