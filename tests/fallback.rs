//! Fallback from Gemini to Apple (#37, FR-TRN-04, 05): a slow or failing
//! Gemini hands the line to the backup, `state` says so, and Gemini comes
//! back when a line works again. The Gemini HTTP API is a fake that can turn
//! slow or fail mid-test; a stand-in named "apple" plays Apple Translation, so
//! this runs in builds without the `apple-translation` feature.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config, Push};
use yokoten::core::fallback::LIMIT;
use yokoten::core::gemini::Settings;
use yokoten::secret::Memory;
use yokoten::server::router;
use yokoten::translate::gemini::{Api, Http};
use yokoten::translate::{Fake, Gemini, Translator};
use yokoten::{db, fallback};

const KEY: &str = "AIzaTestKey-0123456789";
/// A phone on the LAN: /reader's calls must work from there.
const LAN: &str = "192.168.1.20:50000";

/// How the fake Gemini answers the next calls.
#[derive(Clone)]
enum Mode {
    Ok(&'static str),
    /// Answers after this long.
    Slow(Duration),
    Status(u16, Value),
    /// The call never got through, like with Wi-Fi off.
    Down,
}

struct FakeApi {
    mode: Mutex<Mode>,
    calls: Mutex<usize>,
}

impl FakeApi {
    fn new(mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            mode: Mutex::new(mode),
            calls: Mutex::new(0),
        })
    }

    fn set(&self, mode: Mode) {
        *self.mode.lock().unwrap() = mode;
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

fn reply(thai: &str) -> Vec<u8> {
    json!({ "candidates": [{ "content": { "parts": [{ "text": thai }] }, "finishReason": "STOP" }] })
        .to_string()
        .into_bytes()
}

impl Api for FakeApi {
    fn generate<'a>(
        &'a self,
        _key: &'a str,
        _model: &'a str,
        _body: Value,
    ) -> BoxFuture<'a, Result<(u16, Vec<u8>), String>> {
        *self.calls.lock().unwrap() += 1;
        let mode = self.mode.lock().unwrap().clone();
        Box::pin(async move {
            match mode {
                Mode::Ok(thai) => Ok((200, reply(thai))),
                Mode::Slow(wait) => {
                    tokio::time::sleep(wait).await;
                    Ok((200, reply("ช้าไป")))
                }
                Mode::Status(code, body) => Ok((code, body.to_string().into_bytes())),
                Mode::Down => Err("ต่อ Gemini ไม่ได้ ตรวจการเชื่อมต่ออินเทอร์เน็ต".into()),
            }
        })
    }
}

/// Stands in for Apple Translation: same engine name, marks the text.
struct StandIn;

impl Translator for StandIn {
    fn engine(&self) -> &'static str {
        "apple"
    }

    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("(สำรอง) {text}")) })
    }
}

struct Setup {
    app: Arc<App>,
    router: Router,
    _shutdown: watch::Sender<bool>,
}

fn setup_with(
    api: Arc<dyn Api>,
    key: &str,
    backup: Arc<dyn Translator>,
    frames: Option<PathBuf>,
) -> Setup {
    let gemini = Gemini::new(Settings::default(), Arc::new(Memory::with(key)), api, None);
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: frames,
        obs: None,
        translators: vec![Arc::new(gemini), backup],
        frame_interval: Duration::from_millis(5),
    };
    let app = App::new(cfg, shutdown).unwrap();
    Setup {
        router: router(app.clone()),
        app,
        _shutdown: shutdown_tx,
    }
}

fn setup(api: Arc<FakeApi>) -> Setup {
    setup_with(api, KEY, Arc::new(StandIn), None)
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let peer: SocketAddr = LAN.parse().unwrap();
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(peer));
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let req = req
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

/// The `fallback` of each `state` event in an open /events stream, until one
/// matches `until`.
async fn states_until(body: &mut Body, until: impl Fn(&Value) -> bool) -> Vec<Value> {
    let mut buf = String::new();
    let mut seen = Vec::new();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(10), body.frame())
            .await
            .expect("no state event in time")
            .expect("stream ended")
            .unwrap();
        let Ok(data) = frame.into_data() else {
            continue;
        };
        buf.push_str(std::str::from_utf8(&data).unwrap());
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            if !block.contains("event: state\n") {
                continue;
            }
            let data = block
                .lines()
                .find_map(|l| l.strip_prefix("data: "))
                .unwrap();
            let fb = serde_json::from_str::<Value>(data).unwrap()["fallback"].clone();
            let done = until(&fb);
            seen.push(fb);
            if done {
                return seen;
            }
        }
    }
}

fn fallback_state(app: &App) -> Value {
    serde_json::to_value(&app.state.borrow().fallback).unwrap()
}

// ---- FR-TRN-04: the line itself comes from the backup ----

