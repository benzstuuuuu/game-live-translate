//! OBS frame source and the "ตั้ง OBS ให้" button against an in-memory fake
//! OBS (#18): setup is idempotent and only touches yokoten-ocr; frames come
//! from GetSourceScreenshot in memory; a lost OBS, a missing card or a black
//! card stops translating with the right `error.code`.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io::Cursor;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::core::error::AppError;
use yokoten::core::obs::{Action, Filter, Input, SceneItem, SceneView, CROP_FILTER, OCR_SCENE};
use yokoten::obs::{Connect, Link, Obs, ObsError};
use yokoten::server::router;
use yokoten::{db, translate};

const DEVICE: &str = "UGREEN 15390";
const CARD: &str = "Capture Card Device 2";
const LOOPBACK: &str = "127.0.0.1:50000";
const LAN: &str = "192.168.1.20:50000";

#[derive(Default)]
struct World {
    down: bool,
    black: bool,
    inputs: Vec<Input>,
    scenes: BTreeMap<String, SceneView>,
    next_item: i64,
    /// Frames yokoten-ocr will show, then the last one again.
    frames: VecDeque<Vec<u8>>,
    last: Vec<u8>,
    /// What the card shows (GetSourceScreenshot of the card); None: grey.
    card: Option<Vec<u8>>,
}

type Shared = Arc<Mutex<World>>;

fn lock(w: &Shared) -> MutexGuard<'_, World> {
    w.lock().unwrap()
}

struct FakeConnect(Shared);
struct Fake(Shared);

impl Connect for FakeConnect {
    fn connect(&self) -> BoxFuture<'_, Result<Box<dyn Obs>, AppError>> {
        let w = self.0.clone();
        Box::pin(async move {
            if lock(&w).down {
                return Err(AppError::obs_unreachable());
            }
            Ok(Box::new(Fake(w)) as Box<dyn Obs>)
        })
    }
}

impl Fake {
    fn up(&self) -> Result<MutexGuard<'_, World>, ObsError> {
        let w = lock(&self.0);
        if w.down {
            Err(ObsError::Unreachable)
        } else {
            Ok(w)
        }
    }
}

impl Obs for Fake {
    fn inputs(&self) -> BoxFuture<'_, Result<Vec<Input>, ObsError>> {
        Box::pin(async move { Ok(self.up()?.inputs.clone()) })
    }

    fn canvas(&self) -> BoxFuture<'_, Result<(u32, u32), ObsError>> {
        Box::pin(async move { self.up().map(|_| (1920, 1080)) })
    }

    fn scene<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<SceneView>, ObsError>> {
        Box::pin(async move { Ok(self.up()?.scenes.get(name).cloned()) })
    }

    fn apply<'a>(
        &'a self,
        scene: &'a str,
        action: &'a Action,
    ) -> BoxFuture<'a, Result<(), ObsError>> {
        Box::pin(async move {
            let mut w = self.up()?;
            if let Action::CreateScene = action {
                w.scenes.insert(scene.into(), SceneView::default());
                return Ok(());
            }
            w.next_item += 1;
            let id = w.next_item;
            let s = w.scenes.get_mut(scene).ok_or(ObsError::NotFound)?;
            match action {
                Action::CreateScene => unreachable!(),
                Action::AddItem { source } => s.items.push(SceneItem {
                    id,
                    source: source.clone(),
                    enabled: true,
                }),
                Action::SetItemEnabled { id, enabled } => {
                    s.items.iter_mut().find(|i| i.id == *id).unwrap().enabled = *enabled
                }
                Action::CreateCrop { settings } => s.filters.push(Filter {
                    name: CROP_FILTER.into(),
                    kind: "crop_filter".into(),
                    enabled: true,
                    settings: settings.clone(),
                }),
                Action::SetCropSettings { settings } => {
                    s.filters
                        .iter_mut()
                        .find(|f| f.name == CROP_FILTER)
                        .unwrap()
                        .settings = settings.clone()
                }
                Action::SetFilterEnabled { name, enabled } => {
                    s.filters
                        .iter_mut()
                        .find(|f| f.name == *name)
                        .unwrap()
                        .enabled = *enabled
                }
            }
            Ok(())
        })
    }

    fn screenshot<'a>(
        &'a self,
        source: &'a str,
        _size: (u32, u32),
    ) -> BoxFuture<'a, Result<Vec<u8>, ObsError>> {
        Box::pin(async move {
            let mut w = self.up()?;
            if source == OCR_SCENE {
                if !w.scenes.contains_key(OCR_SCENE) {
                    return Err(ObsError::NotFound);
                }
                if let Some(f) = w.frames.pop_front() {
                    w.last = f;
                }
                return Ok(w.last.clone());
            }
            if w.inputs.iter().any(|i| i.name == source) {
                if let (Some(card), false) = (&w.card, w.black) {
                    return Ok(card.clone());
                }
                return Ok(solid(if w.black { 0 } else { 200 }));
            }
            Err(ObsError::NotFound)
        })
    }
}

