//! Fallback (FR-TRN-04, 05): when the selected translator fails a line or
//! takes longer than `LIMIT`, the backup translates that line, and the app
//! stays on the backup until the selected one works again. The selected one
//! gets another try every `RETRY_EVERY`, or on the next line after the user
//! picks it again ("ลอง Gemini อีกครั้ง").

use serde::Serialize;
use std::time::{Duration, Instant};

/// Longest wait for the selected translator before the backup takes the line
/// (FR-TRN-04, NFR-02).
pub const LIMIT: Duration = Duration::from_secs(3);
/// How long fallback lasts before a line tries the selected translator again
/// (FR-TRN-05).
pub const RETRY_EVERY: Duration = Duration::from_secs(30);

/// The engine that stands in for `engine`, if it has one. Only Gemini,
/// Claude (API or CLI) and Codex do: they need the network, Apple
/// Translation does not.
pub fn backup_for(engine: &str) -> Option<&'static str> {
    match engine {
        "gemini" | "claude" | "claude-cli" | "codex-cli" => Some("apple"),
        _ => None,
    }
}

/// How the reasons name the selected engine.
fn name_of(engine: &str) -> &str {
    match engine {
        "gemini" => "Gemini",
        "claude" => "Claude",
        "claude-cli" => "Claude Code",
        "codex-cli" => "Codex",
        other => other,
    }
}

/// Why the selected translator gave no line.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// No answer within `LIMIT`.
    Slow,
    /// It answered with an error; the Thai reason it gave.
    Failed(String),
}

impl Failure {
    /// Thai, shown as the cause beside "ใช้ Apple ชั่วคราว"; `engine` is the
    /// one that failed.
    pub fn reason(&self, engine: &str) -> String {
        let name = name_of(engine);
        match self {
            Failure::Slow => format!("{name} ไม่ตอบภายใน {} วินาที", LIMIT.as_secs()),
            Failure::Failed(r) if r.trim().is_empty() => format!("{name} แปลไม่สำเร็จ"),
            Failure::Failed(r) => r.clone(),
        }
    }
}

/// `AppState.fallback` (SRS 8.4).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct View {
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Whether the app is on the backup, why, and when to try the selected
/// translator again. Times come from the caller so tests need no clock.
#[derive(Debug, Default)]
pub struct Fallback {
    on: Option<On>,
}

#[derive(Debug)]
struct On {
    reason: String,
    retry_at: Instant,
}

impl Fallback {
    /// Should the next line go to the selected translator first?
    pub fn try_selected(&self, now: Instant) -> bool {
        self.on.as_ref().is_none_or(|on| now >= on.retry_at)
    }

    /// Back to the selected translator: it gave a line, or a new session or
    /// another translator starts over.
    pub fn reset(&mut self) {
        self.on = None;
    }

    /// The selected translator failed at `now`: use the backup until
    /// `RETRY_EVERY` from now.
    pub fn failed(&mut self, why: &Failure, engine: &str, now: Instant) {
        self.on = Some(On {
            reason: why.reason(engine),
            retry_at: now + RETRY_EVERY,
        });
    }

    /// The user picked the selected translator again: the next line tries it.
    /// Fallback and its reason stay until that line works.
    pub fn retry_now(&mut self, now: Instant) {
        if let Some(on) = &mut self.on {
            on.retry_at = now;
        }
    }

