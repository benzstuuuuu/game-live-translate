//! Claude API translator (#79, #97, FR-TRN-14): the Messages API call, its
//! fallback to Apple, the key section of /setup and the model and effort
//! picked there. A fake Anthropic server
//! on a random local port answers in place of api.anthropic.com; a stand-in
//! named "apple" plays Apple Translation, so this runs without the
//! `apple-translation` feature. The real API is in the ignored test at the end.

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, Method, Request, StatusCode, Uri};
use axum::routing::post;
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
use yokoten::app::{App, Config};
use yokoten::core::gemini::{Job, Settings};
use yokoten::secret::{KeyStore, Memory};
use yokoten::server::router_with;
use yokoten::translate::claude::Http;
use yokoten::translate::{Claude, Translator};
use yokoten::{db, fallback, translation_api};

const KEY: &str = "sk-ant-api03-TestKey-0123456789";
const LAN: &str = "192.168.1.20:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

// ---- the fake Anthropic API ----

/// One request as the fake server saw it.
#[derive(Clone)]
struct Seen {
    uri: Uri,
    headers: HeaderMap,
    body: Value,
}

/// Answers every POST /v1/messages with `reply` and records the request.
#[derive(Default)]
struct Fake {
    reply: Mutex<(u16, Value)>,
    seen: Mutex<Vec<Seen>>,
}

impl Fake {
    fn answer(&self, status: u16, body: Value) {
        *self.reply.lock().unwrap() = (status, body);
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

fn message(thai: &str) -> Value {
    json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "claude-haiku-5-5",
        "content": [
            { "type": "thinking", "thinking": "", "signature": "sig" },
            { "type": "text", "text": thai }
        ],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 210, "output_tokens": 37 }
    })
}

fn error(kind: &str, message: &str) -> Value {
    json!({ "type": "error", "error": { "type": kind, "message": message }, "request_id": "req_1" })
}

async fn messages(
    State(fake): State<Arc<Fake>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, String) {
    let body = serde_json::from_slice(&body).unwrap_or_default();
    fake.seen.lock().unwrap().push(Seen { uri, headers, body });
    let (status, reply) = fake.reply.lock().unwrap().clone();
    (StatusCode::from_u16(status).unwrap(), reply.to_string())
}

/// Starts the fake on 127.0.0.1 with a port the OS picks; its base URL.
async fn serve(fake: Arc<Fake>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/messages", post(messages))
        .with_state(fake);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

// ---- the app around it ----

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
    fake: Arc<Fake>,
    claude: Claude,
    app: Arc<App>,
    router: Router,
    store: Arc<Memory>,
    _shutdown: watch::Sender<bool>,
}

async fn setup(store: Memory, settings: Settings) -> Setup {
    setup_with_file(store, settings, None).await
}

/// `setup` with Claude's model and effort kept in `file` (settings.json).
async fn setup_with_file(store: Memory, settings: Settings, file: Option<PathBuf>) -> Setup {
    let fake = Arc::new(Fake::default());
    fake.answer(200, message("เราจะไปไหนกัน"));
    let base = serve(fake.clone()).await;
    let store = Arc::new(store);
    let claude = Claude::new(
        move || settings.clone(),
        store.clone(),
        Http::with_base(&base).unwrap(),
        file,
    );
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: None,
        obs: None,
        translators: vec![Arc::new(claude.clone()), Arc::new(StandIn)],
        frame_interval: Duration::from_millis(5),
        live: None,
    };
    let app = App::new(cfg, shutdown).unwrap();
    let router = router_with(
        app.clone(),
        translation_api::claude_routes(app.clone(), claude.clone()),
    );
    Setup {
        fake,
        claude,
        app,
        router,
        store,
        _shutdown: shutdown_tx,
    }
}

async fn with_key() -> Setup {
    setup(Memory::with(KEY), Settings::default()).await
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

fn header<'a>(seen: &'a Seen, name: &str) -> Option<&'a str> {
    seen.headers.get(name).and_then(|v| v.to_str().ok())
}

// ---- the call (FR-TRN-14, NFR-07) ----

