//! Gemini tokens of this session for /setup (FR-TRN-10): counts only, never
//! a cost. Each generateContent reply, and each Live Translate message that
//! reports usage, carries `usageMetadata`; the session's total is the sum of
//! those since the session started.

use serde::Serialize;
use serde_json::Value;
use std::ops::Add;

/// Input and output tokens, as `AppState.tokens` (SRS 8.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
}

impl Usage {
    /// The tokens one reply says it used. Output counts thinking too, since
    /// Gemini bills thoughts as output. Missing or broken metadata reads as 0.
    pub fn from_reply(body: &[u8]) -> Usage {
        let v: Value = serde_json::from_slice(body).unwrap_or_default();
        Usage::from_metadata(&v["usageMetadata"])
    }

    /// The tokens in one `usageMetadata`. Live Translate names its output
    /// `responseTokenCount` where generateContent says `candidatesTokenCount`.
    pub fn from_metadata(meta: &Value) -> Usage {
        let count = |key: &str| meta[key].as_u64().unwrap_or(0);
        Usage {
            input: count("promptTokenCount"),
            output: count("candidatesTokenCount")
                + count("responseTokenCount")
                + count("thoughtsTokenCount"),
        }
    }

    /// What was used after `start`, a total read when the session began.
    pub fn since(self, start: Usage) -> Usage {
        Usage {
            input: self.input.saturating_sub(start.input),
            output: self.output.saturating_sub(start.output),
        }
    }
}

impl Add for Usage {
    type Output = Usage;

    fn add(self, other: Usage) -> Usage {
        Usage {
            input: self.input + other.input,
            output: self.output + other.output,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(prompt: u64, candidates: u64, thoughts: Option<u64>) -> Vec<u8> {
        let mut meta = json!({
            "promptTokenCount": prompt,
            "candidatesTokenCount": candidates,
            "totalTokenCount": prompt + candidates + thoughts.unwrap_or(0),
        });
        if let Some(t) = thoughts {
            meta["thoughtsTokenCount"] = json!(t);
        }
        json!({
            "candidates": [{ "content": { "parts": [{ "text": "สวัสดี" }] } }],
            "usageMetadata": meta,
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn reads_input_and_output_with_thoughts() {
        assert_eq!(
            Usage::from_reply(&reply(120, 9, Some(31))),
            Usage {
                input: 120,
                output: 40
            }
        );
        assert_eq!(
            Usage::from_reply(&reply(80, 7, None)),
            Usage {
                input: 80,
                output: 7
            }
        );
    }

    #[test]
    fn no_metadata_counts_nothing() {
        assert_eq!(Usage::from_reply(b"not json"), Usage::default());
        assert_eq!(
            Usage::from_reply(br#"{"candidates": []}"#),
            Usage::default()
        );
        let odd = json!({ "usageMetadata": { "promptTokenCount": "12" } });
        assert_eq!(
            Usage::from_reply(odd.to_string().as_bytes()),
            Usage::default()
        );
    }

    #[test]
    fn session_tokens_sum_the_replies_since_it_started() {
        // Two lines before the session, then three in it.
        let before =
            Usage::from_reply(&reply(100, 5, Some(20))) + Usage::from_reply(&reply(90, 4, None));
        let total = [
            reply(120, 9, Some(31)),
            reply(130, 8, None),
            reply(140, 10, Some(2)),
        ]
        .iter()
        .fold(before, |sum, r| sum + Usage::from_reply(r));
        assert_eq!(
            total.since(before),
            Usage {
                input: 390,
                output: 60
            }
        );
        assert_eq!(before.since(before), Usage::default());
    }

    #[test]
    fn live_metadata_counts_response_tokens_as_output() {
        let meta = json!({
            "promptTokenCount": 52,
            "responseTokenCount": 9,
            "thoughtsTokenCount": 1,
            "totalTokenCount": 62,
        });
        assert_eq!(
            Usage::from_metadata(&meta),
            Usage {
                input: 52,
                output: 10
            }
        );
        assert_eq!(Usage::from_metadata(&Value::Null), Usage::default());
    }

    #[test]
    fn since_never_goes_below_zero() {
        let start = Usage {
            input: 10,
            output: 10,
        };
        assert_eq!(Usage::default().since(start), Usage::default());
    }

    #[test]
    fn serializes_as_app_state_tokens() {
        let v = serde_json::to_value(Usage {
            input: 3,
            output: 4,
        })
        .unwrap();
        assert_eq!(v, json!({ "input": 3, "output": 4 }));
    }
}