/// 64×36 PNG of one grey level.
fn solid(level: u8) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(64, 36, image::Rgb([level; 3]));
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn card_input() -> Input {
    Input {
        name: CARD.into(),
        kind: "macos-avcapture-fast".into(),
        device_name: Some(DEVICE.into()),
    }
}

/// OBS with the card, a mic and the streamer's own Program scene.
fn world() -> Shared {
    let mut w = World {
        inputs: vec![
            Input {
                name: "Mic/Aux".into(),
                kind: "coreaudio_input_capture".into(),
                device_name: None,
            },
            card_input(),
        ],
        next_item: 10,
        ..World::default()
    };
    w.scenes.insert(
        "Scene".into(),
        SceneView {
            items: vec![
                SceneItem {
                    id: 1,
                    source: CARD.into(),
                    enabled: true,
                },
                SceneItem {
                    id: 2,
                    source: "Mic/Aux".into(),
                    enabled: true,
                },
            ],
            filters: vec![],
        },
    );
    Arc::new(Mutex::new(w))
}

fn app(w: &Shared) -> (Router, watch::Sender<bool>) {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    app_on(w, conn)
}

fn app_on(w: &Shared, conn: rusqlite::Connection) -> (Router, watch::Sender<bool>) {
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: None,
        obs: Some(Link {
            connect: Arc::new(FakeConnect(w.clone())),
            card_device: DEVICE.into(),
        }),
        translators: vec![Arc::new(translate::Fake)],
        frame_interval: Duration::from_millis(5),
    };
    (router(App::new(cfg, shutdown).unwrap()), shutdown_tx)
}

async fn send(app: &Router, req: Request<Body>, peer: &str) -> (StatusCode, Value) {
    let peer: SocketAddr = peer.parse().unwrap();
    let (mut parts, body) = req.into_parts();
    parts.extensions.insert(ConnectInfo(peer));
    let res = app
        .clone()
        .oneshot(Request::from_parts(parts, body))
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn call(app: &Router, method: Method, path: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .unwrap();
    send(app, req, LOOPBACK).await
}

async fn press_setup(app: &Router) -> (StatusCode, Value) {
    call(app, Method::POST, "/api/obs/setup").await
}

/// Poll /api/state until it is `status`; fails after `within`.
async fn wait_status(app: &Router, status: &str, within: Duration) -> Value {
    let started = std::time::Instant::now();
    loop {
        let (_, s) = call(app, Method::GET, "/api/state").await;
        if s["status"] == status {
            return s;
        }
        assert!(started.elapsed() < within, "still {s} after {within:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn setup_twice_gives_one_scene_and_one_filter() {
    let w = world();
    let (app, _s) = app(&w);
    let program_before = lock(&w).scenes["Scene"].clone();

    let (status, first) = press_setup(&app).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["card"], CARD);
    assert_eq!(first["changes"], 3);
    assert_eq!(
        first["crop"],
        json!({"left": 240, "top": 799, "cx": 1440, "cy": 180})
    );

    let (status, again) = press_setup(&app).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["changes"], 0, "second press must change nothing");

    let w = lock(&w);
    assert_eq!(
        w.scenes.keys().collect::<Vec<_>>(),
        vec!["Scene", OCR_SCENE]
    );
    let ours = &w.scenes[OCR_SCENE];
    assert_eq!(ours.items.len(), 1);
    assert_eq!(ours.items[0].source, CARD);
    assert_eq!(ours.filters.len(), 1);
    assert_eq!(ours.filters[0].name, CROP_FILTER);
    assert_eq!(
        ours.filters[0].settings,
        json!({"relative": false, "left": 240, "top": 799, "cx": 1440, "cy": 180})
    );
    assert_eq!(w.scenes["Scene"], program_before, "Program scene untouched");
}

#[tokio::test]
async fn setup_repairs_our_scene_without_deleting_anything() {
    let w = world();
    lock(&w).scenes.insert(
        OCR_SCENE.into(),
        SceneView {
            items: vec![
                SceneItem {
                    id: 5,
                    source: CARD.into(),
                    enabled: false,
                },
                SceneItem {
                    id: 6,
                    source: "Mic/Aux".into(),
                    enabled: true,
                },
            ],
            filters: vec![Filter {
                name: "ocr-crop".into(),
                kind: "crop_filter".into(),
                enabled: true,
                settings: json!({}),
            }],
        },
    );
    let (app, _s) = app(&w);
    let (status, done) = press_setup(&app).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["changes"], 4);

    let w = lock(&w);
    let ours = &w.scenes[OCR_SCENE];
    let items: Vec<(i64, bool)> = ours.items.iter().map(|i| (i.id, i.enabled)).collect();
    assert_eq!(items, vec![(5, true), (6, false)]);
    let filters: Vec<(&str, bool)> = ours
        .filters
        .iter()
        .map(|f| (f.name.as_str(), f.enabled))
        .collect();
    assert_eq!(filters, vec![("ocr-crop", false), (CROP_FILTER, true)]);
}