#[tokio::test]
async fn a_slow_gemini_line_comes_from_apple_within_the_limit() {
    let api = FakeApi::new(Mode::Slow(LIMIT + Duration::from_secs(5)));
    let s = setup(api.clone());
    let started = Instant::now();
    let line = fallback::translate(&s.app, "Hold the gate.", &[])
        .await
        .unwrap();
    let took = started.elapsed();
    assert!(took >= LIMIT, "{took:?}");
    assert!(took < LIMIT + Duration::from_secs(1), "{took:?}");
    assert_eq!(line.engine, "apple");
    assert!(line.is_fallback);
    assert_eq!(line.text, "(สำรอง) Hold the gate.");
    assert_eq!(line.model_id, None);
    assert_eq!(
        fallback_state(&s.app),
        json!({ "active": true, "reason": "Gemini ไม่ตอบภายใน 3 วินาที" })
    );
}

#[tokio::test]
async fn a_gemini_error_line_comes_from_apple_with_the_reason() {
    let quota = json!({ "error": { "code": 429, "status": "RESOURCE_EXHAUSTED" } });
    let api = FakeApi::new(Mode::Status(429, quota));
    let s = setup(api.clone());
    let line = fallback::translate(&s.app, "Run.", &[]).await.unwrap();
    assert_eq!((line.engine, line.is_fallback), ("apple", true));
    let reason = s.app.state.borrow().fallback.reason.clone().unwrap();
    assert!(reason.contains("โควตา"), "{reason}");
    assert!(!reason.contains(KEY));

    // Later lines skip Gemini until the next try is due (FR-TRN-05).
    let line = fallback::translate(&s.app, "Now!", &[]).await.unwrap();
    assert_eq!(line.engine, "apple");
    assert_eq!(api.calls(), 1, "Gemini asked once");
}

#[tokio::test]
async fn a_working_gemini_keeps_its_lines() {
    let api = FakeApi::new(Mode::Ok("ไปกันเถอะ"));
    let s = setup(api.clone());
    let line = fallback::translate(&s.app, "Let's go.", &[]).await.unwrap();
    assert_eq!(line.engine, "gemini");
    assert!(!line.is_fallback);
    assert_eq!(line.model_id.as_deref(), Some("gemini-3.8-flash"));
    assert_eq!(fallback_state(&s.app), json!({ "active": false }));
}

// ---- FR-TRN-05: what pages see, and the way back ----

