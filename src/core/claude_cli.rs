//! Claude Code CLI translation (FR-TRN-15): one `claude -p` process kept
//! open, one line per turn over stream-json. This file holds what to run,
//! what to send, how to read what comes back and when to start a new
//! process. Pure; the processes live in `crate::translate::claude_cli`.
//! The prompt is the one Gemini sends (`super::prompt`).

use super::tokens::Usage;
use serde_json::{json, Value};
use std::time::Duration;

/// Model aliases the CLI takes for `--model`; the first is the default.
pub const MODELS: &[&str] = &["haiku", "sonnet", "opus"];
/// `low` keeps subtitle lines fast and cheap.
pub const EFFORT: &str = "low";
/// Longest wait for one line's reply; past it the process counts as hung.
pub const TURN_LIMIT: Duration = Duration::from_secs(10);
/// Longest wait for a new process to start and answer its first line.
pub const STARTUP_LIMIT: Duration = Duration::from_secs(30);
/// The line a new process translates before it takes real ones: the CLI says
/// what tools it has only once a turn starts (spike #80).
pub const WARM_LINE: &str = "Hello.";
/// Variables that make the CLI think it runs inside another Claude Code.
pub const NESTED_ENV: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SSE_PORT",
];

/// `raw` as one of `MODELS`, or a Thai reason it is not.
pub fn model(raw: &str) -> Result<String, String> {
    let m = raw.trim().to_ascii_lowercase();
    if MODELS.contains(&m.as_str()) {
        Ok(m)
    } else {
        Err(format!(
            "model ของ Claude Code ต้องเป็น {}",
            MODELS.join(", ")
        ))
    }
}

/// Arguments after the program name: print mode reading and writing
/// stream-json, no tools, no MCP servers, no customizations, no saved
/// session, and nothing ever asks for permission. Never a bypass flag.
pub fn argv(model: &str, system: &str) -> Vec<String> {
    [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--model",
        model,
        "--effort",
        EFFORT,
        "--safe-mode",
        "--tools",
        "",
        "--strict-mcp-config",
        "--permission-mode",
        "dontAsk",
        "--permission-prompts",
        "none",
        "--no-session-persistence",
        "--disable-slash-commands",
        "--system-prompt",
        system,
    ]
    .map(str::to_owned)
    .to_vec()
}

/// One user turn as a stream-json line, newline included.
pub fn user_message(text: &str) -> String {
    let msg = json!({ "type": "user", "message": { "role": "user", "content": text } });
    format!("{msg}\n")
}

/// What one line of the CLI's output means to us.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `system`/`init`, sent as each turn starts. `refusal` says why the
    /// process may do more than translate; `model` is the model it runs.
    Init {
        model: Option<String>,
        refusal: Option<String>,
    },
    /// `result`: the turn is over, with the reply or a Thai reason.
    Result {
        reply: Result<String, String>,
        usage: Usage,
    },
    /// Anything else: assistant messages, rate limits, stray text.
    Other,
}

/// Reads one output line.
pub fn decode(line: &str) -> Event {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Event::Other;
    };
    match (v["type"].as_str(), v["subtype"].as_str()) {
        (Some("system"), Some("init")) => Event::Init {
            model: v["model"].as_str().map(str::to_owned),
            refusal: refusal(&v),
        },
        (Some("result"), _) => Event::Result {
            reply: result_reply(&v),
            usage: usage(&v),
        },
        _ => Event::Other,
    }
}

/// Why an init event cannot be trusted: tools or MCP servers it should not
/// have, or no list to check.
fn refusal(init: &Value) -> Option<String> {
    let names = |key: &str| -> Option<Vec<String>> {
        let list = init[key].as_array()?;
        Some(
            list.iter()
                .map(|t| t.as_str().or(t["name"].as_str()).unwrap_or("?").to_owned())
                .collect(),
        )
    };
    match (names("tools"), names("mcp_servers")) {
        (Some(tools), Some(mcp)) if tools.is_empty() && mcp.is_empty() => None,
        (Some(tools), Some(mcp)) => Some(format!(
            "Claude Code เปิดเครื่องมือไว้ ({}) Yokoten จึงไม่ใช้",
            tools.into_iter().chain(mcp).collect::<Vec<_>>().join(", ")
        )),
        _ => Some("Claude Code ไม่บอกว่าเปิดเครื่องมืออะไรไว้ Yokoten จึงไม่ใช้".into()),
    }
}

