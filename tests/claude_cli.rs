//! Claude Code CLI translator (#80, FR-TRN-15): a `claude -p` process kept
//! open, its standby, its fallback to Apple and its section of /setup. A
//! fake CLI (a shell script that speaks stream-json) stands in for `claude`;
//! a stand-in named "apple" plays Apple Translation, so this runs without the
//! `apple-translation` feature. The real CLI is in the ignored test at the end.

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use futures_util::future::BoxFuture;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tower::ServiceExt;
use yokoten::app::{App, Config};
use yokoten::core::claude_cli::Rotation;
use yokoten::core::gemini::{Job, Settings};
use yokoten::core::glossary::Term;
use yokoten::server::router_with;
use yokoten::translate::claude_cli::Options;
use yokoten::translate::{ClaudeCli, Translator};
use yokoten::{db, fallback, translation_api};

// ---- the fake CLI ----

/// A shell script in place of `claude`. `auth status` prints the login;
/// otherwise it logs its start (pid, files in its folder, CLAUDECODE, folder)
/// and its argv, waits `startup` seconds, then answers each stream-json turn
/// with an init event (listing `tools`) and a result "แปล <pid> <turn>".
/// A turn with HANG never ends, DIE exits, EXPLAIN gets a paragraph more.
const SCRIPT: &str = r#"#!/bin/sh
HERE="$(cd "$(dirname "$0")" && pwd)"
if [ "$1" = auth ]; then echo '{"loggedIn": @LOGGED@, "email": "x@example.com"}'; exit 0; fi
printf '%s\0' "$@" > "$HERE/argv.$$"
echo "start $$ $(ls -A | wc -l | tr -d ' ') ${CLAUDECODE-unset} $PWD" >> "$HERE/log"
sleep @STARTUP@
n=0
while IFS= read -r line; do
  n=$((n + 1))
  printf 'line %s %s\n' "$$" "$line" >> "$HERE/log"
  echo '{"type":"system","subtype":"init","tools":@TOOLS@,"mcp_servers":[],"model":"claude-haiku-5-5"}'
  echo '{"type":"assistant","message":{"content":[]}}'
  case "$line" in
    *HANG*) exec sleep 600 ;;
    *DIE*) exit 3 ;;
    *EXPLAIN*) reply='ไปกัน\n\nNote: I kept it casual.' ;;
    *) reply="แปล $$ $n" ;;
  esac
  printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","usage":{"input_tokens":2,"cache_read_input_tokens":100,"output_tokens":7}}\n' "$reply"
done
"#;

struct Fake {
    dir: PathBuf,
}

impl Fake {
    fn new(tools: &str, logged_in: bool, startup: &str) -> Fake {
        let dir = std::env::temp_dir().join(format!("yokoten-fake-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let script = SCRIPT
            .replace("@TOOLS@", tools)
            .replace("@LOGGED@", &logged_in.to_string())
            .replace("@STARTUP@", startup);
        let path = dir.join("claude");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        Fake { dir }
    }

    fn ok() -> Fake {
        Fake::new("[]", true, "0")
    }

    fn program(&self) -> PathBuf {
        self.dir.join("claude")
    }

    fn log(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Pids of every process started, oldest first.
    fn started(&self) -> Vec<u32> {
        self.log()
            .iter()
            .filter_map(|l| l.strip_prefix("start "))
            .map(|l| l.split(' ').next().unwrap().parse().unwrap())
            .collect()
    }

    /// The `start` log line of `pid`, split: [pid, files, CLAUDECODE, folder].
    fn start_of(&self, pid: u32) -> Vec<String> {
        let prefix = format!("start {pid} ");
        let line = self
            .log()
            .into_iter()
            .find(|l| l.starts_with(&prefix))
            .unwrap();
        line["start ".len()..]
            .splitn(4, ' ')
            .map(str::to_owned)
            .collect()
    }

    fn argv(&self, pid: u32) -> Vec<String> {
        let raw = std::fs::read(self.dir.join(format!("argv.{pid}"))).unwrap();
        let text = String::from_utf8(raw).unwrap();
        let mut args: Vec<String> = text.split('\0').map(str::to_owned).collect();
        args.pop(); // after the last NUL
        args
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The pid that answered "แปล <pid> <turn>".
fn pid_of(reply: &str) -> u32 {
    reply.split(' ').nth(1).unwrap().parse().unwrap()
}

/// Running and not a zombie.
fn alive(pid: u32) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&out.stdout);
    let stat = stat.trim();
    !stat.is_empty() && !stat.starts_with('Z')
}

/// Waits up to 3 s for `pid` to be gone.
async fn gone(pid: u32) -> bool {
    for _ in 0..60 {
        if !alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

// ---- the app around it ----

/// Stands in for Apple Translation: same engine name, marks the text.
struct StandIn;

impl Translator for StandIn {
    fn engine(&self) -> &'static str {
        "apple"
    }

    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("(สำรอง) {text}")) })
    }
}

