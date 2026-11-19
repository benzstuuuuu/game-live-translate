//! Codex CLI translation (FR-TRN-15): one `codex app-server` kept open,
//! speaking JSON-RPC over stdio, one line per turn on a thread whose base
//! instructions are the prompt. This file holds what to run, what to send,
//! how to read what comes back and how to tell that the process can do
//! nothing but translate. Pure; the process lives in
//! `crate::translate::codex_proc`. The prompt is the one Gemini sends
//! (`super::prompt`); when to change threads is `super::claude_cli::Rotation`.

use super::tokens::Usage;
use serde_json::{json, Value};
use std::time::Duration;

/// Models offered on /setup; the first is the default.
pub const MODELS: &[&str] = &[
    "gpt-6-luna",
    "gpt-6-sol",
    "gpt-6-astra",
    "gpt-6.1-terra",
    "gpt-6.1-sol",
];
/// `low` keeps subtitle lines fast and cheap.
pub const EFFORT: &str = "low";
/// Longest wait for one line's reply; past it the process counts as hung.
pub const TURN_LIMIT: Duration = Duration::from_secs(10);
/// Longest wait for a new process to start and open its first thread.
pub const STARTUP_LIMIT: Duration = Duration::from_secs(30);

/// Features that give the model a tool or run something, all turned off
/// with `--disable` (spike #81).
pub const FEATURES_OFF: &[&str] = &[
    "shell_tool",
    "unified_exec",
    "apps",
    "plugins",
    "multi_agent",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "image_generation",
    "view_image",
    "hooks",
    "skill_search",
    "tool_suggest",
    "sleep_tool",
    "goals",
    "in_app_browser",
];
/// Turned off but still listed as on by codex 0.163: it is how the shell
/// tool runs commands, and that tool (`shell_tool`) is off.
pub const LISTED_ON_ANYWAY: &[&str] = &["unified_exec"];
/// Item types a translation turn may hold; any other is a tool at work.
const ITEMS: &[&str] = &[
    "userMessage",
    "agentMessage",
    "reasoning",
    "contextCompaction",
];
/// Notifications the process need not send: the reply arrives whole with
/// `turn/completed`.
const QUIET: &[&str] = &["item/agentMessage/delta"];

/// `raw` as one of `MODELS`, or a Thai reason it is not.
pub fn model(raw: &str) -> Result<String, String> {
    let m = raw.trim().to_ascii_lowercase();
    if MODELS.contains(&m.as_str()) {
        Ok(m)
    } else {
        Err(format!("model ของ Codex ต้องเป็น {}", MODELS.join(", ")))
    }
}

/// Arguments after the program name: the app server on stdio with every
/// tool feature off, web search off, no approvals asked, a read-only
/// sandbox, and each of the user's MCP servers (`mcp`, keys from
/// `mcp_servers`) disabled. Never a bypass flag.
pub fn argv(mcp: &[String]) -> Vec<String> {
    let mut a: Vec<String> = [
        "app-server",
        "-c",
        r#"approval_policy="never""#,
        "-c",
        r#"sandbox_mode="read-only""#,
        "-c",
        r#"web_search="disabled""#,
    ]
    .map(str::to_owned)
    .to_vec();
    for f in FEATURES_OFF {
        a.extend(["--disable".to_owned(), (*f).to_owned()]);
    }
    for key in mcp {
        a.extend(["-c".to_owned(), format!("mcp_servers.{key}.enabled=false")]);
    }
    a
}

/// The MCP server keys in a Codex `config.toml`, as written in its table
/// headers (`[mcp_servers.name]`, `[mcp_servers."a name"]`). Only the
/// headers are read; a server set some other way is caught by `mcp`.
pub fn mcp_servers(config: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for line in config.lines() {
        let Some(rest) = line.trim().strip_prefix("[mcp_servers.") else {
            continue;
        };
        let key = match rest.strip_prefix('"') {
            Some(quoted) => quoted
                .split_once('"')
                .map(|(name, after)| (format!("\"{name}\""), name, after)),
            None => {
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                    .unwrap_or(rest.len());
                Some((rest[..end].to_owned(), &rest[..end], &rest[end..]))
            }
        };
        let Some((key, name, after)) = key else {
            continue;
        };
        let closed = after.starts_with(']') || after.starts_with('.');
        if closed && !name.is_empty() && !name.contains('\\') && !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

/// One JSON-RPC request, newline included.
fn request(id: u64, method: &str, params: Value) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}\n")
}

