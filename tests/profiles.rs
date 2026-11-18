//! Game profiles (#38): pick from /reader or /setup, switching mid-session
//! starts a new session, /setup-only edits, start checks, last profile on
//! relaunch, and the M1 checklist. In-memory SQLite, folder frames, fake
//! translators; OBS cases are in tests/obs.rs.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::server::router;
use yokoten::translate::{Fake, Translator, Unavailable};
use yokoten::{db, translate};

const LAN: &str = "192.168.1.20:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

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

fn memory() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    conn
}

/// A profile straight in SQLite, with or without a subtitle box. Drawing the
/// box on /setup is #34.
fn add_profile(conn: &Connection, name: &str, with_box: bool) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let w = with_box.then_some(0.75);
    conn.execute(
        "INSERT INTO profiles (id, name, region_x, region_y, region_w, region_h,
           created_at, last_used_at) VALUES (?1, ?2, 0.125, 0.74, ?3, 0.16, 1, 0)",
        params![id, name, w],
    )
    .unwrap();
    id
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames")
}

fn app_with(
    conn: Connection,
    frames_dir: Option<PathBuf>,
    translators: Vec<Arc<dyn Translator>>,
) -> (Arc<App>, Router, watch::Sender<bool>) {
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir,
        obs: None,
        translators,
        frame_interval: Duration::from_millis(5),
    };
    let app = App::new(cfg, shutdown).unwrap();
    (app.clone(), router(app), shutdown_tx)
}

fn app(conn: Connection) -> (Arc<App>, Router, watch::Sender<bool>) {
    app_with(conn, Some(fixtures()), vec![Arc::new(Fake)])
}

