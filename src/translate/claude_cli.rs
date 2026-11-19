//! Claude Code CLI (FR-TRN-15): lines go to a `claude -p` process kept
//! open, with the user's own login and no key. One process translates while
//! a standby starts ahead of its turn: after `Rotation::every` lines, when a
//! line fails, or when the session starts. A process is used by one line at
//! a time, and none is left running once translating stops (`stop`).
//!
//! The prompt is the one Gemini sends (`crate::core::prompt`).

use super::claude_proc::{Broke, Proc, Spec};
use super::{Translator, Unavailable};
use crate::core::claude_cli::{self as core, Rotation, MODELS};
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
const SETTINGS_KEY: &str = "claude_cli";
/// How long `claude auth status` may take before the check gives up asking.
const AUTH_LIMIT: Duration = Duration::from_secs(10);
const STOPPED: &str = "หยุดแปลแล้ว";

/// What to run and how long to wait; tests point `program` at a fake CLI.
#[derive(Debug, Clone)]
pub struct Options {
    pub program: PathBuf,
    pub rotation: Rotation,
    pub turn_limit: Duration,
    pub startup_limit: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            program: "claude".into(),
            rotation: Rotation::default(),
            turn_limit: core::TURN_LIMIT,
            startup_limit: core::STARTUP_LIMIT,
        }
    }
}

/// A cheap handle: the translator in `Config.translators` and the /setup
/// API (`crate::translation_api`) share one.
#[derive(Clone)]
pub struct ClaudeCli(Arc<Inner>);

struct Inner {
    /// Tone, extra prompt and context lines from /setup, read per line.
    settings: Box<dyn Fn() -> Settings + Send + Sync>,
    opts: Options,
    /// settings.json; None in tests that do not care.
    file: Option<PathBuf>,
    /// The `--model` alias picked on /setup.
    model: RwLock<String>,
    slots: tokio::sync::Mutex<Slots>,
    /// A session is on: keep a process ready between lines.
    running: AtomicBool,
    /// Bumped by `stop`; a line or start in progress gives up when it moves.
    stops: watch::Sender<u64>,
    /// Why the CLI was refused (tools on); stays until the app restarts.
    refused: Mutex<Option<String>>,
    /// The model the CLI last said it runs, for `lines.model_id`.
    seen_model: Mutex<Option<String>>,
    /// Tokens of every turn since the app opened (FR-TRN-10).
    usage: Mutex<Usage>,
}

#[derive(Default)]
struct Slots {
    active: Option<Proc>,
    standby: Option<Standby>,
}

/// A process starting in the background; dropping it kills that process.
struct Standby {
    spec: Spec,
    task: JoinHandle<Result<Proc, Broke>>,
}

impl Drop for Standby {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ClaudeCli {
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

    /// The `--model` alias in use.
    pub fn model(&self) -> String {
        self.0
            .model
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Picks the model (from /setup) and saves it; the next line starts a
    /// process with it.
    pub fn set_model(&self, raw: &str) -> Result<String, String> {
        let model = core::model(raw)?;
        if let Some(path) = &self.0.file {
            let values = Map::from_iter([(SETTINGS_KEY.into(), json!({ "model": model }))]);
            settings::save_values(path, values).map_err(|e| format!("บันทึกค่าตั้งไม่ได้: {e}"))?;
        }
        *self.0.model.write().unwrap_or_else(|e| e.into_inner()) = model.clone();
        if let Ok(mut slots) = self.0.slots.try_lock() {
            slots.standby = None;
            self.start_standby(&mut slots);
        }
        Ok(model)
    }

    /// What GET /api/claude-cli shows: the model, the choices, and whether
    /// the CLI can translate now (with the fix when it cannot).
    pub async fn view(&self) -> Value {
        let mut v = json!({ "model": self.model(), "models": MODELS, "ready": true });
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
        core::check_reply(text, &reply)
    }

    /// One turn on the active process, which is replaced first when it is
    /// due to retire or was started with other settings.
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
        let rotation = self.0.opts.rotation;
        let current = line.slots.active.take();
        let mut proc = match current.filter(|p| p.spec == spec && !rotation.retire(p.lines)) {
            Some(p) => p,
            None => tokio::select! {
                next = self.next(&mut line.slots, spec) => next.map_err(|b| self.broke(b))?,
                _ = stops.changed() => return Err(STOPPED.into()),
            },
        };
        let limit = self.0.opts.turn_limit;
        let outcome = tokio::select! {
            r = tokio::time::timeout(limit, proc.turn(user)) => r,
            _ = stops.changed() => return Err(STOPPED.into()),
        };
        {
            let mut usage = lock(&self.0.usage);
            *usage = *usage + proc.take_usage();
        }
        match outcome {
            Err(_) => Err(format!(
                "Claude Code ไม่ตอบภายใน {} วินาที",
                limit.as_secs_f32()
            )),
            Ok(Err(broke)) => Err(self.broke(broke)),
            Ok(Ok(reply)) => {
                proc.lines += 1;
                *lock(&self.0.seen_model) = proc.model.clone();
                line.slots.active = Some(proc);
                line.ok = true;
                reply
            }
        }
    }

