//! Translate a line again from /reader (FR-TRN-08) with the engine picked
//! for that job on /setup (FR-TRN-16). The new text replaces the line in
//! SQLite and reaches every page as `line-updated`. Answers on the LAN
//! (`core::access`).

use crate::app::{App, Push};
use crate::core::gemini::MAX_CONTEXT_LINES;
use crate::core::jobs::Job;
use crate::db::{self, Line};
use crate::translate::Unavailable;
use crate::{fallback, glossary};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, PartialEq)]
pub enum RetranslateError {
    NotFound,
    /// The picked engine cannot translate now; no other engine stands in.
    Unavailable(Unavailable),
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
        let (status, body) = match self {
            Self::NotFound => (StatusCode::NOT_FOUND, error("ไม่มีบรรทัดนี้แล้ว".into())),
            Self::Unavailable(u) => (
                StatusCode::CONFLICT,
                serde_json::json!({ "error": u.message, "fix": u.fix }),
            ),
            Self::Failed(e) => (
                StatusCode::BAD_GATEWAY,
                error(format!("แปลใหม่ไม่สำเร็จ: {e}")),
            ),
            Self::Db(e) => (StatusCode::INTERNAL_SERVER_ERROR, error(e)),
        };
        (status, Json(body)).into_response()
    }
}

fn error(message: String) -> serde_json::Value {
    serde_json::json!({ "error": message })
}

/// Line `id` translated again by the engine picked for "แปลใหม่", with its
/// game's glossary and the session's lines before it as context; saved and
/// sent as `line-updated`. The line keeps the engine and model that did it,
/// and no longer counts as a backup one (`is_fallback` false).
pub async fn retranslate(app: &App, id: &str) -> Result<Line, RetranslateError> {
    let translator = app.translator_for(Job::Retranslate);
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
    translator
        .check()
        .await
        .map_err(RetranslateError::Unavailable)?;
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

async fn handler(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Line>, RetranslateError> {
    Ok(Json(retranslate(&app, &id).await?))
}
