//! Codex CLI translator (#81, FR-TRN-15): a `codex app-server` kept open,
//! a new thread every so many lines, its fallback to Apple and its section
//! of /setup. A fake app server (a shell script that speaks just enough
//! JSON-RPC) stands in for `codex`; a stand-in named "apple" plays Apple
//! Translation, so this runs without the `apple-translation` feature. The
//! real CLI is in the ignored test at the end.

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
use yokoten::core::codex_cli::FEATURES_OFF;
use yokoten::core::gemini::{Job, Settings};
use yokoten::core::glossary::Term;
use yokoten::server::router_with;
use yokoten::translate::codex_cli::Options;
use yokoten::translate::{CodexCli, Translator};
use yokoten::{db, fallback, translation_api};

// ---- the fake app server ----

/// A shell script in place of `codex`. `login status` says whether there
/// is a login; `app-server` logs its start (pid, files in its folder,
/// folder) and argv, waits `startup` seconds, then answers requests by
/// method. Threads are "th1", "th2" … (a thread takes `thread_delay`
/// seconds to open, answered in the background as the real one does). A turn
/// replies "แปล <pid> <thread> <turn>"; HANG never ends, DIE exits, TOOL
/// runs a command, EXPLAIN gets a paragraph more, FAIL fails.
const SCRIPT: &str = r#"#!/bin/sh
HERE="$(cd "$(dirname "$0")" && pwd)"
if [ "$1" = login ]; then
  if [ @LOGGED@ = true ]; then echo 'Logged in using ChatGPT (x@example.com)' >&2; exit 0; fi
  echo 'Not logged in' >&2; exit 1
fi
printf '%s\0' "$@" > "$HERE/argv.$$"
echo "start $$ $(ls -A | wc -l | tr -d ' ') $PWD" >> "$HERE/log"
sleep @STARTUP@
t=0
n=0
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/^{"jsonrpc":"2.0","id":\([0-9]*\),.*/\1/p')
  method=$(printf '%s' "$line" | sed -n 's/^{"jsonrpc":"2.0",[^m]*"method":"\([^"]*\)".*/\1/p')
  printf '%s %s %s\n' "$method" "$$" "$line" >> "$HERE/log"
  case "$method" in
    initialize) echo "{\"id\":$id,\"result\":{\"userAgent\":\"fake\"}}" ;;
    initialized) ;;
    experimentalFeature/list)
      echo "{\"id\":$id,\"result\":{\"data\":[{\"name\":\"shell_tool\",\"enabled\":@SHELL@},{\"name\":\"unified_exec\",\"enabled\":true},{\"name\":\"fast_mode\",\"enabled\":true}],\"nextCursor\":null}}" ;;
    thread/start)
      t=$((t + 1))
      reply="{\"id\":$id,\"result\":{\"thread\":{\"id\":\"th$t\"},\"model\":\"gpt-6-luna\",\"sandbox\":{\"type\":\"readOnly\",\"networkAccess\":false},\"approvalPolicy\":\"never\"}}"
      (sleep @THREAD@; echo "$reply") & ;;
    mcpServerStatus/list)
      echo "{\"id\":$id,\"result\":{\"data\":[{\"name\":\"notion\",\"runtimeStatus\":\"@MCP@\",\"tools\":{}}]}}" ;;
    thread/unsubscribe) echo "{\"id\":$id,\"result\":{\"status\":\"unsubscribed\"}}" ;;
    turn/start)
      n=$((n + 1))
      th=$(printf '%s' "$line" | sed -n 's/.*"threadId":"\(th[0-9]*\)".*/\1/p')
      echo "{\"id\":$id,\"result\":{\"turn\":{\"id\":\"tu$n\",\"status\":\"inProgress\",\"items\":[]}}}"
      echo "{\"method\":\"turn/started\",\"params\":{\"threadId\":\"$th\"}}"
      case "$line" in
        *HANG*) exec sleep 600 ;;
        *DIE*) exit 3 ;;
        *TOOL*) echo "{\"method\":\"item/started\",\"params\":{\"threadId\":\"$th\",\"item\":{\"type\":\"commandExecution\"}}}" ;;
        *FAIL*) echo "{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"$th\",\"turn\":{\"status\":\"failed\",\"error\":{\"message\":\"overloaded\"}}}}" ;;
        *)
          case "$line" in
            *EXPLAIN*) text='ไปกัน\n\nNote: I kept it casual.' ;;
            *) text="แปล $$ $th $n" ;;
          esac
          echo "{\"method\":\"item/started\",\"params\":{\"threadId\":\"$th\",\"item\":{\"type\":\"agentMessage\"}}}"
          echo "{\"method\":\"thread/tokenUsage/updated\",\"params\":{\"threadId\":\"$th\",\"tokenUsage\":{\"last\":{\"inputTokens\":100,\"outputTokens\":7}}}}"
          printf '%s\n' "{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"$th\",\"turn\":{\"status\":\"completed\",\"items\":[{\"type\":\"agentMessage\",\"text\":\"$text\"}]}}}" ;;
      esac ;;
    *) echo "{\"id\":$id,\"error\":{\"code\":-32601,\"message\":\"unknown $method\"}}" ;;
  esac
