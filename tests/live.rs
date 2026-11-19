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
use std::time::{Duration, Instant};
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

/// What the fake Live Translate server saw, over all its connections.
#[derive(Debug, Default, Clone)]
struct Seen {
    uri: String,
    key: Option<String>,
    /// The setup of each connection, in order.
    setups: Vec<Value>,
    /// The audio of all connections, in order.
    audio: Vec<Vec<u8>>,
    ended: bool,
    /// When each connection was accepted.
    opened: Vec<Instant>,
    /// When `goAway` went out, and when a connection was cut.
    go_away_at: Option<Instant>,
    cut_at: Option<Instant>,
}

/// How the fake answers one connection once it has the setup.
enum Script {
    /// `setupComplete`, then these messages after the first audio chunk,
    /// then a normal close once the audio has ended. Reads audio until the
    /// client leaves or ends it.
    Answer(Vec<Value>),
    /// Like `Answer`, plus the second list once silence (a chunk of zeros)
    /// comes: what the real service sends when the speaker pauses.
    Pause(Vec<Value>, Vec<Value>),
    /// Like `Answer`, then cuts the connection with no close frame.
    Cut(Vec<Value>),
    /// Close at once with this code and reason.
    Refuse(u16, String),
}

/// Serves one connection per script, in order, on a random port; returns
/// its ws:// endpoint.
async fn fake_live(scripts: Vec<Script>) -> (String, Arc<Mutex<Seen>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let record = seen.clone();
    tokio::spawn(async move {
        for script in scripts {
            let (tcp, _) = listener.accept().await.unwrap();
            serve(tcp, script, &record).await;
        }
    });
    (format!("ws://{addr}{PATH}"), seen)
}

async fn serve(tcp: tokio::net::TcpStream, script: Script, record: &Mutex<Seen>) {
    let (req, mut ws) = ServerBuilder::new().accept(tcp).await.unwrap();
    {
        let mut s = record.lock().unwrap();
        s.opened.push(Instant::now());
        s.uri = req.uri().to_string();
        s.key = req
            .headers()
            .get("x-goog-api-key")
            .map(|v| v.to_str().unwrap().to_owned());
    }
    let setup = ws.next().await.unwrap().unwrap();
    let setup = serde_json::from_str(setup.as_text().unwrap()).unwrap();
    record.lock().unwrap().setups.push(setup);
    let (answers, on_pause, cut) = match script {
        Script::Refuse(code, reason) => {
            let code = CloseCode::try_from(code).unwrap();
            let _ = ws.send(Message::close(Some(code), &reason)).await;
            return;
        }
        Script::Answer(answers) => (answers, Vec::new(), false),
        Script::Pause(answers, on_pause) => (answers, on_pause, false),
        Script::Cut(answers) => (answers, Vec::new(), true),
    };
    let text = |v: Value| Message::text(v.to_string());
    ws.send(text(json!({ "setupComplete": {} }))).await.unwrap();
    let (mut answered, mut paused) = (false, false);
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
        let silent = pcm.iter().all(|b| *b == 0);
        record.lock().unwrap().audio.push(pcm);
        let says = if !answered {
            answered = true;
            &answers
        } else if silent && !paused {
            paused = true;
            &on_pause
        } else {
            continue;
        };
        for a in says {
            if a.get("goAway").is_some() {
                record.lock().unwrap().go_away_at = Some(Instant::now());
            }
            // Binary, as the real service sends them.
            ws.send(Message::binary(a.to_string())).await.unwrap();
        }
        if cut {
            record.lock().unwrap().cut_at = Some(Instant::now());
            return;
        }
    }
    let _ = ws
        .send(Message::close(Some(CloseCode::NORMAL_CLOSURE), ""))
        .await;
}

fn handle(h: &str) -> Value {
    json!({ "sessionResumptionUpdate": { "resumable": true, "newHandle": h } })
}

fn go_away(left: &str) -> Value {
    json!({ "goAway": { "timeLeft": left } })
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

fn usage(prompt: u64, response: u64) -> Value {
    json!({ "usageMetadata": {
        "promptTokenCount": prompt, "responseTokenCount": response,
        "totalTokenCount": prompt + response } })
}

/// 16 kHz mono WAV of `chunks` × 100 ms in a fresh temp folder.
fn wav_file(chunks: usize) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yokoten-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("speech.wav");
    std::fs::write(&path, wav::encode(&file_chunks(chunks).concat())).unwrap();
    path
}

