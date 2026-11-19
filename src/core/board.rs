//! What the LCD board reads (#85): `/api/board` as small JSON, and the
//! caching and size rules of `/api/board/line.pbm`.

use crate::core::error::ErrorCode;
use serde::Serialize;

/// Image size when the board asks for none, or asks with something that is
/// not a number.
pub const DEFAULT_SIZE: (u32, u32) = (400, 120);
const WIDTH: (u32, u32) = (8, 800);
const HEIGHT: (u32, u32) = (8, 600);

/// Body of `GET /api/board`.
#[derive(Debug, PartialEq, Serialize)]
pub struct Board<'a> {
    /// `idle`, `translating` or `error`, as in `/api/state`.
    pub status: &'a str,
    pub profile_name: Option<&'a str>,
    pub error_code: Option<ErrorCode>,
    pub line: Option<BoardLine<'a>>,
}

/// The latest line of the session.
#[derive(Debug, PartialEq, Serialize)]
pub struct BoardLine<'a> {
    pub id: &'a str,
    pub source: &'a str,
    pub source_text: &'a str,
    pub target_text: &'a str,
    #[serde(rename = "final")]
    pub done: bool,
    pub rev: String,
}

impl<'a> BoardLine<'a> {
    pub fn new(
        id: &'a str,
        source: &'a str,
        source_text: &'a str,
        target_text: &'a str,
        done: bool,
    ) -> Self {
        Self {
            id,
            source,
            source_text,
            target_text,
            done,
            rev: rev(id, source_text, target_text),
        }
    }
}

/// Short hash of a line's id and texts: changes when the line is rewritten
/// in place (audio still hearing the sentence, retranslate). FNV-1a, so it
/// stays the same across builds and restarts.
pub fn rev(id: &str, source_text: &str, target_text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in [id, source_text, target_text] {
        // The 0xFF after each part never occurs in UTF-8, so ("ab", "c")
        // and ("a", "bc") differ.
        for b in part.bytes().chain([0xFF]) {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{hash:016x}")
}

/// `w` and `h` of the image from the query: kept in range, and the default
/// when missing or not a whole number.
pub fn size(w: Option<&str>, h: Option<&str>) -> (u32, u32) {
    (
        dimension(w, DEFAULT_SIZE.0, WIDTH),
        dimension(h, DEFAULT_SIZE.1, HEIGHT),
    )
}

fn dimension(raw: Option<&str>, default: u32, (min, max): (u32, u32)) -> u32 {
    match raw.map(|r| r.trim().parse::<i64>()) {
        Some(Ok(n)) => n.clamp(min.into(), max.into()) as u32,
        _ => default,
    }
}

/// Strong ETag of the image: the line's `rev` and the size.
pub fn etag(rev: &str, w: u32, h: u32) -> String {
    format!("\"{rev}-{w}x{h}\"")
}

/// True when `If-None-Match` names `etag` (or is `*`): answer 304.
pub fn not_modified(if_none_match: Option<&str>, etag: &str) -> bool {
    if_none_match.is_some_and(|v| {
        v.split(',')
            .map(str::trim)
            .any(|t| t == "*" || t.strip_prefix("W/").unwrap_or(t) == etag)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rev_is_short_and_stable() {
        let r = rev("id1", "Hello", "สวัสดี");
        assert_eq!(r.len(), 16);
        assert_eq!(r, rev("id1", "Hello", "สวัสดี"));
    }

    #[test]
    fn rev_changes_with_any_part() {
        let r = rev("id1", "Hello", "สวัสดี");
        assert_ne!(r, rev("id2", "Hello", "สวัสดี"));
        assert_ne!(r, rev("id1", "Hello there", "สวัสดี"));
        assert_ne!(r, rev("id1", "Hello", "สวัสดีครับ"));
    }

    #[test]
    fn rev_keeps_parts_apart() {
        assert_ne!(rev("ab", "c", ""), rev("a", "bc", ""));
        assert_ne!(rev("", "ab", "c"), rev("", "a", "bc"));
    }

    #[test]
    fn board_json_with_line() {
        let board = Board {
            status: "translating",
            profile_name: Some("Elden Ring"),
            error_code: None,
            line: Some(BoardLine::new("l1", "audio", "Hi", "หวัดดี", false)),
        };
        assert_eq!(
            serde_json::to_value(&board).unwrap(),
            json!({
                "status": "translating",
                "profile_name": "Elden Ring",
                "error_code": null,
                "line": {
                    "id": "l1",
                    "source": "audio",
                    "source_text": "Hi",
                    "target_text": "หวัดดี",
                    "final": false,
                    "rev": rev("l1", "Hi", "หวัดดี"),
                }
            })
        );
    }

    #[test]
    fn board_json_without_line_keeps_every_key() {
        let board = Board {
            status: "error",
            profile_name: None,
            error_code: Some(ErrorCode::ObsUnreachable),
            line: None,
        };
        assert_eq!(
            serde_json::to_value(&board).unwrap(),
            json!({
                "status": "error",
                "profile_name": null,
                "error_code": "obs_unreachable",
                "line": null,
            })
        );
    }

    #[test]
    fn size_defaults() {
        assert_eq!(size(None, None), (400, 120));
        assert_eq!(size(Some("abc"), Some("")), (400, 120));
        assert_eq!(size(Some("1.5"), Some("12px")), (400, 120));
    }

    #[test]
    fn size_in_range_is_kept() {
        assert_eq!(size(Some("296"), Some("128")), (296, 128));
        assert_eq!(size(Some(" 8 "), Some("600")), (8, 600));
    }

    #[test]
    fn size_out_of_range_is_clamped() {
        assert_eq!(size(Some("0"), Some("-5")), (8, 8));
        assert_eq!(size(Some("5000"), Some("99999999999999")), (800, 600));
    }

    #[test]
    fn etag_names_rev_and_size() {
        assert_eq!(etag("abc", 400, 120), "\"abc-400x120\"");
    }

    #[test]
    fn matching_if_none_match_is_not_modified() {
        let tag = etag("abc", 400, 120);
        assert!(not_modified(Some(&tag), &tag));
        assert!(not_modified(Some(&format!("W/{tag}")), &tag));
        assert!(not_modified(Some(&format!("\"x\", {tag}")), &tag));
        assert!(not_modified(Some("*"), &tag));
    }

    #[test]
    fn other_or_missing_if_none_match_is_modified() {
        let tag = etag("abc", 400, 120);
        assert!(!not_modified(None, &tag));
        assert!(!not_modified(Some(""), &tag));
        assert!(!not_modified(Some(&etag("abc", 400, 121)), &tag));
        assert!(!not_modified(Some("abc-400x120"), &tag));
    }
}
