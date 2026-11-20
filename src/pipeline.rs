//! Frames → OCR → dedup → translate → SQLite + /events.

use crate::app::{App, Opener, Push};
use crate::core::dedup::{Change, Dedup};
use crate::core::gemini::MAX_CONTEXT_LINES;
use crate::core::ocr_gate::{thumbnail, Gate};
use crate::db::{self, Line};
use crate::fallback::{self, Translated};
use crate::frames::Frame;
use crate::ocr;
use crate::recovery;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct Session {
    pub profile_id: String,
    pub session_id: String,
}

pub async fn run(app: Arc<App>, frames: Opener, session: Session) {
    // Waits through OBS or the card failing and resumes (FR-ERR-04 to 06).
    let mut frames = recovery::Frames::new(app.clone(), frames);
    // OCR only when the picture changed (#102). Dedup still gets every frame.
    let gate = Arc::new(Mutex::new(Gate::default()));
    let mut dedup = Dedup::default();
    let mut current: Option<Line> = None;
    // Source text of this session's lines, oldest first; the last one is the
    // line on screen. Engines that use context get the ones before it.
    let mut earlier: Vec<String> = Vec::new();
    let mut tick = tokio::time::interval(app.frame_interval);
    // No burst of frames after waiting out a problem (NFR-03).
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let Some(frame) = frames.next().await else {
            break;
        };
        let gate = gate.clone();
        let reading = tokio::task::spawn_blocking(move || {
            read(&mut gate.lock().unwrap_or_else(|e| e.into_inner()), &frame)
        });
        let (lines, ocr_ms) = match reading.await {
            Ok(Ok(read)) => read,
            Ok(Err(e)) => {
                eprintln!("{e}");
                continue;
            }
            Err(_) => continue,
        };
        match dedup.push(&lines, Instant::now()) {
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

/// Text lines of a frame, top to bottom, and how long their OCR took in ms.
type Read = (Vec<String>, i64);

/// What `frame` reads: OCR'd when `gate` says the picture changed, otherwise
/// the last reading again, with the time that one took.
fn read(gate: &mut Gate<Read>, frame: &Frame) -> Result<Read, String> {
    let started = Instant::now();
    let grey = match frame {
        Frame::File(path) => image::open(path),
        Frame::Image(bytes) => image::load_from_memory(bytes),
    }
    .map_err(|e| format!("decode frame: {e}"))?
    .into_luma8();
    let (w, h) = (grey.width() as usize, grey.height() as usize);
    gate.read(thumbnail(grey.as_raw(), w, h), started, || {
        let lines = ocr::read_frame(frame)?;
        Ok((lines, started.elapsed().as_millis() as i64))
    })
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
        is_fallback: done.is_fallback,
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
    line.is_fallback = done.is_fallback;
    line.model_id = done.model_id;
    line.translate_ms = Some(started.elapsed().as_millis() as i64);
    if let Err(e) = db::update_line(&app.db(), line) {
        eprintln!("update line: {e}");
    }
    true
}

/// Translates with the engine in use now, or its backup (FR-TRN-04).
async fn translate_or_log(app: &App, text: &str, earlier: &[String]) -> Option<Translated> {
    let done = fallback::translate(app, text, earlier).await;
    app.count_tokens();
    done.inspect_err(|e| eprintln!("translate: {e}")).ok()
}