    /// The process for the next line: the standby when it matches `spec`
    /// (waiting for it to finish starting), else a new one.
    async fn next(&self, slots: &mut Slots, spec: Spec) -> Result<Proc, Broke> {
        if slots.standby.as_ref().is_some_and(|s| s.spec != spec) {
            slots.standby = None;
        }
        if let Some(standby) = slots.standby.as_mut() {
            // Borrowed, not taken: a line given up meanwhile leaves it here.
            let started = (&mut standby.task).await;
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
                eprintln!("claude-cli: {why}");
                *lock(&self.0.refused) = Some(why.clone());
                why
            }
            Broke::Gone(why) => why,
        }
    }

    /// Starts a standby in the background unless one is there, no session
    /// is on, or the CLI was refused.
    fn start_standby(&self, slots: &mut Slots) {
        if slots.standby.is_some() || !self.running() || self.refusal().is_some() {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let spec = self.spec(&(self.0.settings)());
        let opts = self.0.opts.clone();
        let task = rt.spawn({
            let spec = spec.clone();
            async move { start(&opts, spec).await }
        });
        slots.standby = Some(Standby { spec, task });
    }

    /// Is `claude` there and logged in? Ok when `auth status` says nothing
    /// clear: the first line will tell.
    async fn check_cli(&self) -> Result<(), Unavailable> {
        if let Some(why) = self.refusal() {
            return Err(Unavailable {
                message: why,
                fix: core::REFUSED_FIX.into(),
            });
        }
        let mut cmd = tokio::process::Command::new(&self.0.opts.program);
        cmd.args(["auth", "status"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for var in core::NESTED_ENV {
            cmd.env_remove(var);
        }
        let unavailable = |(message, fix): (&str, &str)| Unavailable {
            message: message.into(),
            fix: fix.into(),
        };
        match tokio::time::timeout(AUTH_LIMIT, cmd.output()).await {
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(unavailable(core::not_found()))
            }
            Ok(Err(e)) => Err(Unavailable {
                message: format!("เปิด Claude Code ไม่ได้: {e}"),
                fix: core::not_found().1.into(),
            }),
            Ok(Ok(out)) if core::logged_in(&out.stdout) == Some(false) => {
                Err(unavailable(core::not_logged_in()))
            }
            _ => Ok(()),
        }
    }
}

/// A new process within the startup limit.
async fn start(opts: &Options, spec: Spec) -> Result<Proc, Broke> {
    match tokio::time::timeout(opts.startup_limit, Proc::start(&opts.program, spec)).await {
        Ok(started) => started,
        Err(_) => Err(Broke::Gone(format!(
            "Claude Code เปิดไม่เสร็จภายใน {} วินาที",
            opts.startup_limit.as_secs()
        ))),
    }
}

/// One line holding the processes. However it ends, even dropped by a
/// timeout midway, it leaves them right: none at all once translating
/// stopped, and a standby starting when this line failed or the active
/// process nears its last line.
struct Line<'a> {
    cli: &'a ClaudeCli,
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
            return;
        }
        let rotation = self.cli.0.opts.rotation;
        let nearly_done = self
            .slots
            .active
            .as_ref()
            .is_some_and(|p| rotation.warm_next(p.lines));
        if !self.ok || nearly_done {
            self.cli.start_standby(&mut self.slots);
        }
    }
}

impl Translator for ClaudeCli {
    fn engine(&self) -> &'static str {
        "claude-cli"
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
            if slots.active.is_none() {
                self.start_standby(&mut slots);
            }
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
