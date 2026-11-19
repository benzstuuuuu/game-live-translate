//! Hearing the capture card (#70): Microphone permission, the card's audio
//! input not found or unplugged, and the checklist, all with a fake card.
//! Nothing here opens a real audio device except the ignored test at the end.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use futures_util::future::{self, BoxFuture};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config, Status};
use yokoten::audio::{AudioSource, Card};
use yokoten::core::card_audio::Permission;
use yokoten::core::error::{AppError, ErrorCode};
use yokoten::core::gemini::Settings;
use yokoten::core::live::CHUNK_BYTES;
use yokoten::recovery;
use yokoten::secret::Memory;
use yokoten::server::router_with;
use yokoten::translate::gemini::Http;
use yokoten::translate::{Fake, Gemini};
use yokoten::{db, live, translation_api};

const DEVICE: &str = "UGREEN 15390";
const KEY: &str = "AIzaTestKey-0123456789";
/// Every byte of a fake chunk, so it is told apart from silence.
const SOUND: u8 = 7;

/// A card that can be unplugged and have its permission turned off.
#[derive(Default)]
struct FakeCard {
    present: AtomicBool,
    denied: AtomicBool,
    opens: AtomicUsize,
    /// Chunks each opened source gives before the card is pulled out.
    unplug_after: Mutex<Option<usize>>,
}

impl FakeCard {
    fn plugged() -> Arc<Self> {
        let card = Self::default();
        card.present.store(true, Ordering::SeqCst);
        Arc::new(card)
    }

    fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }
}

impl Card for FakeCard {
    fn device(&self) -> &str {
        DEVICE
    }

    fn permission(&self) -> Permission {
        if self.denied.load(Ordering::SeqCst) {
            Permission::Denied
        } else {
            Permission::Granted
        }
    }

    fn found(&self) -> bool {
        self.present.load(Ordering::SeqCst)
    }

    fn open(&self) -> Result<Box<dyn AudioSource>, AppError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        if !self.found() {
            return Err(AppError::card_audio_missing(DEVICE));
        }
        let left = *self.unplug_after.lock().unwrap();
        Ok(Box::new(FakeSource { left }))
    }
}

struct FakeSource {
    left: Option<usize>,
}

impl AudioSource for FakeSource {
    fn next_chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, AppError>> {
        let out = match &mut self.left {
            Some(0) => Err(AppError::card_audio_missing(DEVICE)),
            Some(n) => {
                *n -= 1;
                Ok(Some(vec![SOUND; CHUNK_BYTES]))
            }
            None => Ok(Some(vec![SOUND; CHUNK_BYTES])),
        };
        Box::pin(future::ready(out))
    }
}

struct Setup {
    app: Arc<App>,
    router: Router,
    _shutdown: watch::Sender<bool>,
}

