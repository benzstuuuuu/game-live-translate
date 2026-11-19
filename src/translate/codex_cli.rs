//! Codex CLI (FR-TRN-15): lines go to a `codex app-server` kept open, with
//! the user's own login and no key. The process takes every line, on a new
//! thread every `Rotation::every` lines (see `codex_proc`). When it fails or
//! hangs it is dropped and a new one starts in the background, so the line
//! after goes to it. A process is used by one line at a time, and none is
//! left running once translating stops (`stop`).
//!
//! The prompt is the one Gemini sends (`crate::core::prompt`).

use super::claude_proc::Broke;
use super::codex_proc::{Proc, Spec};
use super::{Translator, Unavailable};
use crate::core::claude_cli::{check_reply_of, Rotation};
use crate::core::codex_cli::{self as core, EFFORT, MODELS};
use crate::core::gemini::Settings;
use crate::core::glossary::{self, Term};
use crate::core::prompt;
use crate::core::tokens::Usage;
use crate::settings;
use futures_util::future::BoxFuture;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::{watch, MutexGuard};
use tokio::task::JoinHandle;

/// The key in settings.json that holds `{ "model": … }`.
const SETTINGS_KEY: &str = "codex_cli";
/// How long `codex login status` may take before the check gives up asking.
const LOGIN_LIMIT: Duration = Duration::from_secs(10);
const STOPPED: &str = "หยุดแปลแล้ว";

/// What to run and how long to wait; tests point `program` at a fake app
/// server and `config` at a fake Codex config.
#[derive(Debug, Clone)]
pub struct Options {
    pub program: PathBuf,
    /// The user's Codex `config.toml`; only its MCP server names are read.
    pub config: PathBuf,
    pub rotation: Rotation,
    pub turn_limit: Duration,
    pub startup_limit: Duration,
}

impl Default for Options {
    fn default() -> Self {
        let home = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")));
        Self {
            program: "codex".into(),
            config: home.unwrap_or_default().join("config.toml"),
            rotation: Rotation::default(),
            turn_limit: core::TURN_LIMIT,
            startup_limit: core::STARTUP_LIMIT,
        }
    }
}

/// A cheap handle: the translator in `Config.translators` and the /setup
/// API (`crate::translation_api`) share one.
#[derive(Clone)]
pub struct CodexCli(Arc<Inner>);

struct Inner {
    /// Tone, extra prompt and context lines from /setup, read per line.
    settings: Box<dyn Fn() -> Settings + Send + Sync>,
    opts: Options,
    /// settings.json; None in tests that do not care.
    file: Option<PathBuf>,
    /// The model picked on /setup.
    model: RwLock<String>,
    slots: tokio::sync::Mutex<Slots>,
    /// A session is on: keep a process ready between lines.
    running: AtomicBool,
    /// Bumped by `stop`; a line or start in progress gives up when it moves.
    stops: watch::Sender<u64>,
    /// Why Codex was refused (a tool on); stays until the app restarts.
    refused: Mutex<Option<String>>,
    /// The model Codex last said it runs, for `lines.model_id`.
    seen_model: Mutex<Option<String>>,
    /// Tokens of every turn since the app opened (FR-TRN-10).
    usage: Mutex<Usage>,
}

#[derive(Default)]
struct Slots {
    active: Option<Proc>,
    /// A process starting in the background; dropping it kills that process.
    standby: Option<Standby>,
}

struct Standby(JoinHandle<Result<Proc, Broke>>);

impl Drop for Standby {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl CodexCli {
    /// The model comes from `file` (settings.json) when it holds a valid one.
    pub fn new(
        settings: impl Fn() -> Settings + Send + Sync + 'static,
        opts: Options,
        file: Option<PathBuf>,
    ) -> Self {
        let stored = file.as_deref().map(settings::read).and_then(|all| {
            let raw = all.get(SETTINGS_KEY)?.get("model")?.as_str()?.to_owned();
            core::model(&raw).ok()
        });
        Self(Arc::new(Inner {
            settings: Box::new(settings),
            opts,
            file,
            model: RwLock::new(stored.unwrap_or_else(|| MODELS[0].into())),
            slots: Default::default(),
            running: AtomicBool::new(false),
            stops: watch::channel(0).0,
            refused: Mutex::default(),
            seen_model: Mutex::default(),
            usage: Mutex::default(),
        }))
    }

