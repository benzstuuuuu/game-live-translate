//! Server integration: requests carry a fake peer address so LAN vs loopback
//! can be tested without real network interfaces.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::server::router;
use yokoten::{db, translate};

const LAN: &str = "192.168.1.20:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

fn app() -> (Router, watch::Sender<bool>) {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: None,
        translators: vec![Arc::new(translate::Fake)],
        frame_interval: Duration::from_millis(10),
    };
    (router(App::new(cfg, shutdown).unwrap()), shutdown_tx)
}

async fn get(app: Router, path: &str, peer: &str) -> axum::response::Response {
    let peer: SocketAddr = peer.parse().unwrap();
    let req = Request::get(path)
        .extension(ConnectInfo(peer))
        .body(Body::empty())
        .unwrap();
    app.oneshot(req).await.unwrap()
}

#[tokio::test]
async fn setup_is_forbidden_from_lan() {
    let (app, _s) = app();
    assert_eq!(
        get(app, "/setup", LAN).await.status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn setup_is_served_on_loopback() {
    let (app, _s) = app();
    let res = get(app, "/setup", LOOPBACK).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn ipv6_loopback_counts_as_loopback() {
    let (app, _s) = app();
    assert_eq!(
        get(app, "/setup", "[::1]:50000").await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn reader_and_overlay_open_on_lan() {
    for path in [
        "/reader",
        "/overlay",
        "/tokens.css",
        "/app.js",
        "/fonts.css",
    ] {
        let (app, _s) = app();
        assert_eq!(get(app, path, LAN).await.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn html_files_are_not_reachable_by_file_name() {
    let (app, _s) = app();
    assert_eq!(
        get(app, "/setup.html", LOOPBACK).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn fonts_are_served_as_woff2() {
    let (app, _s) = app();
    let res = get(app, "/fonts/ibm-plex-sans-thai-thai-600-normal.woff2", LAN).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "font/woff2");
}

#[tokio::test]
async fn unknown_api_is_loopback_only() {
    let (app, _s) = app();
    assert_eq!(
        get(app, "/api/settings", LAN).await.status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn api_state_starts_idle_with_default_profile() {
    let (app, _s) = app();
    let res = get(app, "/api/state", LAN).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["status"], "idle");
    assert_eq!(v["profile"]["source"], "subtitle");
}

#[tokio::test]
async fn events_sends_state_on_connect_and_ends_on_shutdown() {
    let (app, shutdown) = app();
    let res = get(app, "/events", LAN).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/event-stream");

    let mut body = res.into_body();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let text = String::from_utf8(frame.to_vec()).unwrap();
    assert!(text.contains("event: state"), "{text}");
    assert!(text.contains(r#""status":"idle""#), "{text}");

    shutdown.send(true).unwrap();
    let rest = tokio::time::timeout(std::time::Duration::from_secs(2), body.collect()).await;
    assert!(rest.is_ok(), "SSE stream must end on shutdown");
}
