//! The audio path (#69): a profile whose text comes from audio streams a WAV
//! file to Live Translate and its lines reach /reader. Live Translate is a
//! fake WebSocket server on a random local port; the real service is in the
//! ignored test at the end.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio_websockets::{CloseCode, Message, ServerBuilder};
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::core::gemini::{Settings, Tone};
use yokoten::core::live::{CHUNK_BYTES, ENDPOINT};
use yokoten::core::wav;
use yokoten::secret::Memory;
use yokoten::server::router_with;
use yokoten::translate::gemini::Http;
use yokoten::translate::{Fake, Gemini};
use yokoten::{db, glossary, live, translation_api};

const KEY: &str = "AIzaTestKey-0123456789";
const LOOPBACK: &str = "127.0.0.1:50000";
const PATH: &str = "/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

/// What the fake Live Translate server saw.
#[derive(Debug, Default, Clone)]
struct Seen {
    uri: String,
    key: Option<String>,
    setup: Value,
    audio: Vec<Vec<u8>>,
    ended: bool,
}

/// How the fake answers once it has the setup.
enum Script {
    /// `setupComplete`, then these messages after the first audio chunk
    /// (`wait(ms)` pauses between them), then a normal close once the audio
    /// has ended.
    Answer(Vec<Value>),
    /// Close at once with this code and reason.
    Refuse(u16, String),
}

/// Serves one session on a random port; returns its ws:// endpoint.
async fn fake_live(script: Script) -> (String, Arc<Mutex<Seen>>) {
    fake_sessions(vec![script]).await
}

/// Serves one session per script, in order, on a random port; `Seen` is of
/// the last one.
async fn fake_sessions(scripts: Vec<Script>) -> (String, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let record = seen.clone();
    tokio::spawn(async move {
        for script in scripts {
            let (tcp, _) = listener.accept().await.unwrap();
            tokio::spawn(serve_session(tcp, script, record.clone()));
        }
    });
    (format!("ws://{addr}{PATH}"), seen)
}

async fn serve_session(tcp: tokio::net::TcpStream, script: Script, record: Arc<Mutex<Seen>>) {
    let (req, mut ws) = ServerBuilder::new().accept(tcp).await.unwrap();
    {
        let mut s = record.lock().unwrap();
        s.uri = req.uri().to_string();
        s.key = req
            .headers()
            .get("x-goog-api-key")
            .map(|v| v.to_str().unwrap().to_owned());
    }
    let setup = ws.next().await.unwrap().unwrap();
    record.lock().unwrap().setup = serde_json::from_str(setup.as_text().unwrap()).unwrap();
    let answers = match script {
        Script::Refuse(code, reason) => {
            let code = CloseCode::try_from(code).unwrap();
            let _ = ws.send(Message::close(Some(code), &reason)).await;
            return;
        }
        Script::Answer(answers) => answers,
    };
    let text = |v: Value| Message::text(v.to_string());
    ws.send(text(json!({ "setupComplete": {} }))).await.unwrap();
    let mut answered = false;
    while let Some(Ok(msg)) = ws.next().await {
        let Some(m) = msg.as_text() else { continue };
        let v: Value = serde_json::from_str(m).unwrap();
        let input = &v["realtimeInput"];
        if input["audioStreamEnd"] == true {
            record.lock().unwrap().ended = true;
            break;
        }
        assert_eq!(input["audio"]["mimeType"], "audio/pcm;rate=16000");
        let pcm = B64
            .decode(input["audio"]["data"].as_str().unwrap())
            .unwrap();
        record.lock().unwrap().audio.push(pcm);
        if !answered {
            answered = true;
            for a in &answers {
                // `{"wait_ms": n}`: a pause in speech, not a message.
                if let Some(ms) = a["wait_ms"].as_u64() {
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    continue;
                }
                // Binary, as the real service sends them.
                ws.send(Message::binary(a.to_string())).await.unwrap();
            }
        }
    }
    let _ = ws
        .send(Message::close(Some(CloseCode::NORMAL_CLOSURE), ""))
        .await;
}

fn en(t: &str) -> Value {
    transcript("inputTranscription", "en", t)
}

fn th(t: &str) -> Value {
    transcript("outputTranscription", "th", t)
}