done
"#;

struct Fake {
    dir: PathBuf,
}

#[derive(Clone, Copy)]
struct Knobs {
    logged_in: bool,
    shell_on: bool,
    mcp: &'static str,
    startup: &'static str,
    thread_delay: &'static str,
}

const OK: Knobs = Knobs {
    logged_in: true,
    shell_on: false,
    mcp: "disabled",
    startup: "0",
    thread_delay: "0",
};

impl Fake {
    fn new(k: Knobs) -> Fake {
        let dir = std::env::temp_dir().join(format!("yokoten-fake-codex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let script = SCRIPT
            .replace("@LOGGED@", &k.logged_in.to_string())
            .replace("@SHELL@", &k.shell_on.to_string())
            .replace("@MCP@", k.mcp)
            .replace("@STARTUP@", k.startup)
            .replace("@THREAD@", k.thread_delay);
        let path = dir.join("codex");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The user's Codex config: only the MCP table names are read.
        let config = "model = \"x\"\n[mcp_servers.notion]\ncommand = \"npx\"\n\
                      [mcp_servers.notion.env]\nTOKEN = \"secret\"\n[mcp_servers.\"my server\"]\n";
        std::fs::write(dir.join("config.toml"), config).unwrap();
        Fake { dir }
    }

    fn ok() -> Fake {
        Fake::new(OK)
    }

    fn program(&self) -> PathBuf {
        self.dir.join("codex")
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

    /// The `start` log line of `pid`, split: [pid, files, folder].
    fn start_of(&self, pid: u32) -> Vec<String> {
        let prefix = format!("start {pid} ");
        let line = self
            .log()
            .into_iter()
            .find(|l| l.starts_with(&prefix))
            .unwrap();
        line["start ".len()..]
            .splitn(3, ' ')
            .map(str::to_owned)
            .collect()
    }

    /// Every request sent with `method`, as JSON.
    fn sent(&self, method: &str) -> Vec<Value> {
        let prefix = format!("{method} ");
        self.log()
            .iter()
            .filter_map(|l| l.strip_prefix(&prefix))
            .map(|l| serde_json::from_str(l.split_once(' ').unwrap().1).unwrap())
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

/// The pid and thread that answered "แปล <pid> <thread> <turn>".
fn who(reply: &str) -> (u32, String) {
    let mut parts = reply.split(' ').skip(1);
    let pid = parts.next().unwrap().parse().unwrap();
    (pid, parts.next().unwrap().to_owned())
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

/// Waits up to 3 s for the fake to have started `n` processes.
async fn started(fake: &Fake, n: usize) -> Vec<u32> {
    for _ in 0..60 {
        if fake.started().len() >= n {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    fake.started()
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
    cli: CodexCli,
    app: Arc<App>,
    router: Router,
    settings_file: PathBuf,
    _shutdown: watch::Sender<bool>,
}

fn options(fake: &Fake) -> Options {
    Options {
        program: fake.program(),
        config: fake.dir.join("config.toml"),
        turn_limit: Duration::from_secs(1),
        startup_limit: Duration::from_secs(5),
        ..Options::default()
    }
}

fn setup(opts: Options) -> Setup {
    setup_in(opts, None)
}

/// With `frames`, `App::start` runs a subtitle session reading that folder.
fn setup_in(opts: Options, frames: Option<PathBuf>) -> Setup {
    let settings_file =
        std::env::temp_dir().join(format!("yokoten-codex-{}.json", uuid::Uuid::new_v4()));
    let cli = CodexCli::new(Settings::default, opts, Some(settings_file.clone()));
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    db::prepare(&conn).unwrap();
    let (shutdown_tx, shutdown) = watch::channel(false);
    let cfg = Config {
        conn,
        frames_dir: frames,
        obs: None,
        translators: vec![Arc::new(cli.clone()), Arc::new(StandIn)],
        frame_interval: Duration::from_millis(5),
        live: None,
    };
    let app = App::new(cfg, shutdown).unwrap();
    let router = router_with(
        app.clone(),
        translation_api::codex_cli_routes(app.clone(), cli.clone()),
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
    let (pid, thread) = who(&thai);
    assert_eq!(thai, format!("แปล {pid} th1 1"), "no warm-up turn");
    assert_eq!(thread, "th1");

    let start = fake.start_of(pid);
    assert_eq!(start[1], "0", "the folder was empty");
    let argv = fake.argv(pid);
    assert_eq!(argv[0], "app-server");
    let values = |flag: &str| -> Vec<String> {
        argv.windows(2)
            .filter(|w| w[0] == flag)
            .map(|w| w[1].clone())
            .collect()
    };
    assert_eq!(values("--disable"), FEATURES_OFF);
    let configs = values("-c");
    for c in [
        r#"approval_policy="never""#,
        r#"sandbox_mode="read-only""#,
        r#"web_search="disabled""#,
        "mcp_servers.notion.enabled=false",
        "mcp_servers.\"my server\".enabled=false",
    ] {
        assert!(configs.contains(&c.to_owned()), "{c} in {configs:?}");
    }
    assert_eq!(
        configs.len(),
        5,
        "nothing else from config.toml: {configs:?}"
    );
    for a in &argv {
        assert!(
            !a.contains("dangerously") && !a.contains("danger-full-access"),
            "{a}"
        );
        assert!(!a.contains("secret"), "{a}");
    }

    // Initialize, features checked, then a read-only ephemeral thread whose
    // MCP servers are checked, then the line.
    let methods: Vec<String> = fake
        .log()
        .iter()
        .skip(1)
        .map(|l| l.split(' ').next().unwrap().to_owned())
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "experimentalFeature/list",
            "thread/start",
            "mcpServerStatus/list",
            "turn/start"
        ]
    );
    let t = &fake.sent("thread/start")[0]["params"];
    // The script sees the folder through /private, where /var links to.
    let real = |p: &str| p.trim_start_matches("/private").to_owned();
    assert_eq!(
        real(t["cwd"].as_str().unwrap()),
        real(&start[2]),
        "its own folder"
    );
    assert_eq!(t["model"], "gpt-6-luna");
    assert_eq!(t["sandbox"], "read-only");
    assert_eq!(t["approvalPolicy"], "never");
    assert_eq!(t["ephemeral"], true);
    assert!(t["baseInstructions"].as_str().unwrap().contains("Thai"));
    assert_eq!(
        fake.sent("mcpServerStatus/list")[0]["params"]["threadId"],
        "th1"
    );

    let turn = &fake.sent("turn/start")[0]["params"];
    assert_eq!(turn["threadId"], "th1");
    assert_eq!(turn["effort"], "low");
    let user = turn["input"][0]["text"].as_str().unwrap();
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
        assert_eq!((t.engine, t.is_fallback), ("codex-cli", false));
        assert_eq!(t.model_id.as_deref(), Some("gpt-6-luna"));
        assert_eq!(t.text, format!("แปล {} th1 {}", who(&t.text).0, i + 1));
    }
    assert_eq!(fake.started().len(), 1);
    s.app.count_tokens();
    let tokens = s.app.state.borrow().tokens;
    assert_eq!((tokens.input, tokens.output), (200, 14));
}

#[tokio::test]
async fn a_reply_with_an_explanation_or_a_failed_turn_keeps_the_process() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let err = s.cli.translate("EXPLAIN this").await.unwrap_err();
    assert!(err.contains("Codex ตอบเกินคำแปล"), "{err}");
    let err = s.cli.translate("FAIL this").await.unwrap_err();
    assert!(err.contains("overloaded"), "{err}");
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
    let hung = who(&s.cli.translate("Hi.").await.unwrap()).0;

    let reason = apple_line(&s, "HANG here").await;
    assert!(reason.contains("Codex ไม่ตอบภายใน 1"), "{reason}");
    assert!(gone(hung).await, "the hung process is killed");

    // "ลอง Codex อีกครั้ง": the next line goes to the process started meanwhile.
    assert!(s.app.set_translator("codex-cli"));
    let line = fallback::translate(&s.app, "Back?", &[]).await.unwrap();
    assert_eq!((line.engine, line.is_fallback), ("codex-cli", false));
    assert_ne!(who(&line.text).0, hung);
    assert!(!s.app.state.borrow().fallback.active);
}

#[tokio::test]
async fn a_process_that_dies_is_replaced() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let dead = who(&s.cli.translate("Hi.").await.unwrap()).0;
    let reason = apple_line(&s, "DIE now").await;
    assert!(reason.contains("ปิดไปเอง"), "{reason}");
    assert!(gone(dead).await);
    // Respawned in the background, before the next line asks.
    assert_eq!(started(&fake, 2).await.len(), 2);

    assert!(s.app.set_translator("codex-cli"));
    let line = fallback::translate(&s.app, "Again.", &[]).await.unwrap();
    assert_eq!(line.engine, "codex-cli");
    assert_ne!(who(&line.text).0, dead);
    assert_eq!(fake.started().len(), 2);
}

#[tokio::test]
async fn a_new_thread_is_ready_ahead_so_rotation_does_not_stall() {
    // Each thread takes a second to open; lines come every 1.5 s as in a game.
    let fake = Fake::new(Knobs {
        thread_delay: "1",
        ..OK
    });
    let s = setup(Options {
        rotation: Rotation {
            every: 3,
            warm_ahead: 1,
        },
        ..options(&fake)
    });
    s.cli.warm();
    s.cli.translate("Line 1.").await.unwrap();
    let mut seen = Vec::new();
    for i in 2..=7 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let started = Instant::now();
        let thai = s.cli.translate(&format!("Line {i}.")).await.unwrap();
        let took = started.elapsed();
        assert!(
            took < Duration::from_millis(700),
            "line {i} waited {took:?}"
        );
        seen.push(who(&thai));
    }
    // Lines 2-3 on th1, 4-6 on th2, 7 on th3, all in one process.
    let threads: Vec<&str> = seen.iter().map(|(_, t)| t.as_str()).collect();
    assert_eq!(threads, ["th1", "th1", "th2", "th2", "th2", "th3"]);
    assert!(seen.iter().all(|(pid, _)| *pid == seen[0].0));
    assert_eq!(fake.started().len(), 1);
    let ended: Vec<Value> = fake
        .sent("thread/unsubscribe")
        .iter()
        .map(|v| v["params"]["threadId"].clone())
        .collect();
    assert_eq!(
        ended,
        [json!("th1"), json!("th2")],
        "retired threads let go"
    );
}

// ---- no process left behind ----

#[tokio::test]
async fn stopping_kills_the_process_and_removes_its_folder() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let pid = who(&s.cli.translate("One.").await.unwrap()).0;
    let folder = PathBuf::from(&fake.start_of(pid)[2]);
    assert!(alive(pid) && folder.exists());

    s.app.stop();
    assert!(gone(pid).await, "{pid} still running");
    assert!(!folder.exists(), "{folder:?}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.started().len(), 1, "no new process after a stop");
}

#[tokio::test]
async fn stopping_while_a_process_starts_kills_it_too() {
    let fake = Fake::new(Knobs { startup: "2", ..OK });
    let s = setup(options(&fake));
    s.cli.warm();
    let pid = started(&fake, 1).await[0];
    s.app.stop();
    assert!(gone(pid).await);
    let folder = PathBuf::from(&fake.start_of(pid)[2]);
    assert!(!folder.exists(), "{folder:?}");
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
    assert_eq!(fake.started().len(), 1, "no new process after a stop");
}

#[tokio::test]
async fn outside_a_session_a_line_leaves_no_process() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    let pid = who(&s.cli.translate("Hi.").await.unwrap()).0;
    assert!(gone(pid).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.started().len(), 1);
}

// ---- refused, missing, not logged in ----

async fn refused(fake: &Fake) -> (Setup, String) {
    let s = setup(options(fake));
    s.cli.warm();
    let reason = apple_line(&s, "Run.").await;
    let pid = fake.started()[0];
    assert!(gone(pid).await);

    assert!(s.app.set_translator("codex-cli"));
    apple_line(&s, "Again.").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.started().len(), 1, "never started again");
    let why = s.cli.check().await.unwrap_err();
    assert!(why.fix.contains("อัปเดต Codex"), "{}", why.fix);
    (s, reason)
}

#[tokio::test]
async fn a_tool_at_work_is_refused_and_not_started_again() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let reason = apple_line(&s, "TOOL please").await;
    assert!(reason.contains("commandExecution"), "{reason}");
    assert!(s.cli.check().await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.started().len(), 1);
}

