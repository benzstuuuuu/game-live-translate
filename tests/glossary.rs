//! Glossary in two layers (#44): the API behind /setup (and POST from a phone
//! on the LAN), then the terms reaching a line: placeholders for engines
//! without a prompt (the fake, like Apple) and the prompt for Gemini. In-memory
//! SQLite, fake translators; the real engines are in the ignored test at the
//! end and in tests/apple_translation.rs.

#[path = "fixtures/glossary_spike.rs"]
mod spike;

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
use yokoten::secret::Memory;
use yokoten::server::router;
use yokoten::translate::gemini::{Api, Http};
use yokoten::translate::{Fake, Gemini, Translator};
use yokoten::{db, fallback};

const LAN: &str = "192.168.1.20:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

struct Setup {
    app: Arc<App>,
    router: Router,
    profile: String,
    _shutdown: watch::Sender<bool>,
}

fn setup(translators: Vec<Arc<dyn Translator>>, frames: Option<PathBuf>) -> Setup {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let profile = db::last_profile(&conn).unwrap().unwrap().id;
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: frames,
        obs: None,
        translators,
        frame_interval: Duration::from_millis(5),
        live: None,
    };
    let app = App::new(cfg, shutdown).unwrap();
    Setup {
        router: router(app.clone()),
        app,
        profile,
        _shutdown: shutdown_tx,
    }
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    peer: &str,
) -> (StatusCode, Value) {
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
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

async fn add(s: &Setup, source: &str, target: &str, game: bool) -> Value {
    let profile = if game { s.profile.as_str() } else { "" };
    let body = json!({ "source": source, "target": target, "profile_id": profile });
    let (status, v) = call(
        &s.router,
        Method::POST,
        "/api/glossary",
        Some(body),
        LOOPBACK,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    v
}

// ---- API (FR-GLS-03, FR-SET-08) ----

#[tokio::test]
async fn setup_adds_edits_and_deletes_terms_in_both_layers() {
    let s = setup(vec![Arc::new(Fake)], None);
    let shared = add(&s, "Warden", "ผู้คุม", false).await;
    assert_eq!(shared["profile_id"], "");
    let game = add(&s, " warden ", "วอร์เดน", true).await;
    assert_eq!(game["profile_id"], s.profile.as_str());
    assert_eq!(game["source"], "warden");

    let (status, all) = call(&s.router, Method::GET, "/api/glossary", None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all.as_array().unwrap().len(), 2);

    let path = format!("/api/glossary/{}", game["id"].as_str().unwrap());
    let edit = json!({ "source": "Warden", "target": "วอร์เดนใหม่" });
    let (status, v) = call(&s.router, Method::PUT, &path, Some(edit), LOOPBACK).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["target"], "วอร์เดนใหม่");
    let (status, v) = call(&s.router, Method::GET, &path, None, LOOPBACK).await;
    assert_eq!(
        (status, v["target"].as_str()),
        (StatusCode::OK, Some("วอร์เดนใหม่"))
    );

    let (status, _) = call(&s.router, Method::DELETE, &path, None, LOOPBACK).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&s.router, Method::DELETE, &path, None, LOOPBACK).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, all) = call(&s.router, Method::GET, "/api/glossary", None, LOOPBACK).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn bad_terms_are_refused_with_a_thai_reason() {
    let s = setup(vec![Arc::new(Fake)], None);
    add(&s, "Paimon", "ไพมอน", false).await;
    let cases = [
        (
            json!({ "source": " ", "target": "ก" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "source": "A", "target": "" }),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({ "source": "PAIMON", "target": "x" }),
            StatusCode::CONFLICT,
        ),
        (
            json!({ "source": "A", "target": "ก", "profile_id": "nope" }),
            StatusCode::NOT_FOUND,
        ),
    ];
    for (body, want) in cases {
        let (status, v) = call(
            &s.router,
            Method::POST,
            "/api/glossary",
            Some(body),
            LOOPBACK,
        )
        .await;
        assert_eq!(status, want, "{v}");
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{v}");
    }
}

#[tokio::test]
async fn lan_may_add_a_term_but_nothing_else() {
    let s = setup(vec![Arc::new(Fake)], None);
    let body = json!({ "source": "Mara", "target": "มารา", "profile_id": s.profile });
    let (status, v) = call(&s.router, Method::POST, "/api/glossary", Some(body), LAN).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let path = format!("/api/glossary/{}", v["id"].as_str().unwrap());
    let edit = json!({ "source": "Mara", "target": "x" });
    for (method, path, body) in [
        (Method::GET, "/api/glossary", None),
        (Method::GET, path.as_str(), None),
        (Method::PUT, path.as_str(), Some(edit)),
        (Method::DELETE, path.as_str(), None),
    ] {
        let (status, _) = call(&s.router, method.clone(), path, body, LAN).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }
}

// ---- terms reaching a line (FR-GLS-01, 04, 05) ----

#[tokio::test]
async fn a_term_added_in_setup_is_used_on_the_next_line() {
    let s = setup(vec![Arc::new(Fake)], None);
    let before = fallback::translate(&s.app, "We met the Warden.", &[])
        .await
        .unwrap();
    assert_eq!(before.text, "(แปล) We met the Warden.");

    add(&s, "Warden", "ผู้คุม", false).await;
    add(&s, "warden", "วอร์เดน", true).await;
    let after = fallback::translate(&s.app, "We met the Warden. The Warden's sword.", &[])
        .await
        .unwrap();
    // The fake echoes the placeholders back, so they turn into the game's term.
    assert_eq!(after.text, "(แปล) We met the วอร์เดน. The วอร์เดน's sword.");
}