/// The first request: who we are, and which notifications to leave out.
pub fn initialize(id: u64) -> String {
    let client =
        json!({ "name": "yokoten", "title": "Yokoten", "version": env!("CARGO_PKG_VERSION") });
    let caps = json!({ "optOutNotificationMethods": QUIET });
    request(
        id,
        "initialize",
        json!({ "clientInfo": client, "capabilities": caps }),
    )
}

/// Sent once `initialize` has its reply.
pub fn initialized() -> String {
    "{\"jsonrpc\":\"2.0\",\"method\":\"initialized\"}\n".into()
}

/// Asks which features are on, for `features`.
pub fn list_features(id: u64) -> String {
    request(id, "experimentalFeature/list", json!({ "limit": 1000 }))
}

/// A new thread in the empty folder `cwd`: read-only, never asking for
/// approval, kept nowhere, with the prompt as its base instructions.
pub fn start_thread(id: u64, cwd: &str, model: &str, instructions: &str) -> String {
    request(
        id,
        "thread/start",
        json!({
            "cwd": cwd,
            "model": model,
            "sandbox": "read-only",
            "approvalPolicy": "never",
            "ephemeral": true,
            "baseInstructions": instructions,
        }),
    )
}

/// Asks how the MCP servers stand for `thread`, for `mcp`.
pub fn list_mcp(id: u64, thread: &str) -> String {
    request(
        id,
        "mcpServerStatus/list",
        json!({ "threadId": thread, "detail": "toolsAndAuthOnly" }),
    )
}

/// One line as a turn on `thread`.
pub fn start_turn(id: u64, thread: &str, text: &str) -> String {
    request(
        id,
        "turn/start",
        json!({
            "threadId": thread,
            "input": [{ "type": "text", "text": text }],
            "effort": EFFORT,
        }),
    )
}

/// Lets go of a thread that takes no more lines.
pub fn end_thread(id: u64, thread: &str) -> String {
    request(id, "thread/unsubscribe", json!({ "threadId": thread }))
}

/// What one line of the server's output means to us.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// The reply to our request `id`: its result, or the error's message.
    Reply {
        id: u64,
        result: Result<Value, String>,
    },
    /// The server asks us something (an approval): a tool wants to run.
    Asks(String),
    /// An item that is not a message: a tool at work.
    Tool(String),
    /// Tokens of the last turn.
    Tokens(Usage),
    /// `turn/completed` on `thread`: the reply, or a Thai reason.
    TurnDone {
        thread: String,
        reply: Result<String, String>,
    },
    /// Anything else: deltas, status changes, stray text.
    Other,
}

/// Reads one output line.
pub fn decode(line: &str) -> Msg {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Msg::Other;
    };
    let method = v["method"].as_str();
    if let (Some(m), false) = (method, v["id"].is_null()) {
        return Msg::Asks(m.to_owned());
    }
    if let Some(id) = v["id"].as_u64() {
        return Msg::Reply {
            id,
            result: match v.get("error") {
                Some(e) => Err(e["message"].as_str().unwrap_or("?").to_owned()),
                None => Ok(v["result"].clone()),
            },
        };
    }
    let p = &v["params"];
    match method {
        Some("item/started" | "item/completed") => {
            let kind = p["item"]["type"].as_str().unwrap_or("?");
            match ITEMS.contains(&kind) {
                true => Msg::Other,
                false => Msg::Tool(kind.to_owned()),
            }
        }
        Some("thread/tokenUsage/updated") => {
            let last = &p["tokenUsage"]["last"];
            let n = |key: &str| last[key].as_u64().unwrap_or(0);
            Msg::Tokens(Usage {
                input: n("inputTokens"),
                output: n("outputTokens"),
            })
        }
        Some("turn/completed") => Msg::TurnDone {
            thread: p["threadId"].as_str().unwrap_or_default().to_owned(),
            reply: turn_reply(&p["turn"]),
        },
        _ => Msg::Other,
    }
}

