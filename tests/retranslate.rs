//! Line actions on /reader (#45, FR-TRN-08, FR-GLS-02): a phone on the LAN
//! retranslates a line with another engine, which replaces the line in
//! SQLite and reaches every page as `line-updated`; a term it adds to the
//! glossary is used from then on. In-memory SQLite, fake translators.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config, Push};
use yokoten::db::{self, Line};
use yokoten::server::router;
use yokoten::translate::{Fake, Translator};

/// A phone on the LAN: /reader's calls must work from there.
const LAN: &str = "192.168.1.20:50000";

/// A second engine: marks the text, keeps the context it was given, and
/// fails when told to.
#[derive(Default)]
struct Other {
    earlier: Mutex<Vec<String>>,
    fail: Mutex<bool>,
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
        let fail = *self.fail.lock().unwrap();
        Box::pin(async move {
            if fail {
                return Err("ต่อ Gemini ไม่ได้".into());
            }
            Ok(format!("(อีกตัว) {text}"))
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

async fn call(s: &Setup, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
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
    let res = s.router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn retranslate(s: &Setup, id: &str, engine: &str) -> (StatusCode, Value) {
    let path = format!("/api/lines/{id}/retranslate");
    call(
        s,
        Method::POST,
        &path,
        Some(json!({ "translator": engine })),
    )
    .await
}

fn row(s: &Setup, id: &str) -> Line {
    db::session_lines(&s.app.db(), "s1", None, 50)
        .unwrap()
        .into_iter()
        .find(|l| l.id == id)
        .unwrap()
}

#[tokio::test]
async fn retranslate_from_lan_replaces_the_line_and_sends_line_updated() {
    let s = setup();
    stored(&s, "Where is the key?", 1);
    let line = stored(&s, "Keep your lantern low.", 2);
    let mut events = s.app.events.subscribe();

    let (status, v) = retranslate(&s, &line.id, "gemini").await;
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
async fn a_term_added_from_lan_is_used_when_retranslating() {
    let s = setup();
    let line = stored(&s, "We met the Warden.", 1);
    let term = json!({ "source": "Warden", "target": "ผู้คุม", "profile_id": s.profile });
    let (status, v) = call(&s, Method::POST, "/api/glossary", Some(term)).await;
    assert!(status.is_success(), "{status} {v}");

    let (status, v) = retranslate(&s, &line.id, "gemini").await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["target_text"], "(อีกตัว) We met the ผู้คุม.");
}

#[tokio::test]
async fn failures_leave_the_line_as_it_was() {
    let s = setup();
    let line = stored(&s, "Keep your lantern low.", 1);
    let mut events = s.app.events.subscribe();

    let (status, v) = retranslate(&s, "nope", "gemini").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
    let (status, v) = retranslate(&s, &line.id, "klingon").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    *s.other.fail.lock().unwrap() = true;
    let (status, v) = retranslate(&s, &line.id, "gemini").await;
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