#[tokio::test]
async fn setup_without_card_is_card_not_found() {
    let w = world();
    lock(&w).inputs.retain(|i| i.name != CARD);
    let (app, _s) = app(&w);
    let (status, body) = press_setup(&app).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "card_not_found");
    assert!(body["error"]["message"].as_str().unwrap().contains(DEVICE));
    assert!(!lock(&w).scenes.contains_key(OCR_SCENE));
}

#[tokio::test]
async fn setup_with_obs_closed_is_obs_unreachable() {
    let w = world();
    lock(&w).down = true;
    let (app, _s) = app(&w);
    let (status, body) = press_setup(&app).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "obs_unreachable");
    assert!(!body["error"]["fix"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn setup_is_loopback_only_and_same_origin_only() {
    let w = world();
    let (app, _s) = app(&w);
    let req = |origin: Option<&str>| {
        let mut b = Request::post("/api/obs/setup").header(header::HOST, "127.0.0.1:8790");
        if let Some(o) = origin {
            b = b.header(header::ORIGIN, o);
        }
        b.body(Body::empty()).unwrap()
    };
    assert_eq!(send(&app, req(None), LAN).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        send(&app, req(Some("http://evil.example")), LOOPBACK)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert!(!lock(&w).scenes.contains_key(OCR_SCENE));
    assert_eq!(
        send(&app, req(Some("http://127.0.0.1:8790")), LOOPBACK)
            .await
            .0,
        StatusCode::OK
    );
}

fn fixture_frames() -> VecDeque<Vec<u8>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();
    files.iter().map(|p| std::fs::read(p).unwrap()).collect()
}

#[tokio::test]
async fn obs_frames_become_lines_in_memory() {
    let w = world();
    lock(&w).frames = fixture_frames();
    let (app, _s) = app(&w);
    assert_eq!(press_setup(&app).await.0, StatusCode::OK);

    let (status, state) = call(&app, Method::POST, "/api/start").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["status"], "translating");

    // Same fixtures as the folder test: three lines, the last one grown.
    let started = std::time::Instant::now();
    let texts = loop {
        let (_, lines) = call(&app, Method::GET, "/api/lines").await;
        let texts: Vec<String> = lines
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["source_text"].as_str().unwrap().to_owned())
            .collect();
        if texts.first().is_some_and(|t| t.ends_with("road left.")) {
            break texts;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "{texts:?}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(
        texts,
        vec![
            "No. But it's the only road left.",
            "You sure about this, Mara?",
            "Keep your lantern low."
        ]
    );
    let (_, state) = call(&app, Method::GET, "/api/state").await;
    assert_eq!(state["status"], "translating");
    assert!(state.get("error").is_none());
}

#[tokio::test]
async fn black_card_for_three_seconds_stops_with_card_black() {
    let w = world();
    lock(&w).black = true;
    let (app, _s) = app(&w);
    assert_eq!(press_setup(&app).await.0, StatusCode::OK);
    let started = std::time::Instant::now();
    call(&app, Method::POST, "/api/start").await;
    let state = wait_status(&app, "error", Duration::from_secs(10)).await;
    let took = started.elapsed();
    assert!(
        took > Duration::from_secs(3) && took < Duration::from_secs(6),
        "{took:?}"
    );
    assert_eq!(state["error"]["code"], "card_black");
}

#[tokio::test]
async fn obs_closing_mid_session_stops_with_obs_unreachable() {
    let w = world();
    lock(&w).frames = fixture_frames();
    let (app, _s) = app(&w);
    assert_eq!(press_setup(&app).await.0, StatusCode::OK);
    call(&app, Method::POST, "/api/start").await;
    wait_status(&app, "translating", Duration::from_secs(1)).await;
    lock(&w).down = true;
    let state = wait_status(&app, "error", Duration::from_secs(5)).await;
    assert_eq!(state["error"]["code"], "obs_unreachable");

    // Retry once OBS is back: a new session starts and the error clears.
    lock(&w).down = false;
    let (status, state) = call(&app, Method::POST, "/api/start").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(state["status"], "translating");
    assert!(state.get("error").is_none());
    let (_, stopped) = call(&app, Method::POST, "/api/stop").await;
    assert_eq!(stopped["status"], "idle");
}

#[tokio::test]
async fn start_without_card_is_card_not_found() {
    let w = world();
    lock(&w).inputs.retain(|i| i.name != CARD);
    let (app, _s) = app(&w);
    call(&app, Method::POST, "/api/start").await;
    let state = wait_status(&app, "error", Duration::from_secs(2)).await;
    assert_eq!(state["error"]["code"], "card_not_found");
}

#[tokio::test]
async fn start_before_setup_asks_for_the_setup_button() {
    let w = world();
    let (app, _s) = app(&w);
    call(&app, Method::POST, "/api/start").await;
    let state = wait_status(&app, "error", Duration::from_secs(2)).await;
    assert_eq!(state["error"]["code"], "card_not_found");
    assert!(state["error"]["message"]
        .as_str()
        .unwrap()
        .contains(OCR_SCENE));
}

#[tokio::test]
async fn card_unplugged_mid_session_is_card_not_found() {
    let w = world();
    lock(&w).frames = fixture_frames();
    let (app, _s) = app(&w);
    assert_eq!(press_setup(&app).await.0, StatusCode::OK);
    call(&app, Method::POST, "/api/start").await;
    wait_status(&app, "translating", Duration::from_secs(1)).await;
    lock(&w).inputs.retain(|i| i.name != CARD);
    let state = wait_status(&app, "error", Duration::from_secs(5)).await;
    assert_eq!(state["error"]["code"], "card_not_found");
}

/// Profile with the box at the top quarter of the frame (#38).
fn add_top_profile(conn: &rusqlite::Connection) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO profiles (id, name, region_x, region_y, region_w, region_h,
           created_at, last_used_at) VALUES (?1, 'Top', 0.0, 0.0, 1.0, 0.25, 1, 0)",
        [&id],
    )
    .unwrap();
    id
}

