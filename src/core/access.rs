//! Who may call what (FR-SET-08): /reader, /overlay, /events and a few
//! commands answer on the LAN; /setup and every other API answer only on
//! loopback. Unknown /api paths are loopback-only by default.

/// API calls a phone on the LAN may make (SRS 8.2): read state, list and
/// pick a profile, start/stop, switch translator, add a glossary term, read
/// and retranslate lines. The LCD board (#85) reads its status and the
/// latest line as a picture.
const LAN_API: &[(&str, &str)] = &[
    ("GET", "/api/state"),
    ("GET", "/api/profiles"),
    ("PUT", "/api/profile"),
    ("POST", "/api/start"),
    ("POST", "/api/stop"),
    ("PUT", "/api/translator"),
    ("GET", "/api/lines"),
    ("POST", "/api/lines/*/retranslate"),
    ("POST", "/api/glossary"),
    ("GET", "/api/board"),
    ("GET", "/api/board/line.pbm"),
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

/// False for a cross-site write: a non-GET request whose `Origin` names
/// another host:port than `Host`. Stops a web page open in the Mac's browser
/// from POSTing to loopback-only APIs such as /api/obs/setup. Requests
/// without `Origin` (curl, same-origin GET) pass.
pub fn same_origin(method: &str, origin: Option<&str>, host: Option<&str>) -> bool {
    if matches!(method, "GET" | "HEAD" | "OPTIONS") {
        return true;
    }
    let Some(origin) = origin else {
        return true;
    };
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false; // "null" and other opaque origins
    };
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => ":80",
        "https" => ":443",
        _ => return false,
    };
    let norm = |a: &str| {
        let a = a.trim().to_ascii_lowercase();
        a.strip_suffix(default_port).map(str::to_owned).unwrap_or(a)
    };
    host.is_some_and(|h| !authority.contains('/') && norm(authority) == norm(h))
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
    use super::{lan_allowed, same_origin};

    const HOST: Option<&str> = Some("127.0.0.1:8790");

    #[test]
    fn reads_pass_whatever_the_origin() {
        for m in ["GET", "HEAD", "OPTIONS"] {
            assert!(same_origin(m, Some("http://evil.example"), HOST), "{m}");
        }
    }

    #[test]
    fn writes_without_origin_pass() {
        assert!(same_origin("POST", None, HOST));
        assert!(same_origin("POST", None, None));
    }

    #[test]
    fn same_origin_write_passes() {
        assert!(same_origin("POST", Some("http://127.0.0.1:8790"), HOST));
        assert!(same_origin(
            "PUT",
            Some("http://192.168.1.20:8790"),
            Some("192.168.1.20:8790")
        ));
        assert!(same_origin(
            "POST",
            Some("HTTP://LocalHost:8790"),
            Some("localhost:8790")
        ));
        assert!(same_origin(
            "POST",
            Some("http://localhost"),
            Some("localhost:80")
        ));
        assert!(same_origin(
            "POST",
            Some("https://localhost:443"),
            Some("localhost")
        ));
    }

    #[test]
    fn cross_origin_write_is_refused() {
        assert!(!same_origin("POST", Some("http://evil.example"), HOST));
        assert!(!same_origin("POST", Some("http://127.0.0.1:9999"), HOST));
        assert!(!same_origin("DELETE", Some("http://localhost:8790"), HOST));
        assert!(!same_origin("POST", Some("null"), HOST));
        assert!(!same_origin("POST", Some("file:///x"), HOST));
        assert!(!same_origin("POST", Some("http://127.0.0.1:8790"), None));
        assert!(!same_origin(
            "POST",
            Some("http://127.0.0.1:8790/path"),
            HOST
        ));
    }

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
        assert!(lan_allowed("GET", "/api/profiles"));
        assert!(lan_allowed("PUT", "/api/profile"));
    }

    #[test]
    fn board_reads_on_lan_and_nothing_else() {
        assert!(lan_allowed("GET", "/api/board"));
        assert!(lan_allowed("GET", "/api/board/line.pbm"));
        for m in ["POST", "PUT", "DELETE"] {
            assert!(!lan_allowed(m, "/api/board"), "{m}");
            assert!(!lan_allowed(m, "/api/board/line.pbm"), "{m}");
        }
        assert!(!lan_allowed("GET", "/api/board/other"));
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
        assert!(!lan_allowed("PUT", "/api/profiles/abc"));
        assert!(!lan_allowed("DELETE", "/api/profiles/abc/lines"));
        assert!(!lan_allowed("GET", "/api/checklist"));
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