fn result_reply(v: &Value) -> Result<String, String> {
    let text = v["result"].as_str().unwrap_or_default().trim();
    let failed =
        v["is_error"] == Value::Bool(true) || v["subtype"].as_str().is_some_and(|s| s != "success");
    if !failed {
        return Ok(text.to_owned());
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("/login") || lower.contains("logged in") {
        return Err(not_logged_in().0.into());
    }
    let cut: String = text.chars().take(200).collect();
    match cut.is_empty() {
        true => Err("Claude Code แปลไม่สำเร็จ".into()),
        false => Err(format!("Claude Code แปลไม่สำเร็จ: {cut}")),
    }
}

/// Tokens of one turn. Cached prompt tokens count as input: the CLI caches
/// the growing conversation, so `input_tokens` alone is near zero.
fn usage(v: &Value) -> Usage {
    let u = &v["usage"];
    let n = |key: &str| u[key].as_u64().unwrap_or(0);
    Usage {
        input: n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens"),
        output: n("output_tokens"),
    }
}

/// The reply as the line's translation, or why it is not one: empty, or
/// clearly more than a translation (paragraphs, many lines, far too long).
pub fn check_reply(source: &str, reply: &str) -> Result<String, String> {
    let reply = reply.trim().replace("\r\n", "\n");
    if reply.is_empty() {
        return Err("Claude Code ไม่ส่งคำแปลกลับมา".into());
    }
    let lines = |s: &str| s.lines().filter(|l| !l.trim().is_empty()).count();
    let paragraphs = reply.contains("\n\n") && !source.contains("\n\n");
    let too_many_lines = lines(&reply) > lines(source) + 1;
    let too_long = reply.chars().count() > 3 * source.chars().count() + 60;
    if paragraphs || too_many_lines || too_long {
        return Err("Claude Code ตอบเกินคำแปล".into());
    }
    Ok(reply)
}

/// When a process makes way for a new one: after `every` lines, so the
/// conversation it carries stays short, with the next one started
/// `warm_ahead` lines before so no line waits for a start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rotation {
    pub every: u32,
    pub warm_ahead: u32,
}

impl Default for Rotation {
    fn default() -> Self {
        Self {
            every: 150,
            warm_ahead: 10,
        }
    }
}

impl Rotation {
    /// A process that has translated `lines` lines takes no more.
    pub fn retire(&self, lines: u32) -> bool {
        lines >= self.every
    }

    /// Time to start the process that takes over.
    pub fn warm_next(&self, lines: u32) -> bool {
        lines.saturating_add(self.warm_ahead) >= self.every
    }
}

/// `loggedIn` from `claude auth status`; None when the output says nothing.
pub fn logged_in(stdout: &[u8]) -> Option<bool> {
    serde_json::from_slice::<Value>(stdout).ok()?["loggedIn"].as_bool()
}

/// (message, fix) when there is no `claude` to run.
pub fn not_found() -> (&'static str, &'static str) {
    (
        "ไม่พบคำสั่ง claude (Claude Code) ในเครื่องนี้",
        "ติดตั้ง Claude Code ให้เรียก claude ใน Terminal ได้ แล้วเปิด Yokoten ใหม่ หรือเลือกตัวแปลอื่น",
    )
}

/// (message, fix) when the CLI has no login.
pub fn not_logged_in() -> (&'static str, &'static str) {
    (
        "Claude Code ยังไม่ได้ login",
        "เปิด Terminal พิมพ์ claude auth login แล้ว login ด้วยบัญชี Claude ของคุณ จากนั้นกดลองอีกครั้ง",
    )
}

/// The fix shown with a refusal (tools turned on).
pub const REFUSED_FIX: &str = "อัปเดต Claude Code แล้วเปิด Yokoten ใหม่ หรือเลือกตัวแปลอื่น";

#[cfg(test)]
mod tests {
    use super::*;

    fn init(tools: Value, mcp: Value) -> String {
        json!({ "type": "system", "subtype": "init", "cwd": "/tmp/x", "tools": tools,
            "mcp_servers": mcp, "model": "claude-haiku-5-5", "permissionMode": "dontAsk" })
        .to_string()
    }