fn crop_settings(w: &Shared) -> Value {
    lock(w).scenes[OCR_SCENE]
        .filters
        .iter()
        .find(|f| f.name == CROP_FILTER)
        .unwrap()
        .settings
        .clone()
}

#[tokio::test]
async fn switching_profile_mid_session_recrops_and_keeps_translating() {
    let w = world();
    lock(&w).frames = fixture_frames();
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let top = add_top_profile(&conn);
    let (app, _s) = app_on(&w, conn);
    assert_eq!(press_setup(&app).await.0, StatusCode::OK);
    let (_, started) = call(&app, Method::POST, "/api/start").await;
    assert_eq!(started["status"], "translating");

    let req = Request::put("/api/profile")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "id": top }).to_string()))
        .unwrap();
    let (status, switched) = send(&app, req, LAN).await;
    assert_eq!(status, StatusCode::OK, "{switched}");
    assert_eq!(switched["status"], "translating");
    assert_ne!(switched["session_id"], started["session_id"]);
    assert_eq!(
        crop_settings(&w),
        json!({"relative": false, "left": 0, "top": 0, "cx": 1920, "cy": 270})
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, s) = call(&app, Method::GET, "/api/state").await;
    assert_eq!(s["status"], "translating", "{s}");
    call(&app, Method::POST, "/api/stop").await;
}