#[tokio::test]
async fn translates_with_the_key_in_the_header_and_nowhere_else() {
    let s = setup(
        Memory::with(KEY),
        Settings::default()
            .apply(serde_json::from_value(json!({ "tone": "formal" })).unwrap())
            .unwrap(),
    )
    .await;
    let terms = [yokoten::core::glossary::Term::new("Warden", "วอร์เดน")];
    let earlier = vec!["Where is the gate?".to_owned()];
    let thai = s
        .claude
        .translate_with_glossary("The Warden waits.", &earlier, &terms)
        .await
        .unwrap();
    assert_eq!(thai, "เราจะไปไหนกัน");

    let seen = s.fake.seen();
    assert_eq!(seen.len(), 1);
    let sent = &seen[0];
    assert_eq!(sent.uri.path(), "/v1/messages");
    assert_eq!(sent.uri.query(), None);
    assert!(!sent.uri.to_string().contains(KEY));
    assert_eq!(header(sent, "x-api-key"), Some(KEY));
    assert_eq!(header(sent, "anthropic-version"), Some("2026-10-21"));
    assert_eq!(header(sent, "content-type"), Some("application/json"));
    assert_eq!(header(sent, "authorization"), None);
    for (name, value) in &sent.headers {
        if name != "x-api-key" {
            assert!(!value.to_str().unwrap_or_default().contains(KEY), "{name}");
        }
    }
    assert!(!sent.body.to_string().contains(KEY), "key in the body");

    assert_eq!(sent.body["model"], "claude-sonnet-5-5");
    assert_eq!(sent.body["output_config"]["effort"], "low");
    let system = sent.body["system"].as_str().unwrap();
    assert!(system.contains("formal written Thai"), "{system}");
    let user = sent.body["messages"][0]["content"].as_str().unwrap();
    assert!(user.contains("- Warden → วอร์เดน"), "{user}");
    assert!(user.contains("- Where is the gate?"), "{user}");
}