/// Nothing listens at the Live Translate endpoint: these tests never get
/// that far.
fn setup(card: Arc<FakeCard>) -> Setup {
    let gemini = Gemini::new(
        Settings::default(),
        Arc::new(Memory::with(KEY)),
        Arc::new(Http::new().unwrap()),
        None,
    );
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: None,
        obs: None,
        translators: vec![Arc::new(Fake)],
        frame_interval: Duration::from_millis(5),
        live: Some(live::Link {
            endpoint: "ws://127.0.0.1:9/".into(),
            card: Some(card),
            ..live::Link::new(gemini.clone(), None)
        }),
    };
    let app = App::new(cfg, shutdown).unwrap();
    let router = router_with(app.clone(), translation_api::routes(app.clone(), gemini));
    Setup {
        app,
        router,
        _shutdown: shutdown_tx,
    }
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(peer));
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Makes the profile in use an audio profile.
async fn audio_profile(s: &Setup) {
    let id = s.app.state.borrow().profile.clone().unwrap().id;
    let path = format!("/api/profiles/{id}/source");
    let (status, p) = call(
        &s.router,
        Method::PUT,
        &path,
        Some(json!({ "source": "audio" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p}");
}

fn error(app: &App) -> AppError {
    app.state.borrow().error.clone().expect("an error is shown")
}

fn is_silence(chunk: &[u8]) -> bool {
    chunk.len() == CHUNK_BYTES && chunk.iter().all(|&b| b == 0)
}

fn is_sound(chunk: &[u8]) -> bool {
    chunk.len() == CHUNK_BYTES && chunk.iter().all(|&b| b == SOUND)
}

#[tokio::test]
async fn microphone_off_refuses_start_and_says_where_to_turn_it_on() {
    let card = FakeCard::plugged();
    card.denied.store(true, Ordering::SeqCst);
    let s = setup(card.clone());
    audio_profile(&s).await;

    let (status, body) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["error"].as_str().unwrap().contains("Microphone"),
        "{body}"
    );

    let (_, state) = call(&s.router, Method::GET, "/api/state", None).await;
    assert_eq!(state["status"], "error");
    assert_eq!(state["error"]["code"], "mic_permission_denied");
    assert!(state["error"]["fix"]
        .as_str()
        .unwrap()
        .contains("System Settings › Privacy & Security › Microphone"));
    assert!(
        state["error"].get("retry_s").is_none(),
        "only the player can fix it"
    );
    assert_eq!(card.opens(), 0, "the card is not opened");
}

#[tokio::test]
async fn microphone_turned_off_during_a_session_ends_it() {
    let card = FakeCard::plugged();
    let s = setup(card.clone());
    let mut audio = recovery::Audio::new(s.app.clone(), card.clone());
    assert!(is_sound(&audio.next_chunk().await.unwrap().unwrap()));

    // "Don't Allow" on the first ask, or switched off in System Settings.
    card.denied.store(true, Ordering::SeqCst);
    let e = audio.next_chunk().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::MicPermissionDenied);
    assert!(e.fix.contains("Privacy & Security › Microphone"));
}

#[tokio::test(start_paused = true)]
async fn card_not_found_shows_why_sends_silence_and_comes_back_by_itself() {
    let card = Arc::new(FakeCard::default());
    let s = setup(card.clone());
    let mut audio = recovery::Audio::new(s.app.clone(), card.clone());

    assert!(is_silence(&audio.next_chunk().await.unwrap().unwrap()));
    assert_eq!(s.app.state.borrow().status, Status::Error);
    let e = error(&s.app);
    assert_eq!(e.code, ErrorCode::CardNotFound);
    assert!(e.message.contains(DEVICE), "{}", e.message);
    assert_eq!(e.retry_s, Some(5));

    // Until the next try: silence, and the card is not asked again.
    assert!(is_silence(&audio.next_chunk().await.unwrap().unwrap()));
    assert_eq!(card.opens(), 1);

    card.present.store(true, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(4)).await;
    assert!(is_silence(&audio.next_chunk().await.unwrap().unwrap()));
    assert_eq!(card.opens(), 1);

    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(is_sound(&audio.next_chunk().await.unwrap().unwrap()));
    assert_eq!(card.opens(), 2);
    let state = s.app.state.borrow().clone();
    assert_eq!(state.status, Status::Translating);
    assert_eq!(state.error, None);
}

#[tokio::test(start_paused = true)]
async fn card_unplugged_while_translating_is_opened_again() {
    let card = FakeCard::plugged();
    *card.unplug_after.lock().unwrap() = Some(2);
    let s = setup(card.clone());
    let mut audio = recovery::Audio::new(s.app.clone(), card.clone());

    for _ in 0..2 {
        assert!(is_sound(&audio.next_chunk().await.unwrap().unwrap()));
    }
    assert!(is_silence(&audio.next_chunk().await.unwrap().unwrap()));
    let e = error(&s.app);
    assert_eq!(e.code, ErrorCode::CardNotFound);
    assert_eq!(e.retry_s, Some(5));

    *card.unplug_after.lock().unwrap() = None;
    tokio::time::advance(Duration::from_secs(5)).await;
    assert!(is_sound(&audio.next_chunk().await.unwrap().unwrap()));
    assert_eq!(card.opens(), 2);
    assert_eq!(s.app.state.borrow().status, Status::Translating);
}

#[tokio::test]
async fn checklist_of_an_audio_profile_checks_card_audio_and_microphone() {
    let card = Arc::new(FakeCard::default());
    let s = setup(card.clone());
    let item = |list: &Value, id: &str| {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == id)
            .cloned()
    };

    // A subtitle profile does not hear the card.
    let (_, list) = call(&s.router, Method::GET, "/api/checklist", None).await;
    assert!(item(&list, "card-input").is_none() && item(&list, "mic").is_none());

    audio_profile(&s).await;
    let (_, list) = call(&s.router, Method::GET, "/api/checklist", None).await;
    let input = item(&list, "card-input").unwrap();
    assert_eq!(input["state"], "fail");
    assert!(input["detail"].as_str().unwrap().contains(DEVICE));
    assert_eq!(item(&list, "mic").unwrap()["state"], "ok");

    card.present.store(true, Ordering::SeqCst);
    card.denied.store(true, Ordering::SeqCst);
    let (_, list) = call(&s.router, Method::GET, "/api/checklist", None).await;
    assert_eq!(item(&list, "card-input").unwrap()["state"], "ok");
    let mic = item(&list, "mic").unwrap();
    assert_eq!(mic["state"], "fail");
    assert!(mic["fix"].as_str().unwrap().contains("Microphone"));
    assert_eq!(card.opens(), 0, "the checklist only looks");
}

/// Opens the real capture card next to OBS and listens for 3 seconds. Run by
/// hand with the game making sound:
/// `cargo test --test card_audio -- --ignored --nocapture`
#[tokio::test]
#[ignore = "opens the real capture card (UGREEN 15390) and may ask for Microphone permission"]
async fn real_card_gives_sound_while_obs_has_it_open() {
    let card = yokoten::card_audio::Cpal::new(DEVICE);
    assert!(card.found(), "no audio input named {DEVICE}");
    println!("Microphone permission: {:?}", card.permission());
    let mut source = card.open().expect("card opens");
    let mut peak = 0i32;
    for _ in 0..30 {
        let chunk = source.next_chunk().await.unwrap().unwrap();
        assert_eq!(chunk.len(), CHUNK_BYTES);
        let loudest = chunk
            .chunks_exact(2)
            .map(|b| i32::from(i16::from_le_bytes([b[0], b[1]])).abs())
            .max()
            .unwrap_or(0);
        peak = peak.max(loudest);
    }
    println!("peak {peak} / 32768");
    assert!(peak > 100, "3 s of near silence: is the game making sound?");
}
