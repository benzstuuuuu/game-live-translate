//! The audio path (FR-AUD-04, 06, 07, FR-GLS-06): audio chunks go to Live
//! Translate over a WebSocket; the English and Thai pieces that come back
//! become lines on /reader. Audio profiles always use Live Translate,
//! whatever translator is picked, with no backup.
//!
//! The key comes from the same store as Gemini's and travels only in the
//! `x-goog-api-key` header of the upgrade request (NFR-07); every reason that
//! is shown or logged has it blanked out.
//!
//! A session outlives one connection (FR-AUD-07): `core::reconnect` says
//! when to move to a new one and when to give up; audio heard in between
//! waits and goes out on the new connection, and the splitter carries on, so
//! no line is lost or shown twice. Stopping sends 2 s of silence and
//! `audioStreamEnd`, then waits a little for the last sentence.

use crate::app::{App, Push};
use crate::audio::{AudioSource, Card};
use crate::bar::Hider;
use crate::core::audio_lines::{Splitter, Step, Text};
use crate::core::error::{AppError, ErrorCode};
use crate::core::gemini::redact;
use crate::core::live::{self as core, Event};
use crate::core::reconnect::{Backlog, Ending, Next, Reconnect, CONNECT_LIMIT};
use crate::core::recovery::{retry_for, Outage, RECONNECT_EVERY};
use crate::db::{self, Line};
use crate::pipeline::Session;
use crate::translate::{Gemini, Unavailable};
use futures_util::{SinkExt, StreamExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_websockets::{ClientBuilder, MaybeTlsStream, Message, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// How to reach Live Translate and where audio comes from.
#[derive(Clone)]
pub struct Link {
    /// BidiGenerateContent; tests point it at a local fake server.
    pub endpoint: String,
    /// The key (Keychain) and the tone and extra prompt of /setup.
    pub gemini: Gemini,
    /// `--audio <file.wav>` for dev and tests, heard instead of the card.
    pub audio_file: Option<PathBuf>,
    /// The capture card's audio input (#70).
    pub card: Option<Arc<dyn Card>>,
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
            card: None,
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

/// One audio session: runs until the audio runs out or `stop` fires, over
/// as many connections as it takes. Failing 3 tries in a row lands in
/// `AppState.error` and is tried again by itself (#43).
pub async fn run(
    app: Arc<App>,
    link: Link,
    source: Box<dyn AudioSource>,
    session: Session,
    instruction: String,
    stop: oneshot::Receiver<()>,
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
    let mut stream = Stream {
        app: &app,
        link: &link,
        key: &key,
        instruction: &instruction,
        audio: Audio::start(source, link.chunk_interval),
        stop: Some(stop),
        plan: Reconnect::default(),
        backlog: Backlog::default(),
        outage: Outage::default(),
        ending: None,
        why: None,
    };
    let result = stream.run(&mut lines).await;
    let stopped = stream.stop.is_none();
    lines.close();
    if let Err(e) = result {
        let e = stream.redacted(e);
        eprintln!("live translate: {}", e.message);
        if !stopped {
            app.fail(e);
        }
    }
}

/// Reads the source in a task of its own, so audio keeps coming while a
/// connection opens.
struct Audio {
    rx: mpsc::UnboundedReceiver<Result<Vec<u8>, AppError>>,
    task: JoinHandle<()>,
    /// The source ran out or was closed, and everything read was taken.
    done: bool,
}

impl Audio {
    fn start(mut source: Box<dyn AudioSource>, every: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(every);
            loop {
                tick.tick().await;
                let chunk = source.next_chunk().await;
                let more = matches!(chunk, Ok(Some(_)));
                if let Some(chunk) = chunk.transpose() {
                    if tx.send(chunk).is_err() {
                        return;
                    }
                }
                if !more {
                    return;
                }
            }
        });
        Self {
            rx,
            task,
            done: false,
        }
    }

    /// Stops reading; what was read still comes out of `rx`.
    fn close(&mut self) {
        self.task.abort();
        self.rx.close();
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A session's state across its connections.
struct Stream<'a> {
    app: &'a App,
    link: &'a Link,
    key: &'a str,
    instruction: &'a str,
    audio: Audio,
    /// None once the player stopped the session.
    stop: Option<oneshot::Receiver<()>>,
    plan: Reconnect,
    /// Audio not sent yet: heard between connections or before one is ready.
    backlog: Backlog,
    outage: Outage,
    /// Set once the audio ran out or the player stopped.
    ending: Option<Ending>,
    /// Why the last connection ended or could not open.
    why: Option<AppError>,
}

impl Stream<'_> {
    async fn run(&mut self, lines: &mut Lines<'_>) -> Result<(), AppError> {
        loop {
            // Stopping while a connection opens frees the card at once.
            let setup = core::setup_message(self.instruction, self.plan.handle());
            let opened = tokio::select! {
                r = open(&self.link.endpoint, self.key, setup) => r,
                _ = stopped(&mut self.stop), if self.stop.is_some() => {
                    self.stop = None;
                    return Ok(());
                }
            };
            let next = match opened {
                Ok(ws) => match self.connection(ws, lines).await? {
                    Some(next) => next,
                    None => return Ok(()),
                },
                Err(e) => {
                    eprintln!("live translate: {}", self.redacted(e.clone()).message);
                    self.why = Some(e);
                    self.plan.lost(false)
                }
            };
            let delay = match next {
                Next::Stay | Next::Reconnect => continue,
                Next::RetryIn(delay) => delay,
                Next::Fail => {
                    let delay = retry_for(ErrorCode::LiveTranslateFailed)
                        .delay()
                        .unwrap_or(RECONNECT_EVERY);
                    let why = self.why.take().unwrap_or_else(closed);
                    if let Some(shown) = self.outage.fail(why.with_retry(delay)) {
                        self.app.fail(self.redacted(shown));
                    }
                    delay
                }
            };
            if !self.wait(delay).await? {
                return Ok(());
            }
        }
    }

    /// Waits `delay` between tries, keeping the audio; false if the player
    /// stopped meanwhile.
    async fn wait(&mut self, delay: Duration) -> Result<bool, AppError> {
        let until = tokio::time::sleep(delay);
        tokio::pin!(until);
        loop {
            tokio::select! {
                _ = &mut until => return Ok(true),
                _ = stopped(&mut self.stop), if self.stop.is_some() => {
                    self.stop = None;
                    return Ok(false);
                }
                chunk = self.audio.rx.recv(), if !self.audio.done => self.heard(chunk)?,
            }
        }
    }

    /// One connection, from its setup until it ends: Some(next) when the
    /// session goes on over another one, None when the session is over.
    async fn connection(
        &mut self,
        mut ws: Socket,
        lines: &mut Lines<'_>,
    ) -> Result<Option<Next>, AppError> {
        let mut ready = false;
        let mut pace = tokio::time::interval(self.link.chunk_interval);
        loop {
            let wake = [
                self.plan.wake_at(),
                self.ending.as_ref().and_then(Ending::give_up_at),
            ]
            .into_iter()
            .flatten()
            .min();
            let silence_due = ready
                && self.backlog.is_empty()
                && self.ending.as_ref().is_some_and(Ending::sending);
            tokio::select! {
                chunk = self.audio.rx.recv(), if !self.audio.done => {
                    self.heard(chunk)?;
                    if ready {
                        if let Err(e) = self.flush(&mut ws).await {
                            return Ok(self.lost(true, e));
                        }
                    }
                }
                _ = stopped(&mut self.stop), if self.stop.is_some() => {
                    self.stop = None;
                    if !ready {
                        return Ok(None);
                    }
                    // What was read still goes out, then the silence.
                    self.audio.close();
                }
                _ = pace.tick(), if silence_due => {
                    let ending = self.ending.as_mut().expect("ending");
                    let message = match ending.silence() {
                        Some(pcm) => core::audio_message(&pcm),
                        None => {
                            ending.ended(Instant::now());
                            core::audio_end_message()
                        }
                    };
                    if let Err(e) = send(&mut ws, message).await {
                        return Ok(self.lost(true, e));
                    }
                    if self.over(lines) {
                        close(ws).await;
                        return Ok(None);
                    }
                }
                _ = sleep_until(wake), if wake.is_some() => {
                    if self.over(lines) {
                        close(ws).await;
                        return Ok(None);
                    }
                    if self.ending.is_none() && self.plan.tick(Instant::now()) == Next::Reconnect {
                        close(ws).await;
                        return Ok(Some(Next::Reconnect));
                    }
                }
                msg = ws.next() => {
                    let msg = match msg {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => return Ok(self.lost(ready, dropped(&e))),
                        None => return Ok(self.lost(ready, closed())),
                    };
                    if let Some((code, reason)) = msg.as_close() {
                        let why = core::close_reason(code.into(), reason)
                            .map_or_else(closed, AppError::live_translate_failed);
                        return Ok(self.lost(ready, why));
                    }
                    let text = msg.as_text().map(str::to_owned).or_else(|| {
                        msg.is_binary().then(|| String::from_utf8_lossy(msg.as_payload()).into_owned())
                    });
                    for event in core::parse(&text.unwrap_or_default()) {
                        let next = match event {
                            Event::Ready => {
                                ready = true;
                                self.plan.ready();
                                if self.outage.frame() {
                                    self.app.resume();
                                }
                                if let Err(e) = self.flush(&mut ws).await {
                                    return Ok(self.lost(true, e));
                                }
                                Next::Stay
                            }
                            Event::En(piece) => {
                                lines.en(&piece);
                                Next::Stay
                            }
                            Event::Th(piece) => {
                                lines.th(&piece);
                                if piece.trim().is_empty() {
                                    self.plan.silence()
                                } else {
                                    Next::Stay
                                }
                            }
                            Event::Usage(u) => {
                                self.app.add_live_usage(u);
                                Next::Stay
                            }
                            Event::GoAway(left) => {
                                eprintln!("live translate: server closes in {left:?}");
                                self.plan.go_away(Instant::now(), left)
                            }
                            Event::Handle(handle) => {
                                self.plan.new_handle(handle);
                                Next::Stay
                            }
                        };
                        if next == Next::Reconnect && self.ending.is_none() {
                            close(ws).await;
                            return Ok(Some(Next::Reconnect));
                        }
                    }
                    if self.over(lines) {
                        close(ws).await;
                        return Ok(None);
                    }
                }
            }
        }
    }

    /// A chunk from the source, or its end.
    fn heard(&mut self, chunk: Option<Result<Vec<u8>, AppError>>) -> Result<(), AppError> {
        match chunk {
            Some(pcm) => self.backlog.push(pcm?),
            None => {
                self.audio.done = true;
                self.ending = Some(Ending::default());
            }
        }
        Ok(())
    }

    /// Sends the waiting audio, oldest first.
    async fn flush(&mut self, ws: &mut Socket) -> Result<(), AppError> {
        while let Some(pcm) = self.backlog.front() {
            send(ws, core::audio_message(pcm)).await?;
            self.backlog.pop();
        }
        Ok(())
    }

    /// The end went out and the last sentence is in, or it waited enough.
    fn over(&self, lines: &Lines<'_>) -> bool {
        self.ending
            .as_ref()
            .is_some_and(|e| e.done(Instant::now(), lines.open.is_some()))
    }

    /// The connection ended by itself because of `why`. A session that was
    /// already ending is over; otherwise `plan` says what next.
    fn lost(&mut self, ready: bool, why: AppError) -> Option<Next> {
        if ready && self.ending.is_some() {
            return None;
        }
        eprintln!("live translate: {}", self.redacted(why.clone()).message);
        self.why = Some(why);
        Some(self.plan.lost(ready))
    }

    fn redacted(&self, e: AppError) -> AppError {
        AppError {
            message: redact(&e.message, self.key),
            ..e
        }
    }
}

/// A new connection with `setup` sent (resuming when it holds a handle).
async fn open(endpoint: &str, key: &str, setup: serde_json::Value) -> Result<Socket, AppError> {
    let opening = async {
        let mut ws = connect(endpoint, key).await?;
        send(&mut ws, setup).await?;
        Ok(ws)
    };
    tokio::time::timeout(CONNECT_LIMIT, opening)
        .await
        .unwrap_or_else(|_| {
            Err(AppError::live_translate_failed(
                "Live Translate ไม่ตอบ".into(),
            ))
        })
}

/// Resolves once the player stops, or the app dropped the sender.
async fn stopped(stop: &mut Option<oneshot::Receiver<()>>) {
    match stop {
        Some(rx) => {
            let _ = rx.await;
        }
        None => std::future::pending().await,
    }
}

async fn sleep_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at.into()).await,
        None => std::future::pending().await,
    }
}

/// Closes a connection Yokoten is done with, without waiting long for the
/// server's answer.
async fn close(mut ws: Socket) {
    let _ = tokio::time::timeout(Duration::from_secs(1), ws.close()).await;
}

fn closed() -> AppError {
    AppError::live_translate_failed("Live Translate ปิดการเชื่อมต่อ".into())
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
            bar: Hider::spawn(app, &session.session_id),
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