/// A transcript piece shaped like the real service's; silence ("") comes
/// with a language code and no `text` at all.
fn transcript(kind: &str, lang: &str, t: &str) -> Value {
    let mut piece = json!({ "languageCode": lang });
    if !t.is_empty() {
        piece["text"] = json!(t);
    }
    json!({ "serverContent": { kind: piece } })
}

/// A pause of `ms` in the fake's answers.
fn wait(ms: u64) -> Value {
    json!({ "wait_ms": ms })
}

fn usage(prompt: u64, response: u64) -> Value {
    json!({ "usageMetadata": {
        "promptTokenCount": prompt, "responseTokenCount": response,
        "totalTokenCount": prompt + response } })
}

/// 16 kHz mono WAV of `chunks` × 100 ms in a fresh temp folder.
fn wav_file(chunks: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yokoten-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let samples: Vec<u8> = (0..chunks * CHUNK_BYTES).map(|i| (i % 251) as u8).collect();
    let path = dir.join("speech.wav");
    std::fs::write(&path, wav::encode(&samples)).unwrap();
    path
}

fn remove(path: &Path) {
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

struct Setup {
    app: Arc<App>,
    router: Router,
    gemini: Gemini,
    _shutdown: watch::Sender<bool>,
}

fn setup(store: Memory, endpoint: &str, audio_file: Option<PathBuf>, every: Duration) -> Setup {
    let gemini = Gemini::new(
        Settings::default(),
        Arc::new(store),
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
            endpoint: endpoint.into(),
            gemini: gemini.clone(),
            audio_file,
            chunk_interval: every,
        }),
    };
    let app = App::new(cfg, shutdown).unwrap();
    let router = router_with(
        app.clone(),
        translation_api::routes(app.clone(), gemini.clone()),
    );
    Setup {
        app,
        router,
        gemini,
        _shutdown: shutdown_tx,
    }
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let res = request(app, method, path, body).await;
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> axum::response::Response {
    let peer: SocketAddr = LOOPBACK.parse().unwrap();
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(peer));
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    app.clone().oneshot(req.body(body).unwrap()).await.unwrap()
}

/// Makes the profile in use an audio profile; returns its id.
async fn audio_profile(s: &Setup) -> String {
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
    assert_eq!(p["source"], "audio");
    assert_eq!(
        s.app.state.borrow().profile.as_ref().unwrap().source,
        "audio"
    );
    id
}

/// Reads SSE events (name, data), skipping `state`, until `done` says so;
/// `show` prints each one as it comes (for the real-service test).
async fn read_until(
    body: &mut Body,
    done: impl Fn(&[(String, Value)]) -> bool,
    show: bool,
) -> Vec<(String, Value)> {
    read_timed(body, done, show)
        .await
        .into_iter()
        .map(|(name, data, _)| (name, data))
        .collect()
}

/// `read_until`, with the (tokio) time each event was read.
async fn read_timed(
    body: &mut Body,
    done: impl Fn(&[(String, Value)]) -> bool,
    show: bool,
) -> Vec<(String, Value, tokio::time::Instant)> {
    let mut buf = String::new();
    let mut out = Vec::new();
    let mut times = Vec::new();
    while !done(&out) {
        let frame = body.frame().await.expect("stream ended").unwrap();
        let Ok(data) = frame.into_data() else {
            continue;
        };
        buf.push_str(std::str::from_utf8(&data).unwrap());
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            let field = |k: &str| {
                block
                    .lines()
                    .find_map(|l| l.strip_prefix(k).map(str::to_owned))
            };
            if let (Some(name), Some(data)) = (field("event: "), field("data: ")) {
                if name != "state" {
                    let data: Value = serde_json::from_str(&data).unwrap();
                    if show {
                        println!(
                            "{name} final={} EN {} | TH {}",
                            data["final"], data["source_text"], data["target_text"]
                        );
                    }
                    out.push((name, data));
                    times.push(tokio::time::Instant::now());
                }
            }
        }
    }
    out.into_iter()
        .zip(times)
        .map(|((name, data), at)| (name, data, at))
        .collect()
}

fn finals(events: &[(String, Value)]) -> Vec<(String, String)> {
    events
        .iter()
        .filter(|(_, l)| l["final"] == true)
        .map(|(_, l)| {
            let text = |k: &str| l[k].as_str().unwrap().to_owned();
            (text("source_text"), text("target_text"))
        })
        .collect()
}