struct Setup {
    cli: ClaudeCli,
    app: Arc<App>,
    router: Router,
    settings_file: PathBuf,
    _shutdown: watch::Sender<bool>,
}

fn options(fake: &Fake) -> Options {
    Options {
        program: fake.program(),
        turn_limit: Duration::from_secs(1),
        startup_limit: Duration::from_secs(5),
        ..Options::default()
    }
}

fn setup(opts: Options) -> Setup {
    let settings_file =
        std::env::temp_dir().join(format!("yokoten-cli-{}.json", uuid::Uuid::new_v4()));
    let cli = ClaudeCli::new(Settings::default, opts, Some(settings_file.clone()));
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: None,
        obs: None,
        translators: vec![Arc::new(cli.clone()), Arc::new(StandIn)],
        frame_interval: Duration::from_millis(5),
        live: None,
    };
    let app = App::new(cfg, shutdown).unwrap();
    let router = router_with(
        app.clone(),
        translation_api::claude_cli_routes(app.clone(), cli.clone()),
    );
    Setup {
        cli,
        app,
        router,
        settings_file,
        _shutdown: shutdown_tx,
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.app.stop();
        let _ = std::fs::remove_file(&self.settings_file);
    }
}

async fn call(
    app: &Router,
    method: Method,
    path: &str,
    body: Option<Value>,
    peer: &str,
) -> (StatusCode, Value) {
    let peer: SocketAddr = peer.parse().unwrap();
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(peer));
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let req = req
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or_default())
}

// ---- translating (FR-TRN-15) ----

#[tokio::test]
async fn translates_in_an_empty_folder_with_every_tool_off() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    let terms = [Term::new("Warden", "วอร์เดน"), Term::new("Paimon", "ไพมอน")];
    let earlier = vec!["Where is the gate?".to_owned()];
    let thai = s
        .cli
        .translate_with_glossary("The Warden waits.", &earlier, &terms)
        .await
        .unwrap();
    let pid = pid_of(&thai);
    assert_eq!(thai, format!("แปล {pid} 2"), "turn 1 is the warm-up");

    let start = fake.start_of(pid);
    assert_eq!(start[1], "0", "the folder was empty");
    assert_eq!(start[2], "unset", "CLAUDECODE is not passed on");
    let argv = fake.argv(pid);
    let after = |flag: &str| argv[argv.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(argv[0], "-p");
    assert_eq!(after("--input-format"), "stream-json");
    assert_eq!(after("--output-format"), "stream-json");
    assert_eq!(after("--model"), "haiku");
    assert_eq!(after("--effort"), "low");
    assert_eq!(after("--tools"), "");
    assert_eq!(after("--permission-mode"), "dontAsk");
    assert!(after("--system-prompt").contains("Thai"));
    assert!(argv.iter().all(|a| !a.contains("dangerously")));

    let sent: Vec<String> = fake
        .log()
        .into_iter()
        .filter(|l| l.starts_with("line "))
        .collect();
    assert_eq!(sent.len(), 2);
    let turn: Value = serde_json::from_str(sent[1].splitn(3, ' ').nth(2).unwrap()).unwrap();
    assert_eq!(turn["type"], "user");
    let user = turn["message"]["content"].as_str().unwrap();
    assert!(user.contains("- Where is the gate?"), "{user}");
    assert!(user.contains("- Warden → วอร์เดน"), "{user}");
    assert!(!user.contains("Paimon"), "only terms the line uses: {user}");
    assert!(user.ends_with("Line:\nThe Warden waits."), "{user}");
}