#[tokio::test]
async fn start_with_a_crop_that_differs_only_reports_it() {
    let w = world();
    let (app, _s) = app(&w);
    assert_eq!(press_setup(&app).await.0, StatusCode::OK);
    let off = json!({"relative": false, "left": 0, "top": 0, "cx": 10, "cy": 10});
    lock(&w).scenes.get_mut(OCR_SCENE).unwrap().filters[0].settings = off.clone();
    call(&app, Method::POST, "/api/start").await;
    let state = wait_status(&app, "error", Duration::from_secs(2)).await;
    assert_eq!(state["error"]["code"], "card_not_found");
    assert!(state["error"]["message"]
        .as_str()
        .unwrap()
        .contains(CROP_FILTER));
    assert_eq!(
        crop_settings(&w),
        off,
        "start only checks; it fixes nothing"
    );
}

#[tokio::test]
async fn start_with_obs_closed_shows_obs_unreachable() {
    let w = world();
    let (app, _s) = app(&w);
    assert_eq!(press_setup(&app).await.0, StatusCode::OK);
    lock(&w).down = true;
    call(&app, Method::POST, "/api/start").await;
    let state = wait_status(&app, "error", Duration::from_secs(2)).await;
    assert_eq!(state["error"]["code"], "obs_unreachable");
    assert_eq!(state["error"]["message"], "ต่อ OBS ไม่ได้");
}

async fn checklist(app: &Router) -> Value {
    let (status, list) = call(app, Method::GET, "/api/checklist").await;
    assert_eq!(status, StatusCode::OK);
    list
}

fn states(list: &Value) -> Vec<String> {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            format!(
                "{}:{}",
                i["id"].as_str().unwrap(),
                i["state"].as_str().unwrap()
            )
        })
        .collect()
}

#[tokio::test]
async fn checklist_follows_obs_without_changing_it() {
    let w = world();
    let (app, _s) = app(&w);

    let before = checklist(&app).await;
    assert_eq!(
        states(&before),
        [
            "obs:ok",
            "card:ok",
            "scene:fail",
            "translator:ok",
            "profile:ok"
        ]
    );
    assert_eq!(before["ready"], false);
    assert!(
        !lock(&w).scenes.contains_key(OCR_SCENE),
        "checking creates nothing"
    );

    assert_eq!(press_setup(&app).await.0, StatusCode::OK);
    let after = checklist(&app).await;
    assert_eq!(after["ready"], true, "{after}");
    assert!(after["items"][1]["detail"].as_str().unwrap().contains(CARD));

    lock(&w).inputs.retain(|i| i.name != CARD);
    assert_eq!(
        states(&checklist(&app).await),
        [
            "obs:ok",
            "card:fail",
            "scene:skip",
            "translator:ok",
            "profile:ok"
        ]
    );

    lock(&w).down = true;
    let down = checklist(&app).await;
    assert_eq!(
        states(&down),
        [
            "obs:fail",
            "card:skip",
            "scene:skip",
            "translator:ok",
            "profile:ok"
        ]
    );
    assert!(down["items"][0]["fix"].as_str().unwrap().contains("4455"));
}

// Subtitle box picker on /setup (#34): card still, live OCR, "ใช้กรอบนี้".

fn card_world() -> Shared {
    let w = world();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/card.png");
    lock(&w).card = Some(std::fs::read(path).unwrap());
    w
}

async fn send_json(app: &Router, method: Method, path: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(app, req, LOOPBACK).await
}

/// Box `top` to `bottom` px on the 1080p card, x 240 to 1680 like the
/// default box.
fn rows(top: f64, bottom: f64) -> Value {
    json!({"x": 0.125, "y": top / 1080.0, "w": 0.75, "h": (bottom - top) / 1080.0})
}

async fn take_still(app: &Router) {
    let (status, _) = call(app, Method::GET, "/api/obs/still").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn still_is_the_whole_card_in_memory_and_loopback_only() {
    let w = card_world();
    let (app, _s) = app(&w);
    let req = || Request::get("/api/obs/still").body(Body::empty()).unwrap();
    assert_eq!(send(&app, req(), LAN).await.0, StatusCode::FORBIDDEN);

    let mut req = req();
    req.extensions_mut()
        .insert(ConnectInfo(LOOPBACK.parse::<SocketAddr>().unwrap()));
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let img = image::load_from_memory(&bytes).unwrap();
    assert_eq!((img.width(), img.height()), (1920, 1080));
}

#[tokio::test]
async fn still_without_obs_is_obs_unreachable() {
    let w = card_world();
    lock(&w).down = true;
    let (app, _s) = app(&w);
    let (status, body) = call(&app, Method::GET, "/api/obs/still").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "obs_unreachable");
}