#[tokio::test]
async fn audio_lines_reach_reader_with_glossary_tone_and_tokens() {
    let (endpoint, seen) = fake_live(Script::Answer(vec![
        en("Paimon, wait for me"),
        th("ไพมอน รอฉันด้วย"),
        usage(50, 7),
        en(" at the gate of Mondstadt."),
        th(" ที่ประตูมอนด์สตัดท์"),
        en(""),
        th(""),
        usage(48, 9),
        en("The bridge is out, so we will swim across."),
        th("สะพานพัง เราเลยต้องว่ายข้าม"),
        en(" Stay close"),
        th("อยู่ใกล้ ๆ"),
    ]))
    .await;
    let file = wav_file(10);
    let s = setup(
        Memory::with(KEY),
        &endpoint,
        Some(file.clone()),
        Duration::from_millis(2),
    );
    let id = audio_profile(&s).await;
    glossary::add(&s.app.db(), "", "Paimon", "ไพมอน").unwrap();
    glossary::add(&s.app.db(), &id, "Mondstadt", "มอนด์สตัดท์").unwrap();
    let change = json!({ "tone": "formal", "extra_prompt": "Mara is a woman." });
    s.gemini
        .update(serde_json::from_value(change).unwrap())
        .unwrap();

    let mut events = request(&s.router, Method::GET, "/events", None)
        .await
        .into_body();
    let (status, state) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    assert_eq!(state["status"], "translating");

    // Three lines: two closed by the rules, the last when the session ends.
    let got = tokio::time::timeout(
        Duration::from_secs(10),
        read_until(&mut events, |e| finals(e).len() == 3, false),
    )
    .await
    .expect("lines too slow");
    remove(&file);
    let pair = |e: &str, t: &str| (e.to_owned(), t.to_owned());
    assert_eq!(
        finals(&got),
        vec![
            pair(
                "Paimon, wait for me at the gate of Mondstadt.",
                "ไพมอน รอฉันด้วย ที่ประตูมอนด์สตัดท์"
            ),
            pair(
                "The bridge is out, so we will swim across.",
                "สะพานพัง เราเลยต้องว่ายข้าม"
            ),
            pair("Stay close", "อยู่ใกล้ ๆ"),
        ]
    );
    // A line opens with `line`, grows with `line-updated` (final false)
    // and closes with `line-updated` (final true), keeping its id.
    let (first, line) = &got[0];
    assert_eq!(first, "line");
    assert_eq!(line["final"], false);
    assert_eq!(line["source_text"], "Paimon, wait for me");
    assert_eq!(line["source"], "audio");
    assert_eq!(line["engine"], "live-translate");
    assert_eq!(line["model_id"], "gemini-3.5-live-translate-preview");
    let of_first: Vec<&(String, Value)> =
        got.iter().filter(|(_, l)| l["id"] == line["id"]).collect();
    assert!(of_first[1..].iter().all(|(n, _)| n == "line-updated"));
    assert!(of_first[1..of_first.len() - 1]
        .iter()
        .all(|(_, l)| l["final"] == false));
    assert_eq!(got.iter().filter(|(n, _)| n == "line").count(), 3);

    // What Live Translate got: key in the header only, the glossary of both
    // layers, the tone and the extra prompt, then every chunk of the file.
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.key.as_deref(), Some(KEY));
    assert!(!seen.uri.contains(KEY), "{}", seen.uri);
    assert_eq!(seen.uri, PATH);
    let setup = &seen.setup["setup"];
    assert_eq!(setup["model"], "models/gemini-3.5-live-translate-preview");
    assert_eq!(
        setup["generationConfig"]["responseModalities"],
        json!(["TEXT"])
    );
    assert_eq!(
        setup["generationConfig"]["translationConfig"]["targetLanguageCode"],
        "th"
    );
    let instruction = setup["systemInstruction"]["parts"][0]["text"]
        .as_str()
        .unwrap();
    assert!(instruction.contains("- Paimon → ไพมอน"), "{instruction}");
    assert!(
        instruction.contains("- Mondstadt → มอนด์สตัดท์"),
        "{instruction}"
    );
    assert!(
        instruction.contains(Tone::Formal.instruction()),
        "{instruction}"
    );
    assert!(instruction.contains("Mara is a woman."), "{instruction}");
    assert_eq!(seen.audio.len(), 10);
    assert!(seen.audio.iter().all(|c| c.len() == CHUNK_BYTES));
    assert_eq!(seen.audio[0][..4], [0, 1, 2, 3]);
    assert!(seen.ended, "audioStreamEnd after the file");

    // Tokens of the session and the history on /reader.
    let (_, state) = call(&s.router, Method::GET, "/api/state", None).await;
    assert_eq!(state["tokens"], json!({ "input": 98, "output": 16 }));
    let (_, history) = call(&s.router, Method::GET, "/api/lines", None).await;
    let texts: Vec<&str> = history
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["target_text"].as_str().unwrap())
        .collect();
    assert_eq!(
        texts,
        vec![
            "อยู่ใกล้ ๆ",
            "สะพานพัง เราเลยต้องว่ายข้าม",
            "ไพมอน รอฉันด้วย ที่ประตูมอนด์สตัดท์"
        ]
    );
    assert!(history
        .as_array()
        .unwrap()
        .iter()
        .all(|l| l["source"] == "audio"));
    let (_, stopped) = call(&s.router, Method::POST, "/api/stop", None).await;
    assert_eq!(stopped["status"], "idle");
}

