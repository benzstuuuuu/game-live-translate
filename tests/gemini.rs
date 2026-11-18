//! Gemini translator and the translation section of /setup (#36): settings API,
//! key storage and test, context lines through the pipeline. The HTTP API and
//! the Keychain are stand-ins; the real API is in the ignored test at the end.

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
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config, Push};
use yokoten::core::gemini::Settings;
use yokoten::secret::{KeyStore, Memory};
use yokoten::server::router_with;
use yokoten::translate::gemini::{Api, Http};
use yokoten::translate::{Fake, Gemini, Translator};
use yokoten::{db, translation_api};

const KEY: &str = "AIzaTestKey-0123456789";
const LAN: &str = "192.168.1.20:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

/// Answers every call with one canned reply and records what was sent.
struct FakeApi {
    status: u16,
    reply: Value,
    sent: Mutex<Vec<Sent>>,
}

#[derive(Clone)]
struct Sent {
    key: String,
    model: String,
    body: Value,
}

impl FakeApi {
    fn ok(thai: &str) -> Arc<Self> {
        let reply = json!({ "candidates": [{
            "content": { "parts": [{ "text": thai }] }, "finishReason": "STOP" }] });
        Self::new(200, reply)
    }

    fn new(status: u16, reply: Value) -> Arc<Self> {
        Arc::new(Self {
            status,
            reply,
            sent: Mutex::new(Vec::new()),
        })
    }

    fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }
}

impl Api for FakeApi {
    fn generate<'a>(
        &'a self,
        key: &'a str,
        model: &'a str,
        body: Value,
    ) -> BoxFuture<'a, Result<(u16, Vec<u8>), String>> {
        self.sent.lock().unwrap().push(Sent {
            key: key.into(),
            model: model.into(),
            body,
        });
        let reply = self.reply.to_string().into_bytes();
        Box::pin(async move { Ok((self.status, reply)) })
    }
}

struct Setup {
    app: Arc<App>,
    router: Router,
    store: Arc<Memory>,
    _shutdown: watch::Sender<bool>,
}

fn setup(
    api: Arc<FakeApi>,
    store: Memory,
    file: Option<PathBuf>,
    frames: Option<PathBuf>,
) -> Setup {
    let store = Arc::new(store);
    let gemini = Gemini::new(Settings::default(), store.clone(), api, file);
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: frames,
        obs: None,
        translators: vec![Arc::new(gemini.clone()), Arc::new(Fake)],
        frame_interval: Duration::from_millis(5),
    };
    let app = App::new(cfg, shutdown).unwrap();
    let router = router_with(app.clone(), translation_api::routes(app.clone(), gemini));
    Setup {
        app,
        router,
        store,
        _shutdown: shutdown_tx,
    }
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    peer: &str,
) -> (StatusCode, String) {
    let peer: SocketAddr = peer.parse().unwrap();
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
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_default()
}

fn temp_file() -> PathBuf {
    std::env::temp_dir().join(format!("yokoten-gemini-{}.json", uuid::Uuid::new_v4()))
}

// ---- settings API (FR-SET-03, NFR-07) ----

#[tokio::test]
async fn settings_api_reports_a_saved_key_but_never_the_key() {
    let s = setup(FakeApi::ok("สวัสดี"), Memory::with(KEY), None, None);
    let (status, body) = call(&s.router, Method::GET, "/api/settings", None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(KEY), "key leaked: {body}");
    let v = parse(&body);
    assert_eq!(v["gemini"]["key_saved"], true);
    assert_eq!(v["context_lines"], 3);
    assert_eq!(v["tone"], "casual");
    assert_eq!(v["gemini"]["live_model"], "gemini-3.8-flash");

    let change = json!({ "tone": "formal" });
    let (status, body) = call(
        &s.router,
        Method::PUT,
        "/api/settings",
        Some(change),
        LOOPBACK,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(KEY), "key leaked: {body}");
}

#[tokio::test]
async fn settings_without_a_key_say_so() {
    let s = setup(FakeApi::ok("x"), Memory::default(), None, None);
    let (_, body) = call(&s.router, Method::GET, "/api/settings", None, LOOPBACK).await;
    assert_eq!(parse(&body)["gemini"]["key_saved"], false);
}