#[tokio::test]
async fn sse_state_shows_fallback_and_clears_when_gemini_is_back() {
    let api = FakeApi::new(Mode::Down);
    let s = setup(api.clone());
    let (status, _) = call(&s.router, Method::GET, "/api/state", None).await;
    assert_eq!(status, StatusCode::OK);
    let req = Request::get("/events")
        .extension(ConnectInfo::<SocketAddr>(LAN.parse().unwrap()))
        .body(Body::empty())
        .unwrap();
    let mut events = s.router.clone().oneshot(req).await.unwrap().into_body();
    let first = states_until(&mut events, |_| true).await;
    assert_eq!(first, vec![json!({ "active": false })]);

    // Wi-Fi off: the line goes to Apple and every page hears why.
    fallback::translate(&s.app, "Where is she?", &[])
        .await
        .unwrap();
    let on = states_until(&mut events, |f| f["active"] == true).await;
    assert_eq!(
        on.last().unwrap(),
        &json!({ "active": true, "reason": "ต่อ Gemini ไม่ได้ ตรวจการเชื่อมต่ออินเทอร์เน็ต" })
    );

    // Wi-Fi back; "ลอง Gemini อีกครั้ง" picks Gemini again from the phone.
    api.set(Mode::Ok("เธออยู่ไหน"));
    let (status, body) = call(
        &s.router,
        Method::PUT,
        "/api/translator",
        Some(json!({ "translator": "gemini" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["fallback"]["active"], true,
        "still on Apple until a line works"
    );

    let line = fallback::translate(&s.app, "Where is she?", &[])
        .await
        .unwrap();
    assert_eq!((line.engine, line.is_fallback), ("gemini", false));
    let off = states_until(&mut events, |f| f["active"] == false).await;
    assert_eq!(off.last().unwrap(), &json!({ "active": false }));
    assert_eq!(api.calls(), 2);
}

#[tokio::test]
async fn retry_that_fails_again_stays_on_apple() {
    let api = FakeApi::new(Mode::Down);
    let s = setup(api.clone());
    fallback::translate(&s.app, "One.", &[]).await.unwrap();
    assert!(s.app.set_translator("gemini"));
    let line = fallback::translate(&s.app, "Two.", &[]).await.unwrap();
    assert_eq!(line.engine, "apple");
    assert_eq!(api.calls(), 2, "the retry asked Gemini");
    assert!(s.app.state.borrow().fallback.active);
}

#[tokio::test]
async fn picking_another_translator_or_stopping_ends_fallback() {
    let api = FakeApi::new(Mode::Down);
    let s = setup(api.clone());
    fallback::translate(&s.app, "One.", &[]).await.unwrap();
    assert!(s.app.state.borrow().fallback.active);
    assert!(s.app.set_translator("apple"));
    assert_eq!(fallback_state(&s.app), json!({ "active": false }));

    assert!(s.app.set_translator("gemini"));
    fallback::translate(&s.app, "Two.", &[]).await.unwrap();
    assert!(s.app.state.borrow().fallback.active);
    s.app.stop();
    assert_eq!(fallback_state(&s.app), json!({ "active": false }));
}

// ---- without a backup (no Apple in the build, or another engine in use) ----

#[tokio::test]
async fn without_a_backup_a_failed_line_is_dropped_and_no_fallback_shows() {
    let api = FakeApi::new(Mode::Down);
    let s = setup_with(api.clone(), KEY, Arc::new(Fake), None);
    let err = fallback::translate(&s.app, "One.", &[])
        .await
        .err()
        .unwrap();
    assert!(err.contains("ต่อ Gemini ไม่ได้"), "{err}");
    assert_eq!(fallback_state(&s.app), json!({ "active": false }));
}

#[tokio::test]
async fn without_a_backup_a_slow_gemini_is_waited_for() {
    let api = FakeApi::new(Mode::Slow(LIMIT + Duration::from_millis(500)));
    let s = setup_with(api, KEY, Arc::new(Fake), None);
    let line = fallback::translate(&s.app, "One.", &[]).await.unwrap();
    assert_eq!((line.engine, line.text.as_str()), ("gemini", "ช้าไป"));
}

// ---- pipeline: saved and pushed lines carry it (lines.is_fallback) ----

#[tokio::test]
async fn pipeline_lines_from_apple_are_marked_fallback() {
    let frames = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames");
    let api = FakeApi::new(Mode::Down);
    let s = setup_with(api, KEY, Arc::new(StandIn), Some(frames));
    let mut pushes = s.app.events.subscribe();
    let (status, body) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let line = loop {
        let push = tokio::time::timeout(Duration::from_secs(60), pushes.recv())
            .await
            .expect("pipeline too slow")
            .unwrap();
        if let Push::Line(l) = push {
            break l;
        }
    };
    assert_eq!(line.engine, "apple");
    assert!(line.is_fallback);
    assert_eq!(line.target_text, "(สำรอง) Keep your lantern low.");
    assert!(s.app.state.borrow().fallback.active);

    let (_, history) = call(&s.router, Method::GET, "/api/lines", None).await;
    let saved = history
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == line.id.as_str())
        .unwrap()
        .clone();
    assert_eq!(saved["is_fallback"], true);
    assert_eq!(saved["engine"], "apple");
    s.app.stop();
}

// ---- real Gemini (ignored) ----

/// `GEMINI_API_KEY` from the environment or the first `.env` above the crate.
fn real_key() -> Option<String> {
    if let Ok(k) = std::env::var("GEMINI_API_KEY") {
        return Some(k);
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.ancestors().find_map(|d| {
        let text = std::fs::read_to_string(d.join(".env")).ok()?;
        text.lines().find_map(|l| {
            let v = l.trim().strip_prefix("GEMINI_API_KEY=")?;
            Some(v.trim().trim_matches(['"', '\'']).to_owned())
        })
    })
}

/// Run with `cargo test --test fallback -- --ignored --nocapture`. Three
/// calls to the real API: the live model should answer inside the limit, so
/// no line falls back. Then a wrong key, which must fall back with its reason.
#[tokio::test]
#[ignore = "calls the real Gemini API with GEMINI_API_KEY"]
async fn real_gemini_stays_inside_the_limit_and_a_bad_key_falls_back() {
    let key = real_key().expect("GEMINI_API_KEY not set and no .env found");
    let http: Arc<dyn Api> = Arc::new(Http::new().unwrap());
    let s = setup_with(http.clone(), &key, Arc::new(StandIn), None);
    for text in ["The bridge is out.", "Then we swim."] {
        let started = Instant::now();
        let line = fallback::translate(&s.app, text, &[]).await.unwrap();
        println!(
            "{} in {} ms → {}",
            line.engine,
            started.elapsed().as_millis(),
            line.text
        );
        assert_eq!(
            line.engine,
            "gemini",
            "fell back: {:?}",
            s.app.state.borrow().fallback
        );
        assert!(!line.text.contains(&key));
    }

    let wrong = setup_with(http, KEY, Arc::new(StandIn), None);
    let line = fallback::translate(&wrong.app, "Run.", &[]).await.unwrap();
    let reason = wrong.app.state.borrow().fallback.reason.clone().unwrap();
    println!("wrong key → {} · {reason}", line.engine);
    assert_eq!(line.engine, "apple");
    assert!(reason.contains("API key"), "{reason}");
    assert!(!reason.contains(&key));
}