/// /overlay's bar on the audio path (#72, FR-AUD-06): it follows speech by
/// `line-updated` and goes by `subtitle-gone` 3 s after a line closes, but
/// not when speech starts again before then. Tokio time is paused: the
/// fake's pauses and the wait take no real time.
#[tokio::test(start_paused = true)]
async fn overlay_bar_hides_three_seconds_after_speech_stops() {
    let (endpoint, _) = fake_live(Script::Answer(vec![
        en("Wait for me"),
        th("รอฉันด้วย"),
        en(""),
        th(""),
        // Speech again 2 s after the close: the bar stays.
        wait(2000),
        en("Over here"),
        th("ทางนี้"),
        wait(2000),
        en(""),
        th(""),
        // The session goes on in silence.
        wait(10_000),
    ]))
    .await;
    let file = wav_file(2);
    let s = setup(
        Memory::with(KEY),
        &endpoint,
        Some(file.clone()),
        Duration::from_millis(100),
    );
    audio_profile(&s).await;
    let mut events = request(&s.router, Method::GET, "/events", None)
        .await
        .into_body();
    let (status, state) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK, "{state}");

    let gone = |e: &[(String, Value)]| e.iter().any(|(n, _)| n == "subtitle-gone");
    let got = tokio::time::timeout(
        Duration::from_secs(60),
        read_timed(&mut events, gone, false),
    )
    .await
    .expect("no subtitle-gone");
    remove(&file);
    let names: Vec<(&str, &Value)> = got
        .iter()
        .map(|(n, l, _)| (n.as_str(), &l["final"]))
        .collect();
    let (f, t) = (&json!(false), &json!(true));
    assert_eq!(
        names,
        vec![
            ("line", f),
            ("line-updated", f),
            ("line-updated", t),
            ("line", f),
            ("line-updated", f),
            ("line-updated", t),
            ("subtitle-gone", &Value::Null),
        ],
        "one subtitle-gone, after the second line only"
    );
    assert_eq!(got[1].1["target_text"], "รอฉันด้วย");
    let (closed, gone_at) = (got[5].2, got[6].2);
    let waited = gone_at - closed;
    assert!(
        waited >= Duration::from_secs(3) && waited < Duration::from_millis(3100),
        "{waited:?}"
    );
    // The first close had no subtitle-gone in the 2 s before speech.
    assert_eq!(got[3].2 - got[2].2, Duration::from_secs(2));
}

