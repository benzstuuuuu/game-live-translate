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
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: None,
        obs: Some(Link {
            connect: Arc::new(FakeConnect(w.clone())),
            card_device: DEVICE.into(),
        }),
        translator: Arc::new(translate::Fake),
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
        json!({"left": 240, "top": 799, "cx": 1440, "cy": 173})
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
        json!({"relative": false, "left": 240, "top": 799, "cx": 1440, "cy": 173})
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
