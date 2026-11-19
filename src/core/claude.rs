//! Claude Haiku translation (FR-TRN-14): the Messages API body for one line,
//! how to read the reply and its tokens, and why a call failed. Pure; the
//! HTTP call lives in `crate::translate::claude`. The prompt is the one
//! Gemini sends (`super::prompt`).

use super::gemini::Settings;
use super::glossary::Term;
use super::prompt;
use super::tokens::Usage;
use serde_json::{json, Value};

/// The one model this translator uses.
pub const MODEL: &str = "claude-haiku-5-5";
/// `low` keeps subtitle lines fast and cheap; Haiku 5.5 defaults to `medium`.
pub const EFFORT: &str = "low";
/// Room for adaptive thinking plus one Thai line: thinking counts toward
/// `max_tokens`, so a cap sized for the line alone could cut it off.
pub const MAX_TOKENS: u32 = 1024;

/// Body of `POST /v1/messages` for one line: the shared prompt, the user
/// turn last (Haiku 5.5 refuses an assistant prefill), effort `low`.
pub fn request_body(text: &str, context: &[String], glossary: &[Term], s: &Settings) -> Value {
    json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        "system": prompt::system(s),
        "messages": [{ "role": "user", "content": prompt::user(text, context, glossary, s) }],
        "output_config": { "effort": EFFORT },
    })
}