/// The audio of `wav_file(chunks)` in 100 ms chunks; none is all zeros.
fn file_chunks(chunks: usize) -> Vec<Vec<u8>> {
    let samples: Vec<u8> = (0..chunks * CHUNK_BYTES).map(|i| (i % 251) as u8).collect();
    samples.chunks(CHUNK_BYTES).map(<[u8]>::to_vec).collect()
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
            card: None,
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
    let mut buf = String::new();
    let mut out = Vec::new();
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
                }
            }
        }
    }
    out
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
    let (endpoint, seen) = fake_live(vec![Script::Answer(vec![
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
    ])])
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
    assert_eq!(seen.setups.len(), 1);
    let setup = &seen.setups[0]["setup"];
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
    assert_eq!(
        setup["contextWindowCompression"],
        json!({ "slidingWindow": {} })
    );
    assert_eq!(setup["sessionResumption"], json!({}));
    // The file, then 2 s of silence so the last sentence is translated.
    assert_eq!(seen.audio.len(), 10 + 20);
    assert!(seen.audio.iter().all(|c| c.len() == CHUNK_BYTES));
    assert_eq!(seen.audio[..10], file_chunks(10)[..]);
    assert!(seen.audio[10..].iter().flatten().all(|b| *b == 0));
    assert!(seen.ended, "audioStreamEnd after the file and silence");

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
    // Refused on every try: after the third the cause card shows.
    let reason = format!("Quota exceeded for {KEY}");
    let refuse = || Script::Refuse(1008, reason.clone());
    let (endpoint, seen) = fake_live(vec![refuse(), refuse(), refuse()]).await;
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
    // Tried again by itself, like OBS (#43).
    assert_eq!(error["retry_s"], 5);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.setups.len(), 3);
    assert!(seen.audio.is_empty(), "no audio before setupComplete");
}

/// An audio profile streaming a file of `chunks` × 100 ms, one chunk every
/// `every`, started with /events open.
async fn started(endpoint: &str, chunks: usize, every: Duration) -> (Setup, Body, PathBuf) {
    let file = wav_file(chunks);
    let s = setup(Memory::with(KEY), endpoint, Some(file.clone()), every);
    audio_profile(&s).await;
    let events = request(&s.router, Method::GET, "/events", None)
        .await
        .into_body();
    let (status, state) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK, "{state}");
    (s, events, file)
}

async fn lines_until(events: &mut Body, finals_wanted: usize) -> Vec<(String, Value)> {
    tokio::time::timeout(
        Duration::from_secs(15),
        read_until(events, |e| finals(e).len() == finals_wanted, false),
    )
    .await
    .expect("lines too slow")
}

fn pair(en: &str, th: &str) -> (String, String) {
    (en.to_owned(), th.to_owned())
}

/// How many lines were opened: one per line, so a repeat would show.
fn opened(events: &[(String, Value)]) -> usize {
    events.iter().filter(|(n, _)| n == "line").count()
}

#[tokio::test]
async fn go_away_moves_to_a_new_connection_at_the_next_silence() {
    let (endpoint, seen) = fake_live(vec![
        Script::Answer(vec![
            handle("h-1"),
            en("Paimon, wait for me at the gate."),
            th("ไพมอน รอฉันที่ประตู"),
            go_away("50s"),
            handle("h-2"),
            // The pause after the sentence: the moment to move.
            en(""),
            th(""),
        ]),
        Script::Answer(vec![en("The bridge is out."), th("สะพานพัง")]),
    ])
    .await;
    let (s, mut events, file) = started(&endpoint, 60, Duration::from_millis(5)).await;
    let got = lines_until(&mut events, 2).await;
    remove(&file);
    assert_eq!(
        finals(&got),
        vec![
            pair("Paimon, wait for me at the gate.", "ไพมอน รอฉันที่ประตู"),
            pair("The bridge is out.", "สะพานพัง"),
        ]
    );
    assert_eq!(opened(&got), 2, "no line twice");

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.setups.len(), 2);
    let (first, second) = (&seen.setups[0]["setup"], &seen.setups[1]["setup"]);
    assert_eq!(first["sessionResumption"], json!({}));
    assert_eq!(second["sessionResumption"], json!({ "handle": "h-2" }));
    assert_eq!(second["systemInstruction"], first["systemInstruction"]);
    assert_eq!(
        second["contextWindowCompression"],
        json!({ "slidingWindow": {} })
    );
    // Every chunk of the file once and in order across both connections,
    // then the silence.
    assert_eq!(seen.audio.len(), 60 + 20);
    assert_eq!(seen.audio[..60], file_chunks(60)[..]);
    assert!(seen.ended);
    let (_, state) = call(&s.router, Method::GET, "/api/state", None).await;
    assert_eq!(state["status"], "translating", "{state}");
}

