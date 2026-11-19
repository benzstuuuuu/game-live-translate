//! /overlay (FR-STR-01 to 03): the page, the SSE it follows (line,
//! subtitle-gone, bar) and the bar position kept per profile.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::core::bar;
use yokoten::server::router;
use yokoten::{db, translate};

const LAN: &str = "192.168.1.20:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

fn app_with(
    conn: rusqlite::Connection,
    frames_dir: Option<PathBuf>,
) -> (Router, watch::Sender<bool>) {
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir,
        obs: None,
        translators: vec![Arc::new(translate::Fake)],
        frame_interval: Duration::from_millis(5),
        live: None,
    };
    (router(App::new(cfg, shutdown).unwrap()), shutdown_tx)
}

fn app(frames_dir: Option<PathBuf>) -> (Router, watch::Sender<bool>) {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    app_with(conn, frames_dir)
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames")
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    peer: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let peer: SocketAddr = peer.parse().unwrap();
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(peer));
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let body = body.map_or(Body::empty(), |b| Body::from(b.to_string()));
    app.clone().oneshot(req.body(body).unwrap()).await.unwrap()
}

async fn json_of(res: axum::response::Response) -> Value {
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

async fn get_json(app: &Router, path: &str) -> Value {
    json_of(call(app, Method::GET, path, LOOPBACK, None).await).await
}

/// Reads SSE events (name, data), skipping `state`, until `done` says so.
async fn read_until(
    body: &mut Body,
    done: impl Fn(&[(String, Value)]) -> bool,
) -> Vec<(String, Value)> {
    let mut buf = String::new();
    let mut out = Vec::new();
    while !done(&out) {
        let frame = body.frame().await.expect("stream ended").unwrap();
        let Ok(data) = frame.into_data() else {
            continue;
        };
        buf.push_str(std::str::from_utf8(&data).unwrap());
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            let field = |k: &str| {
                block
                    .lines()
                    .find_map(|l| l.strip_prefix(k).map(str::to_owned))
            };
            if let (Some(name), Some(data)) = (field("event: "), field("data: ")) {
                if name != "state" {
                    out.push((name, serde_json::from_str(&data).unwrap()));
                }
            }
        }
    }
    out
}

fn has(events: &[(String, Value)], name: &str) -> bool {
    events.iter().any(|(n, _)| n == name)
}

#[tokio::test]
async fn overlay_page_is_transparent_without_buttons_and_open_on_lan() {
    let (app, _s) = app(None);
    let res = call(&app, Method::GET, "/overlay", LAN, None).await;
    assert_eq!(res.status(), StatusCode::OK);
    let html =
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    assert!(html.contains("background: transparent"));
    assert!(
        !html.contains("<button"),
        "the stream must not show controls"
    );
    assert!(html.contains("overlay.js"));
    let js = call(&app, Method::GET, "/overlay.js", LAN, None).await;
    assert_eq!(js.status(), StatusCode::OK);
}

#[tokio::test]
async fn overlay_events_carry_line_and_subtitle_gone() {
    let (app, _s) = app(Some(fixtures()));
    let mut events = call(&app, Method::GET, "/events", LAN, None)
        .await
        .into_body();
    let res = call(&app, Method::POST, "/api/start", LOOPBACK, None).await;
    assert_eq!(res.status(), StatusCode::OK);

    let got = tokio::time::timeout(
        Duration::from_secs(60),
        read_until(&mut events, |e| has(e, "subtitle-gone")),
    )
    .await
    .expect("pipeline too slow");
    let (name, line) = &got[0];
    assert_eq!(name, "line");
    assert_eq!(line["target_text"], "(แปล) Keep your lantern low.");
    assert!(line["id"].is_string());
    assert_eq!(got.last().unwrap().0, "subtitle-gone");
}

#[tokio::test]
async fn bar_move_reaches_overlay_and_stays_with_the_profile() {
    let (app, _s) = app(None);
    assert_eq!(
        get_json(&app, "/api/bar").await,
        serde_json::to_value(bar::DEFAULT).unwrap()
    );

    // Moving the bar is a /setup job: loopback only.
    let want = json!({ "x": 0.2, "y": 0.3, "w": 0.6 });
    let lan = call(&app, Method::PUT, "/api/bar", LAN, Some(want.clone())).await;
    assert_eq!(lan.status(), StatusCode::FORBIDDEN);

    let mut events = call(&app, Method::GET, "/events", LAN, None)
        .await
        .into_body();
    // Hanging over the bottom edge: stored inside the frame.
    let res = call(
        &app,
        Method::PUT,
        "/api/bar",
        LOOPBACK,
        Some(json!({ "x": 0.2, "y": 0.99, "w": 0.6 })),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let saved = json_of(res).await;
    let y = saved["y"].as_f64().unwrap();
    assert!((y - (1.0 - bar::HEIGHT)).abs() < 1e-9, "y = {y}");

    let got = tokio::time::timeout(
        Duration::from_secs(5),
        read_until(&mut events, |e| has(e, "bar")),
    )
    .await
    .expect("no bar event");
    assert_eq!(got.last().unwrap(), &("bar".to_owned(), saved.clone()));
    assert_eq!(get_json(&app, "/api/bar").await, saved);

    // Another profile has its own bar; going back brings the moved one back.
    let first = get_json(&app, "/api/state").await["profile"]["id"].clone();
    let other = json_of(
        call(
            &app,
            Method::POST,
            "/api/profiles",
            LOOPBACK,
            Some(json!({ "name": "B" })),
        )
        .await,
    )
    .await["id"]
        .clone();
    let pick = |id: &Value| Some(json!({ "id": id }));
    call(&app, Method::PUT, "/api/profile", LOOPBACK, pick(&other)).await;
    assert_eq!(
        get_json(&app, "/api/bar").await,
        serde_json::to_value(bar::DEFAULT).unwrap()
    );
    call(&app, Method::PUT, "/api/profile", LOOPBACK, pick(&first)).await;
    assert_eq!(get_json(&app, "/api/bar").await, saved);
}

#[tokio::test]
async fn bar_survives_a_restart() {
    let dir = std::env::temp_dir().join(format!("yokoten-bar-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("yokoten.db");
    let open = |p: &Path| db::open(p).unwrap();
    let moved = json!({ "x": 0.15, "y": 0.5, "w": 0.7 });
    {
        let (app, _s) = app_with(open(&path), None);
        let res = call(&app, Method::PUT, "/api/bar", LOOPBACK, Some(moved.clone())).await;
        assert_eq!(res.status(), StatusCode::OK);
    }
    let (app, _s) = app_with(open(&path), None);
    assert_eq!(get_json(&app, "/api/bar").await, moved);
    std::fs::remove_dir_all(dir).unwrap();
}
