//! Frames → OCR → dedup → translate → SQLite + /events.

use crate::app::{App, FramesOpening, Push};
use crate::core::dedup::{Change, Dedup};
use crate::core::gemini::MAX_CONTEXT_LINES;
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
    // Source text of this session's lines, oldest first; the last one is the
    // line on screen. Engines that use context get the ones before it.
    let mut earlier: Vec<String> = Vec::new();
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
                let shown = new_line(&app, &session, text.clone(), ocr_ms, &earlier).await;
                earlier.push(text);
                let cap = MAX_CONTEXT_LINES as usize + 1;
                if earlier.len() > cap {
                    earlier.drain(..earlier.len() - cap);
                }
                if let Some(line) = shown {
                    let _ = app.events.send(Push::Line(line.clone()));
                    current = Some(line);
                }
            }
            Some(Change::Update(text)) => {
                if let Some(line) = current.as_mut() {
                    let before = &earlier[..earlier.len().saturating_sub(1)];
                    if retranslate(&app, line, text, before).await {
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

async fn new_line(
    app: &App,
    s: &Session,
    text: String,
    ocr_ms: i64,
    earlier: &[String],
) -> Option<Line> {
    let started = Instant::now();
    let done = translate_or_log(app, &text, earlier).await?;
    let line = Line {
        id: uuid::Uuid::new_v4().to_string(),
        profile_id: s.profile_id.clone(),
        session_id: s.session_id.clone(),
        source: "subtitle".into(),
        source_text: text,
        target_text: done.text,
        engine: done.engine.into(),
        is_fallback: false,
        model_id: done.model_id,
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

async fn retranslate(app: &App, line: &mut Line, text: String, earlier: &[String]) -> bool {
    let started = Instant::now();
    let Some(done) = translate_or_log(app, &text, earlier).await else {
        return false;
    };
    line.source_text = text;
    line.target_text = done.text;
    line.engine = done.engine.into();
    line.model_id = done.model_id;
    line.translate_ms = Some(started.elapsed().as_millis() as i64);
    if let Err(e) = db::update_line(&app.db(), line) {
        eprintln!("update line: {e}");
    }
    true
}

struct Translated {
    engine: &'static str,
    model_id: Option<String>,
    text: String,
}

/// Translates with the engine in use now.
async fn translate_or_log(app: &App, text: &str, earlier: &[String]) -> Option<Translated> {
    let translator = app.translator();
    match translator.translate_in_context(text, earlier).await {
        Ok(t) => Some(Translated {
            engine: translator.engine(),
            model_id: translator.model_id(),
            text: thai::fix_am(&t),
        }),
        Err(e) => {
            eprintln!("translate: {e}");
            None
        }
    }
}