    /// The model in use.
    pub fn model(&self) -> String {
        self.0
            .model
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Picks the model (from /setup) and saves it; the next line opens a
    /// new thread with it in the same process.
    pub fn set_model(&self, raw: &str) -> Result<String, String> {
        let model = core::model(raw)?;
        if let Some(path) = &self.0.file {
            let values = Map::from_iter([(SETTINGS_KEY.into(), json!({ "model": model }))]);
            settings::save_values(path, values).map_err(|e| format!("บันทึกค่าตั้งไม่ได้: {e}"))?;
        }
        *self.0.model.write().unwrap_or_else(|e| e.into_inner()) = model.clone();
        Ok(model)
    }

    /// What GET /api/codex-cli shows: the model, the choices, the effort,
    /// and whether Codex can translate now (with the fix when it cannot).
    pub async fn view(&self) -> Value {
        let mut v =
            json!({ "model": self.model(), "models": MODELS, "effort": EFFORT, "ready": true });
        if let Err(e) = self.check_cli().await {
            v["ready"] = false.into();
            v["message"] = e.message.into();
            v["fix"] = e.fix.into();
        }
        v
    }

    fn spec(&self, s: &Settings) -> Spec {
        Spec {
            model: self.model(),
            system: prompt::system(s),
        }
    }

    fn refusal(&self) -> Option<String> {
        lock(&self.0.refused).clone()
    }

    fn running(&self) -> bool {
        self.0.running.load(Ordering::SeqCst)
    }

    /// Only the glossary terms the line uses go in the prompt (FR-GLS-04).
    async fn translate_line(
        &self,
        text: &str,
        earlier: &[String],
        glossary: &[Term],
    ) -> Result<String, String> {
        let s = (self.0.settings)();
        let terms = glossary::used(text, glossary);
        let user = prompt::user(text, earlier, &terms, &s);
        let reply = self.ask(&user, self.spec(&s)).await?;
        check_reply_of("Codex", text, &reply)
    }

    /// One turn on the active process, started first when there is none.
    async fn ask(&self, user: &str, spec: Spec) -> Result<String, String> {
        if let Some(why) = self.refusal() {
            return Err(why);
        }
        let mut stops = self.0.stops.subscribe();
        let gen = *stops.borrow_and_update();
        let mut line = Line {
            cli: self,
            slots: self.0.slots.lock().await,
            gen,
            ok: false,
        };
        let mut proc = match line.slots.active.take() {
            Some(p) => p,
            None => tokio::select! {
                next = self.next(&mut line.slots, spec.clone()) => next.map_err(|b| self.broke(b))?,
                _ = stops.changed() => return Err(STOPPED.into()),
            },
        };
        let limit = self.0.opts.turn_limit;
        let outcome = tokio::select! {
            r = tokio::time::timeout(limit, proc.turn(user, &spec)) => r,
            _ = stops.changed() => return Err(STOPPED.into()),
        };
        {
            let mut usage = lock(&self.0.usage);
            *usage = *usage + proc.take_usage();
        }
        match outcome {
            Err(_) => Err(format!("Codex ไม่ตอบภายใน {} วินาที", limit.as_secs_f32())),
            Ok(Err(broke)) => Err(self.broke(broke)),
            Ok(Ok(reply)) => {
                *lock(&self.0.seen_model) = proc.model();
                line.slots.active = Some(proc);
                line.ok = true;
                reply
            }
        }
    }

    /// The process for the next line: the standby (waiting for it to
    /// finish starting), else a new one.
    async fn next(&self, slots: &mut Slots, spec: Spec) -> Result<Proc, Broke> {
        if let Some(standby) = slots.standby.as_mut() {
            // Borrowed, not taken: a line given up meanwhile leaves it here.
            let started = (&mut standby.0).await;
            slots.standby = None;
            if let Ok(done) = started {
                return done;
            }
        }
        start(&self.0.opts, spec).await
    }

    /// The reason for a broken process; a refusal also stops any new one.
    fn broke(&self, b: Broke) -> String {
        match b {
            Broke::Refused(why) => {
                eprintln!("codex-cli: {why}");
                *lock(&self.0.refused) = Some(why.clone());
                why
            }
            Broke::Gone(why) => why,
        }
    }

    /// Starts a process in the background unless one is there, no session
    /// is on, or Codex was refused.
    fn start_standby(&self, slots: &mut Slots) {
        let there = slots.active.is_some() || slots.standby.is_some();
        if there || !self.running() || self.refusal().is_some() {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let spec = self.spec(&(self.0.settings)());
        let opts = self.0.opts.clone();
        slots.standby = Some(Standby(rt.spawn(async move { start(&opts, spec).await })));
    }

    /// Is `codex` there and logged in? Ok when `login status` says nothing
    /// clear: the first line will tell.
    async fn check_cli(&self) -> Result<(), Unavailable> {
        if let Some(why) = self.refusal() {
            return Err(Unavailable {
                message: why,
                fix: core::REFUSED_FIX.into(),
            });
        }
        let mut cmd = tokio::process::Command::new(&self.0.opts.program);
        cmd.args(["login", "status"])
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let unavailable = |(message, fix): (&str, &str)| Unavailable {
            message: message.into(),
            fix: fix.into(),
        };
        match tokio::time::timeout(LOGIN_LIMIT, cmd.output()).await {
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(unavailable(core::not_found()))
            }
            Ok(Err(e)) => Err(Unavailable {
                message: format!("เปิด Codex ไม่ได้: {e}"),
                fix: core::not_found().1.into(),
            }),
            Ok(Ok(out)) => {
                let said = [out.stdout, out.stderr].concat();
                match core::logged_in(out.status.success(), &String::from_utf8_lossy(&said)) {
                    Some(false) => Err(unavailable(core::not_logged_in())),
                    _ => Ok(()),
                }
            }
            Err(_) => Ok(()),
        }
    }
}

/// A new process within the startup limit, with the MCP servers named in
/// the user's Codex config turned off.
async fn start(opts: &Options, spec: Spec) -> Result<Proc, Broke> {
    let config = std::fs::read_to_string(&opts.config).unwrap_or_default();
    let mcp = core::mcp_servers(&config);
    let starting = Proc::start(&opts.program, &mcp, spec, opts.rotation);
    match tokio::time::timeout(opts.startup_limit, starting).await {
        Ok(started) => started,
        Err(_) => Err(Broke::Gone(format!(
            "Codex เปิดไม่เสร็จภายใน {} วินาที",
            opts.startup_limit.as_secs()
        ))),
    }
}

/// One line holding the processes. However it ends, even dropped by a
/// timeout midway, it leaves them right: none at all once translating
/// stopped, and a new one starting when this line lost its process.
struct Line<'a> {
    cli: &'a CodexCli,
    slots: MutexGuard<'a, Slots>,
    gen: u64,
    ok: bool,
}

impl Drop for Line<'_> {
    fn drop(&mut self) {
        let stopped = *self.cli.0.stops.borrow() != self.gen;
        if stopped || !self.cli.running() {
            self.slots.active = None;
            self.slots.standby = None;
        } else if !self.ok {
            self.cli.start_standby(&mut self.slots);
        }
    }
}

