//! Read-only API for the LCD board (#85): status and the latest line as
//! small JSON, and that line in Thai as a 1-bit PBM drawn on the Mac. The
//! board pulls both over the LAN (`core::access`).

pub mod draw;

use crate::app::{App, Status};
use crate::core::board::{self, Board, BoardLine};
use crate::core::pbm;
use crate::db::{self, Line};
use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

/// Routes of this module; `server::router` merges them under its guard.
pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/api/board", get(status))
        .route("/api/board/line.pbm", get(line_pbm))
}

async fn status(State(app): State<Arc<App>>) -> Response {
    let state = app.state.borrow().clone();
    let line = match latest(&app) {
        Ok(line) => line,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    Json(Board {
        status: match state.status {
            Status::Idle => "idle",
            Status::Translating => "translating",
            Status::Error => "error",
        },
        profile_name: state.profile.as_ref().map(|p| p.name.as_str()),
        error_code: state.error.as_ref().map(|e| e.code),
        line: line
            .as_ref()
            .map(|l| BoardLine::new(&l.id, &l.source, &l.source_text, &l.target_text, l.done)),
    })
    .into_response()
}

#[derive(Deserialize)]
struct Size {
    w: Option<String>,
    h: Option<String>,
}

/// 204 while there is no line or it has no Thai yet; 304 when the board
/// already has this picture.
async fn line_pbm(
    State(app): State<Arc<App>>,
    Query(q): Query<Size>,
    headers: HeaderMap,
) -> Response {
    let (w, h) = board::size(q.w.as_deref(), q.h.as_deref());
    let line = match latest(&app) {
        Ok(line) => line.filter(|l| !l.target_text.trim().is_empty()),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let Some(line) = line else {
        return StatusCode::NO_CONTENT.into_response();
    };
    let etag = board::etag(
        &board::rev(&line.id, &line.source_text, &line.target_text),
        w,
        h,
    );
    let seen = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok());
    if board::not_modified(seen, &etag) {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag)]).into_response();
    }
    let text = line.target_text.trim().to_owned();
    let drawn = tokio::task::spawn_blocking(move || draw::gray(&draw::fit(&text, w, h), w, h));
    match drawn.await {
        Ok(gray) => (
            [
                (header::CONTENT_TYPE, "image/x-portable-bitmap".to_owned()),
                (header::ETAG, etag),
            ],
            pbm::encode(&gray, w, h),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// The latest line of the current session, as `/api/lines?limit=1` gives it.
fn latest(app: &App) -> rusqlite::Result<Option<Line>> {
    let Some(session) = app.state.borrow().session_id.clone() else {
        return Ok(None);
    };
    Ok(db::session_lines(&app.db(), &session, None, 1)?.pop())
}