#[tokio::test]
async fn a_feature_left_on_is_refused() {
    let fake = Fake::new(Knobs {
        shell_on: true,
        ..OK
    });
    let (_s, reason) = refused(&fake).await;
    assert!(reason.contains("shell_tool"), "{reason}");
}

#[tokio::test]
async fn an_mcp_server_left_on_is_refused() {
    let fake = Fake::new(Knobs {
        mcp: "connected",
        ..OK
    });
    let (s, reason) = refused(&fake).await;
    assert!(reason.contains("notion"), "{reason}");
    assert!(s.cli.check().await.unwrap_err().message.contains("MCP"));
}

#[tokio::test]
async fn a_missing_cli_says_how_to_install_it() {
    let s = setup(Options {
        program: "/nonexistent/yokoten/codex".into(),
        ..Options::default()
    });
    let why = s.cli.check().await.unwrap_err();
    assert!(why.message.contains("ไม่พบคำสั่ง codex"), "{}", why.message);
    assert!(why.fix.contains("ติดตั้ง Codex"), "{}", why.fix);
    s.cli.warm();
    let reason = apple_line(&s, "Run.").await;
    assert!(reason.contains("ไม่พบคำสั่ง codex"), "{reason}");
}

#[tokio::test]
async fn a_cli_without_login_says_how_to_log_in() {
    let fake = Fake::new(Knobs {
        logged_in: false,
        ..OK
    });
    let s = setup(options(&fake));
    let why = s.cli.check().await.unwrap_err();
    assert!(why.message.contains("ยังไม่ได้ login"), "{}", why.message);
    assert!(why.fix.contains("codex login"), "{}", why.fix);
    let ok = Fake::ok();
    let s = setup(options(&ok));
    assert!(s.cli.check().await.is_ok());
}