impl Translator for CodexCli {
    fn engine(&self) -> &'static str {
        "codex-cli"
    }

    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(self.translate_line(text, &[], &[]))
    }

    fn translate_in_context<'a>(
        &'a self,
        text: &'a str,
        earlier: &'a [String],
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(self.translate_line(text, earlier, &[]))
    }

    fn translate_with_glossary<'a>(
        &'a self,
        text: &'a str,
        earlier: &'a [String],
        glossary: &'a [Term],
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(self.translate_line(text, earlier, glossary))
    }

    fn check(&self) -> BoxFuture<'_, Result<(), Unavailable>> {
        Box::pin(self.check_cli())
    }

    fn model_id(&self) -> Option<String> {
        lock(&self.0.seen_model)
            .clone()
            .or_else(|| Some(self.model()))
    }

    fn usage(&self) -> Usage {
        *lock(&self.0.usage)
    }

    /// Starts a process now so the session's first line does not wait.
    fn warm(&self) {
        self.0.running.store(true, Ordering::SeqCst);
        if let Ok(mut slots) = self.0.slots.try_lock() {
            self.start_standby(&mut slots);
        }
    }

    /// Kills every process: idle ones now, one in a line as that line sees
    /// the stop.
    fn stop(&self) {
        self.0.running.store(false, Ordering::SeqCst);
        self.0.stops.send_modify(|g| *g += 1);
        if let Ok(mut slots) = self.0.slots.try_lock() {
            *slots = Slots::default();
        }
    }
}