#[tokio::test]
async fn one_process_takes_line_after_line_and_counts_tokens() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    for (i, line) in ["One.", "Two."].into_iter().enumerate() {
        let t = fallback::translate(&s.app, line, &[]).await.unwrap();
        assert_eq!((t.engine, t.is_fallback), ("claude-cli", false));
        assert_eq!(t.model_id.as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(t.text, format!("แปล {} {}", pid_of(&t.text), i + 2));
    }
    assert_eq!(fake.started().len(), 1);
    // Three turns with the warm-up, cached prompt included (FR-TRN-10).
    s.app.count_tokens();
    let tokens = s.app.state.borrow().tokens;
    assert_eq!((tokens.input, tokens.output), (306, 21));
}

#[tokio::test]
async fn a_reply_with_an_explanation_fails_but_keeps_the_process() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let err = s.cli.translate("EXPLAIN this").await.unwrap_err();
    assert!(err.contains("เกินคำแปล"), "{err}");
    s.cli.translate("Next.").await.unwrap();
    assert_eq!(fake.started().len(), 1);
}

// ---- failing and coming back (FR-TRN-04, 05) ----

async fn apple_line(s: &Setup, text: &str) -> String {
    let line = fallback::translate(&s.app, text, &[]).await.unwrap();
    assert_eq!((line.engine, line.is_fallback), ("apple", true), "{text}");
    s.app.state.borrow().fallback.reason.clone().unwrap()
}

#[tokio::test]
async fn a_hung_line_times_out_to_apple_and_a_new_process_takes_the_next() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let first = s.cli.translate("Hi.").await.unwrap();
    let hung = pid_of(&first);

    let reason = apple_line(&s, "HANG here").await;
    assert!(reason.contains("Claude Code ไม่ตอบภายใน 1"), "{reason}");
    assert!(gone(hung).await, "the hung process is killed");

    // "ลอง Claude Code อีกครั้ง": the next line goes to the standby.
    assert!(s.app.set_translator("claude-cli"));
    let line = fallback::translate(&s.app, "Back?", &[]).await.unwrap();
    assert_eq!((line.engine, line.is_fallback), ("claude-cli", false));
    assert_ne!(pid_of(&line.text), hung);
    assert!(!s.app.state.borrow().fallback.active);
}

#[tokio::test]
async fn a_process_that_dies_is_replaced() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let dead = pid_of(&s.cli.translate("Hi.").await.unwrap());
    let reason = apple_line(&s, "DIE now").await;
    assert!(reason.contains("ปิดไปเอง"), "{reason}");
    assert!(gone(dead).await);

    assert!(s.app.set_translator("claude-cli"));
    let line = fallback::translate(&s.app, "Again.", &[]).await.unwrap();
    assert_eq!(line.engine, "claude-cli");
    assert_ne!(pid_of(&line.text), dead);
    assert_eq!(fake.started().len(), 2);
}

#[tokio::test]
async fn rotation_hands_over_to_a_warm_standby_without_a_stall() {
    // Each start takes a second; lines come every 1.5 s as in a game.
    let fake = Fake::new("[]", true, "1");
    let s = setup(Options {
        rotation: Rotation {
            every: 3,
            warm_ahead: 1,
        },
        ..options(&fake)
    });
    s.cli.warm();
    s.cli.translate("Line 1.").await.unwrap();
    let mut pids = Vec::new();
    for i in 2..=7 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let started = Instant::now();
        let thai = s.cli.translate(&format!("Line {i}.")).await.unwrap();
        let took = started.elapsed();
        assert!(
            took < Duration::from_millis(700),
            "line {i} waited {took:?}"
        );
        pids.push(pid_of(&thai));
    }
    // Lines 1-3 on the first process, 4-6 on the second, 7 on the third.
    assert_eq!(pids[0], pids[1]);
    assert_ne!(pids[1], pids[2]);
    assert_eq!(pids[2], pids[4]);
    assert_ne!(pids[4], pids[5]);
    assert!(gone(pids[0]).await, "a retired process is killed");
}

// ---- no process left behind ----

