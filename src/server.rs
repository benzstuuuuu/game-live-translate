//! HTTP server: pages from `web/`, SSE at /events, JSON API, and the loopback
//! guard for /setup and settings APIs (FR-SET-08).

use crate::app::{App, AppState, Push, StartError};
use crate::core::access::{lan_allowed, same_origin};
use crate::{db, obs};
use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::{header, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{stream, Stream, StreamExt};
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};

#[derive(RustEmbed)]
#[folder = "web/"]
struct Web;

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/reader") }))
        .route("/reader", get(|| async { page("reader.html") }))
        .route("/overlay", get(|| async { page("overlay.html") }))
        .route("/setup", get(|| async { page("setup.html") }))
        .route("/events", get(events))
        .route("/api/state", get(current_state))
        .route("/api/start", post(start))
        .route("/api/stop", post(stop))
        .route("/api/lines", get(lines))
        .route("/api/obs/setup", post(obs_setup))
        .fallback(asset)
        .layer(middleware::from_fn(loopback_guard))
        .with_state(app)
}

async fn loopback_guard(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let same = {
        let header = |name| req.headers().get(name).and_then(|v| v.to_str().ok());
        same_origin(
            req.method().as_str(),
            header(header::ORIGIN),
            header(header::HOST),
        )
    };
    if !same {
        return (StatusCode::FORBIDDEN, "คำสั่งจากเว็บอื่นถูกปฏิเสธ").into_response();
    }
    let loopback = peer.ip().to_canonical().is_loopback();
    if loopback || lan_allowed(req.method().as_str(), req.uri().path()) {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "ใช้ได้จากเครื่อง Mac ที่รัน Yokoten เท่านั้น").into_response()
    }
}

async fn events(State(app): State<Arc<App>>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut shutdown = app.shutdown.clone();
    let states = WatchStream::new(app.state.subscribe()).map(|s| json_event("state", &s));
    // A lagging phone skips missed lines; it reloads history on the next state.
    let pushes = BroadcastStream::new(app.events.subscribe()).filter_map(|p| async move {
        p.ok().map(|p| match p {
            Push::Line(l) => json_event("line", &l),
            Push::LineUpdated(l) => json_event("line-updated", &l),
            Push::SubtitleGone => Event::default().event("subtitle-gone").data("{}"),
        })
    });
    let all = stream::select(states, pushes)
        .map(|e| Ok(e.retry(Duration::from_secs(2))))
        .take_until(async move {
            let _ = shutdown.wait_for(|stop| *stop).await;
        });
    Sse::new(all).keep_alive(KeepAlive::default())
}

fn json_event(name: &str, data: &impl serde::Serialize) -> Event {
    Event::default()
        .event(name)
        .data(serde_json::to_string(data).unwrap_or_default())
}

async fn current_state(State(app): State<Arc<App>>) -> Json<AppState> {
    Json(app.state.borrow().clone())
}

async fn start(State(app): State<Arc<App>>) -> Response {
    let reason = match app.start() {
        Ok(()) => return Json(app.state.borrow().clone()).into_response(),
        Err(StartError::NoFrameSource) => "ยังไม่มีแหล่งภาพ".to_string(),
        Err(StartError::FrameSource(e)) => format!("เปิดแหล่งภาพไม่ได้: {e}"),
        Err(StartError::NoProfile) => "ยังไม่มีโปรไฟล์เกม".to_string(),
    };
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({ "error": reason })),
    )
        .into_response()
}

async fn stop(State(app): State<Arc<App>>) -> Json<AppState> {
    app.stop();
    Json(app.state.borrow().clone())
}

/// "ตั้ง OBS ให้" (FR-ONB-07): scene yokoten-ocr + crop from the profile's box.
async fn obs_setup(State(app): State<Arc<App>>) -> Response {
    let Some(link) = app.obs().cloned() else {
        return conflict("ไม่ได้ต่อ OBS");
    };
    let region = app
        .state
        .borrow()
        .profile
        .as_ref()
        .map(|p| db::profile_region(&app.db(), &p.id));
    let region = match region {
        Some(Ok(Some(r))) => r,
        Some(Err(e)) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        _ => return conflict("โปรไฟล์นี้ยังไม่มีกรอบ subtitle"),
    };
    match obs::setup(&link, region).await {
        Ok(done) => Json(done).into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

fn conflict(reason: &str) -> Response {
    (
        StatusCode::CONFLICT,
        Json(serde_json::json!({ "error": { "message": reason } })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct Page {
    before: Option<i64>,
    limit: Option<u32>,
}

/// History of the current session, newest first.
async fn lines(State(app): State<Arc<App>>, Query(p): Query<Page>) -> Response {
    let Some(session) = app.state.borrow().session_id.clone() else {
        return Json(Vec::<db::Line>::new()).into_response();
    };
    match db::session_lines(
        &app.db(),
        &session,
        p.before,
        p.limit.unwrap_or(50).min(200),
    ) {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn page(name: &str) -> Response {
    match Web::get(name) {
        Some(f) => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], f.data).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Static files (css, js, fonts). HTML only goes through the named routes so
/// /setup.html cannot slip past the guard.
async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.ends_with(".html") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match Web::get(path) {
        Some(f) => (
            [(header::CONTENT_TYPE, f.metadata.mimetype().to_owned())],
            f.data,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