fn turn_reply(turn: &Value) -> Result<String, String> {
    if turn["status"] == "completed" {
        let items = turn["items"].as_array().map_or(&[][..], Vec::as_slice);
        let last = items.iter().rfind(|i| i["type"] == "agentMessage");
        return Ok(last
            .and_then(|i| i["text"].as_str())
            .unwrap_or_default()
            .trim()
            .to_owned());
    }
    Err(failed(
        turn["error"]["message"].as_str().unwrap_or_default(),
    ))
}

/// A Thai reason for a failed turn or request, `why` being Codex's own.
pub fn failed(why: &str) -> String {
    let lower = why.to_ascii_lowercase();
    if ["401", "unauthorized", "log in", "login"]
        .iter()
        .any(|s| lower.contains(s))
    {
        return not_logged_in().0.into();
    }
    let cut: String = why.trim().chars().take(200).collect();
    match cut.is_empty() {
        true => "Codex แปลไม่สำเร็จ".into(),
        false => format!("Codex แปลไม่สำเร็จ: {cut}"),
    }
}

/// A thread the server opened as asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    pub model: Option<String>,
}

/// The `thread/start` result as a thread, or why it cannot be trusted: not
/// read-only, network on, or approvals not `never`.
pub fn thread(result: &Value) -> Result<Thread, String> {
    let sandbox = &result["sandbox"];
    let read_only = sandbox["type"] == "readOnly" && sandbox["networkAccess"] != true;
    if !read_only || result["approvalPolicy"] != "never" {
        return Err(format!(
            "Codex ไม่ได้เปิดแบบอ่านอย่างเดียว (sandbox {}, approval {}) Yokoten จึงไม่ใช้",
            sandbox, result["approvalPolicy"]
        ));
    }
    match result["thread"]["id"].as_str() {
        Some(id) if !id.is_empty() => Ok(Thread {
            id: id.to_owned(),
            model: result["model"].as_str().map(str::to_owned),
        }),
        _ => Err("Codex ไม่ส่งเลข thread กลับมา Yokoten จึงไม่ใช้".into()),
    }
}

/// Why the `experimentalFeature/list` result cannot be trusted: one of
/// `FEATURES_OFF` is on, or there is no list to check.
pub fn features(result: &Value) -> Option<String> {
    let Some(list) = result["data"].as_array() else {
        return Some("Codex ไม่บอกว่าเปิด feature อะไรไว้ Yokoten จึงไม่ใช้".into());
    };
    let on: Vec<&str> = list
        .iter()
        .filter(|f| f["enabled"] == true)
        .filter_map(|f| f["name"].as_str())
        .filter(|n| FEATURES_OFF.contains(n) && !LISTED_ON_ANYWAY.contains(n))
        .collect();
    match on.is_empty() {
        true => None,
        false => Some(format!(
            "Codex เปิดเครื่องมือไว้ ({}) Yokoten จึงไม่ใช้",
            on.join(", ")
        )),
    }
}

/// Why the `mcpServerStatus/list` result cannot be trusted: a server that
/// is not disabled, or there is no list to check.
pub fn mcp(result: &Value) -> Option<String> {
    let Some(list) = result["data"].as_array() else {
        return Some("Codex ไม่บอกว่าเปิด MCP server อะไรไว้ Yokoten จึงไม่ใช้".into());
    };
    let on: Vec<&str> = list
        .iter()
        .filter(|s| s["runtimeStatus"] != "disabled")
        .map(|s| s["name"].as_str().unwrap_or("?"))
        .collect();
    match on.is_empty() {
        true => None,
        false => Some(format!(
            "Codex เปิด MCP server ไว้ ({}) Yokoten จึงไม่ใช้",
            on.join(", ")
        )),
    }
}