#[tokio::test]
async fn stopping_kills_the_process_and_its_standby_and_removes_their_folders() {
    let fake = Fake::ok();
    let s = setup(Options {
        rotation: Rotation {
            every: 2,
            warm_ahead: 1,
        },
        ..options(&fake)
    });
    s.cli.warm();
    s.cli.translate("One.").await.unwrap();
    // The standby starts after the first line (2 - 1).
    for _ in 0..40 {
        if fake.started().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let pids = fake.started();
    assert_eq!(pids.len(), 2);
    assert!(pids.iter().all(|p| alive(*p)));

    s.app.stop();
    for pid in &pids {
        assert!(gone(*pid).await, "{pid} still running");
        let folder = PathBuf::from(&fake.start_of(*pid)[3]);
        assert!(!folder.exists(), "{folder:?}");
    }
}

#[tokio::test]
async fn stopping_mid_line_kills_the_process_in_use() {
    let fake = Fake::ok();
    let s = setup(Options {
        turn_limit: Duration::from_secs(30),
        ..options(&fake)
    });
    s.cli.warm();
    let cli = s.cli.clone();
    let line = tokio::spawn(async move { cli.translate("HANG forever").await });
    // Wait until the process has the line.
    for _ in 0..100 {
        if fake.log().iter().any(|l| l.contains("HANG")) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let pid = fake.started()[0];
    assert!(alive(pid));

    s.app.stop();
    let err = line.await.unwrap().unwrap_err();
    assert!(err.contains("หยุด"), "{err}");
    assert!(gone(pid).await);
    assert_eq!(fake.started().len(), 1, "no standby after a stop");
}

#[tokio::test]
async fn outside_a_session_a_line_leaves_no_process() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    let pid = pid_of(&s.cli.translate("Hi.").await.unwrap());
    assert!(gone(pid).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.started().len(), 1, "and no standby");
}

// ---- refused, missing, not logged in ----

#[tokio::test]
async fn a_cli_with_tools_is_refused_and_not_started_again() {
    let fake = Fake::new(r#"["Bash","Read"]"#, true, "0");
    let s = setup(options(&fake));
    s.cli.warm();
    let reason = apple_line(&s, "Run.").await;
    assert!(reason.contains("Bash, Read"), "{reason}");
    let pid = fake.started()[0];
    assert!(gone(pid).await);

    assert!(s.app.set_translator("claude-cli"));
    apple_line(&s, "Again.").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.started().len(), 1, "never started again");

    let why = s.cli.check().await.unwrap_err();
    assert!(why.message.contains("Bash"), "{}", why.message);
    assert!(why.fix.contains("อัปเดต Claude Code"), "{}", why.fix);
}

#[tokio::test]
async fn a_missing_cli_says_how_to_install_it() {
    let s = setup(Options {
        program: "/nonexistent/yokoten/claude".into(),
        ..Options::default()
    });
    let why = s.cli.check().await.unwrap_err();
    assert!(why.message.contains("ไม่พบคำสั่ง claude"), "{}", why.message);
    assert!(why.fix.contains("ติดตั้ง Claude Code"), "{}", why.fix);
    s.cli.warm();
    let reason = apple_line(&s, "Run.").await;
    assert!(reason.contains("ไม่พบคำสั่ง claude"), "{reason}");
}

#[tokio::test]
async fn a_cli_without_login_says_how_to_log_in() {
    let fake = Fake::new("[]", false, "0");
    let s = setup(options(&fake));
    let why = s.cli.check().await.unwrap_err();
    assert!(why.message.contains("ยังไม่ได้ login"), "{}", why.message);
    assert!(why.fix.contains("claude auth login"), "{}", why.fix);
    let ok = Fake::ok();
    let s = setup(options(&ok));
    assert!(s.cli.check().await.is_ok());
}

// ---- both jobs (FR-TRN-02, 16) ----

#[tokio::test]
async fn claude_cli_can_retranslate_while_another_engine_is_live() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    assert!(s.app.set_translator("apple"));
    assert!(s.app.set_retranslator(Some("claude-cli")));
    assert_eq!(s.app.translator_for(Job::Live).engine(), "apple");
    let again = s.app.translator_for(Job::Retranslate);
    assert_eq!(again.engine(), "claude-cli");
    assert!(again.translate("Hi.").await.unwrap().starts_with("แปล "));
}

// ---- /setup (FR-TRN-15) ----

const LAN: &str = "192.168.1.20:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

#[tokio::test]
async fn setup_shows_the_model_and_whether_the_cli_is_ready() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    let (status, v) = call(&s.router, Method::GET, "/api/claude-cli", None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["model"], "haiku");
    assert_eq!(v["models"], json!(["haiku", "sonnet", "opus"]));
    assert_eq!(v["ready"], true);
    assert!(
        !v.to_string().contains("example.com"),
        "no account details: {v}"
    );

    let logged_out = Fake::new("[]", false, "0");
    let s = setup(options(&logged_out));
    let (_, v) = call(&s.router, Method::GET, "/api/claude-cli", None, LOOPBACK).await;
    assert_eq!(v["ready"], false);
    assert!(v["fix"].as_str().unwrap().contains("claude auth login"));
}

