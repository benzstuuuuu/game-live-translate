//! The audio path (FR-AUD-04, 06, FR-GLS-06): audio chunks go to Live
//! Translate over a WebSocket; the English and Thai pieces that come back
//! become lines on /reader. Audio profiles always use Live Translate,
//! whatever translator is picked, with no backup.
//!
//! The key comes from the same store as Gemini's and travels only in the
//! `x-goog-api-key` header of the upgrade request (NFR-07); every reason that
//! is shown or logged has it blanked out.
//!
//! Not here yet: reading the card (#70), and GoAway, reconnecting and
//! silence before stopping (#71).

use crate::app::{App, Push};
use crate::audio::AudioSource;
use crate::bar::Hider;
use crate::core::audio_lines::{Splitter, Step, Text};
use crate::core::error::AppError;
use crate::core::gemini::redact;
use crate::core::live::{self as core, Event};
use crate::db::{self, Line};
use crate::pipeline::Session;
use crate::translate::{Gemini, Unavailable};
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_websockets::{ClientBuilder, MaybeTlsStream, Message, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How to reach Live Translate and where audio comes from.
#[derive(Clone)]
pub struct Link {
    /// BidiGenerateContent; tests point it at a local fake server.
    pub endpoint: String,
    /// The key (Keychain) and the tone and extra prompt of /setup.
    pub gemini: Gemini,
    /// `--audio <file.wav>` for dev and tests; the card comes in #70.
    pub audio_file: Option<PathBuf>,
    /// Pause between 100 ms chunks: real time in the app, faster in tests.
    pub chunk_interval: Duration,
}

impl Link {
    /// The real service, sending audio in real time.
    pub fn new(gemini: Gemini, audio_file: Option<PathBuf>) -> Self {
        Self {
            endpoint: core::ENDPOINT.into(),
            gemini,
            audio_file,
            chunk_interval: Duration::from_millis(100),
        }
    }

    /// Can an audio session start? Only a saved key is needed; whether it
    /// works shows when the session opens.
    pub async fn check(&self) -> Result<(), Unavailable> {
        match self.gemini.key().await {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(Unavailable {
                message: "ยังไม่ได้ใส่ Gemini API key โปรไฟล์เสียงใช้ Live Translate ของ Gemini".into(),
                fix: "เปิด /setup บน Mac ใส่ API key ในส่วนแปลภาษาแล้วกดบันทึก หรือเปลี่ยนแหล่งข้อความของโปรไฟล์เป็น subtitle"
                    .into(),
            }),
            Err(message) => Err(Unavailable {
                message,
                fix: "เปิด Keychain Access แล้วอนุญาตให้ Yokoten อ่าน key หรือบันทึก key ใหม่ใน /setup"
                    .into(),
            }),
        }
    }

    /// The system instruction for a session of `profile_id`: its glossary
    /// (both layers) as it is now, with the tone and extra prompt. Terms
    /// changed later apply from the next session (FR-GLS-06).
    pub fn instruction(&self, app: &App, profile_id: &str) -> String {
        let terms = crate::glossary::terms(&app.db(), profile_id).unwrap_or_else(|e| {
            eprintln!("glossary: {e}");
            Vec::new()
        });
        let s = self.gemini.settings();
        core::system_instruction(&terms, s.tone, &s.extra_prompt)
    }
}

/// One audio session: runs until the server closes it or the app stops the
/// task. A refusal or a dropped link lands in `AppState.error`.
pub async fn run(
    app: Arc<App>,
    link: Link,
    source: Box<dyn AudioSource>,
    session: Session,
    instruction: String,
) {
    let key = match link.gemini.key().await {
        Ok(Some(key)) => key,
        Ok(None) => {
            let reason = link.check().await.err().map(|e| e.message);
            app.fail(AppError::live_translate_failed(reason.unwrap_or_default()));
            return;
        }
        Err(reason) => {
            app.fail(AppError::live_translate_failed(reason));
            return;
        }
    };
    let mut lines = Lines::new(&app, &session);
    let result = stream(&app, &link, &key, source, &instruction, &mut lines).await;
    lines.close();
    if let Err(e) = result {
        let e = AppError {
            message: redact(&e.message, &key),
            ..e
        };
        eprintln!("live translate: {}", e.message);
        app.fail(e);
    }
}

/// Sends the setup, then audio once the server is ready, and turns what
/// comes back into lines.
async fn stream(
    app: &App,
    link: &Link,
    key: &str,
    mut source: Box<dyn AudioSource>,
    instruction: &str,
    lines: &mut Lines<'_>,
) -> Result<(), AppError> {
    let mut ws = connect(&link.endpoint, key).await?;
    send(&mut ws, core::setup_message(instruction)).await?;
    let mut tick = tokio::time::interval(link.chunk_interval);
    let (mut ready, mut sending) = (false, true);
    loop {
        tokio::select! {
            _ = tick.tick(), if ready && sending => match source.next_chunk().await? {
                Some(pcm) => send(&mut ws, core::audio_message(&pcm)).await?,
                None => {
                    sending = false;
                    send(&mut ws, core::audio_end_message()).await?;
                }
            },
            msg = ws.next() => {
                let msg = match msg {
                    None => return Ok(()),
                    Some(Err(e)) => return Err(dropped(&e)),
                    Some(Ok(m)) => m,
                };
                if let Some((code, reason)) = msg.as_close() {
                    return match core::close_reason(code.into(), reason) {
                        Some(why) => Err(AppError::live_translate_failed(why)),
                        None => Ok(()),
                    };
                }
                let text = msg.as_text().map(str::to_owned).or_else(|| {
                    msg.is_binary().then(|| String::from_utf8_lossy(msg.as_payload()).into_owned())
                });
                for event in core::parse(&text.unwrap_or_default()) {
                    match event {
                        Event::Ready => ready = true,
                        Event::En(piece) => lines.en(&piece),
                        Event::Th(piece) => lines.th(&piece),
                        Event::Usage(u) => app.add_live_usage(u),
                        Event::GoAway => eprintln!("live translate: server will close soon"),
                    }
                }
            }
        }
    }
}

async fn connect(endpoint: &str, key: &str) -> Result<Socket, AppError> {
    let header = key
        .parse()
        .map_err(|_| AppError::live_translate_failed("API key มีตัวอักษรที่ใช้ไม่ได้".into()))?;
    let builder = ClientBuilder::new()
        .uri(endpoint)
        .map_err(|_| AppError::live_translate_failed("ที่อยู่ Live Translate ไม่ถูกต้อง".into()))?
        .add_header("x-goog-api-key".parse().expect("valid header name"), header)
        .map_err(|e| dropped(&e))?;
    let (ws, _) = builder.connect().await.map_err(|e| dropped(&e))?;
    Ok(ws)
}

async fn send(ws: &mut Socket, message: serde_json::Value) -> Result<(), AppError> {
    ws.send(Message::text(message.to_string()))
        .await
        .map_err(|e| dropped(&e))
}

fn dropped(e: &tokio_websockets::Error) -> AppError {
    AppError::live_translate_failed(format!("ต่อ Live Translate ไม่ได้ ({e})"))
}

/// The session's lines: the splitter's steps saved and pushed to /events.
struct Lines<'a> {
    app: &'a App,
    session: &'a Session,
    splitter: Splitter,
    /// The line still being heard; `final: false` until it closes.
    open: Option<Line>,
    /// Hides /overlay's bar once speech stops (FR-AUD-06).
    bar: Hider,
}

