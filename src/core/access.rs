//! Who may call what (FR-SET-08): /reader, /overlay, /events and a few
//! commands answer on the LAN; /setup and every other API answer only on
//! loopback. Unknown /api paths are loopback-only by default.

/// API calls a phone on the LAN may make (SRS 8.2): read state, start/stop,
/// switch translator, add a glossary term, read and retranslate lines.
const LAN_API: &[(&str, &str)] = &[
    ("GET", "/api/state"),
    ("GET", "/api/profiles"),
    ("POST", "/api/start"),
    ("POST", "/api/stop"),
    ("PUT", "/api/translator"),
    ("GET", "/api/lines"),
    ("POST", "/api/lines/*/retranslate"),
    ("POST", "/api/glossary"),
];

/// True when a caller that is not loopback may make this request.
pub fn lan_allowed(method: &str, path: &str) -> bool {
    if path == "/setup" || path.starts_with("/setup/") {
        return false;
    }
    if path != "/api" && !path.starts_with("/api/") {
        return true;
    }
    LAN_API
        .iter()
        .any(|(m, pattern)| *m == method && matches(pattern, path))
}

/// `*` matches exactly one non-empty path segment.
fn matches(pattern: &str, path: &str) -> bool {
    let mut p = pattern.split('/');
    let mut s = path.split('/');
    loop {
        match (p.next(), s.next()) {
            (None, None) => return true,
            (Some("*"), Some(seg)) if !seg.is_empty() => {}
            (Some(a), Some(b)) if a == b => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::lan_allowed;

    #[test]
    fn pages_and_events_open_on_lan() {
        for path in [
            "/",
            "/reader",
            "/overlay",
            "/events",
            "/tokens.css",
            "/fonts/a.woff2",
        ] {
            assert!(lan_allowed("GET", path), "{path}");
        }
    }

    #[test]
    fn setup_is_loopback_only() {
        assert!(!lan_allowed("GET", "/setup"));
        assert!(!lan_allowed("GET", "/setup/"));
        assert!(!lan_allowed("POST", "/setup/anything"));
    }

    #[test]
    fn setup_prefix_without_slash_is_not_setup() {
        assert!(lan_allowed("GET", "/setupx"));
    }

    #[test]
    fn lan_commands_allowed() {
        assert!(lan_allowed("GET", "/api/state"));
        assert!(lan_allowed("POST", "/api/start"));
        assert!(lan_allowed("POST", "/api/stop"));
        assert!(lan_allowed("PUT", "/api/translator"));
        assert!(lan_allowed("POST", "/api/glossary"));
        assert!(lan_allowed("GET", "/api/lines"));
        assert!(lan_allowed("POST", "/api/lines/abc-123/retranslate"));
    }

    #[test]
    fn method_must_match() {
        assert!(!lan_allowed("DELETE", "/api/stop"));
        assert!(!lan_allowed("POST", "/api/state"));
    }

    #[test]
    fn settings_secrets_and_profile_edits_are_loopback_only() {
        assert!(!lan_allowed("GET", "/api/settings"));
        assert!(!lan_allowed("PUT", "/api/settings"));
        assert!(!lan_allowed("PUT", "/api/secret/gemini-key"));
        assert!(!lan_allowed("POST", "/api/profiles"));
        assert!(!lan_allowed("DELETE", "/api/profiles/abc"));
        assert!(!lan_allowed("PUT", "/api/glossary/abc"));
        assert!(!lan_allowed("GET", "/api/obs/still"));
        assert!(!lan_allowed("GET", "/api"));
    }

    #[test]
    fn wildcard_needs_exactly_one_segment() {
        assert!(!lan_allowed("POST", "/api/lines//retranslate"));
        assert!(!lan_allowed("POST", "/api/lines/a/b/retranslate"));
        assert!(!lan_allowed("POST", "/api/lines/retranslate"));
        assert!(!lan_allowed("GET", "/api/lines/extra"));
    }
}
