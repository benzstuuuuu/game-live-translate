//! HTTP server: pages from `web/`, SSE at /events, and the loopback guard
//! for /setup and settings APIs (FR-SET-08).

use crate::core::access::lan_allowed;
use crate::db::ProfileRef;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{Stream, StreamExt};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio_stream::wrappers::WatchStream;

/// AppState of SRS 8.4, trimmed to what exists so far.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppState {
    pub status: Status,
    pub profile: Option<ProfileRef>,
    pub translator: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Idle,
    Translating,
    Error,
}

impl AppState {
    pub fn idle(profile: Option<ProfileRef>) -> Self {
        Self {
            status: Status::Idle,
            profile,
            translator: "gemini",
        }
    }
}

pub struct App {
    pub state: watch::Sender<AppState>,
    /// Flips to true on Ctrl-C so open SSE streams end and shutdown can finish.
    pub shutdown: watch::Receiver<bool>,
}

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
        .fallback(asset)
        .layer(middleware::from_fn(loopback_guard))
        .with_state(app)
}

async fn loopback_guard(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let loopback = peer.ip().to_canonical().is_loopback();
    if loopback || lan_allowed(req.method().as_str(), req.uri().path()) {
        next.run(req).await
    } else {
        (StatusCode::FORBIDDEN, "ใช้ได้จากเครื่อง Mac ที่รัน Yokoten เท่านั้น").into_response()
    }
}

async fn events(State(app): State<Arc<App>>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut shutdown = app.shutdown.clone();
    let stream = WatchStream::new(app.state.subscribe())
        .map(|s| {
            Ok(Event::default()
                .event("state")
                .retry(Duration::from_secs(2))
                .data(serde_json::to_string(&s).unwrap_or_default()))
        })
        .take_until(async move {
            let _ = shutdown.wait_for(|stop| *stop).await;
        });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn current_state(State(app): State<Arc<App>>) -> Json<AppState> {
    Json(app.state.borrow().clone())
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