/// The wait to hide the bar ends with its session: a line closes, the
/// session stops and a new one shows a line within 3 s; no `subtitle-gone`
/// may hide that new line.
#[tokio::test(start_paused = true)]
async fn stopped_session_does_not_hide_the_next_sessions_bar() {
    let (endpoint, _) = fake_sessions(vec![
        Script::Answer(vec![en("Wait for me"), th("รอฉันด้วย"), th(""), wait(60_000)]),
        Script::Answer(vec![en("Over here"), th("ทางนี้"), wait(60_000)]),
    ])
    .await;
    let file = wav_file(2);
    let s = setup(
        Memory::with(KEY),
        &endpoint,
        Some(file.clone()),
        Duration::from_millis(100),
    );
    audio_profile(&s).await;
    let mut events = request(&s.router, Method::GET, "/events", None)
        .await
        .into_body();
    let (status, state) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    let closed = |e: &[(String, Value)]| e.iter().any(|(_, l)| l["final"] == true);
    let first = tokio::time::timeout(
        Duration::from_secs(60),
        read_timed(&mut events, closed, false),
    )
    .await
    .expect("first line did not close");
    let closed_at = first.last().unwrap().2;

    tokio::time::sleep(Duration::from_secs(1)).await;
    let (_, stopped) = call(&s.router, Method::POST, "/api/stop", None).await;
    assert_eq!(stopped["status"], "idle");
    let (status, state) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    let new_line = |e: &[(String, Value)]| e.iter().any(|(n, _)| n == "line");
    let second = tokio::time::timeout(
        Duration::from_secs(60),
        read_timed(&mut events, new_line, false),
    )
    .await
    .expect("no line from the new session");
    let (_, line, shown_at) = second.iter().find(|(n, _, _)| n == "line").unwrap();
    assert_eq!(line["source_text"], "Over here");
    assert!(shown_at.duration_since(closed_at) < Duration::from_secs(3));

    // Well past 3 s after the old close: still no subtitle-gone.
    let gone = |e: &[(String, Value)]| e.iter().any(|(n, _)| n == "subtitle-gone");
    let late = tokio::time::timeout(
        Duration::from_secs(10),
        read_timed(&mut events, gone, false),
    )
    .await;
    remove(&file);
    assert!(late.is_err(), "subtitle-gone hid the new session's line");
}

#[tokio::test]
async fn audio_profile_without_key_cannot_start_and_says_why() {
    let file = wav_file(1);
    // Nothing listens there: starting must not even try.
    let s = setup(
        Memory::default(),
        "ws://127.0.0.1:9/",
        Some(file.clone()),
        Duration::from_millis(2),
    );
    audio_profile(&s).await;
    let (status, body) = call(&s.router, Method::POST, "/api/start", None).await;
    remove(&file);
    assert_eq!(status, StatusCode::CONFLICT);
    let reason = body["error"].as_str().unwrap();
    assert!(
        reason.contains("API key") && reason.contains("Live Translate"),
        "{reason}"
    );
    let (_, state) = call(&s.router, Method::GET, "/api/state", None).await;
    assert_eq!(state["status"], "idle");

    // The checklist says the same, and skips the subtitle box.
    let (_, list) = call(&s.router, Method::GET, "/api/checklist", None).await;
    let item = |id: &str| {
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(item("translator")["state"], "fail");
    assert!(item("translator")["label"]
        .as_str()
        .unwrap()
        .contains("Live Translate"));
    assert_eq!(item("scene")["state"], "skip");
    assert_eq!(item("profile")["state"], "ok");
    assert_eq!(list["ready"], false);
}

#[tokio::test]
async fn audio_profile_needs_an_audio_source() {
    let s = setup(
        Memory::with(KEY),
        "ws://127.0.0.1:9/",
        None,
        Duration::from_millis(2),
    );
    audio_profile(&s).await;
    let (status, body) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["error"].as_str().unwrap().contains("แหล่งเสียง"),
        "{body}"
    );

    let bad = std::env::temp_dir().join(format!("yokoten-{}.wav", uuid::Uuid::new_v4()));
    std::fs::write(&bad, b"not a wav").unwrap();
    let s = setup(
        Memory::with(KEY),
        "ws://127.0.0.1:9/",
        Some(bad.clone()),
        Duration::from_millis(2),
    );
    audio_profile(&s).await;
    let (status, body) = call(&s.router, Method::POST, "/api/start", None).await;
    std::fs::remove_file(bad).unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("WAV"), "{body}");
}

#[tokio::test]
async fn refused_session_shows_why_without_the_key() {
    let reason = format!("Quota exceeded for {KEY}");
    let (endpoint, seen) = fake_live(Script::Refuse(1008, reason)).await;
    let file = wav_file(3);
    let s = setup(
        Memory::with(KEY),
        &endpoint,
        Some(file.clone()),
        Duration::from_millis(2),
    );
    audio_profile(&s).await;
    let mut state = s.app.state.subscribe();
    let (status, _) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK);
    let failed = tokio::time::timeout(
        Duration::from_secs(10),
        state.wait_for(|s| s.error.is_some()),
    )
    .await
    .expect("no error shown")
    .unwrap()
    .clone();
    remove(&file);
    let error = serde_json::to_value(failed.error.unwrap()).unwrap();
    assert_eq!(error["code"], "live_translate_failed");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("1008") && message.contains("[key]"),
        "{message}"
    );
    assert!(!message.contains(KEY));
    assert!(
        seen.lock().unwrap().audio.is_empty(),
        "no audio before setupComplete"
    );
}