async fn call(
    app: &Router,
    peer: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let peer: SocketAddr = peer.parse().unwrap();
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .extension(ConnectInfo(peer))
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn state(app: &Router) -> Value {
    call(app, LAN, Method::GET, "/api/state", None).await.1
}

async fn pick(app: &Router, peer: &str, id: &str) -> (StatusCode, Value) {
    call(
        app,
        peer,
        Method::PUT,
        "/api/profile",
        Some(json!({ "id": id })),
    )
    .await
}

/// Lines /reader would load, once there are at least `n`.
async fn wait_lines(app: &Router, n: usize) -> Vec<Value> {
    let started = Instant::now();
    loop {
        let (_, lines) = call(app, LAN, Method::GET, "/api/lines", None).await;
        let lines = lines.as_array().cloned().unwrap_or_default();
        if lines.len() >= n {
            return lines;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "{lines:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn switching_profile_mid_session_starts_a_new_session() {
    let conn = memory();
    let first = db::last_profile(&conn).unwrap().unwrap();
    let second = add_profile(&conn, "Genshin", true);
    let (_, app, _s) = app(conn);

    let (status, started) = call(&app, LAN, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK);
    let old_session = started["session_id"].as_str().unwrap().to_owned();
    let old_lines = wait_lines(&app, 1).await;

    // From /reader on the phone.
    let (status, switched) = pick(&app, LAN, &second).await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["status"], "translating");
    assert_eq!(switched["profile"]["id"], second.as_str());
    assert_eq!(switched["profile"]["name"], "Genshin");
    let new_session = switched["session_id"].as_str().unwrap().to_owned();
    assert_ne!(new_session, old_session);

    let lines = wait_lines(&app, 1).await;
    for l in &lines {
        assert_eq!(l["session_id"], new_session.as_str(), "{l}");
        assert_eq!(l["profile_id"], second.as_str());
        assert!(!old_lines.iter().any(|o| o["id"] == l["id"]));
    }
    // The old profile keeps its history across sessions (FR-GAME-05).
    let (_, all) = call(&app, LAN, Method::GET, "/api/profiles", None).await;
    let old = all
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == first.id.as_str())
        .unwrap();
    assert!(old["line_count"].as_i64().unwrap() >= 1, "{old}");
    call(&app, LAN, Method::POST, "/api/stop", None).await;
}

#[tokio::test]
async fn picking_while_idle_clears_the_reader_and_waits() {
    let conn = memory();
    let second = add_profile(&conn, "B", true);
    let (_, app, _s) = app(conn);
    let (status, s) = pick(&app, LOOPBACK, &second).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(s["status"], "idle");
    assert_eq!(s["profile"]["name"], "B");
    assert!(s["session_id"].is_null());
    let (_, lines) = call(&app, LAN, Method::GET, "/api/lines", None).await;
    assert_eq!(lines, json!([]));
    assert_eq!(pick(&app, LAN, "nope").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn start_without_a_box_is_a_problem_with_its_cause() {
    let conn = memory();
    let bare = add_profile(&conn, "ใหม่", false);
    let (_, app, _s) = app(conn);
    pick(&app, LOOPBACK, &bare).await;

    let (status, body) = call(&app, LAN, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].is_string());
    let s = state(&app).await;
    assert_eq!(s["status"], "error");
    assert_eq!(s["error"]["code"], "no_region");
    assert!(s["error"]["message"].as_str().unwrap().contains("ใหม่"));
    assert!(!s["error"]["fix"].as_str().unwrap().is_empty());
    assert!(s["session_id"].is_null(), "no session was opened");
}

#[tokio::test]
async fn start_with_translator_not_ready_is_a_problem_with_its_cause() {
    let (_, app, _s) = app_with(memory(), Some(fixtures()), vec![Arc::new(NoPack)]);
    let (status, _) = call(&app, LAN, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let s = state(&app).await;
    assert_eq!(s["status"], "error");
    assert_eq!(s["error"]["code"], "translator_unavailable");
    assert_eq!(s["error"]["fix"], "ดาวน์โหลดใน System Settings");
}

#[tokio::test]
async fn picking_another_profile_clears_its_box_problem() {
    let conn = memory();
    let first = db::last_profile(&conn).unwrap().unwrap().id;
    let bare = add_profile(&conn, "ใหม่", false);
    let (_, app, _s) = app(conn);
    pick(&app, LOOPBACK, &bare).await;
    call(&app, LAN, Method::POST, "/api/start", None).await;
    assert_eq!(state(&app).await["status"], "error");

    let (_, s) = pick(&app, LAN, &first).await;
    assert_eq!(s["status"], "idle");
    assert!(s.get("error").is_none(), "{s}");
}

#[tokio::test]
async fn translator_problem_survives_a_profile_switch() {
    let conn = memory();
    let second = add_profile(&conn, "B", true);
    let (_, app, _s) = app_with(conn, Some(fixtures()), vec![Arc::new(NoPack)]);
    call(&app, LAN, Method::POST, "/api/start", None).await;
    let (_, s) = pick(&app, LAN, &second).await;
    assert_eq!(s["error"]["code"], "translator_unavailable");
}

#[tokio::test]
async fn create_rename_clear_and_delete_from_setup() {
    let (_, app, _s) = app(memory());
    let (status, created) = call(
        &app,
        LOOPBACK,
        Method::POST,
        "/api/profiles",
        Some(json!({ "name": "  Final  Fantasy " })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["name"], "Final Fantasy");
    let id = created["id"].as_str().unwrap().to_owned();

    let (status, dup) = call(
        &app,
        LOOPBACK,
        Method::POST,
        "/api/profiles",
        Some(json!({ "name": "เกมแรก" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(dup["error"], "มีโปรไฟล์ชื่อนี้แล้ว");

    let (_, all) = call(&app, LAN, Method::GET, "/api/profiles", None).await;
    assert_eq!(all[1]["name"], "Final Fantasy");
    assert_eq!(all[1]["has_region"], false);

    // Renaming the profile in use reaches /reader's header at once.
    pick(&app, LOOPBACK, &id).await;
    let path = format!("/api/profiles/{id}");
    let (status, renamed) = call(
        &app,
        LOOPBACK,
        Method::PUT,
        &path,
        Some(json!({ "name": "FF XVI" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["name"], "FF XVI");
    assert_eq!(state(&app).await["profile"]["name"], "FF XVI");

    let (status, cleared) = call(
        &app,
        LOOPBACK,
        Method::DELETE,
        &format!("{path}/lines"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cleared["deleted"], 0);

    // Deleting the profile in use falls back to the last used one.
    let (status, after) = call(&app, LOOPBACK, Method::DELETE, &path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(after["profile"]["name"], "เกมแรก");
    let (_, all) = call(&app, LAN, Method::GET, "/api/profiles", None).await;
    assert_eq!(all.as_array().unwrap().len(), 1);
    assert_eq!(
        call(&app, LOOPBACK, Method::DELETE, &path, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn profile_edits_and_checklist_are_mac_only() {
    let (_, app, _s) = app(memory());
    let id = db::last_profile(&memory()).unwrap().unwrap().id;
    let forbidden = [
        (Method::POST, "/api/profiles".to_string()),
        (Method::PUT, format!("/api/profiles/{id}")),
        (Method::DELETE, format!("/api/profiles/{id}")),
        (Method::DELETE, format!("/api/profiles/{id}/lines")),
        (Method::GET, "/api/checklist".to_string()),
    ];
    for (method, path) in forbidden {
        let body = Some(json!({ "name": "x" }));
        let (status, _) = call(&app, LAN, method.clone(), &path, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}");
    }
    assert_eq!(
        call(&app, LAN, Method::GET, "/api/profiles", None).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn picking_from_another_site_is_refused() {
    let conn = memory();
    let second = add_profile(&conn, "B", true);
    let (_, app, _s) = app(conn);
    let peer: SocketAddr = LOOPBACK.parse().unwrap();
    let req = Request::put("/api/profile")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "127.0.0.1:8790")
        .header(header::ORIGIN, "http://evil.example")
        .extension(ConnectInfo(peer))
        .body(Body::from(json!({ "id": second }).to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(state(&app).await["profile"]["name"], "เกมแรก");
}

/// Ctrl-C and run again: the last profile, its box and its history are back.
#[tokio::test]
async fn relaunch_opens_the_last_profile_with_its_box_and_history() {
    let dir = std::env::temp_dir().join(format!("yokoten-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("yokoten.db");
    let second = add_profile(&db::open(&path).unwrap(), "Genshin", true);

    let (app_state, app, _s) = app(db::open(&path).unwrap());
    pick(&app, LAN, &second).await;
    call(&app, LAN, Method::POST, "/api/start", None).await;
    let lines = wait_lines(&app, 1).await;
    app_state.stop();
    drop((app_state, app));

    let again = reopen(&path);
    let s = again.state.borrow().clone();
    assert_eq!(s.profile.as_ref().unwrap().id, second);
    assert_eq!(s.profile.as_ref().unwrap().name, "Genshin");
    let conn = db::open(&path).unwrap();
    assert!(db::profile_region(&conn, &second).unwrap().is_some());
    let kept: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM lines WHERE profile_id = ?1",
            [&second],
            |r| r.get(0),
        )
        .unwrap();
    assert!(kept as usize >= lines.len());
    let _ = std::fs::remove_dir_all(dir);
}

fn reopen(path: &Path) -> Arc<App> {
    let (_, shutdown) = watch::channel(false);
    let cfg = Config {
        conn: db::open(path).unwrap(),
        frames_dir: None,
        obs: None,
        translators: vec![Arc::new(translate::Fake)],
        frame_interval: Duration::from_millis(5),
    };
    App::new(cfg, shutdown).unwrap()
}

#[tokio::test]
async fn checklist_in_folder_mode_skips_obs_and_flags_a_missing_box() {
    let conn = memory();
    let bare = add_profile(&conn, "ใหม่", false);
    let (_, app, _s) = app(conn);

    let (status, list) = call(&app, LOOPBACK, Method::GET, "/api/checklist", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["ready"], true, "{list}");
    let ids: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["obs", "card", "scene", "translator", "profile"]);
    assert_eq!(list["items"][0]["state"], "skip");

    pick(&app, LOOPBACK, &bare).await;
    let (_, list) = call(&app, LOOPBACK, Method::GET, "/api/checklist", None).await;
    assert_eq!(list["ready"], false);
    assert_eq!(list["items"][4]["state"], "fail");
    assert_eq!(list["items"][4]["anchor"], "region");
}

#[tokio::test]
async fn checklist_only_reads_the_translator() {
    let (_, app, _s) = app_with(memory(), None, vec![Arc::new(NoPack)]);
    let (_, list) = call(&app, LOOPBACK, Method::GET, "/api/checklist", None).await;
    assert_eq!(list["items"][3]["state"], "fail");
    assert_eq!(list["items"][3]["fix"], "ดาวน์โหลดใน System Settings");
    // No OBS link at all: the OBS item says so.
    assert_eq!(list["items"][0]["state"], "fail");
    assert_eq!(
        state(&app).await["status"],
        "idle",
        "checking stops nothing"
    );
}