#[tokio::test]
async fn settings_and_key_are_loopback_only() {
    let s = setup(FakeApi::ok("x"), Memory::with(KEY), None, None);
    let key = Some(json!({ "key": "AIzaOther" }));
    for (method, path, body) in [
        (Method::GET, "/api/settings", None),
        (
            Method::PUT,
            "/api/settings",
            Some(json!({ "tone": "formal" })),
        ),
        (Method::PUT, "/api/secret/gemini-key", key.clone()),
        (Method::POST, "/api/secret/gemini-key/test", None),
    ] {
        let (status, _) = call(&s.router, method, path, body, LAN).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
    assert_eq!(s.store.get().unwrap().as_deref(), Some(KEY));
}

#[tokio::test]
async fn key_save_from_another_site_is_refused() {
    let s = setup(FakeApi::ok("x"), Memory::default(), None, None);
    let req = Request::put("/api/secret/gemini-key")
        .header(header::ORIGIN, "http://evil.example")
        .header(header::HOST, "127.0.0.1:8790")
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo::<SocketAddr>(LOOPBACK.parse().unwrap()))
        .body(Body::from(r#"{"key":"AIzaEvil"}"#))
        .unwrap();
    let res = s.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(s.store.get().unwrap(), None);
}

#[tokio::test]
async fn settings_change_is_saved_to_the_file() {
    let path = temp_file();
    std::fs::write(&path, r#"{"translator": "apple"}"#).unwrap();
    let s = setup(
        FakeApi::ok("x"),
        Memory::default(),
        Some(path.clone()),
        None,
    );
    let change = json!({ "context_lines": 5, "extra_prompt": "เรียกตัวเอกว่า เจ้า",
        "gemini": { "live_model": "gemini-4-hydrogen" } });
    let (status, body) = call(
        &s.router,
        Method::PUT,
        "/api/settings",
        Some(change),
        LOOPBACK,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(file["translator"], "apple", "other keys stay");
    assert_eq!(file["context_lines"], 5);
    assert_eq!(file["gemini"]["live_model"], "gemini-4-hydrogen");
    let reloaded = Settings::from_stored(&file);
    assert_eq!(reloaded.extra_prompt, "เรียกตัวเอกว่า เจ้า");
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn bad_settings_are_refused_with_a_reason() {
    let s = setup(FakeApi::ok("x"), Memory::default(), None, None);
    for change in [
        json!({ "context_lines": 50 }),
        json!({ "tone": "shouty" }),
        json!({ "gemini": { "models": [{ "id": "gemini-3.8-flash", "thinking": "xhigh" }] } }),
    ] {
        let (status, body) = call(
            &s.router,
            Method::PUT,
            "/api/settings",
            Some(change),
            LOOPBACK,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(parse(&body)["error"].is_string(), "{body}");
    }
    let (_, body) = call(&s.router, Method::GET, "/api/settings", None, LOOPBACK).await;
    assert_eq!(parse(&body)["context_lines"], 3, "nothing changed");
}

// ---- key (FR-SET-07) ----

#[tokio::test]
async fn saving_a_key_makes_gemini_ready() {
    let s = setup(FakeApi::ok("x"), Memory::default(), None, None);
    let (status, body) = call(&s.router, Method::POST, "/api/start", None, LOOPBACK).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body.contains("Gemini API key"), "{body}");
    assert_eq!(
        s.app.state.borrow().error.as_ref().unwrap().message,
        "ยังไม่ได้ใส่ Gemini API key"
    );

    let save = Some(json!({ "key": format!("  {KEY}\n") }));
    let (status, body) = call(
        &s.router,
        Method::PUT,
        "/api/secret/gemini-key",
        save,
        LOOPBACK,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(KEY), "key leaked: {body}");
    assert_eq!(
        s.store.get().unwrap().as_deref(),
        Some(KEY),
        "saved trimmed"
    );
    assert!(s.app.state.borrow().error.is_none(), "unavailable cleared");
}

#[tokio::test]
async fn empty_or_odd_keys_are_refused() {
    let s = setup(FakeApi::ok("x"), Memory::default(), None, None);
    for key in ["", "   ", "has space"] {
        let body = Some(json!({ "key": key }));
        let (status, _) = call(
            &s.router,
            Method::PUT,
            "/api/secret/gemini-key",
            body,
            LOOPBACK,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{key:?}");
    }
    assert_eq!(s.store.get().unwrap(), None);
}

#[tokio::test]
async fn key_test_says_it_works() {
    let api = FakeApi::ok("เราจะไปไหนกัน");
    let s = setup(api.clone(), Memory::with(KEY), None, None);
    let (status, body) = call(
        &s.router,
        Method::POST,
        "/api/secret/gemini-key/test",
        None,
        LOOPBACK,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v = parse(&body);
    assert_eq!(v["ok"], true);
    assert_eq!(v["model"], "gemini-3.8-flash");
    assert_eq!(v["message"], "เราจะไปไหนกัน");
    assert_eq!(api.sent()[0].key, KEY, "saved key in the header");
}

#[tokio::test]
async fn key_test_uses_the_typed_key_first() {
    let api = FakeApi::ok("ok");
    let s = setup(api.clone(), Memory::with(KEY), None, None);
    let typed = Some(json!({ "key": "AIzaTyped" }));
    call(
        &s.router,
        Method::POST,
        "/api/secret/gemini-key/test",
        typed,
        LOOPBACK,
    )
    .await;
    assert_eq!(api.sent()[0].key, "AIzaTyped");
    assert_eq!(
        s.store.get().unwrap().as_deref(),
        Some(KEY),
        "testing does not save"
    );
}

#[tokio::test]
async fn key_test_says_why_it_fails_without_echoing_the_key() {
    let reply = json!({ "error": { "code": 400, "status": "INVALID_ARGUMENT",
        "message": format!("API key not valid: {KEY}"),
        "details": [{ "reason": "API_KEY_INVALID" }] } });
    let s = setup(FakeApi::new(400, reply), Memory::with(KEY), None, None);
    let (status, body) = call(
        &s.router,
        Method::POST,
        "/api/secret/gemini-key/test",
        None,
        LOOPBACK,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(KEY), "key leaked: {body}");
    let v = parse(&body);
    assert_eq!(v["ok"], false);
    assert!(
        v["message"].as_str().unwrap().contains("API key ไม่ถูกต้อง"),
        "{v}"
    );
}

#[tokio::test]
async fn key_test_without_any_key_is_refused() {
    let s = setup(FakeApi::ok("x"), Memory::default(), None, None);
    let (status, body) = call(
        &s.router,
        Method::POST,
        "/api/secret/gemini-key/test",
        None,
        LOOPBACK,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(parse(&body)["error"].is_string());
}

#[tokio::test]
async fn translate_error_never_carries_the_key() {
    let reply = json!({ "error": { "code": 400, "message": format!("bad request for {KEY}") } });
    let gemini = Gemini::new(
        Settings::default(),
        Arc::new(Memory::with(KEY)),
        FakeApi::new(400, reply),
        None,
    );
    let err = gemini.translate("Hello.").await.unwrap_err();
    assert!(!err.contains(KEY), "{err}");
    assert!(err.contains("[key]"), "{err}");
}

// ---- pipeline (FR-TRN-03) ----

#[tokio::test]
async fn gemini_gets_earlier_lines_and_records_the_model() {
    let frames = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames");
    let api = FakeApi::ok("คำแปล");
    let s = setup(api.clone(), Memory::with(KEY), None, Some(frames));
    let mut events = s.app.events.subscribe();
    let (status, body) = call(&s.router, Method::POST, "/api/start", None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // Same frames as tests/pipeline.rs: two lines, gone, a third, its update.
    let mut first = None;
    for _ in 0..5 {
        let push = tokio::time::timeout(Duration::from_secs(60), events.recv())
            .await
            .expect("pipeline too slow")
            .unwrap();
        if let (None, Push::Line(l)) = (&first, push) {
            first = Some(l);
        }
    }
    s.app.stop();
    let line = first.unwrap();
    assert_eq!(line.engine, "gemini");
    assert_eq!(line.model_id.as_deref(), Some("gemini-3.8-flash"));
    assert_eq!(line.target_text, "คำแปล");

    let prompts: Vec<String> = api
        .sent()
        .iter()
        .inspect(|s| {
            assert_eq!(
                (s.key.as_str(), s.model.as_str()),
                (KEY, "gemini-3.8-flash")
            )
        })
        .map(|s| {
            s.body["contents"][0]["parts"][0]["text"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(prompts.len(), 4, "{prompts:#?}");
    assert!(!prompts[0].contains("Earlier lines"), "{}", prompts[0]);
    assert!(
        prompts[1].contains("- Keep your lantern low.\n"),
        "{}",
        prompts[1]
    );
    // The update of line three sees lines one and two, not its own first read.
    let update = &prompts[3];
    assert!(
        update.ends_with("No. But it's the only road left."),
        "{update}"
    );
    assert!(
        update.contains("- You sure about this, Mara?\n"),
        "{update}"
    );
    assert!(!update.contains("- No. But it's the\n"), "{update}");
}

// ---- real API (ignored; a handful of calls) ----

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

fn has_thai(s: &str) -> bool {
    s.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c))
}

/// Run with `cargo test --test gemini -- --ignored --nocapture`. Makes four
/// calls: key test, one line on each of two models, and a wrong key. Prints
/// Thai output and timings only.
#[tokio::test]
#[ignore = "calls the real Gemini API with GEMINI_API_KEY"]
async fn real_gemini_translates_and_names_a_bad_key() {
    let key = real_key().expect("GEMINI_API_KEY not set and no .env found");
    let http: Arc<dyn Api> = Arc::new(Http::new().unwrap());
    let gemini = Gemini::new(
        Settings::default(),
        Arc::new(Memory::with(&key)),
        http.clone(),
        None,
    );

    let test = gemini.test_key(None).await.unwrap();
    println!(
        "key test: ok={} model={} {} ms → {}",
        test.ok, test.model, test.ms, test.message
    );
    assert!(test.ok, "{}", test.message);
    assert!(has_thai(&test.message));

    let earlier = vec!["The bridge is out.".to_owned(), "Then we swim.".to_owned()];
    for model in ["gemini-3.8-flash", "gemini-4-hydrogen"] {
        let change = serde_json::from_value(json!({ "gemini": { "live_model": model } })).unwrap();
        gemini.update(change).unwrap();
        let started = std::time::Instant::now();
        let thai = gemini
            .translate_in_context("You can't be serious, Mara.", &earlier)
            .await
            .unwrap();
        println!("{model}: {} ms → {thai}", started.elapsed().as_millis());
        assert!(has_thai(&thai));
        assert!(!thai.contains(&key));
    }

    let wrong = "AIzaSyD-not-a-real-key-000000000000000";
    let bad = Gemini::new(
        Settings::default(),
        Arc::new(Memory::with(wrong)),
        http,
        None,
    );
    let test = bad.test_key(None).await.unwrap();
    println!("wrong key: ok={} → {}", test.ok, test.message);
    assert!(!test.ok);
    assert!(test.message.contains("API key"), "{}", test.message);
}

/// The real login Keychain under a throwaway service, deleted afterwards.
/// Run with `cargo test --test gemini -- --ignored keychain`.
#[test]
#[ignore = "writes to the login Keychain (throwaway service, removed after)"]
fn real_keychain_round_trip() {
    let service = format!("yokoten-test-{}", uuid::Uuid::new_v4());
    let chain = yokoten::secret::Keychain::new(&service, "gemini-api-key");
    assert_eq!(chain.get().unwrap(), None);
    chain.set("AIzaThrowaway-1").unwrap();
    chain.set("AIzaThrowaway-2").unwrap();
    let read = chain.get();
    chain.delete().unwrap();
    assert_eq!(read.unwrap().as_deref(), Some("AIzaThrowaway-2"));
    assert_eq!(chain.get().unwrap(), None);
    chain.delete().unwrap();
}