#[tokio::test]
async fn preview_needs_a_still_first() {
    let w = card_world();
    let (app, _s) = app(&w);
    let (status, body) =
        send_json(&app, Method::POST, "/api/ocr-preview", rows(799.0, 979.0)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(!body["error"]["message"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn preview_of_the_default_box_reads_both_dialogue_lines() {
    let w = card_world();
    let (app, _s) = app(&w);
    take_still(&app).await;
    let (status, body) =
        send_json(&app, Method::POST, "/api/ocr-preview", rows(799.0, 979.0)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["lines"], json!(["You sure about this,", "Mara?"]));
    assert_eq!(body["hint"], Value::Null);
}

#[tokio::test]
async fn preview_of_a_box_too_short_says_to_widen_the_bottom() {
    let w = card_world();
    let (app, _s) = app(&w);
    take_still(&app).await;
    let (status, body) =
        send_json(&app, Method::POST, "/api/ocr-preview", rows(799.0, 890.0)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["lines"], json!(["You sure about this,"]));
    let hint = body["hint"].as_str().unwrap();
    assert!(hint.contains("ขอบล่าง"), "{hint}");
}

#[tokio::test]
async fn preview_of_an_empty_box_says_nothing_is_read() {
    let w = card_world();
    let (app, _s) = app(&w);
    take_still(&app).await;
    let (status, body) = send_json(
        &app,
        Method::POST,
        "/api/ocr-preview",
        json!({"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["lines"], json!([]));
    assert!(body["hint"].as_str().unwrap().contains("ยังไม่เห็นข้อความ"));
}

#[tokio::test]
async fn use_this_box_saves_it_and_sets_yokoten_crop() {
    let w = card_world();
    let (app, _s) = app(&w);
    let program_before = lock(&w).scenes["Scene"].clone();
    let b = json!({"x": 0.1, "y": 0.7, "w": 0.8, "h": 0.2});
    let (status, body) = send_json(&app, Method::PUT, "/api/region", b.clone()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let crop = json!({"left": 192, "top": 756, "cx": 1536, "cy": 216});
    assert_eq!(body["crop"], crop);
    assert_eq!(body["region"], b);
    assert_eq!(call(&app, Method::GET, "/api/region").await.1["region"], b);
    {
        let w = lock(&w);
        let f = &w.scenes[OCR_SCENE].filters;
        assert_eq!(f.len(), 1);
        let mut want = crop.clone();
        want["relative"] = json!(false);
        assert_eq!(f[0].settings, want);
        assert_eq!(w.scenes["Scene"], program_before, "Program scene untouched");
    }

    // A box dragged past the frame is pulled back inside before saving.
    let (status, body) = send_json(
        &app,
        Method::PUT,
        "/api/region",
        json!({"x": 0.9, "y": 0.95, "w": 0.3, "h": 0.1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["crop"],
        json!({"left": 1344, "top": 972, "cx": 576, "cy": 108})
    );
    assert_eq!(lock(&w).scenes[OCR_SCENE].filters.len(), 1);
}

#[tokio::test]
async fn box_stays_saved_when_obs_is_closed() {
    let w = card_world();
    lock(&w).down = true;
    let (app, _s) = app(&w);
    let b = json!({"x": 0.1, "y": 0.7, "w": 0.8, "h": 0.2});
    let (status, body) = send_json(&app, Method::PUT, "/api/region", b.clone()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "obs_unreachable");
    assert_eq!(body["region"], b);
    assert_eq!(call(&app, Method::GET, "/api/region").await.1["region"], b);
}

#[tokio::test]
async fn saved_box_comes_back_after_a_restart() {
    let dir = std::env::temp_dir().join(format!("yokoten-region-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("yokoten.db");
    let w = card_world();
    let b = json!({"x": 0.05, "y": 0.6, "w": 0.9, "h": 0.25});
    {
        let (app, _s) = app_on(&w, db::open(&path).unwrap());
        let default = call(&app, Method::GET, "/api/region").await.1;
        assert_eq!(
            default["region"],
            json!({"x": 0.125, "y": 0.74, "w": 0.75, "h": 180.0 / 1080.0}),
            "default box is 180 px tall on 1080p"
        );
        assert_eq!(
            send_json(&app, Method::PUT, "/api/region", b.clone())
                .await
                .0,
            StatusCode::OK
        );
    }
    let (app, _s) = app_on(&w, db::open(&path).unwrap());
    assert_eq!(call(&app, Method::GET, "/api/region").await.1["region"], b);
    std::fs::remove_dir_all(dir).unwrap();
}

// FR-ERR-04 to 06 (#43): the session stays on through a problem, tries again
// by itself and goes back to translating once it is fixed.

/// Setup done, translating fixture frames from the fake OBS.
async fn translating(w: &Shared) -> (Router, watch::Sender<bool>, Value) {
    lock(w).frames = fixture_frames();
    let (app, s) = app(w);
    assert_eq!(press_setup(&app).await.0, StatusCode::OK);
    call(&app, Method::POST, "/api/start").await;
    let state = wait_status(&app, "translating", Duration::from_secs(1)).await;
    (app, s, state)
}

#[tokio::test]
async fn obs_coming_back_resumes_translating_within_five_seconds() {
    let w = world();
    let (app, _s, before) = translating(&w).await;
    lock(&w).down = true;
    let state = wait_status(&app, "error", Duration::from_secs(5)).await;
    assert_eq!(state["error"]["code"], "obs_unreachable");
    assert_eq!(state["error"]["retry_s"], 5, "{state}");

    lock(&w).down = false;
    let back = std::time::Instant::now();
    let state = wait_status(&app, "translating", Duration::from_secs(7)).await;
    assert!(
        back.elapsed() < Duration::from_millis(6500),
        "{:?}",
        back.elapsed()
    );
    assert!(state.get("error").is_none(), "{state}");
    assert_eq!(
        state["session_id"], before["session_id"],
        "same session goes on"
    );
    call(&app, Method::POST, "/api/stop").await;
}

#[tokio::test]
async fn card_picture_coming_back_resumes_translating() {
    let w = world();
    let (app, _s, before) = translating(&w).await;
    lock(&w).black = true;
    let state = wait_status(&app, "error", Duration::from_secs(6)).await;
    assert_eq!(state["error"]["code"], "card_black");
    assert!(state["error"]["retry_s"].as_u64().is_some(), "{state}");

    lock(&w).black = false;
    let back = std::time::Instant::now();
    let state = wait_status(&app, "translating", Duration::from_secs(3)).await;
    assert!(
        back.elapsed() < Duration::from_secs(2),
        "{:?}",
        back.elapsed()
    );
    assert_eq!(state["session_id"], before["session_id"]);
    call(&app, Method::POST, "/api/stop").await;
}

#[tokio::test]
async fn cause_card_follows_the_problem_until_the_card_is_back() {
    let w = world();
    let (app, _s, _) = translating(&w).await;
    lock(&w).down = true;
    let state = wait_status(&app, "error", Duration::from_secs(5)).await;
    assert_eq!(state["error"]["code"], "obs_unreachable");

    // OBS is back, but the card was unplugged meanwhile.
    {
        let mut w = lock(&w);
        w.inputs.retain(|i| i.name != CARD);
        w.down = false;
    }
    let started = std::time::Instant::now();
    let state = loop {
        let (_, s) = call(&app, Method::GET, "/api/state").await;
        if s["error"]["code"] == "card_not_found" {
            break s;
        }
        assert!(started.elapsed() < Duration::from_secs(7), "{s}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(state["status"], "error");
    assert!(state["error"]["message"].as_str().unwrap().contains(DEVICE));

    lock(&w).inputs.push(card_input());
    let state = wait_status(&app, "translating", Duration::from_secs(7)).await;
    assert!(state.get("error").is_none());
    call(&app, Method::POST, "/api/stop").await;
}

#[tokio::test]
async fn stop_while_waiting_for_obs_stops_trying() {
    let w = world();
    let (app, _s, _) = translating(&w).await;
    lock(&w).down = true;
    wait_status(&app, "error", Duration::from_secs(5)).await;
    let (_, stopped) = call(&app, Method::POST, "/api/stop").await;
    assert_eq!(stopped["status"], "idle");

    lock(&w).down = false;
    tokio::time::sleep(Duration::from_millis(5500)).await;
    let (_, s) = call(&app, Method::GET, "/api/state").await;
    assert_eq!(s["status"], "idle", "{s}");
}
