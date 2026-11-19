//! Line actions on /reader (#45, FR-TRN-08, FR-GLS-02): a phone on the LAN
//! retranslates a line with the engine picked for that job on /setup
//! (FR-TRN-16, #78), which replaces the line in SQLite and reaches every page
//! as `line-updated`; a term it adds to the glossary is used from then on.
//! In-memory SQLite, fake translators.

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
use yokoten::db::{self, Line};
use yokoten::server::router;
use yokoten::translate::{Fake, Translator, Unavailable};
use yokoten::{fallback, settings};

/// A phone on the LAN: /reader's calls must work from there.
const LAN: &str = "192.168.1.20:50000";
/// /setup on the Mac.
const LOOPBACK: &str = "127.0.0.1:50000";

/// A second engine: marks the text, keeps the context it was given, counts
/// its calls, and fails or is not ready when told to.
#[derive(Default)]
struct Other {
    earlier: Mutex<Vec<String>>,
    calls: Mutex<usize>,
    fail: Mutex<bool>,
    unready: Mutex<bool>,
}

impl Translator for Other {
    fn engine(&self) -> &'static str {
        "gemini"
    }

    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>> {
        self.translate_in_context(text, &[])
    }

    fn translate_in_context<'a>(
        &'a self,
        text: &'a str,
        earlier: &'a [String],
    ) -> BoxFuture<'a, Result<String, String>> {
        *self.earlier.lock().unwrap() = earlier.to_vec();
        *self.calls.lock().unwrap() += 1;
        let fail = *self.fail.lock().unwrap();
        Box::pin(async move {
            if fail {
                return Err("ต่อ Gemini ไม่ได้".into());
            }
            Ok(format!("(อีกตัว) {text}"))
        })
    }

    fn check(&self) -> BoxFuture<'_, Result<(), Unavailable>> {
        let unready = *self.unready.lock().unwrap();
        Box::pin(async move {
            if unready {
                return Err(Unavailable {
                    message: "ยังไม่ได้ใส่ Gemini API key".into(),
                    fix: "ใส่ API key ใน /setup".into(),
                });
            }
            Ok(())
        })
    }

    fn model_id(&self) -> Option<String> {
        Some("model-x".into())
    }
}

struct Setup {
    app: Arc<App>,
    router: Router,
    other: Arc<Other>,
    profile: String,
    _shutdown: watch::Sender<bool>,
}

/// Live translates with Fake (the first engine) unless switched.
fn setup() -> Setup {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let profile = db::last_profile(&conn).unwrap().unwrap().id;
    let (shutdown_tx, shutdown) = watch::channel(false);
    let other = Arc::new(Other::default());
    let cfg = Config {
        conn,
        frames_dir: None,
        obs: None,
        translators: vec![Arc::new(Fake), other.clone()],
        frame_interval: Duration::from_millis(5),
        live: None,
    };
    let app = App::new(cfg, shutdown).unwrap();
    Setup {
        router: router(app.clone()),
        app,
        other,
        profile,
        _shutdown: shutdown_tx,
    }
}

/// A line the backup translated, stored as the pipeline stores it.
fn stored(s: &Setup, text: &str, at: i64) -> Line {
    let line = Line {
        id: uuid::Uuid::new_v4().to_string(),
        profile_id: s.profile.clone(),
        session_id: "s1".into(),
        source: "subtitle".into(),
        source_text: text.into(),
        target_text: format!("(แปล) {text}"),
        engine: "fake".into(),
        is_fallback: true,
        model_id: None,
        done: true,
        created_at: at,
        ocr_ms: Some(10),
        translate_ms: Some(1),
    };
    db::insert_line(&s.app.db(), &line).unwrap();
    line
}