#[tokio::test]
async fn only_glossary_terms_in_the_line_are_sent() {
    let s = with_key().await;
    let terms = [
        yokoten::core::glossary::Term::new("Warden", "วอร์เดน"),
        yokoten::core::glossary::Term::new("Paimon", "ไพมอน"),
    ];
    s.claude
        .translate_with_glossary("Paimon, run!", &[], &terms)
        .await
        .unwrap();
    let user = s.fake.seen()[0].body["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(user.contains("- Paimon → ไพมอน"), "{user}");
    assert!(!user.contains("Warden"), "{user}");
}

#[tokio::test]
async fn tokens_from_usage_add_up_in_the_session() {
    let s = with_key().await;
    for line in ["One.", "Two."] {
        let t = fallback::translate(&s.app, line, &[]).await.unwrap();
        assert_eq!((t.engine, t.is_fallback), ("claude", false));
        assert_eq!(t.model_id.as_deref(), Some("claude-sonnet-5-5"));
    }
    // The pipeline counts after each line (FR-TRN-10).
    s.app.count_tokens();
    let tokens = s.app.state.borrow().tokens;
    assert_eq!((tokens.input, tokens.output), (420, 74));
}

#[tokio::test]
async fn an_error_never_carries_the_key() {
    let s = with_key().await;
    s.fake.answer(
        400,
        error("invalid_request_error", &format!("bad request for {KEY}")),
    );
    let err = s.claude.translate("Hello.").await.unwrap_err();
    assert!(!err.contains(KEY), "{err}");
    assert!(err.contains("[key]"), "{err}");
}

// ---- fallback to Apple and back (FR-TRN-04, 05) ----

async fn line_from_apple(s: &Setup, status: u16, reply: Value) -> String {
    s.fake.answer(status, reply);
    let line = fallback::translate(&s.app, "Run.", &[]).await.unwrap();
    assert_eq!((line.engine, line.is_fallback), ("apple", true), "{status}");
    assert_eq!(line.text, "(สำรอง) Run.");
    let reason = s.app.state.borrow().fallback.reason.clone().unwrap();
    assert!(!reason.contains(KEY), "{reason}");
    reason
}

#[tokio::test]
async fn a_bad_key_hands_the_line_to_apple_and_says_how_to_fix_it() {
    let s = with_key().await;
    let reason = line_from_apple(&s, 401, error("authentication_error", "invalid x-api-key")).await;
    assert!(reason.contains("API key ของ Claude ไม่ถูกต้อง"), "{reason}");
    assert!(reason.contains("/setup"), "{reason}");
}

#[tokio::test]
async fn quota_and_server_errors_hand_the_line_to_apple() {
    for (status, kind, says) in [
        (429, "rate_limit_error", "โควตา"),
        (500, "api_error", "ชั่วคราว"),
        (529, "overloaded_error", "ชั่วคราว"),
    ] {
        let s = with_key().await;
        let reason = line_from_apple(&s, status, error(kind, "m")).await;
        assert!(reason.contains(says), "{status}: {reason}");
    }
}

// ---- replies that are more than a translation (#98) ----

const EXPLAINED: [&str; 2] = [
    "วิ่ง\n\nThis line is a short order, so I kept it short.",
    "วิ่ง (the line \"Run\" is an order, so I kept it short)",
];

#[tokio::test]
async fn an_explained_reply_hands_the_line_to_apple() {
    for reply in EXPLAINED {
        let s = with_key().await;
        let reason = line_from_apple(&s, 200, message(reply)).await;
        assert!(reason.contains("Claude ตอบเกินคำแปล"), "{reason}");
    }
}

#[tokio::test]
async fn a_reply_keeping_names_from_the_line_passes() {
    let s = with_key().await;
    s.fake
        .answer(200, message("ปลดล็อก Jason & Lucia Activities Trophies แล้ว"));
    let line = fallback::translate(&s.app, "Jason & Lucia Activities Trophies unlocked.", &[])
        .await
        .unwrap();
    assert_eq!((line.engine, line.is_fallback), ("claude", false));
    assert_eq!(line.text, "ปลดล็อก Jason & Lucia Activities Trophies แล้ว");
}

#[tokio::test]
async fn an_explained_retranslation_keeps_the_old_text() {
    let s = with_key().await;
    let profile = db::last_profile(&s.app.db()).unwrap().unwrap().id;
    let line = db::Line {
        id: "line-1".into(),
        profile_id: profile,
        session_id: "s1".into(),
        source: "subtitle".into(),
        source_text: "Run.".into(),
        target_text: "วิ่งเร็ว".into(),
        engine: "apple".into(),
        is_fallback: true,
        model_id: None,
        done: true,
        created_at: 1,
        ocr_ms: Some(10),
        translate_ms: Some(1),
    };
    db::insert_line(&s.app.db(), &line).unwrap();
    s.fake.answer(200, message(EXPLAINED[0]));
    let path = "/api/lines/line-1/retranslate";
    let (status, body) = call(&s.router, Method::POST, path, None, LAN).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    let error = parse(&body)["error"].as_str().unwrap().to_owned();
    assert!(error.contains("แปลใหม่ไม่สำเร็จ"), "{error}");
    assert!(error.contains("Claude ตอบเกินคำแปล"), "{error}");
    let kept = db::find_line(&s.app.db(), "line-1").unwrap().unwrap();
    assert_eq!(kept.target_text, "วิ่งเร็ว");
    assert_eq!((kept.engine.as_str(), kept.is_fallback), ("apple", true));
}

#[tokio::test]
async fn claude_comes_back_when_a_line_works_again() {
    let s = with_key().await;
    line_from_apple(&s, 529, error("overloaded_error", "busy")).await;
    // Later lines skip Claude until the next try is due.
    fallback::translate(&s.app, "Again.", &[]).await.unwrap();
    assert_eq!(s.fake.seen().len(), 1);

    // "ลอง Claude อีกครั้ง": the next line tries it, and it works.
    s.fake.answer(200, message("กลับมาแล้ว"));
    assert!(s.app.set_translator("claude"));
    let line = fallback::translate(&s.app, "Back?", &[]).await.unwrap();
    assert_eq!((line.engine, line.is_fallback), ("claude", false));
    assert_eq!(line.text, "กลับมาแล้ว");
    assert!(!s.app.state.borrow().fallback.active);
}

#[tokio::test]
async fn without_a_key_claude_is_unavailable_and_lines_go_to_apple() {
    let s = setup(Memory::default(), Settings::default()).await;
    let why = s.claude.check().await.unwrap_err();
    assert!(why.message.contains("Claude API key"), "{}", why.message);
    assert!(why.fix.contains("/setup"), "{}", why.fix);

    let line = fallback::translate(&s.app, "Run.", &[]).await.unwrap();
    assert_eq!((line.engine, line.is_fallback), ("apple", true));
    assert!(s.fake.seen().is_empty(), "no call without a key");
}

// ---- both jobs (FR-TRN-02, 16) ----

#[tokio::test]
async fn claude_can_retranslate_while_another_engine_is_live() {
    let s = with_key().await;
    assert!(s.app.set_translator("apple"));
    assert!(s.app.set_retranslator(Some("claude")));
    assert_eq!(s.app.translator_for(Job::Live).engine(), "apple");
    let again = s.app.translator_for(Job::Retranslate);
    assert_eq!(again.engine(), "claude");
    assert_eq!(again.model_id().as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(again.translate("Hi.").await.unwrap(), "เราจะไปไหนกัน");
}

// ---- the key on /setup (FR-SET-07, NFR-07) ----

const KEY_PATH: &str = "/api/secret/anthropic-key";
const TEST_PATH: &str = "/api/secret/anthropic-key/test";

#[tokio::test]
async fn key_state_shows_whether_a_key_is_saved_but_never_the_key() {
    let s = with_key().await;
    let (status, body) = call(&s.router, Method::GET, KEY_PATH, None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(KEY), "key leaked: {body}");
    assert_eq!(
        parse(&body),
        json!({ "key_saved": true, "model": "claude-sonnet-5-5" })
    );
    let s = setup(Memory::default(), Settings::default()).await;
    let (_, body) = call(&s.router, Method::GET, KEY_PATH, None, LOOPBACK).await;
    assert_eq!(parse(&body)["key_saved"], false);
}

#[tokio::test]
async fn saving_a_key_makes_claude_ready() {
    let s = setup(Memory::default(), Settings::default()).await;
    assert!(s.app.check_translator().await.is_err());
    assert!(s.app.state.borrow().error.is_some());

    let save = Some(json!({ "key": format!("  {KEY}\n") }));
    let (status, body) = call(&s.router, Method::PUT, KEY_PATH, save, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(KEY), "key leaked: {body}");
    assert_eq!(
        s.store.get().unwrap().as_deref(),
        Some(KEY),
        "saved trimmed"
    );
    assert!(s.app.state.borrow().error.is_none(), "unavailable cleared");

    for bad in ["", "  ", "has space"] {
        let body = Some(json!({ "key": bad }));
        let (status, _) = call(&s.router, Method::PUT, KEY_PATH, body, LOOPBACK).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}");
    }
    assert_eq!(s.store.get().unwrap().as_deref(), Some(KEY));
}

#[tokio::test]
async fn key_test_translates_a_line_with_the_typed_key_first() {
    let s = with_key().await;
    let (status, body) = call(&s.router, Method::POST, TEST_PATH, None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK);
    let v = parse(&body);
    assert_eq!(v["ok"], true);
    assert_eq!(v["model"], "claude-sonnet-5-5");
    assert_eq!(v["message"], "เราจะไปไหนกัน");
    assert_eq!(header(&s.fake.seen()[0], "x-api-key"), Some(KEY));

    let typed = Some(json!({ "key": "sk-ant-typed" }));
    call(&s.router, Method::POST, TEST_PATH, typed, LOOPBACK).await;
    assert_eq!(header(&s.fake.seen()[1], "x-api-key"), Some("sk-ant-typed"));
    assert_eq!(
        s.store.get().unwrap().as_deref(),
        Some(KEY),
        "testing does not save"
    );
}

#[tokio::test]
async fn key_test_says_why_a_bad_key_fails_without_echoing_it() {
    let s = with_key().await;
    s.fake.answer(
        401,
        error("authentication_error", &format!("invalid x-api-key {KEY}")),
    );
    let (status, body) = call(&s.router, Method::POST, TEST_PATH, None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains(KEY), "key leaked: {body}");
    let v = parse(&body);
    assert_eq!(v["ok"], false);
    assert!(v["message"].as_str().unwrap().contains("ไม่ถูกต้อง"), "{v}");
}

#[tokio::test]
async fn key_test_without_any_key_is_refused() {
    let s = setup(Memory::default(), Settings::default()).await;
    let (status, body) = call(&s.router, Method::POST, TEST_PATH, None, LOOPBACK).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(parse(&body)["error"].is_string());
    assert!(s.fake.seen().is_empty());
}

#[tokio::test]
async fn key_routes_are_loopback_only() {
    let s = with_key().await;
    let key = Some(json!({ "key": "sk-ant-other" }));
    for (method, path, body) in [
        (Method::GET, KEY_PATH, None),
        (Method::PUT, KEY_PATH, key),
        (Method::POST, TEST_PATH, None),
    ] {
        let (status, _) = call(&s.router, method, path, body, LAN).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
    assert_eq!(s.store.get().unwrap().as_deref(), Some(KEY));
    assert!(s.fake.seen().is_empty());
}

// ---- model and effort on /setup (#97) ----

const PICK_PATH: &str = "/api/claude";

fn temp_file() -> PathBuf {
    std::env::temp_dir().join(format!("yokoten-claude-{}.json", uuid::Uuid::new_v4()))
}

#[tokio::test]
async fn the_pick_shows_sonnet_low_and_the_choices() {
    let s = with_key().await;
    let (status, body) = call(&s.router, Method::GET, PICK_PATH, None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains(KEY), "key leaked: {body}");
    assert_eq!(
        parse(&body),
        json!({
            "model": "claude-sonnet-5-5",
            "effort": "low",
            "models": ["claude-sonnet-5-5", "claude-haiku-5-5"],
            "efforts": ["no-think", "low", "medium", "high", "xhigh", "max"],
            "key_saved": true,
        })
    );
}

#[tokio::test]
async fn a_new_pick_reaches_the_next_line_and_its_model_id() {
    let s = with_key().await;
    let line = fallback::translate(&s.app, "One.", &[]).await.unwrap();
    assert_eq!(line.model_id.as_deref(), Some("claude-sonnet-5-5"));

    let pick = Some(json!({ "model": "claude-haiku-5-5", "effort": "no-think" }));
    let (status, body) = call(&s.router, Method::PUT, PICK_PATH, pick, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(parse(&body)["model"], "claude-haiku-5-5");
    assert_eq!(parse(&body)["effort"], "no-think");

    // No stop needed: the next line uses it.
    let line = fallback::translate(&s.app, "Two.", &[]).await.unwrap();
    assert_eq!((line.engine, line.is_fallback), ("claude", false));
    assert_eq!(line.model_id.as_deref(), Some("claude-haiku-5-5"));
    let sent = s.fake.seen();
    let last = &sent.last().unwrap().body;
    assert_eq!(last["model"], "claude-haiku-5-5");
    assert_eq!(last["output_config"], json!({ "effort": "low" }));
    assert_eq!(last["thinking"], json!({ "type": "disabled" }));
    assert_eq!(last["max_tokens"], 1024);

    // Only the effort sent changes.
    let effort = Some(json!({ "effort": "xhigh" }));
    call(&s.router, Method::PUT, PICK_PATH, effort, LOOPBACK).await;
    fallback::translate(&s.app, "Three.", &[]).await.unwrap();
    let last = s.fake.seen().last().unwrap().body.clone();
    assert_eq!(last["model"], "claude-haiku-5-5");
    assert_eq!(last["output_config"], json!({ "effort": "xhigh" }));
    assert_eq!(last["max_tokens"], 4096);
    assert!(last.get("thinking").is_none(), "{last}");
}

#[tokio::test]
async fn sonnet_no_think_sends_between_tools() {
    let s = with_key().await;
    s.claude.set_pick(None, Some("no-think")).unwrap();
    s.claude.translate("Hi.").await.unwrap();
    let last = s.fake.seen().last().unwrap().body.clone();
    assert_eq!(last["model"], "claude-sonnet-5-5");
    assert_eq!(last["thinking"], json!({ "type": "between_tools" }));
}

#[tokio::test]
async fn a_pick_is_saved_to_the_file_and_read_back() {
    let file = temp_file();
    std::fs::write(&file, r#"{ "translator": "claude" }"#).unwrap();
    let s = setup_with_file(Memory::with(KEY), Settings::default(), Some(file.clone())).await;
    let pick = Some(json!({ "model": "claude-haiku-5-5", "effort": "medium" }));
    let (status, body) = call(&s.router, Method::PUT, PICK_PATH, pick, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(
        saved["claude"],
        json!({ "model": "claude-haiku-5-5", "effort": "medium" })
    );
    assert_eq!(saved["translator"], "claude", "other keys kept");

    let again = setup_with_file(Memory::with(KEY), Settings::default(), Some(file.clone())).await;
    assert_eq!(again.claude.pick().model, "claude-haiku-5-5");
    assert_eq!(again.claude.pick().effort, "medium");

    // A bad value in the file reads as the default.
    std::fs::write(
        &file,
        r#"{ "claude": { "model": "gpt", "effort": "turbo" } }"#,
    )
    .unwrap();
    let bad = setup_with_file(Memory::with(KEY), Settings::default(), Some(file.clone())).await;
    assert_eq!(bad.claude.pick().model, "claude-sonnet-5-5");
    assert_eq!(bad.claude.pick().effort, "low");
    let _ = std::fs::remove_file(file);
}

#[tokio::test]
async fn an_unknown_pick_is_refused_and_changes_nothing() {
    let s = with_key().await;
    for bad in [
        json!({ "model": "claude-opus-5-5" }),
        json!({ "effort": "turbo" }),
        json!({ "model": "claude-haiku-5-5", "effort": "" }),
    ] {
        let (status, body) = call(&s.router, Method::PUT, PICK_PATH, Some(bad), LOOPBACK).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(parse(&body)["error"].is_string(), "{body}");
    }
    assert_eq!(s.claude.pick().model, "claude-sonnet-5-5");
    assert_eq!(s.claude.pick().effort, "low");
}

#[tokio::test]
async fn the_pick_is_loopback_only() {
    let s = with_key().await;
    let pick = Some(json!({ "model": "claude-haiku-5-5" }));
    for (method, body) in [(Method::GET, None), (Method::PUT, pick)] {
        let (status, _) = call(&s.router, method, PICK_PATH, body, LAN).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert_eq!(s.claude.pick().model, "claude-sonnet-5-5");
}

// ---- real API (ignored; three calls) ----

/// `ANTHROPIC_API_KEY` from the environment, else the key saved in /setup.
fn real_key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .or_else(|| yokoten::secret::Keychain::anthropic().get().ok().flatten())
}

fn has_thai(s: &str) -> bool {
    s.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c))
}

/// A Claude with the real API and `key`, set to `model` and `effort`.
fn real_claude(key: &str, model: &str, effort: &str) -> Claude {
    let claude = Claude::new(
        Settings::default,
        Arc::new(Memory::with(key)),
        Http::new().unwrap(),
        None,
    );
    claude.set_pick(Some(model), Some(effort)).unwrap();
    claude
}

/// Run with `cargo test --test claude -- --ignored --nocapture`. Makes two
/// calls: the key test and one line in context, with Sonnet `low`. Prints
/// Thai, timings and tokens only; skips when no key is set.
#[tokio::test]
#[ignore = "calls the real Claude API with ANTHROPIC_API_KEY or the saved key"]
async fn real_sonnet_low_translates_a_line() {
    let Some(key) = real_key() else {
        println!("skipped: no ANTHROPIC_API_KEY and no key saved in /setup");
        return;
    };
    let claude = real_claude(&key, "claude-sonnet-5-5", "low");
    let test = claude.test_key(None).await.unwrap();
    println!(
        "key test: ok={} model={} {} ms → {}",
        test.ok, test.model, test.ms, test.message
    );
    assert!(test.ok, "{}", test.message);
    real_line(&claude, &key).await;
}

/// One line with Haiku `no-think` (thinking `disabled`). Same run as above.
#[tokio::test]
#[ignore = "calls the real Claude API with ANTHROPIC_API_KEY or the saved key"]
async fn real_haiku_no_think_translates_a_line() {
    let Some(key) = real_key() else {
        println!("skipped: no ANTHROPIC_API_KEY and no key saved in /setup");
        return;
    };
    let claude = real_claude(&key, "claude-haiku-5-5", "no-think");
    real_line(&claude, &key).await;
}

/// Translates one line in context and checks it is Thai, without the key.
async fn real_line(claude: &Claude, key: &str) {
    let earlier = vec!["The bridge is out.".to_owned(), "Then we swim.".to_owned()];
    let started = std::time::Instant::now();
    let thai = claude
        .translate_in_context("You can't be serious, Mara.", &earlier)
        .await
        .unwrap();
    let used = claude.usage();
    println!(
        "{}: {} ms, {} in / {} out → {thai}",
        claude.pick().model,
        started.elapsed().as_millis(),
        used.input,
        used.output
    );
    assert!(has_thai(&thai));
    assert!(!thai.contains(key));
    assert!(used.input > 0 && used.output > 0);
}