    // ---- argv ----

    #[test]
    fn argv_reads_and_writes_stream_json_with_the_model_and_low_effort() {
        let a = argv("sonnet", "SYS");
        assert_eq!(a[0], "-p");
        let after = |flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].clone();
        assert_eq!(after("--input-format"), "stream-json");
        assert_eq!(after("--output-format"), "stream-json");
        assert_eq!(after("--model"), "sonnet");
        assert_eq!(after("--effort"), "low");
        assert_eq!(after("--system-prompt"), "SYS");
    }

    #[test]
    fn argv_turns_every_tool_off_and_never_bypasses_permissions() {
        let a = argv("haiku", "SYS");
        let after = |flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].clone();
        assert_eq!(after("--tools"), "");
        assert_eq!(after("--permission-mode"), "dontAsk");
        assert_eq!(after("--permission-prompts"), "none");
        for flag in [
            "--safe-mode",
            "--strict-mcp-config",
            "--no-session-persistence",
            "--disable-slash-commands",
        ] {
            assert!(a.iter().any(|x| x == flag), "{flag}");
        }
        for x in &a {
            assert!(!x.contains("dangerously"), "{x}");
            assert!(!x.contains("bypass"), "{x}");
            assert_ne!(x, "--mcp-config");
            assert_ne!(x, "--add-dir");
        }
    }

    #[test]
    fn a_system_prompt_with_quotes_stays_one_argument() {
        let a = argv("haiku", "Say \"hi\" --tools Bash");
        assert_eq!(a.last().unwrap(), "Say \"hi\" --tools Bash");
        assert_eq!(a.iter().filter(|x| *x == "--tools").count(), 1);
    }

    #[test]
    fn models_are_the_cli_aliases_with_haiku_first() {
        assert_eq!(MODELS[0], "haiku");
        assert_eq!(model(" Sonnet ").unwrap(), "sonnet");
        assert!(model("gpt-5").unwrap_err().contains("haiku"));
        assert!(model("").is_err());
        assert!(model("--dangerously-skip-permissions").is_err());
    }

    // ---- stream-json in ----

    #[test]
    fn a_user_turn_is_one_json_line() {
        let line = user_message("Line:\n\"Run!\"");
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "{line}");
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"], "Line:\n\"Run!\"");
    }

    // ---- stream-json out ----

    #[test]
    fn an_init_without_tools_or_mcp_is_trusted() {
        let e = decode(&init(json!([]), json!([])));
        assert_eq!(
            e,
            Event::Init {
                model: Some("claude-haiku-5-5".into()),
                refusal: None
            }
        );
    }

    #[test]
    fn an_init_with_tools_is_refused_and_names_them() {
        let Event::Init { refusal, .. } = decode(&init(json!(["Bash", "Read"]), json!([]))) else {
            panic!("not init");
        };
        let why = refusal.unwrap();
        assert!(why.contains("Bash, Read"), "{why}");
    }

    #[test]
    fn an_init_with_mcp_servers_is_refused() {
        let mcp = json!([{ "name": "notion", "status": "connected" }]);
        let Event::Init { refusal, .. } = decode(&init(json!([]), mcp)) else {
            panic!("not init");
        };
        assert!(refusal.unwrap().contains("notion"));
    }

    #[test]
    fn an_init_that_does_not_list_tools_is_refused() {
        let bare = json!({ "type": "system", "subtype": "init" }).to_string();
        let Event::Init { refusal, model } = decode(&bare) else {
            panic!("not init");
        };
        assert!(refusal.unwrap().contains("ไม่บอก"));
        assert_eq!(model, None);
        let odd = init(json!("all"), json!([]));
        assert!(matches!(
            decode(&odd),
            Event::Init {
                refusal: Some(_),
                ..
            }
        ));
    }

    fn result(v: Value) -> Event {
        decode(&v.to_string())
    }

    #[test]
    fn a_result_carries_the_reply_and_all_input_tokens() {
        let e = result(json!({
            "type": "result", "subtype": "success", "is_error": false,
            "result": " สวัสดี \n", "stop_reason": "end_turn",
            "usage": { "input_tokens": 2, "cache_creation_input_tokens": 839,
                "cache_read_input_tokens": 100, "output_tokens": 17 }
        }));
        assert_eq!(
            e,
            Event::Result {
                reply: Ok("สวัสดี".into()),
                usage: Usage {
                    input: 941,
                    output: 17
                }
            }
        );
    }

    #[test]
    fn an_error_result_gives_its_reason() {
        let Event::Result { reply, usage } = result(json!({
            "type": "result", "subtype": "error_during_execution", "is_error": true,
            "result": "API Error: 529 overloaded"
        })) else {
            panic!("not result");
        };
        assert!(reply.unwrap_err().contains("529 overloaded"));
        assert_eq!(usage, Usage::default());
        let Event::Result { reply, .. } = result(json!({ "type": "result", "is_error": true }))
        else {
            panic!("not result");
        };
        assert_eq!(reply.unwrap_err(), "Claude Code แปลไม่สำเร็จ");
    }

    #[test]
    fn a_login_error_says_how_to_log_in() {
        let Event::Result { reply, .. } = result(json!({
            "type": "result", "subtype": "success", "is_error": true,
            "result": "Not logged in · Please run /login"
        })) else {
            panic!("not result");
        };
        assert_eq!(reply.unwrap_err(), not_logged_in().0);
        assert!(not_logged_in().1.contains("claude auth login"));
    }

    #[test]
    fn other_lines_are_skipped() {
        for line in [
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"x"}]}}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}"#,
            r#"{"type":"system","subtype":"status"}"#,
            "not json at all",
            "",
        ] {
            assert_eq!(decode(line), Event::Other, "{line}");
        }
    }

    // ---- reply sanity ----

    #[test]
    fn a_plain_translation_passes_trimmed() {
        assert_eq!(
            check_reply("Where are we going?", " เราจะไปไหนกัน \n").unwrap(),
            "เราจะไปไหนกัน"
        );
        let two = "Wait.\nDon't go.";
        assert_eq!(check_reply(two, "เดี๋ยว\nอย่าไป").unwrap(), "เดี๋ยว\nอย่าไป");
    }

    #[test]
    fn an_empty_reply_fails() {
        assert!(check_reply("Hi.", "  \n").unwrap_err().contains("ไม่ส่ง"));
    }

    #[test]
    fn a_reply_with_explanation_fails() {
        let explained = "ไปกันเถอะ\n\nNote: \"Let's go\" is casual, so I used ไปกันเถอะ.";
        assert!(check_reply("Let's go.", explained)
            .unwrap_err()
            .contains("เกินคำแปล"));
        let lines = "หนึ่ง\nสอง\nสาม";
        assert!(check_reply("One.", lines).is_err());
        let long = "ก".repeat(200);
        assert!(check_reply("Run.", &long).is_err());
    }

    // ---- rotation ----

    #[test]
    fn a_process_retires_after_its_lines() {
        let r = Rotation::default();
        assert_eq!((r.every, r.warm_ahead), (150, 10));
        assert!(!r.retire(0));
        assert!(!r.retire(149));
        assert!(r.retire(150));
        assert!(r.retire(151));
    }

    #[test]
    fn the_next_process_warms_ahead_of_the_switch() {
        let r = Rotation::default();
        assert!(!r.warm_next(0));
        assert!(!r.warm_next(139));
        assert!(r.warm_next(140));
        assert!(r.warm_next(150));
        let tight = Rotation {
            every: 3,
            warm_ahead: 1,
        };
        assert!(!tight.warm_next(1));
        assert!(tight.warm_next(2));
        let huge = Rotation {
            every: 5,
            warm_ahead: u32::MAX,
        };
        assert!(huge.warm_next(0));
    }

    // ---- login ----

    #[test]
    fn auth_status_says_whether_logged_in() {
        assert_eq!(
            logged_in(br#"{"loggedIn": true, "authMethod": "claude.ai"}"#),
            Some(true)
        );
        assert_eq!(logged_in(br#"{"loggedIn": false}"#), Some(false));
        assert_eq!(logged_in(b"{}"), None);
        assert_eq!(logged_in(b"Usage: claude auth"), None);
    }

    #[test]
    fn reasons_say_what_to_do() {
        assert!(not_found().1.contains("ติดตั้ง Claude Code"));
        assert!(REFUSED_FIX.contains("อัปเดต"));
        assert!(NESTED_ENV.contains(&"CLAUDECODE"));
    }
}