#[tokio::test]
async fn a_new_model_is_saved_and_used_by_the_next_process() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let first = pid_of(&s.cli.translate("Hi.").await.unwrap());
    let (status, v) = call(
        &s.router,
        Method::PUT,
        "/api/claude-cli",
        Some(json!({ "model": "sonnet" })),
        LOOPBACK,
    )
    .await;
    assert_eq!(
        (status, v["model"].clone()),
        (StatusCode::OK, json!("sonnet"))
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(&s.settings_file).unwrap()).unwrap();
    assert_eq!(saved["claude_cli"]["model"], "sonnet");

    let next = pid_of(&s.cli.translate("Hi again.").await.unwrap());
    assert_ne!(next, first);
    let argv = fake.argv(next);
    assert_eq!(
        argv[argv.iter().position(|a| a == "--model").unwrap() + 1],
        "sonnet"
    );
    assert!(gone(first).await);

    // Read back on the next app start.
    let again = ClaudeCli::new(
        Settings::default,
        options(&fake),
        Some(s.settings_file.clone()),
    );
    assert_eq!(again.model(), "sonnet");
}

#[tokio::test]
async fn an_unknown_model_is_refused_and_the_routes_are_loopback_only() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    let bad = Some(json!({ "model": "gpt-5" }));
    let (status, v) = call(&s.router, Method::PUT, "/api/claude-cli", bad, LOOPBACK).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("haiku"));
    assert_eq!(s.cli.model(), "haiku");
    let (status, _) = call(&s.router, Method::GET, "/api/claude-cli", None, LAN).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// ---- the real CLI (ignored) ----

fn has_thai(s: &str) -> bool {
    s.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c))
}

const LINES: [&str; 20] = [
    "Where are we going?",
    "Stay close to me.",
    "The bridge is out.",
    "Then we swim.",
    "Did you hear that?",
    "Keep your voice down.",
    "We need to find shelter.",
    "It's getting dark.",
    "Light the torch.",
    "Something is moving out there.",
    "Run!",
    "Don't look back.",
    "We made it.",
    "Is everyone okay?",
    "I lost my map.",
    "Let's rest here tonight.",
    "I'll take the first watch.",
    "Wake me if anything happens.",
    "Good morning.",
    "Let's keep moving.",
];

/// Run with `cargo test --test claude_cli -- --ignored --nocapture`. Uses
/// the `claude` on PATH with its own login: one warm-up and 20 short lines
/// in an empty temp folder with every tool off. Prints timings and tokens.
#[tokio::test]
#[ignore = "runs the real Claude Code CLI with the user's login"]
async fn real_cli_translates_twenty_lines_at_two_seconds_p50() {
    let cli = ClaudeCli::new(Settings::default, Options::default(), None);
    if let Err(e) = cli.check().await {
        println!("skipped: {} ({})", e.message, e.fix);
        return;
    }
    let started = Instant::now();
    cli.warm();
    let mut times = Vec::new();
    let mut earlier: Vec<String> = Vec::new();
    for (i, line) in LINES.iter().enumerate() {
        let t = Instant::now();
        let thai = cli.translate_in_context(line, &earlier).await.unwrap();
        let ms = t.elapsed().as_millis();
        if i == 0 {
            println!("first line (includes start): {ms} ms → {thai}");
        } else {
            times.push(ms);
            println!("{ms:>5} ms  {line} → {thai}");
        }
        assert!(has_thai(&thai), "{thai}");
        earlier.push((*line).to_owned());
    }
    cli.stop();
    times.sort_unstable();
    let p50 = times[times.len() / 2];
    let p90 = times[times.len() * 9 / 10];
    let used = cli.usage();
    println!(
        "p50 {p50} ms, p90 {p90} ms over {} warm lines; total {} ms; tokens {} in / {} out; model {:?}",
        times.len(),
        started.elapsed().as_millis(),
        used.input,
        used.output,
        cli.model_id()
    );
    assert!(p50 <= 2000, "p50 {p50} ms");
}