#[tokio::test]
async fn source_must_be_subtitle_or_audio() {
    let s = setup(
        Memory::with(KEY),
        "ws://127.0.0.1:9/",
        None,
        Duration::from_millis(2),
    );
    let id = s.app.state.borrow().profile.clone().unwrap().id;
    let path = format!("/api/profiles/{id}/source");
    let (status, _) = call(
        &s.router,
        Method::PUT,
        &path,
        Some(json!({ "source": "ocr" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &s.router,
        Method::PUT,
        "/api/profiles/nope/source",
        Some(json!({ "source": "audio" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = call(&s.router, Method::GET, "/api/profiles", None).await;
    assert_eq!(list[0]["source"], "subtitle");
}

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

/// Speech from macOS `say` as 16 kHz mono WAV with 6 s of silence after it,
/// so the last line closes before the file ends (an empty Thai piece needs
/// silence; after `audioStreamEnd` nothing more comes). Written to a temp
/// folder, never committed.
fn say_wav(text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yokoten-say-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let raw = dir.join("raw.wav");
    let ok = std::process::Command::new("say")
        .args(["--file-format=WAVE", "--data-format=LEI16@16000", "-o"])
        .arg(&raw)
        .arg(text)
        .status()
        .unwrap()
        .success();
    assert!(ok, "say failed");
    let bytes = std::fs::read(&raw).unwrap();
    let mut pcm = wav::pcm(&bytes).unwrap().to_vec();
    pcm.extend(std::iter::repeat_n(0u8, 6 * 32_000));
    let path = dir.join("speech.wav");
    std::fs::write(&path, wav::encode(&pcm)).unwrap();
    path
}

/// Run with `cargo test --test live -- --ignored --nocapture`. Streams about
/// fifteen seconds of `say` speech and silence in real time; prints every
/// line event, then the closed lines and the tokens. Passes once both
/// sentences are closed lines with English and Thai: the first split off at
/// its full stop, the second closed by the silence after it.
#[tokio::test]
#[ignore = "calls the real Live Translate with GEMINI_API_KEY"]
async fn real_live_translate_turns_say_audio_into_lines() {
    let key = real_key().expect("GEMINI_API_KEY not set and no .env found");
    let file = say_wav(
        "Paimon, wait for me at the gate of Mondstadt. \
         The Anemo Archon has not been seen in Teyvat for a very long time.",
    );
    let s = setup(
        Memory::with(&key),
        ENDPOINT,
        Some(file.clone()),
        Duration::from_millis(100),
    );
    let id = audio_profile(&s).await;
    for (src, dst) in [
        ("Paimon", "ไพมอน"),
        ("Mondstadt", "มอนด์สตัดท์"),
        ("Anemo", "อะนีโม"),
    ] {
        glossary::add(&s.app.db(), &id, src, dst).unwrap();
    }
    let mut events = request(&s.router, Method::GET, "/events", None)
        .await
        .into_body();
    let (status, body) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got = tokio::time::timeout(
        Duration::from_secs(60),
        read_until(&mut events, |e| finals(e).len() >= 2, true),
    )
    .await;
    let (_, state) = call(&s.router, Method::GET, "/api/state", None).await;
    call(&s.router, Method::POST, "/api/stop", None).await;
    remove(&file);
    let got = got.unwrap_or_else(|_| panic!("no lines in 60 s; state {state}"));
    for (en, th) in finals(&got) {
        println!("EN {en}\nTH {th}");
        assert!(!en.contains(&key) && !th.contains(&key));
    }
    println!("events {} tokens {}", got.len(), state["tokens"]);
    let lines = finals(&got);
    assert!(lines.iter().all(
        |(en, th)| !en.is_empty() && th.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c))
    ));
    assert!(state["tokens"]["input"].as_u64().unwrap() > 0);
}
