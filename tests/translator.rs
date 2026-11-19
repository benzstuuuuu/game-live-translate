//! One translator for the whole app (FR-TRN-02), its readiness check before a
//! session (FR-TRN-11), and the sara am fix on every engine's output (FR-TRN-01).
//! Stand-in engines only; the real Apple one is in tests/apple_translation.rs.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::server::router;
use yokoten::translate::{Fake, Translator, Unavailable};
use yokoten::{db, settings};

/// Like Apple without the language pack.
struct NoPack;

impl Translator for NoPack {
    fn engine(&self) -> &'static str {
        "apple"
    }

    fn translate<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Err("no pack".into()) })
    }

    fn check(&self) -> BoxFuture<'_, Result<(), Unavailable>> {
        Box::pin(async {
            Err(Unavailable {
                message: "ยังไม่มีชุดภาษา".into(),
                fix: "ดาวน์โหลดใน System Settings".into(),
            })
        })
    }
}

/// Writes sara am the way some engines do: nikhahit + sara aa.
struct SplitAm;

impl Translator for SplitAm {
    fn engine(&self) -> &'static str {
        "split-am"
    }

    fn translate<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("ท\u{0E4D}\u{0E32}ได้".into()) })
    }
}

fn app_with(
    translators: Vec<Arc<dyn Translator>>,
    frames_dir: Option<PathBuf>,
) -> (Arc<App>, Router, watch::Sender<bool>) {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir,
        obs: None,
        translators,
        frame_interval: Duration::from_millis(5),
        live: None,
    };
    let app = App::new(cfg, shutdown).unwrap();
    (app.clone(), router(app), shutdown_tx)
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let peer: SocketAddr = "192.168.1.20:50000".parse().unwrap();
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo(peer))
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn put_translator(app: &Router, name: &str) -> (StatusCode, serde_json::Value) {
    let body = format!(r#"{{"translator":"{name}"}}"#);
    call(app, Method::PUT, "/api/translator", Some(&body)).await
}

#[tokio::test]
async fn first_translator_is_the_default() {
    let (_, app, _s) = app_with(vec![Arc::new(Fake), Arc::new(SplitAm)], None);
    let (_, state) = call(&app, Method::GET, "/api/state", None).await;
    assert_eq!(state["translator"], "fake");
    assert!(state.get("error").is_none(), "{state}");
}

#[tokio::test]
async fn switch_from_lan_reaches_every_open_page() {
    let (app_state, app, _s) = app_with(vec![Arc::new(Fake), Arc::new(SplitAm)], None);
    // Two pages following /events, e.g. /setup and /reader.
    let mut pages = [app_state.state.subscribe(), app_state.state.subscribe()];

    let (status, state) = put_translator(&app, "split-am").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["translator"], "split-am");
    for page in &mut pages {
        page.changed().await.unwrap();
        assert_eq!(page.borrow_and_update().translator, "split-am");
    }
    assert_eq!(app_state.translator().engine(), "split-am");
}

#[tokio::test]
async fn unknown_translator_is_rejected() {
    let (_, app, _s) = app_with(vec![Arc::new(Fake)], None);
    let (status, body) = put_translator(&app, "gemini").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].is_string());
    let (_, state) = call(&app, Method::GET, "/api/state", None).await;
    assert_eq!(state["translator"], "fake");
}

#[tokio::test]
async fn start_without_language_pack_explains_and_how_to_fix() {
    let (_, app, _s) = app_with(vec![Arc::new(NoPack), Arc::new(Fake)], None);
    let (status, body) = call(&app, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "ยังไม่มีชุดภาษา");

    let (_, state) = call(&app, Method::GET, "/api/state", None).await;
    assert_eq!(state["status"], "error");
    assert_eq!(state["error"]["code"], "translator_unavailable");
    assert_eq!(state["error"]["message"], "ยังไม่มีชุดภาษา");
    assert_eq!(state["error"]["fix"], "ดาวน์โหลดใน System Settings");
}

#[tokio::test]
async fn switching_to_a_ready_translator_clears_the_error() {
    let (_, app, _s) = app_with(vec![Arc::new(NoPack), Arc::new(Fake)], None);
    call(&app, Method::POST, "/api/start", None).await;

    let (status, state) = put_translator(&app, "fake").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["status"], "idle");
    assert!(state.get("error").is_none(), "{state}");
}

#[tokio::test]
async fn switching_to_an_unready_translator_reports_at_once() {
    let (_, app, _s) = app_with(vec![Arc::new(Fake), Arc::new(NoPack)], None);
    let (status, state) = put_translator(&app, "apple").await;
    assert_eq!(status, StatusCode::OK, "the choice itself is kept");
    assert_eq!(state["translator"], "apple");
    assert_eq!(state["error"]["code"], "translator_unavailable");
}

#[tokio::test]
async fn every_engine_output_gets_sara_am() {
    let frames = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames");
    let (app_state, app, _s) = app_with(vec![Arc::new(SplitAm)], Some(frames));
    let mut lines = app_state.events.subscribe();
    let (status, _) = call(&app, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK);

    let push = tokio::time::timeout(Duration::from_secs(60), lines.recv())
        .await
        .expect("pipeline too slow")
        .unwrap();
    let yokoten::app::Push::Line(line) = push else {
        panic!("first push should be a line");
    };
    assert_eq!(line.target_text, "ทำได้");
    assert_eq!(line.engine, "split-am");
    app_state.stop();
}

#[tokio::test]
async fn chosen_translator_is_saved_to_settings() {
    let (app_state, app, _s) = app_with(vec![Arc::new(Fake), Arc::new(SplitAm)], None);
    let path = std::env::temp_dir().join(format!("yokoten-test-{}.json", uuid::Uuid::new_v4()));
    let saver = tokio::spawn(settings::keep_translator(
        app_state.state.subscribe(),
        path.clone(),
    ));

    put_translator(&app, "split-am").await;
    for _ in 0..100 {
        if settings::translator(&path).as_deref() == Some("split-am") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(settings::translator(&path).as_deref(), Some("split-am"));
    saver.abort();
    let _ = std::fs::remove_file(path);
}