/// The Thai text of a Messages reply, or a Thai reason there is none. The
/// reply may open with `thinking` blocks, so only `text` blocks count.
pub fn parse_reply(body: &[u8]) -> Result<String, String> {
    let v: Value =
        serde_json::from_slice(body).map_err(|_| "อ่านคำตอบของ Claude ไม่ได้".to_owned())?;
    let stop = v["stop_reason"].as_str();
    if stop == Some("refusal") {
        return Err("Claude ไม่ยอมแปลบรรทัดนี้ (refusal)".into());
    }
    let text: String = v["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    let text = text.trim();
    if !text.is_empty() {
        return Ok(text.to_owned());
    }
    match stop {
        Some(r) if r != "end_turn" => Err(format!("Claude ไม่ส่งคำแปลกลับมา ({r})")),
        _ => Err("Claude ไม่ส่งคำแปลกลับมา".into()),
    }
}

/// The tokens one reply says it used; missing or broken usage reads as 0.
/// Output includes thinking, which is billed as output.
pub fn usage(body: &[u8]) -> Usage {
    let v: Value = serde_json::from_slice(body).unwrap_or_default();
    let count = |key: &str| v["usage"][key].as_u64().unwrap_or(0);
    Usage {
        input: count("input_tokens"),
        output: count("output_tokens"),
    }
}

/// Thai reason for a failed call, for the key test, the fallback card and
/// the log.
pub fn failure_reason(status: u16, body: &[u8]) -> String {
    let v: Value = serde_json::from_slice(body).unwrap_or_default();
    let message: String = v["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect();
    match status {
        401 => "API key ของ Claude ไม่ถูกต้อง คัดลอก key จาก Claude Console มาใหม่ให้ครบ \
                แล้วบันทึกใน /setup"
            .into(),
        402 => "บัญชี Anthropic มีปัญหาเรื่องการชำระเงิน ตรวจ billing ใน Claude Console".into(),
        403 => "key นี้ไม่มีสิทธิ์ใช้ Claude API ตรวจใน Claude Console".into(),
        404 => format!("บัญชีนี้ใช้ model {MODEL} ไม่ได้"),
        429 => "ใช้ Claude เกินโควตาหรือถี่เกินไป รอสักครู่หรือตรวจ billing".into(),
        400 => format!("Claude ไม่รับคำขอ: {message}"),
        500..=599 => "Claude มีปัญหาชั่วคราว ลองอีกครั้ง".into(),
        _ => format!("Claude ตอบกลับ HTTP {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::gemini::{Job, Patch, Tone};

    fn settings(v: Value) -> Settings {
        let patch: Patch = serde_json::from_value(v).unwrap();
        Settings::default().apply(patch).unwrap()
    }

    fn bytes(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    // ---- request ----

    #[test]
    fn request_names_the_model_effort_and_a_small_cap() {
        let body = request_body("Hi.", &[], &[], &Settings::default());
        assert_eq!(body["model"], "claude-haiku-5-5");
        assert_eq!(body["output_config"], json!({ "effort": "low" }));
        assert_eq!(body["max_tokens"], 1024);
        for unsent in ["temperature", "top_p", "top_k", "thinking", "stream"] {
            assert!(body.get(unsent).is_none(), "{unsent}: {body}");
        }
    }

    #[test]
    fn system_prompt_has_the_tone_and_extra_prompt() {
        let s = settings(json!({ "tone": "formal", "extra_prompt": "Call the hero Traveler." }));
        let body = request_body("Hi.", &[], &[], &s);
        let system = body["system"].as_str().unwrap();
        assert!(system.contains("Thai"), "{system}");
        assert!(system.contains(Tone::Formal.instruction()), "{system}");
        assert!(system.contains("Call the hero Traveler."), "{system}");
    }

    #[test]
    fn one_user_turn_carries_context_glossary_and_line_without_prefill() {
        let s = settings(json!({ "context_lines": 1 }));
        let context = vec!["Too old.".to_owned(), "Where is the gate?".to_owned()];
        let terms = [Term::new("Warden", "วอร์เดน")];
        let body = request_body("The Warden waits.", &context, &terms, &s);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1, "no assistant prefill: {body}");
        assert_eq!(messages[0]["role"], "user");
        let user = messages[0]["content"].as_str().unwrap();
        let gate = user.find("- Where is the gate?").unwrap();
        let term = user.find("- Warden → วอร์เดน").unwrap();
        let line = user.find("Line:\nThe Warden waits.").unwrap();
        assert!(gate < term && term < line, "{user}");
        assert!(!user.contains("Too old."), "cut to context_lines: {user}");
    }

    #[test]
    fn the_prompt_is_the_one_gemini_sends() {
        let s = settings(json!({ "tone": "neutral", "extra_prompt": "x" }));
        let context = vec!["before".to_owned()];
        let terms = [Term::new("Paimon", "ไพมอน")];
        let claude = request_body("Paimon!", &context, &terms, &s);
        let gemini = crate::core::gemini::request_body("Paimon!", &context, &terms, &s, Job::Live);
        assert_eq!(
            claude["system"],
            gemini["systemInstruction"]["parts"][0]["text"]
        );
        assert_eq!(
            claude["messages"][0]["content"],
            gemini["contents"][0]["parts"][0]["text"]
        );
    }

    // ---- reply ----

    #[test]
    fn reply_text_skips_thinking_blocks_and_trims() {
        let body = bytes(json!({
            "type": "message", "role": "assistant", "model": "claude-haiku-5-5",
            "content": [
                { "type": "thinking", "thinking": "", "signature": "sig" },
                { "type": "text", "text": " ไปทาง" },
                { "type": "text", "text": "เหนือกัน\n" }
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 120, "output_tokens": 15 }
        }));
        assert_eq!(parse_reply(&body).unwrap(), "ไปทางเหนือกัน");
    }

    #[test]
    fn a_refusal_says_so_even_with_text() {
        let body = bytes(json!({
            "content": [{ "type": "text", "text": "partial" }],
            "stop_reason": "refusal",
            "stop_details": { "category": "general_harms" }
        }));
        let err = parse_reply(&body).unwrap_err();
        assert!(err.contains("ไม่ยอมแปล"), "{err}");
    }

    #[test]
    fn no_text_names_the_stop_reason() {
        let cut = bytes(json!({
            "content": [{ "type": "thinking", "thinking": "", "signature": "s" }],
            "stop_reason": "max_tokens"
        }));
        let err = parse_reply(&cut).unwrap_err();
        assert!(err.contains("max_tokens"), "{err}");
        let empty = bytes(json!({ "content": [], "stop_reason": "end_turn" }));
        assert!(!parse_reply(&empty).unwrap_err().contains("end_turn"));
        assert!(parse_reply(b"{}").is_err());
        assert!(parse_reply(b"<html>").unwrap_err().contains("อ่านคำตอบ"));
    }

    // ---- tokens (FR-TRN-10) ----

    #[test]
    fn usage_reads_input_and_output_tokens() {
        let body = bytes(json!({
            "content": [{ "type": "text", "text": "สวัสดี" }],
            "usage": { "input_tokens": 210, "output_tokens": 37 }
        }));
        assert_eq!(
            usage(&body),
            Usage {
                input: 210,
                output: 37
            }
        );
    }

    #[test]
    fn missing_usage_counts_nothing() {
        assert_eq!(usage(b"not json"), Usage::default());
        assert_eq!(usage(b"{}"), Usage::default());
        let odd = bytes(json!({ "usage": { "input_tokens": "12" } }));
        assert_eq!(usage(&odd), Usage::default());
    }

    // ---- failures ----

    fn error(kind: &str, message: &str) -> Vec<u8> {
        bytes(json!({ "type": "error",
            "error": { "type": kind, "message": message }, "request_id": "req_1" }))
    }

    #[test]
    fn a_bad_key_says_what_to_do() {
        let reason = failure_reason(401, &error("authentication_error", "invalid x-api-key"));
        assert!(reason.contains("API key ของ Claude ไม่ถูกต้อง"), "{reason}");
        assert!(reason.contains("/setup"), "{reason}");
    }

    #[test]
    fn each_failure_has_its_own_reason() {
        let r = |status, kind| failure_reason(status, &error(kind, "m"));
        assert!(r(402, "billing_error").contains("billing"));
        assert!(r(403, "permission_error").contains("ไม่มีสิทธิ์"));
        assert!(r(404, "not_found_error").contains(MODEL));
        assert!(r(429, "rate_limit_error").contains("โควตา"));
        assert!(r(500, "api_error").contains("ชั่วคราว"));
        assert!(r(529, "overloaded_error").contains("ชั่วคราว"));
        let bad = failure_reason(400, &error("invalid_request_error", "max_tokens: too big"));
        assert!(bad.contains("max_tokens: too big"), "{bad}");
        assert!(failure_reason(418, b"").contains("418"));
    }

    #[test]
    fn long_messages_are_cut() {
        let body = error("invalid_request_error", &"x".repeat(1000));
        assert!(failure_reason(400, &body).chars().count() < 300);
    }
}
