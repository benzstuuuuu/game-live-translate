//! Translate a line again with an engine the player picks on /reader
//! (FR-TRN-08). The new text replaces the line in SQLite and reaches every
//! page as `line-updated`. Answers on the LAN (`core::access`).

use crate::app::{App, Push};
use crate::core::gemini::MAX_CONTEXT_LINES;
use crate::db::{self, Line};
use crate::{fallback, glossary};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, PartialEq)]
pub enum RetranslateError {
    NotFound,
    UnknownTranslator,
    /// The engine could not translate it; the reason from the engine.
    Failed(String),
    Db(String),
}

impl From<rusqlite::Error> for RetranslateError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl IntoResponse for RetranslateError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, "ไม่มีบรรทัดนี้แล้ว".to_string()),
            Self::UnknownTranslator => (StatusCode::BAD_REQUEST, "ไม่รู้จักตัวแปลนี้".to_string()),
            Self::Failed(e) => (StatusCode::BAD_GATEWAY, format!("แปลใหม่ไม่สำเร็จ: {e}")),
            Self::Db(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// Line `id` translated again by `engine`, with its game's glossary and the
/// session's lines before it as context; saved and sent as `line-updated`.
/// The player picked the engine, so the line no longer counts as a backup
/// one (`is_fallback` false). Gemini uses its model for this job (FR-TRN-13).
pub async fn retranslate(app: &App, id: &str, engine: &str) -> Result<Line, RetranslateError> {
    let picked = app
        .find_translator(engine)
        .ok_or(RetranslateError::UnknownTranslator)?;
    let translator = picked.retranslator().unwrap_or(picked);
    let (mut line, earlier, terms) = {
        let conn = app.db();
        let line = db::find_line(&conn, id)?.ok_or(RetranslateError::NotFound)?;
        let before = db::session_lines(
            &conn,
            &line.session_id,
            Some(line.created_at),
            MAX_CONTEXT_LINES as u32,
        )?;
        let earlier: Vec<String> = before.into_iter().rev().map(|l| l.source_text).collect();
        let terms = glossary::terms(&conn, &line.profile_id)?;
        (line, earlier, terms)
    };
    let started = Instant::now();
    let text = translator
        .translate_with_glossary(&line.source_text, &earlier, &terms)
        .await;
    app.count_tokens();
    let text = text.map_err(RetranslateError::Failed)?;
    let done = fallback::done(&*translator, &text, false);
    line.target_text = done.text;
    line.engine = done.engine.into();
    line.is_fallback = done.is_fallback;
    line.model_id = done.model_id;
    line.translate_ms = Some(started.elapsed().as_millis() as i64);
    db::update_line(&app.db(), &line)?;
    let _ = app.events.send(Push::LineUpdated(line.clone()));
    Ok(line)
}

/// Routes of this module; `server::router` merges them under its guard.
pub fn routes() -> Router<Arc<App>> {
    Router::new().route("/api/lines/{id}/retranslate", post(handler))
}

#[derive(Deserialize)]
struct Choice {
    translator: String,
}

async fn handler(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(choice): Json<Choice>,
) -> Result<Json<Line>, RetranslateError> {
    Ok(Json(retranslate(&app, &id, &choice.translator).await?))
}