    pub fn view(&self) -> View {
        View {
            active: self.on.is_some(),
            reason: self.on.as_ref().map(|on| on.reason.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_at(why: Failure, now: Instant) -> Fallback {
        let mut f = Fallback::default();
        f.failed(&why, "gemini", now);
        f
    }

    // ---- which engine stands in ----

    #[test]
    fn only_the_network_engines_have_a_backup_and_it_is_apple() {
        assert_eq!(backup_for("gemini"), Some("apple"));
        assert_eq!(backup_for("claude"), Some("apple"));
        assert_eq!(backup_for("claude-cli"), Some("apple"));
        assert_eq!(backup_for("codex-cli"), Some("apple"));
        assert_eq!(backup_for("apple"), None);
        assert_eq!(backup_for("fake"), None);
        assert_eq!(backup_for(""), None);
    }

    // ---- reason text ----

    #[test]
    fn slow_names_the_limit() {
        assert_eq!(Failure::Slow.reason("gemini"), "Gemini ไม่ตอบภายใน 3 วินาที");
    }

    #[test]
    fn an_error_keeps_the_reason_gemini_gave() {
        let why = Failure::Failed("ต่อ Gemini ไม่ได้ ตรวจการเชื่อมต่ออินเทอร์เน็ต".into());
        assert_eq!(
            why.reason("gemini"),
            "ต่อ Gemini ไม่ได้ ตรวจการเชื่อมต่ออินเทอร์เน็ต"
        );
    }

    #[test]
    fn reasons_name_the_engine_that_failed() {
        assert_eq!(Failure::Slow.reason("claude"), "Claude ไม่ตอบภายใน 3 วินาที");
        assert_eq!(
            Failure::Failed(String::new()).reason("claude"),
            "Claude แปลไม่สำเร็จ"
        );
        let mut f = Fallback::default();
        f.failed(&Failure::Slow, "claude", Instant::now());
        assert_eq!(f.view().reason.as_deref(), Some("Claude ไม่ตอบภายใน 3 วินาที"));
        assert_eq!(
            Failure::Slow.reason("claude-cli"),
            "Claude Code ไม่ตอบภายใน 3 วินาที"
        );
        assert_eq!(Failure::Slow.reason("codex-cli"), "Codex ไม่ตอบภายใน 3 วินาที");
    }

    #[test]
    fn an_empty_error_still_says_something() {
        assert_eq!(
            Failure::Failed("  ".into()).reason("gemini"),
            "Gemini แปลไม่สำเร็จ"
        );
    }

    // ---- starts on the selected translator ----

    #[test]
    fn a_fresh_one_uses_the_selected_translator() {
        let f = Fallback::default();
        assert!(f.try_selected(Instant::now()));
        assert_eq!(f.view(), View::default());
    }

    // ---- too slow (FR-TRN-04) ----

    #[test]
    fn too_slow_turns_fallback_on_with_its_reason() {
        let t0 = Instant::now();
        let f = failed_at(Failure::Slow, t0);
        assert_eq!(
            f.view(),
            View {
                active: true,
                reason: Some("Gemini ไม่ตอบภายใน 3 วินาที".into())
            }
        );
        assert!(!f.try_selected(t0));
    }

    // ---- error (FR-TRN-04) ----

    #[test]
    fn an_error_turns_fallback_on_and_later_lines_skip_gemini() {
        let t0 = Instant::now();
        let f = failed_at(Failure::Failed("API key ไม่ถูกต้อง".into()), t0);
        assert_eq!(f.view().reason.as_deref(), Some("API key ไม่ถูกต้อง"));
        assert!(!f.try_selected(t0 + Duration::from_secs(1)));
        assert!(!f.try_selected(t0 + RETRY_EVERY - Duration::from_millis(1)));
    }

    #[test]
    fn a_new_failure_replaces_the_reason_and_pushes_the_retry_back() {
        let t0 = Instant::now();
        let mut f = failed_at(Failure::Slow, t0);
        let t1 = t0 + RETRY_EVERY;
        f.failed(&Failure::Failed("ต่อ Gemini ไม่ได้".into()), "gemini", t1);
        assert_eq!(f.view().reason.as_deref(), Some("ต่อ Gemini ไม่ได้"));
        assert!(!f.try_selected(t1 + Duration::from_secs(29)));
        assert!(f.try_selected(t1 + RETRY_EVERY));
    }

    // ---- comes back (FR-TRN-05) ----

    #[test]
    fn tries_gemini_again_after_thirty_seconds() {
        let t0 = Instant::now();
        let f = failed_at(Failure::Slow, t0);
        assert!(f.try_selected(t0 + RETRY_EVERY));
        assert!(f.try_selected(t0 + RETRY_EVERY + Duration::from_secs(5)));
        assert!(f.view().active, "still on the backup until the try works");
    }

    #[test]
    fn a_line_from_gemini_ends_fallback() {
        let t0 = Instant::now();
        let mut f = failed_at(Failure::Slow, t0);
        f.reset();
        assert_eq!(f.view(), View::default());
        assert!(f.try_selected(t0));
    }

    #[test]
    fn reset_without_fallback_changes_nothing() {
        let mut f = Fallback::default();
        f.reset();
        assert_eq!(f.view(), View::default());
    }

    #[test]
    fn retry_now_lets_the_next_line_try_gemini_but_keeps_the_reason() {
        let t0 = Instant::now();
        let mut f = failed_at(Failure::Slow, t0);
        let t1 = t0 + Duration::from_secs(4);
        f.retry_now(t1);
        assert!(f.try_selected(t1));
        assert!(f.view().active);
        assert_eq!(f.view().reason.as_deref(), Some("Gemini ไม่ตอบภายใน 3 วินาที"));
    }

    #[test]
    fn retry_now_without_fallback_does_nothing() {
        let mut f = Fallback::default();
        f.retry_now(Instant::now());
        assert_eq!(f.view(), View::default());
    }

    // ---- AppState.fallback (SRS 8.4) ----

    #[test]
    fn view_serializes_as_srs_shape() {
        let off = serde_json::to_value(View::default()).unwrap();
        assert_eq!(off, serde_json::json!({ "active": false }));
        let on = failed_at(Failure::Slow, Instant::now()).view();
        assert_eq!(
            serde_json::to_value(on).unwrap(),
            serde_json::json!({ "active": true, "reason": "Gemini ไม่ตอบภายใน 3 วินาที" })
        );
    }
}