/// Whether `codex login status` says there is a login; None when its
/// output (both streams) says nothing clear.
pub fn logged_in(success: bool, output: &str) -> Option<bool> {
    if output.contains("Not logged in") {
        Some(false)
    } else if success && output.contains("Logged in") {
        Some(true)
    } else {
        None
    }
}

/// (message, fix) when there is no `codex` to run.
pub fn not_found() -> (&'static str, &'static str) {
    (
        "ไม่พบคำสั่ง codex (Codex CLI) ในเครื่องนี้",
        "ติดตั้ง Codex CLI ให้เรียก codex ใน Terminal ได้ แล้วเปิด Yokoten ใหม่ หรือเลือกตัวแปลอื่น",
    )
}

/// (message, fix) when the CLI has no login.
pub fn not_logged_in() -> (&'static str, &'static str) {
    (
        "Codex ยังไม่ได้ login",
        "เปิด Terminal พิมพ์ codex login แล้ว login ด้วยบัญชี ChatGPT ของคุณ จากนั้นกดลองอีกครั้ง",
    )
}

/// The fix shown with a refusal (a tool, MCP server or write access on).
pub const REFUSED_FIX: &str = "อัปเดต Codex แล้วเปิด Yokoten ใหม่ หรือเลือกตัวแปลอื่น";

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Value {
        assert!(line.ends_with('\n'), "{line}");
        assert_eq!(line.matches('\n').count(), 1, "{line}");
        serde_json::from_str(line).unwrap()
    }

    // ---- argv ----

    #[test]
    fn argv_runs_the_app_server_read_only_without_approvals_or_web() {
        let a = argv(&[]);
        assert_eq!(a[0], "app-server");
        let configs: Vec<&str> = a
            .windows(2)
            .filter(|w| w[0] == "-c")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(
            configs,
            [
                r#"approval_policy="never""#,
                r#"sandbox_mode="read-only""#,
                r#"web_search="disabled""#
            ]
        );
    }

    #[test]
    fn argv_disables_every_tool_feature() {
        let a = argv(&[]);
        let off: Vec<&str> = a
            .windows(2)
            .filter(|w| w[0] == "--disable")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(off, FEATURES_OFF);
        for f in [
            "shell_tool",
            "unified_exec",
            "apps",
            "plugins",
            "multi_agent",
            "browser_use",
            "computer_use",
            "image_generation",
            "hooks",
            "skill_search",
            "goals",
        ] {
            assert!(off.contains(&f), "{f}");
        }
        assert!(LISTED_ON_ANYWAY.iter().all(|f| FEATURES_OFF.contains(f)));
    }

    #[test]
    fn argv_disables_each_mcp_server_and_never_bypasses_anything() {
        let a = argv(&["notion".into(), "\"my server\"".into()]);
        assert!(a.contains(&"mcp_servers.notion.enabled=false".to_owned()));
        assert!(a.contains(&"mcp_servers.\"my server\".enabled=false".to_owned()));
        for x in &a {
            assert!(!x.contains("dangerously"), "{x}");
            assert!(!x.contains("bypass"), "{x}");
            assert!(!x.contains("danger-full-access"), "{x}");
            assert!(!x.contains("workspace-write"), "{x}");
            assert_ne!(x, "--full-auto");
            assert_ne!(x, "--enable");
        }
    }

    #[test]
    fn models_have_luna_first() {
        assert_eq!(MODELS[0], "gpt-6-luna");
        assert_eq!(model(" GPT-6-Sol ").unwrap(), "gpt-6-sol");
        assert!(model("haiku").unwrap_err().contains("gpt-6-luna"));
        assert!(model("").is_err());
        assert!(model("--dangerously-bypass-approvals-and-sandbox").is_err());
    }

    // ---- config.toml ----

    #[test]
    fn mcp_servers_are_read_from_table_headers_only() {
        let config = r#"
model = "gpt-6-luna"
[mcp_servers.notion]
command = "npx"
[mcp_servers.notion.env]
TOKEN = "secret"
  [mcp_servers.second-brain]
url = "https://x"
[mcp_servers."my server"]
[mcp_servers.code_graph] # comment
[mcp_servers]
[mcp_servers.]
[mcp_servers.bad"name]
[mcp_servers."esc\"aped"]
[[mcp_servers.array]]
[profiles.mcp_servers.x]
# [mcp_servers.commented]
"#;
        assert_eq!(
            mcp_servers(config),
            ["notion", "second-brain", "\"my server\"", "code_graph"]
        );
        assert!(mcp_servers("").is_empty());
    }

    // ---- requests ----

    #[test]
    fn initialize_names_yokoten_and_quiets_deltas() {
        let v = parse(&initialize(1));
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["method"], "initialize");
        assert_eq!(v["params"]["clientInfo"]["name"], "yokoten");
        assert!(v["params"]["clientInfo"]["version"].is_string());
        assert_eq!(
            v["params"]["capabilities"]["optOutNotificationMethods"],
            json!(["item/agentMessage/delta"])
        );
        let n = parse(&initialized());
        assert_eq!(n, json!({ "jsonrpc": "2.0", "method": "initialized" }));
    }

    #[test]
    fn requests_start_with_their_id_and_method() {
        let line = list_features(7);
        assert!(
            line.starts_with(r#"{"jsonrpc":"2.0","id":7,"method":"experimentalFeature/list","#),
            "{line}"
        );
        assert_eq!(parse(&line)["params"]["limit"], 1000);
    }

    #[test]
    fn a_thread_starts_read_only_ephemeral_with_the_prompt() {
        let v = parse(&start_thread(
            2,
            "/tmp/empty",
            "gpt-6-luna",
            "Say \"hi\"\nin Thai",
        ));
        assert_eq!(v["method"], "thread/start");
        let p = &v["params"];
        assert_eq!(p["cwd"], "/tmp/empty");
        assert_eq!(p["model"], "gpt-6-luna");
        assert_eq!(p["sandbox"], "read-only");
        assert_eq!(p["approvalPolicy"], "never");
        assert_eq!(p["ephemeral"], true);
        assert_eq!(p["baseInstructions"], "Say \"hi\"\nin Thai");
    }

    #[test]
    fn mcp_status_is_asked_for_the_thread() {
        let v = parse(&list_mcp(3, "th1"));
        assert_eq!(v["method"], "mcpServerStatus/list");
        assert_eq!(v["params"]["threadId"], "th1");
    }

    #[test]
    fn a_turn_is_the_line_as_text_at_low_effort() {
        let v = parse(&start_turn(4, "th1", "Line:\n\"Run!\""));
        assert_eq!(v["id"], 4);
        assert_eq!(v["method"], "turn/start");
        let p = &v["params"];
        assert_eq!(p["threadId"], "th1");
        assert_eq!(p["effort"], "low");
        assert_eq!(
            p["input"],
            json!([{ "type": "text", "text": "Line:\n\"Run!\"" }])
        );
        let end = parse(&end_thread(5, "th1"));
        assert_eq!(end["method"], "thread/unsubscribe");
        assert_eq!(end["params"]["threadId"], "th1");
    }

    // ---- reading ----

    #[test]
    fn replies_carry_their_result_or_error() {
        assert_eq!(
            decode(r#"{"id":1,"result":{"userAgent":"x"}}"#),
            Msg::Reply {
                id: 1,
                result: Ok(json!({ "userAgent": "x" }))
            }
        );
        assert_eq!(
            decode(r#"{"id":2,"error":{"code":-32600,"message":"bad model"}}"#),
            Msg::Reply {
                id: 2,
                result: Err("bad model".into())
            }
        );
        assert_eq!(
            decode(r#"{"id":3,"error":{}}"#),
            Msg::Reply {
                id: 3,
                result: Err("?".into())
            }
        );
    }

    #[test]
    fn a_request_from_the_server_means_a_tool_wants_to_run() {
        let line = r#"{"id":0,"method":"item/commandExecution/requestApproval","params":{}}"#;
        assert_eq!(
            decode(line),
            Msg::Asks("item/commandExecution/requestApproval".into())
        );
        let named = r#"{"id":"a1","method":"item/tool/call","params":{}}"#;
        assert_eq!(decode(named), Msg::Asks("item/tool/call".into()));
    }

    #[test]
    fn items_other_than_messages_are_tools() {
        let item = |kind: &str| {
            json!({ "method": "item/started", "params": { "threadId": "t", "item": { "type": kind } } })
                .to_string()
        };
        for ok in [
            "userMessage",
            "agentMessage",
            "reasoning",
            "contextCompaction",
        ] {
            assert_eq!(decode(&item(ok)), Msg::Other, "{ok}");
        }
        for tool in [
            "commandExecution",
            "mcpToolCall",
            "webSearch",
            "fileChange",
            "plan",
        ] {
            assert_eq!(decode(&item(tool)), Msg::Tool(tool.into()));
        }
        let done = r#"{"method":"item/completed","params":{"item":{}}}"#;
        assert_eq!(decode(done), Msg::Tool("?".into()));
    }

    #[test]
    fn token_usage_is_the_last_turn() {
        let line = json!({ "method": "thread/tokenUsage/updated", "params": { "threadId": "t",
            "tokenUsage": { "total": { "inputTokens": 99999, "outputTokens": 999 },
                "last": { "inputTokens": 11008, "cachedInputTokens": 2432, "outputTokens": 12 } } } });
        assert_eq!(
            decode(&line.to_string()),
            Msg::Tokens(Usage {
                input: 11008,
                output: 12
            })
        );
    }

    fn done(turn: Value) -> Msg {
        decode(
            &json!({ "method": "turn/completed", "params": { "threadId": "th1", "turn": turn } })
                .to_string(),
        )
    }

    #[test]
    fn a_completed_turn_carries_the_last_agent_message() {
        let msg = done(
            json!({ "id": "tu1", "status": "completed", "error": null, "items": [
            { "type": "reasoning", "id": "r" },
            { "type": "agentMessage", "id": "a", "text": "draft" },
            { "type": "agentMessage", "id": "b", "text": " สวัสดีนักเดินทาง! \n", "phase": "final_answer" }
        ] }),
        );
        assert_eq!(
            msg,
            Msg::TurnDone {
                thread: "th1".into(),
                reply: Ok("สวัสดีนักเดินทาง!".into())
            }
        );
        let empty = done(json!({ "status": "completed", "items": [] }));
        assert_eq!(
            empty,
            Msg::TurnDone {
                thread: "th1".into(),
                reply: Ok(String::new())
            }
        );
    }

    #[test]
    fn a_failed_turn_gives_its_reason() {
        let Msg::TurnDone { reply, .. } = done(json!({ "status": "failed",
            "error": { "message": "stream disconnected before completion" } }))
        else {
            panic!("not done");
        };
        assert!(reply.unwrap_err().contains("stream disconnected"));
        let Msg::TurnDone { reply, .. } = done(json!({ "status": "interrupted" })) else {
            panic!("not done");
        };
        assert_eq!(reply.unwrap_err(), "Codex แปลไม่สำเร็จ");
    }

    #[test]
    fn a_login_error_says_how_to_log_in() {
        let Msg::TurnDone { reply, .. } = done(json!({ "status": "failed",
            "error": { "message": "unexpected status 401 Unauthorized" } }))
        else {
            panic!("not done");
        };
        assert_eq!(reply.unwrap_err(), not_logged_in().0);
        assert!(not_logged_in().1.contains("codex login"));
        assert_eq!(failed("Please log in again"), not_logged_in().0);
    }

    #[test]
    fn other_lines_are_skipped() {
        for line in [
            r#"{"method":"thread/started","params":{"thread":{"id":"t"}}}"#,
            r#"{"method":"item/agentMessage/delta","params":{"delta":"ส"}}"#,
            r#"{"method":"account/rateLimits/updated","params":{}}"#,
            r#"{"method":"initialized"}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(decode(line), Msg::Other, "{line}");
        }
    }

    // ---- checks ----

    fn started(sandbox: Value, approval: &str) -> Value {
        json!({ "thread": { "id": "th1" }, "model": "gpt-6-luna", "sandbox": sandbox,
            "approvalPolicy": approval, "instructionSources": ["/Users/x/.codex/AGENTS.md"] })
    }

    #[test]
    fn a_read_only_thread_without_approvals_is_trusted() {
        let r = started(
            json!({ "type": "readOnly", "networkAccess": false }),
            "never",
        );
        assert_eq!(
            thread(&r).unwrap(),
            Thread {
                id: "th1".into(),
                model: Some("gpt-6-luna".into())
            }
        );
    }

    #[test]
    fn a_thread_that_can_write_ask_or_reach_the_network_is_refused() {
        let write = started(json!({ "type": "workspaceWrite" }), "never");
        assert!(thread(&write).unwrap_err().contains("อ่านอย่างเดียว"));
        let full = started(json!({ "type": "dangerFullAccess" }), "never");
        assert!(thread(&full).is_err());
        let net = started(
            json!({ "type": "readOnly", "networkAccess": true }),
            "never",
        );
        assert!(thread(&net).is_err());
        let asks = started(json!({ "type": "readOnly" }), "on-request");
        assert!(thread(&asks).unwrap_err().contains("on-request"));
        let no_id = json!({ "sandbox": { "type": "readOnly" }, "approvalPolicy": "never" });
        assert!(thread(&no_id).unwrap_err().contains("thread"));
    }

    #[test]
    fn features_left_on_are_refused_by_name() {
        let list = |shell: bool| {
            json!({ "data": [
                { "name": "shell_tool", "enabled": shell },
                { "name": "unified_exec", "enabled": true },
                { "name": "fast_mode", "enabled": true },
                { "name": "goals", "enabled": false }
            ] })
        };
        assert_eq!(features(&list(false)), None);
        let why = features(&list(true)).unwrap();
        assert!(why.contains("(shell_tool)"), "{why}");
        assert!(features(&json!({})).unwrap().contains("ไม่บอก"));
    }

    #[test]
    fn mcp_servers_must_all_be_disabled() {
        let ok =
            json!({ "data": [{ "name": "notion", "runtimeStatus": "disabled", "tools": {} }] });
        assert_eq!(mcp(&ok), None);
        assert_eq!(mcp(&json!({ "data": [] })), None);
        let on = json!({ "data": [
            { "name": "notion", "runtimeStatus": "disabled" },
            { "name": "plugin-x", "runtimeStatus": "connected" },
            { "name": "y", "runtimeStatus": null }
        ] });
        let why = mcp(&on).unwrap();
        assert!(why.contains("plugin-x, y"), "{why}");
        assert!(mcp(&json!(null)).unwrap().contains("ไม่บอก"));
    }

    // ---- login ----

    #[test]
    fn login_status_says_whether_logged_in() {
        assert_eq!(logged_in(true, "Logged in using ChatGPT\n"), Some(true));
        assert_eq!(logged_in(false, "Not logged in\n"), Some(false));
        assert_eq!(logged_in(false, "WARNING: x\nNot logged in"), Some(false));
        assert_eq!(logged_in(false, "Logged in using ChatGPT"), None);
        assert_eq!(logged_in(true, "Usage: codex login"), None);
    }

    #[test]
    fn reasons_say_what_to_do() {
        assert!(not_found().1.contains("ติดตั้ง Codex"));
        assert!(REFUSED_FIX.contains("อัปเดต Codex"));
    }
}