impl<'a> Lines<'a> {
    fn new(app: &'a App, session: &'a Session) -> Self {
        Self {
            app,
            session,
            splitter: Splitter::default(),
            open: None,
            bar: Hider::spawn(app.events.clone()),
        }
    }

    fn en(&mut self, piece: &str) {
        let steps = self.splitter.en(piece);
        self.apply(steps);
    }

    fn th(&mut self, piece: &str) {
        let steps = self.splitter.th(piece);
        self.apply(steps);
    }

    /// Closes the line still open when the session ends.
    fn close(&mut self) {
        let step = self.splitter.close();
        self.apply(step.into_iter().collect());
    }

    fn apply(&mut self, steps: Vec<Step>) {
        for step in steps {
            match step {
                Step::Open(text) => self.show(text, false),
                Step::Close(text) => self.show(text, true),
            }
        }
    }

    /// A new line goes out as `line`, changes to it as `line-updated`.
    fn show(&mut self, text: Text, done: bool) {
        let (line, push) = match self.open.take() {
            None => {
                let line = self.new_line(text, done);
                if let Err(e) = db::insert_line(&self.app.db(), &line) {
                    eprintln!("save line: {e}");
                }
                (line.clone(), Push::Line(line))
            }
            Some(mut line) => {
                line.source_text = text.en;
                line.target_text = text.th;
                line.done = done;
                if let Err(e) = db::update_line(&self.app.db(), &line) {
                    eprintln!("update line: {e}");
                }
                (line.clone(), Push::LineUpdated(line))
            }
        };
        let _ = self.app.events.send(push);
        self.bar.line(done);
        if !done {
            self.open = Some(line);
        }
    }

    fn new_line(&self, text: Text, done: bool) -> Line {
        Line {
            id: uuid::Uuid::new_v4().to_string(),
            profile_id: self.session.profile_id.clone(),
            session_id: self.session.session_id.clone(),
            source: "audio".into(),
            source_text: text.en,
            target_text: text.th,
            engine: core::ENGINE.into(),
            is_fallback: false,
            model_id: Some(core::MODEL.into()),
            done,
            created_at: db::now_ms(),
            ocr_ms: None,
            translate_ms: None,
        }
    }
}