async fn call(
    s: &Setup,
    method: Method,
    path: &str,
    body: Option<Value>,
    from: &str,
) -> (StatusCode, Value) {
    let peer: SocketAddr = from.parse().unwrap();
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
    let res = s.router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// "แปลใหม่" on /reader, from a phone.
async fn retranslate(s: &Setup, id: &str) -> (StatusCode, Value) {
    let path = format!("/api/lines/{id}/retranslate");
    call(s, Method::POST, &path, None, LAN).await
}

/// The retranslate pick on /setup; null = the live engine.
async fn pick(s: &Setup, engine: Value) -> (StatusCode, Value) {
    let body = json!({ "translator": engine });
    call(s, Method::PUT, "/api/retranslator", Some(body), LOOPBACK).await
}

fn row(s: &Setup, id: &str) -> Line {
    db::session_lines(&s.app.db(), "s1", None, 50)
        .unwrap()
        .into_iter()
        .find(|l| l.id == id)
        .unwrap()
}

fn temp_file() -> PathBuf {
    std::env::temp_dir().join(format!("yokoten-retranslate-{}.json", uuid::Uuid::new_v4()))
}

#[tokio::test]
async fn retranslate_from_lan_replaces_the_line_and_sends_line_updated() {
    let s = setup();
    stored(&s, "Where is the key?", 1);
    let line = stored(&s, "Keep your lantern low.", 2);
    let (status, v) = pick(&s, json!("gemini")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let mut events = s.app.events.subscribe();

    let (status, v) = retranslate(&s, &line.id).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["id"], line.id.as_str());
    assert_eq!(v["target_text"], "(อีกตัว) Keep your lantern low.");
    assert_eq!(v["engine"], "gemini");
    assert_eq!(v["is_fallback"], false);
    assert_eq!(v["model_id"], "model-x");

    let Ok(Push::LineUpdated(pushed)) = events.try_recv() else {
        panic!("no line-updated");
    };
    assert_eq!(pushed, row(&s, &line.id));
    assert_eq!(pushed.target_text, "(อีกตัว) Keep your lantern low.");
    assert_eq!(pushed.source_text, "Keep your lantern low.");
    assert_eq!(pushed.created_at, 2);
    // Engines that use context get the session's lines before this one.
    assert_eq!(
        *s.other.earlier.lock().unwrap(),
        vec!["Where is the key?".to_string()]
    );
}

#[tokio::test]
async fn live_uses_one_engine_and_retranslate_the_other() {
    let s = setup();
    let (status, v) = pick(&s, json!("gemini")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["translator"], "fake");
    assert_eq!(v["retranslator"], "gemini");

    // A live line goes through the live engine only.
    let live = fallback::translate(&s.app, "Hold the gate.", &[])
        .await
        .unwrap();
    assert_eq!(live.engine, "fake");
    assert_eq!(*s.other.calls.lock().unwrap(), 0);
    let mut line = stored(&s, "Hold the gate.", 1);
    line.target_text = live.text;
    line.engine = live.engine.into();
    line.is_fallback = live.is_fallback;
    db::update_line(&s.app.db(), &line).unwrap();

    let (status, v) = retranslate(&s, &line.id).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let saved = row(&s, &line.id);
    assert_eq!(saved.engine, "gemini");
    assert_eq!(saved.model_id.as_deref(), Some("model-x"));
    assert_eq!(saved.target_text, "(อีกตัว) Hold the gate.");
    assert_eq!(*s.other.calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn without_a_pick_retranslate_follows_the_live_engine() {
    let s = setup();
    let line = stored(&s, "Keep your lantern low.", 1);
    let (_, state) = call(&s, Method::GET, "/api/state", None, LAN).await;
    assert_eq!(state["retranslator"], Value::Null);

    let (status, v) = retranslate(&s, &line.id).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["engine"], "fake");
    assert_eq!(v["model_id"], Value::Null);
    assert_eq!(*s.other.calls.lock().unwrap(), 0);

    assert!(s.app.set_translator("gemini"));
    let (status, v) = retranslate(&s, &line.id).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["engine"], "gemini");

    // Picking "same as live" again clears an earlier pick.
    assert!(s.app.set_translator("fake"));
    pick(&s, json!("gemini")).await;
    let (status, v) = pick(&s, json!("")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["retranslator"], Value::Null);
    let (_, v) = retranslate(&s, &line.id).await;
    assert_eq!(v["engine"], "fake");
}

#[tokio::test]
async fn old_settings_without_the_key_load_and_retranslate_with_live() {
    let s = setup();
    let path = temp_file();
    std::fs::write(&path, r#"{"translator": "gemini", "tone": "formal"}"#).unwrap();
    settings::restore(&s.app, &path);
    assert_eq!(s.app.state.borrow().translator, "gemini");
    assert_eq!(s.app.state.borrow().retranslator, None);

    let line = stored(&s, "Keep your lantern low.", 1);
    let (status, v) = retranslate(&s, &line.id).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["engine"], "gemini");

    // A saved pick comes back; one this build lacks reads as "same as live".
    std::fs::write(&path, r#"{"translator": "fake", "retranslator": "gemini"}"#).unwrap();
    settings::restore(&s.app, &path);
    assert_eq!(s.app.state.borrow().retranslator, Some("gemini"));
    std::fs::write(&path, r#"{"translator": "fake", "retranslator": "claude"}"#).unwrap();
    settings::restore(&s.app, &path);
    assert_eq!(s.app.state.borrow().retranslator, None);
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn the_pick_is_saved_to_settings_keeping_other_keys() {
    let s = setup();
    let path = temp_file();
    std::fs::write(&path, r#"{"tone": "formal"}"#).unwrap();
    let saver = tokio::spawn(settings::keep_translator(
        s.app.state.subscribe(),
        path.clone(),
    ));
    assert!(s.app.set_retranslator(Some("gemini")));
    for _ in 0..100 {
        if settings::retranslator(&path).as_deref() == Some("gemini") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    saver.abort();
    assert_eq!(settings::retranslator(&path).as_deref(), Some("gemini"));
    assert_eq!(settings::translator(&path).as_deref(), Some("fake"));
    assert_eq!(settings::read(&path)["tone"], "formal");
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn the_pick_is_set_on_setup_only_and_must_be_a_known_engine() {
    let s = setup();
    let body = json!({ "translator": "gemini" });
    let (status, _) = call(&s, Method::PUT, "/api/retranslator", Some(body), LAN).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, v) = pick(&s, json!("klingon")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"].is_string(), "{v}");
    assert_eq!(s.app.state.borrow().retranslator, None);
}

#[tokio::test]
async fn an_unready_pick_says_why_and_no_other_engine_steps_in() {
    let s = setup();
    let line = stored(&s, "Keep your lantern low.", 1);
    pick(&s, json!("gemini")).await;
    *s.other.unready.lock().unwrap() = true;
    let mut events = s.app.events.subscribe();

    let (status, v) = retranslate(&s, &line.id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"], "ยังไม่ได้ใส่ Gemini API key");
    assert_eq!(v["fix"], "ใส่ API key ใน /setup");
    assert_eq!(*s.other.calls.lock().unwrap(), 0);
    assert_eq!(row(&s, &line.id), line);
    assert!(events.try_recv().is_err());
}

#[tokio::test]
async fn a_term_added_from_lan_is_used_when_retranslating() {
    let s = setup();
    pick(&s, json!("gemini")).await;
    let line = stored(&s, "We met the Warden.", 1);
    let term = json!({ "source": "Warden", "target": "ผู้คุม", "profile_id": s.profile });
    let (status, v) = call(&s, Method::POST, "/api/glossary", Some(term), LAN).await;
    assert!(status.is_success(), "{status} {v}");

    let (status, v) = retranslate(&s, &line.id).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["target_text"], "(อีกตัว) We met the ผู้คุม.");
}

#[tokio::test]
async fn failures_leave_the_line_as_it_was() {
    let s = setup();
    pick(&s, json!("gemini")).await;
    let line = stored(&s, "Keep your lantern low.", 1);
    let mut events = s.app.events.subscribe();

    let (status, v) = retranslate(&s, "nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
    *s.other.fail.lock().unwrap() = true;
    let (status, v) = retranslate(&s, &line.id).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{v}");
    assert!(
        v["error"]
            .as_str()
            .is_some_and(|e| e.contains("ต่อ Gemini ไม่ได้")),
        "{v}"
    );

    assert_eq!(row(&s, &line.id), line);
    assert!(events.try_recv().is_err());
}