#[tokio::test]
async fn go_away_without_silence_moves_when_five_seconds_are_left() {
    let (endpoint, seen) = fake_live(vec![
        Script::Answer(vec![
            handle("h-9"),
            en("We keep talking and talking"),
            th("เราคุยกันไปเรื่อย ๆ"),
            go_away("5.3s"),
        ]),
        Script::Answer(vec![en(" all night long."), th(" ทั้งคืน")]),
    ])
    .await;
    let (_s, mut events, file) = started(&endpoint, 200, Duration::from_millis(5)).await;
    let got = lines_until(&mut events, 1).await;
    remove(&file);
    // One line across both connections.
    assert_eq!(
        finals(&got),
        vec![pair(
            "We keep talking and talking all night long.",
            "เราคุยกันไปเรื่อย ๆ ทั้งคืน"
        )]
    );
    assert_eq!(opened(&got), 1);

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.setups.len(), 2);
    assert_eq!(
        seen.setups[1]["setup"]["sessionResumption"],
        json!({ "handle": "h-9" })
    );
    // 5.3 s left: it moves 0.3 s later, not at once and not at the close.
    let waited = seen.opened[1] - seen.go_away_at.unwrap();
    assert!(
        waited >= Duration::from_millis(250) && waited < Duration::from_secs(3),
        "{waited:?}"
    );
    assert_eq!(seen.audio.len(), 200 + 20);
    assert_eq!(seen.audio[..200], file_chunks(200)[..]);
}

#[tokio::test]
async fn dropped_connection_resumes_at_once_and_three_failures_show_the_card() {
    let refuse = || Script::Refuse(1011, "Internal error".into());
    let (endpoint, seen) = fake_live(vec![
        Script::Cut(vec![handle("h-5"), en("Hold the gate"), th("รักษาประตูไว้")]),
        refuse(),
        refuse(),
        refuse(),
        Script::Answer(vec![en(" until we are back."), th(" จนกว่าเราจะกลับมา")]),
    ])
    .await;
    let (s, mut events, file) = started(&endpoint, 50, Duration::from_millis(5)).await;
    let mut state = s.app.state.subscribe();
    let failed = tokio::time::timeout(
        Duration::from_secs(10),
        state.wait_for(|s| s.error.is_some()),
    )
    .await
    .expect("no error shown")
    .unwrap()
    .clone();
    let error = serde_json::to_value(failed.error.unwrap()).unwrap();
    assert_eq!(error["code"], "live_translate_failed");
    assert!(
        error["message"].as_str().unwrap().contains("1011"),
        "{error}"
    );
    assert_eq!(error["retry_s"], 5);
    {
        let seen = seen.lock().unwrap();
        // The cut, then three failed tries that each resumed with the handle.
        assert_eq!(seen.setups.len(), 4);
        for setup in &seen.setups[1..] {
            assert_eq!(
                setup["setup"]["sessionResumption"],
                json!({ "handle": "h-5" })
            );
        }
        let at_once = seen.opened[1] - seen.cut_at.unwrap();
        assert!(at_once < Duration::from_millis(500), "{at_once:?}");
        let gap = seen.opened[2] - seen.opened[1];
        assert!(gap >= Duration::from_millis(900), "{gap:?}");
    }

    // Tried again 5 s later (#43): it works, and translating goes on.
    tokio::time::timeout(
        Duration::from_secs(10),
        state.wait_for(|s| s.error.is_none() && s.status == yokoten::app::Status::Translating),
    )
    .await
    .expect("not back to translating")
    .unwrap();
    let got = lines_until(&mut events, 1).await;
    remove(&file);
    assert_eq!(
        finals(&got),
        vec![pair(
            "Hold the gate until we are back.",
            "รักษาประตูไว้ จนกว่าเราจะกลับมา"
        )]
    );
    assert_eq!(opened(&got), 1);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.setups.len(), 5);
    // After giving up on the handle the session starts afresh.
    assert_eq!(seen.setups[4]["setup"]["sessionResumption"], json!({}));
}

