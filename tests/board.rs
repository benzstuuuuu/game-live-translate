//! LCD board API (#85): status and the latest line as JSON, and that line
//! drawn in Thai by CoreText as a 1-bit PBM. Both answer GET from the LAN
//! and refuse writes. In-memory SQLite, fake peer address.

use axum::body::{Body, Bytes};
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::board::draw;
use yokoten::db::{self, Line};
use yokoten::server::router;
use yokoten::translate::Fake;

/// The board on the LAN.
const LAN: &str = "192.168.1.30:50000";
const SESSION: &str = "s1";

struct Setup {
    app: Arc<App>,
    router: Router,
    profile: String,
    _shutdown: watch::Sender<bool>,
}

fn setup() -> Setup {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let profile = db::last_profile(&conn).unwrap().unwrap().id;
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: None,
        obs: None,
        translators: vec![Arc::new(Fake)],
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

/// A session with one stored line, as the pipeline stores it.
fn with_line(s: &Setup, source_text: &str, target_text: &str) -> Line {
    s.app
        .state
        .send_modify(|st| st.session_id = Some(SESSION.into()));
    let line = Line {
        id: uuid::Uuid::new_v4().to_string(),
        profile_id: s.profile.clone(),
        session_id: SESSION.into(),
        source: "audio".into(),
        source_text: source_text.into(),
        target_text: target_text.into(),
        engine: "fake".into(),
        is_fallback: false,
        model_id: None,
        done: true,
        created_at: 1,
        ocr_ms: None,
        translate_ms: None,
    };
    db::insert_line(&s.app.db(), &line).unwrap();
    line
}

async fn send(
    s: &Setup,
    method: Method,
    path: &str,
    if_none_match: Option<&str>,
) -> (StatusCode, HeaderMap, Bytes) {
    let peer: SocketAddr = LAN.parse().unwrap();
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(peer));
    if let Some(tag) = if_none_match {
        req = req.header(header::IF_NONE_MATCH, tag);
    }
    let res = s
        .router
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = res.into_parts();
    let bytes = body.collect().await.unwrap().to_bytes();
    (parts.status, parts.headers, bytes)
}

async fn board(s: &Setup) -> Value {
    let (status, _, body) = send(s, Method::GET, "/api/board", None).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&body).unwrap()
}

async fn pbm(s: &Setup, path: &str, tag: Option<&str>) -> (StatusCode, HeaderMap, Bytes) {
    send(s, Method::GET, path, tag).await
}

#[tokio::test]
async fn board_without_session_has_no_line() {
    let s = setup();
    let v = board(&s).await;
    assert_eq!(v["status"], "idle");
    assert!(v["profile_name"].is_string(), "{v}");
    assert_eq!(v["error_code"], Value::Null);
    assert_eq!(v["line"], Value::Null);
}

#[tokio::test]
async fn board_shows_the_latest_line() {
    let s = setup();
    let line = with_line(&s, "Hello, hero", "สวัสดีครับ ผู้กล้า");
    let v = board(&s).await;
    let l = &v["line"];
    assert_eq!(l["id"], line.id.as_str());
    assert_eq!(l["source"], "audio");
    assert_eq!(l["source_text"], "Hello, hero");
    assert_eq!(l["target_text"], "สวัสดีครับ ผู้กล้า");
    // As /api/lines gives it: SQLite does not keep `final`, so a stored
    // line reads back as final.
    assert_eq!(l["final"], true);
    assert!(l["rev"].as_str().is_some_and(|r| !r.is_empty()), "{v}");
}

#[tokio::test]
async fn rev_changes_when_the_line_is_rewritten_in_place() {
    let s = setup();
    let mut line = with_line(&s, "Hello", "สวัสดี");
    let before = board(&s).await["line"]["rev"].clone();
    line.source_text = "Hello, hero".into();
    line.target_text = "สวัสดีครับ ผู้กล้า".into();
    db::update_line(&s.app.db(), &line).unwrap();
    let after = board(&s).await;
    assert_eq!(after["line"]["id"], line.id.as_str());
    assert_ne!(after["line"]["rev"], before);
}