// ---- switching engines mid-session (FR-TRN-02, 15, 16) ----

fn session(fake: &Fake) -> Setup {
    let s = setup_in(options(fake), Some(fake.dir.clone()));
    s.app.start().unwrap();
    s
}

#[tokio::test]
async fn switching_live_away_stops_the_process_and_switching_back_warms_it() {
    let fake = Fake::ok();
    let s = session(&fake);
    let first = started(&fake, 1).await;
    assert_eq!(first.len(), 1, "start warms a process");

    assert!(s.app.set_translator("apple"));
    assert!(gone(first[0]).await, "no codex-cli process left");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.started().len(), 1);

    assert!(s.app.set_translator("codex-cli"));
    let again = started(&fake, 2).await;
    assert_eq!(again.len(), 2, "switching back warms it again");
    assert!(alive(again[1]));
}

#[tokio::test]
async fn codex_can_retranslate_while_another_engine_is_live() {
    let fake = Fake::ok();
    let s = session(&fake);
    assert!(s.app.set_retranslator(Some("codex-cli")));
    let pid = started(&fake, 1).await[0];
    assert!(s.app.set_translator("apple"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(alive(pid), "still the retranslate engine");
    assert_eq!(s.app.translator_for(Job::Live).engine(), "apple");
    let again = s.app.translator_for(Job::Retranslate);
    assert_eq!(again.engine(), "codex-cli");
    assert!(again.translate("Hi.").await.unwrap().starts_with("แปล "));

    assert!(s.app.set_retranslator(None));
    assert!(gone(pid).await, "nothing uses codex-cli now");
}

// ---- /setup (FR-TRN-15) ----

const LAN: &str = "192.168.1.20:50000";
const LOOPBACK: &str = "127.0.0.1:50000";

#[tokio::test]
async fn setup_shows_the_model_and_whether_the_cli_is_ready() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    let (status, v) = call(&s.router, Method::GET, "/api/codex-cli", None, LOOPBACK).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["model"], "gpt-6-luna");
    assert_eq!(v["effort"], "low");
    assert_eq!(v["models"][0], "gpt-6-luna");
    assert_eq!(v["ready"], true);
    assert!(
        !v.to_string().contains("example.com"),
        "no account details: {v}"
    );

    let logged_out = Fake::new(Knobs {
        logged_in: false,
        ..OK
    });
    let s = setup(options(&logged_out));
    let (_, v) = call(&s.router, Method::GET, "/api/codex-cli", None, LOOPBACK).await;
    assert_eq!(v["ready"], false);
    assert!(v["fix"].as_str().unwrap().contains("codex login"));
}