#[tokio::test]
async fn stop_mid_sentence_still_shows_the_last_sentence() {
    let (endpoint, seen) = fake_live(vec![Script::Pause(
        vec![en("We have to reach the tower"), th("เราต้องไปถึงหอคอย")],
        // What the service says once the silence before the end comes.
        vec![en(" before dark."), th(" ก่อนมืด"), en(""), th("")],
    )])
    .await;
    let (s, mut events, file) = started(&endpoint, 2000, Duration::from_millis(5)).await;
    // Mid-sentence: the line is open.
    let mut got = tokio::time::timeout(
        Duration::from_secs(10),
        read_until(&mut events, |e| !e.is_empty(), false),
    )
    .await
    .expect("no line");
    assert!(finals(&got).is_empty());
    let (_, stopped) = call(&s.router, Method::POST, "/api/stop", None).await;
    assert_eq!(stopped["status"], "idle");
    got.extend(lines_until(&mut events, 1).await);
    remove(&file);
    assert_eq!(
        finals(&got),
        vec![pair(
            "We have to reach the tower before dark.",
            "เราต้องไปถึงหอคอย ก่อนมืด"
        )]
    );
    assert_eq!(opened(&got), 1);

    // 2 s of silence went out, then audioStreamEnd.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !seen.lock().unwrap().ended && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let seen = seen.lock().unwrap().clone();
    assert!(seen.ended, "audioStreamEnd");
    let silent = |c: &Vec<u8>| c.iter().all(|b| *b == 0);
    let n = seen.audio.len();
    assert!(n > 20 && n < 2000 + 20, "{n}");
    assert!(seen.audio[n - 20..].iter().all(silent));
    assert!(!silent(&seen.audio[n - 21]));
    let (_, state) = call(&s.router, Method::GET, "/api/state", None).await;
    assert_eq!(state["status"], "idle", "{state}");
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
    let dir = say_dir();
    let mut pcm = say_pcm(&dir, text);
    pcm.extend(std::iter::repeat_n(0u8, 6 * 32_000));
    let path = dir.join("speech.wav");
    std::fs::write(&path, wav::encode(&pcm)).unwrap();
    path
}

fn say_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yokoten-say-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `text` spoken by macOS `say` as 16 kHz s16le mono PCM.
fn say_pcm(dir: &Path, text: &str) -> Vec<u8> {
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
    wav::pcm(&bytes).unwrap().to_vec()
}

/// `sentences` spoken in turn with a 1.5 s pause after each, over and over
/// until the file lasts `long`.
fn long_say_wav(sentences: &[&str], long: Duration) -> PathBuf {
    let dir = say_dir();
    let spoken: Vec<Vec<u8>> = sentences.iter().map(|s| say_pcm(&dir, s)).collect();
    let want = (long.as_secs_f64() * 32_000.0) as usize;
    let mut pcm = Vec::with_capacity(want);
    for s in spoken.iter().cycle() {
        if pcm.len() >= want {
            break;
        }
        pcm.extend_from_slice(s);
        pcm.extend(std::iter::repeat_n(0u8, 48_000));
    }
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

/// Run alone with `cargo test --test live real_live_translate_outlives_go_away
/// -- --ignored --nocapture`; it takes about 11 minutes of API time. Streams
/// `say` speech in real time past the end of one connection: the service
/// sends `goAway` at about 9:00 and closes at 9:50 (spike #13), so a line
/// that closes after 10:30 means the session moved to a new connection and
/// kept translating. Watch stderr for "server closes in".
#[tokio::test]
#[ignore = "calls the real Live Translate for about 11 minutes"]
async fn real_live_translate_outlives_go_away() {
    let key = real_key().expect("GEMINI_API_KEY not set and no .env found");
    let file = long_say_wav(
        &[
            "Paimon, wait for me at the gate of Mondstadt.",
            "The bridge is out, so we will have to swim across the river.",
            "Stay close to me and keep your sword ready.",
        ],
        Duration::from_secs(11 * 60),
    );
    let s = setup(
        Memory::with(&key),
        ENDPOINT,
        Some(file.clone()),
        Duration::from_millis(100),
    );
    audio_profile(&s).await;
    let mut events = request(&s.router, Method::GET, "/events", None)
        .await
        .into_body();
    let (status, body) = call(&s.router, Method::POST, "/api/start", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let started = Instant::now();
    let past_the_close = Duration::from_secs(10 * 60 + 30);
    let got = tokio::time::timeout(
        Duration::from_secs(12 * 60),
        read_until(
            &mut events,
            |e| {
                started.elapsed() >= past_the_close
                    && e.last().is_some_and(|(_, l)| l["final"] == true)
            },
            true,
        ),
    )
    .await;
    let (_, state) = call(&s.router, Method::GET, "/api/state", None).await;
    call(&s.router, Method::POST, "/api/stop", None).await;
    remove(&file);
    let got = got.unwrap_or_else(|_| panic!("no line after 10:30; state {state}"));
    println!(
        "lines {} after {:?}; state {state}",
        finals(&got).len(),
        started.elapsed()
    );
    assert_eq!(state["status"], "translating", "{state}");
    let (en, th) = finals(&got).pop().unwrap();
    assert!(!en.is_empty() && !en.contains(&key));
    assert!(th.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c)));
}