#[tokio::test]
async fn pbm_has_the_asked_size() {
    let s = setup();
    with_line(&s, "Hello, hero", "สวัสดีครับ ผู้กล้า");
    let (status, headers, body) = pbm(&s, "/api/board/line.pbm?w=296&h=128", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/x-portable-bitmap");
    let head = b"P4 296 128\n";
    assert!(body.starts_with(head));
    assert_eq!(body.len(), head.len() + 37 * 128);
    assert!(body[head.len()..].iter().any(|&b| b != 0), "no black pixel");
}

#[tokio::test]
async fn pbm_defaults_and_clamps_the_size() {
    let s = setup();
    with_line(&s, "Hi", "หวัดดี");
    let (_, _, body) = pbm(&s, "/api/board/line.pbm", None).await;
    assert!(body.starts_with(b"P4 400 120\n"));
    let (_, _, body) = pbm(&s, "/api/board/line.pbm?w=abc&h=1", None).await;
    assert!(body.starts_with(b"P4 400 8\n"));
    let (_, _, body) = pbm(&s, "/api/board/line.pbm?w=9999&h=9999", None).await;
    assert!(body.starts_with(b"P4 800 600\n"));
}

#[tokio::test]
async fn pbm_is_not_modified_when_the_etag_matches() {
    let s = setup();
    let mut line = with_line(&s, "Hi", "หวัดดี");
    let (_, headers, _) = pbm(&s, "/api/board/line.pbm", None).await;
    let tag = headers[header::ETAG].to_str().unwrap().to_owned();
    let rev = board(&s).await["line"]["rev"].as_str().unwrap().to_owned();
    assert_eq!(tag, format!("\"{rev}-400x120\""));

    let (status, headers, body) = pbm(&s, "/api/board/line.pbm", Some(&tag)).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(headers[header::ETAG], tag.as_str());
    assert!(body.is_empty());

    // Another size or a rewritten line is a new picture.
    let (status, _, _) = pbm(&s, "/api/board/line.pbm?w=200", Some(&tag)).await;
    assert_eq!(status, StatusCode::OK);
    line.target_text = "หวัดดีครับ".into();
    db::update_line(&s.app.db(), &line).unwrap();
    let (status, _, _) = pbm(&s, "/api/board/line.pbm", Some(&tag)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn pbm_is_no_content_without_a_line() {
    let s = setup();
    let (status, _, body) = pbm(&s, "/api/board/line.pbm", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty());
}

#[tokio::test]
async fn pbm_is_no_content_while_the_line_has_no_thai() {
    for target in ["", "  \n "] {
        let s = setup();
        with_line(&s, "Still hearing", target);
        let (status, _, _) = pbm(&s, "/api/board/line.pbm", None).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{target:?}");
    }
}

#[tokio::test]
async fn writes_from_lan_are_refused() {
    let s = setup();
    for path in ["/api/board", "/api/board/line.pbm"] {
        for m in [Method::POST, Method::PUT, Method::DELETE] {
            let (status, _, _) = send(&s, m.clone(), path, None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{m} {path}");
        }
    }
}

// CoreText drawing.

const W: u32 = 400;
const H: u32 = 120;

fn black(gray: &[u8], x: u32, y: u32) -> bool {
    gray[(y * W + x) as usize] < 128
}

/// Runs of rows with black pixels, top to bottom: one per drawn text line.
fn bands(gray: &[u8]) -> usize {
    let inked: Vec<bool> = (0..H).map(|y| (0..W).any(|x| black(gray, x, y))).collect();
    inked.windows(2).filter(|p| !p[0] && p[1]).count() + usize::from(inked[0])
}

#[test]
fn thai_is_drawn_inside_the_image() {
    let fitted = draw::fit("สวัสดีครับ ผู้กล้า", W, H);
    assert_eq!(fitted.text, "สวัสดีครับ ผู้กล้า");
    let gray = draw::gray(&fitted, W, H);
    assert_eq!(gray.len(), (W * H) as usize);
    let inked = (0..H)
        .flat_map(|y| (0..W).map(move |x| (x, y)))
        .filter(|&(x, y)| black(&gray, x, y))
        .count();
    assert!(inked > 200, "only {inked} black pixels");
    for x in 0..W {
        assert!(!black(&gray, x, 0) && !black(&gray, x, H - 1), "edge x={x}");
    }
    for y in 0..H {
        assert!(!black(&gray, 0, y) && !black(&gray, W - 1, y), "edge y={y}");
    }
}

#[test]
fn long_thai_wraps_onto_smaller_lines() {
    let short = draw::fit("สวัสดีครับ", W, H);
    let long = draw::fit("เจ้าต้องเดินทางผ่านป่าแห่งความมืดเพื่อไปพบผู้คุมประตูเมืองเก่า", W, H);
    assert!(long.size < short.size, "{} vs {}", long.size, short.size);
    assert!(!long.text.ends_with('…'));
    assert!(bands(&draw::gray(&long, W, H)) >= 2, "not wrapped");
}

#[test]
fn too_long_thai_is_cut_with_an_ellipsis() {
    let text = "เจ้าต้องเดินทางผ่านป่าแห่งความมืดเพื่อไปพบผู้คุมประตูเมืองเก่า ".repeat(10);
    let fitted = draw::fit(&text, W, H);
    assert!(fitted.text.ends_with('…'), "{}", fitted.text);
    assert!(fitted.text.len() < text.len());
    let gray = draw::gray(&fitted, W, H);
    assert!(bands(&gray) >= 3, "small text should fill several lines");
}