#[tokio::test]
async fn a_new_model_is_saved_and_takes_a_new_thread_in_the_same_process() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    s.cli.warm();
    let (pid, _) = who(&s.cli.translate("Hi.").await.unwrap());
    let (status, v) = call(
        &s.router,
        Method::PUT,
        "/api/codex-cli",
        Some(json!({ "model": "gpt-6-sol" })),
        LOOPBACK,
    )
    .await;
    assert_eq!(
        (status, v["model"].clone()),
        (StatusCode::OK, json!("gpt-6-sol"))
    );
    let saved: Value = serde_json::from_slice(&std::fs::read(&s.settings_file).unwrap()).unwrap();
    assert_eq!(saved["codex_cli"]["model"], "gpt-6-sol");

    let (next, thread) = who(&s.cli.translate("Hi again.").await.unwrap());
    assert_eq!((next, thread.as_str()), (pid, "th2"));
    assert_eq!(fake.sent("thread/start")[1]["params"]["model"], "gpt-6-sol");

    let again = CodexCli::new(
        Settings::default,
        options(&fake),
        Some(s.settings_file.clone()),
    );
    assert_eq!(again.model(), "gpt-6-sol");
}

#[tokio::test]
async fn an_unknown_model_is_refused_and_the_routes_are_loopback_only() {
    let fake = Fake::ok();
    let s = setup(options(&fake));
    let bad = Some(json!({ "model": "haiku" }));
    let (status, v) = call(&s.router, Method::PUT, "/api/codex-cli", bad, LOOPBACK).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("gpt-6-luna"));
    assert_eq!(s.cli.model(), "gpt-6-luna");
    let (status, _) = call(&s.router, Method::GET, "/api/codex-cli", None, LAN).await;
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

/// Run with `cargo test --test codex_cli -- --ignored --nocapture`. Uses the
/// `codex` on PATH with its own login: 20 short lines in an empty temp
/// folder with every tool and MCP server off. Prints timings and tokens.
#[tokio::test]
#[ignore = "runs the real Codex CLI with the user's login"]
async fn real_cli_translates_twenty_lines_at_two_and_a_half_seconds_p50() {
    let cli = CodexCli::new(Settings::default, Options::default(), None);
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
    assert!(p50 <= 2500, "p50 {p50} ms");
}
