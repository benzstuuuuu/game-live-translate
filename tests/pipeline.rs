//! Server + folder of frames (real Vision OCR) + fake translator: /events must
//! carry each line once, in order, and subtitle-gone when the box empties.
//! Frames are hand-made: see tests/fixtures/make_frames.py.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::server::router;
use yokoten::{db, translate};

fn app(frames_dir: Option<PathBuf>) -> (Router, watch::Sender<bool>) {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir,
        obs: None,
        translators: vec![Arc::new(translate::Fake)],
        frame_interval: Duration::from_millis(5),
    };
    (router(App::new(cfg, shutdown).unwrap()), shutdown_tx)
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames")
}

async fn call(app: &Router, method: Method, path: &str) -> axum::response::Response {
    let peer: SocketAddr = "192.168.1.20:50000".parse().unwrap();
    let req = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(peer))
        .body(Body::empty())
        .unwrap();
    app.clone().oneshot(req).await.unwrap()
}

async fn json(res: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

/// Reads SSE events (name, data) until `want` non-state events arrived.
async fn read_events(body: &mut Body, want: usize) -> Vec<(String, serde_json::Value)> {
    let mut buf = String::new();
    let mut out = Vec::new();
    while out.len() < want {
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

#[tokio::test]
async fn folder_frames_become_lines_once_in_order() {
    let (app, _s) = app(Some(fixtures()));
    let mut events = call(&app, Method::GET, "/events").await.into_body();

    let res = call(&app, Method::POST, "/api/start").await;
    assert_eq!(res.status(), StatusCode::OK);
    let state = json(res).await;
    assert_eq!(state["status"], "translating");

    let got = tokio::time::timeout(Duration::from_secs(60), read_events(&mut events, 5))
        .await
        .expect("pipeline too slow");
    let summary: Vec<(String, String)> = got
        .iter()
        .map(|(n, d)| {
            (
                n.clone(),
                d["source_text"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    let expect = |n: &str, t: &str| (n.to_owned(), t.to_owned());
    assert_eq!(
        summary,
        vec![
            expect("line", "Keep your lantern low."),
            expect("line", "You sure about this, Mara?"),
            expect("subtitle-gone", ""),
            expect("line", "No. But it's the"),
            expect("line-updated", "No. But it's the only road left."),
        ]
    );
    assert_eq!(got[0].1["target_text"], "(แปล) Keep your lantern low.");
    assert_eq!(got[0].1["engine"], "fake");
    assert_eq!(got[3].1["id"], got[4].1["id"], "update keeps the line id");

    // History of the session: three lines, newest first, update applied.
    let history = json(call(&app, Method::GET, "/api/lines").await).await;
    let texts: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["source_text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        vec![
            "No. But it's the only road left.",
            "You sure about this, Mara?",
            "Keep your lantern low."
        ]
    );

    let stopped = json(call(&app, Method::POST, "/api/stop").await).await;
    assert_eq!(stopped["status"], "idle");
}

#[tokio::test]
async fn start_without_frame_source_is_conflict() {
    let (app, _s) = app(None);
    let res = call(&app, Method::POST, "/api/start").await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let state = json(call(&app, Method::GET, "/api/state").await).await;
    assert_eq!(state["status"], "idle");
}

#[tokio::test]
async fn lines_empty_before_any_session() {
    let (app, _s) = app(Some(fixtures()));
    let lines = json(call(&app, Method::GET, "/api/lines").await).await;
    assert_eq!(lines, serde_json::json!([]));
}