/// Like Apple when it drops a placeholder: answers "[n]" lines without them.
struct LosesPlaceholders {
    seen: Mutex<Vec<String>>,
}

impl Translator for LosesPlaceholders {
    fn engine(&self) -> &'static str {
        "apple"
    }

    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>> {
        self.seen.lock().unwrap().push(text.to_owned());
        let out = if text.contains('[') {
            "เราพบผู้คุม".to_owned()
        } else {
            format!("เราพบ {text}")
        };
        Box::pin(async move { Ok(out) })
    }
}

#[tokio::test]
async fn lost_placeholder_retranslates_and_replaces_leftover_english() {
    let engine = Arc::new(LosesPlaceholders {
        seen: Mutex::new(Vec::new()),
    });
    let s = setup(vec![engine.clone()], None);
    add(&s, "Warden", "วอร์เดน", true).await;
    let line = fallback::translate(&s.app, "the Warden", &[])
        .await
        .unwrap();
    assert_eq!(line.text, "เราพบ the วอร์เดน");
    assert_eq!(*engine.seen.lock().unwrap(), ["the [1]", "the Warden"]);
}

/// Answers every Gemini call with one Thai line and keeps the prompts.
struct FakeApi(Mutex<Vec<String>>);

impl Api for FakeApi {
    fn generate<'a>(
        &'a self,
        _key: &'a str,
        _model: &'a str,
        body: Value,
    ) -> BoxFuture<'a, Result<(u16, Vec<u8>), String>> {
        let prompt = body["contents"][0]["parts"][0]["text"].as_str().unwrap();
        self.0.lock().unwrap().push(prompt.to_owned());
        let reply = json!({ "candidates": [{ "content": { "parts": [{ "text": "แปลแล้ว" }] } }] });
        Box::pin(async move { Ok((200, reply.to_string().into_bytes())) })
    }
}

#[tokio::test]
async fn gemini_gets_only_the_terms_of_the_line_in_its_prompt() {
    let api = Arc::new(FakeApi(Mutex::new(Vec::new())));
    let gemini = Gemini::new(
        Settings::default(),
        Arc::new(Memory::with("AIzaTestKey-0123456789")),
        api.clone(),
        None,
    );
    let s = setup(vec![Arc::new(gemini)], None);
    add(&s, "Paimon", "ไพมอน", false).await;
    add(&s, "Captain Rhee", "กัปตันรี", false).await;
    add(&s, "Rhee", "รี", true).await;

    let line = fallback::translate(&s.app, "Captain Rhee wants a word.", &[])
        .await
        .unwrap();
    assert_eq!(line.text, "แปลแล้ว");
    let prompts = api.0.lock().unwrap().clone();
    let p = &prompts[0];
    assert!(p.contains("- Captain Rhee → กัปตันรี\n"), "{p}");
    assert!(!p.contains("Paimon") && !p.contains("- Rhee →"), "{p}");
    assert!(p.ends_with("Line:\nCaptain Rhee wants a word."), "{p}");
}

#[tokio::test]
async fn pipeline_line_carries_the_glossary_term() {
    let frames = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames");
    let s = setup(vec![Arc::new(Fake)], Some(frames));
    add(&s, "Mara", "มารา", true).await;
    let mut events = s.app.events.subscribe();
    let (status, v) = call(&s.router, Method::POST, "/api/start", None, LAN).await;
    assert_eq!(status, StatusCode::OK, "{v}");

    let mut texts = Vec::new();
    while texts.len() < 2 {
        let push = tokio::time::timeout(Duration::from_secs(60), events.recv())
            .await
            .expect("pipeline too slow")
            .unwrap();
        if let Push::Line(l) = push {
            texts.push(l.target_text);
        }
    }
    s.app.stop();
    assert_eq!(
        texts,
        [
            "(แปล) Keep your lantern low.",
            "(แปล) You sure about this, มารา?"
        ]
    );
}

// ---- real Gemini (ignored) ----

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

/// The 20 lines of the #27 spike through the real Gemini with the glossary in
/// the prompt. Run with `cargo test --test glossary -- --ignored --nocapture`
/// (20 calls on the live model). Prints Thai output only.
#[tokio::test]
#[ignore = "calls the real Gemini API with GEMINI_API_KEY"]
async fn real_gemini_keeps_the_spike_glossary() {
    let key = real_key().expect("GEMINI_API_KEY not set and no .env found");
    let gemini = Gemini::new(
        Settings::default(),
        Arc::new(Memory::with(&key)),
        Arc::new(Http::new().unwrap()),
        None,
    );
    let terms = spike::terms();
    let mut ok = 0;
    for line in spike::LINES {
        let thai = gemini
            .translate_with_glossary(line, &[], &terms)
            .await
            .unwrap();
        let missing = spike::missing(line, &thai);
        ok += missing.is_empty() as usize;
        println!(
            "{} {line}\n   → {thai}",
            if missing.is_empty() { "OK" } else { "--" }
        );
    }
    println!("gemini glossary: {ok}/{}", spike::LINES.len());
    assert_eq!(ok, spike::LINES.len());
}
