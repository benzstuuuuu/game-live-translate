//! Live Translate (FR-AUD-04, 07, FR-GLS-06): the setup message with its
//! system instruction and resumption, the audio chunks, and what the server
//! sends back. Pure; the WebSocket lives in `crate::live`.

use super::gemini::Tone;
use super::glossary::Term;
use super::tokens::Usage;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use std::time::Duration;

/// The Live Translate model; also what `lines.model_id` records.
pub const MODEL: &str = "gemini-3.5-live-translate-preview";
/// `lines.engine` of audio lines (SRS 8.4).
pub const ENGINE: &str = "live-translate";
pub const ENDPOINT: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";
/// Audio goes out as 16 kHz signed 16-bit little-endian mono.
pub const SAMPLE_RATE: u32 = 16_000;
/// 100 ms of that audio.
pub const CHUNK_BYTES: usize = 3200;

/// The system instruction a session starts with: the game's glossary (both
/// layers, all terms), the tone and the player's extra prompt from /setup.
/// Worded like condition B of the glossary spike, which got game names right
/// in both the English transcript and the Thai.
pub fn system_instruction(glossary: &[Term], tone: Tone, extra_prompt: &str) -> String {
    let mut p = format!(
        "You translate spoken English video game dialogue into Thai for a player following \
         along live.\nTone: {}.\n",
        tone.instruction()
    );
    if !glossary.is_empty() {
        p.push_str("\nGlossary: translate these terms exactly as given:\n");
        for t in glossary {
            p.push_str(&format!("- {} → {}\n", t.source, t.target));
        }
    }
    let extra = extra_prompt.trim();
    if !extra.is_empty() {
        p.push_str("\nThe player's own instructions:\n");
        p.push_str(extra);
        p.push('\n');
    }
    p
}

/// The first message of a connection: Thai text only, with both
/// transcripts, a sliding context window and resumption (FR-AUD-07). With a
/// `handle` the connection carries on the session that handle came from.
pub fn setup_message(instruction: &str, handle: Option<&str>) -> Value {
    let resume = handle.map_or_else(|| json!({}), |h| json!({ "handle": h }));
    json!({ "setup": {
        "model": format!("models/{MODEL}"),
        "systemInstruction": { "parts": [{ "text": instruction }] },
        "inputAudioTranscription": {},
        "outputAudioTranscription": {},
        "generationConfig": {
            "responseModalities": ["TEXT"],
            "translationConfig": { "targetLanguageCode": "th", "echoTargetLanguage": false },
        },
        "contextWindowCompression": { "slidingWindow": {} },
        "sessionResumption": resume,
    }})
}

/// One chunk of audio (`CHUNK_BYTES` or less at the end).
pub fn audio_message(pcm: &[u8]) -> Value {
    json!({ "realtimeInput": { "audio": {
        "data": B64.encode(pcm),
        "mimeType": format!("audio/pcm;rate={SAMPLE_RATE}"),
    }}})
}

/// Says no more audio is coming.
pub fn audio_end_message() -> Value {
    json!({ "realtimeInput": { "audioStreamEnd": true } })
}

/// What one server message carries, in the order to act on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The setup was accepted; audio may flow.
    Ready,
    /// A piece of the English transcript; empty (no `text`) in silence.
    En(String),
    /// A piece of the Thai translation; empty in silence.
    Th(String),
    /// Tokens this message reports (FR-TRN-10).
    Usage(Usage),
    /// The server will close the connection after this long.
    GoAway(Duration),
    /// The session can resume on a new connection with this handle.
    Handle(String),
}

/// The events of one server message; anything unreadable has none.
pub fn parse(message: &str) -> Vec<Event> {
    let v: Value = serde_json::from_str(message).unwrap_or_default();
    let mut out = Vec::new();
    if v.get("setupComplete").is_some() {
        out.push(Event::Ready);
    }
    let content = &v["serverContent"];
    // A transcript with no `text` (only `languageCode`) is how silence comes.
    let piece = |key: &str| {
        content
            .get(key)
            .map(|t| t["text"].as_str().unwrap_or_default().to_owned())
    };
    if let Some(en) = piece("inputTranscription") {
        out.push(Event::En(en));
    }
    if let Some(th) = piece("outputTranscription") {
        out.push(Event::Th(th));
    }
    if let Some(meta) = v.get("usageMetadata") {
        out.push(Event::Usage(Usage::from_metadata(meta)));
    }
    if let Some(away) = v.get("goAway") {
        out.push(Event::GoAway(time_left(&away["timeLeft"])));
    }
    let update = &v["sessionResumptionUpdate"];
    if update["resumable"] == true {
        if let Some(h) = update["newHandle"].as_str().filter(|h| !h.is_empty()) {
            out.push(Event::Handle(h.to_owned()));
        }
    }
    out
}

/// A protobuf JSON duration like `"50s"` or `"49.5s"`; anything else is no
/// time at all, so the session moves at once.
fn time_left(v: &Value) -> Duration {
    v.as_str()
        .and_then(|s| s.strip_suffix('s'))
        .and_then(|s| s.parse::<f64>().ok())
        .and_then(|s| Duration::try_from_secs_f64(s).ok())
        .unwrap_or_default()
}

/// Thai reason a session ended when the server closed it with `code`, or
/// None for a normal close.
pub fn close_reason(code: u16, reason: &str) -> Option<String> {
    let reason: String = reason.chars().take(200).collect();
    let invalid_key = reason.contains("API key not valid") || reason.contains("API_KEY_INVALID");
    match code {
        1000 => None,
        _ if invalid_key => Some("API key ไม่ถูกต้อง ตรวจว่าคัดลอกมาครบแล้วบันทึกใหม่".into()),
        _ if reason.is_empty() => Some(format!("Live Translate ปิดการเชื่อมต่อ (รหัส {code})")),
        _ => Some(format!("Live Translate ปิดการเชื่อมต่อ (รหัส {code}): {reason}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms() -> Vec<Term> {
        vec![
            Term::new("Paimon", "ไพมอน"),
            Term::new("Captain Rhee", "กัปตันรี"),
        ]
    }

    #[test]
    fn instruction_has_glossary_and_tone_like_the_spike() {
        let p = system_instruction(&terms(), Tone::Casual, "");
        assert_eq!(
            p,
            "You translate spoken English video game dialogue into Thai for a player \
             following along live.\n\
             Tone: casual spoken Thai, the way friends talk; drop polite particles \
             (ครับ/ค่ะ) unless the speaker is clearly being polite.\n\
             \n\
             Glossary: translate these terms exactly as given:\n\
             - Paimon → ไพมอน\n\
             - Captain Rhee → กัปตันรี\n"
        );
    }

    #[test]
    fn instruction_ends_with_the_players_prompt() {
        let p = system_instruction(&terms(), Tone::Formal, "  Mara is a woman.\n");
        assert!(p.contains(Tone::Formal.instruction()));
        assert!(p.ends_with("\nThe player's own instructions:\nMara is a woman.\n"));
        let glossary_at = p.find("Glossary:").unwrap();
        assert!(glossary_at < p.find("Mara").unwrap());
    }

    #[test]
    fn instruction_without_glossary_or_prompt() {
        let p = system_instruction(&[], Tone::Neutral, "   ");
        assert!(p.contains(Tone::Neutral.instruction()));
        assert!(!p.contains("Glossary"));
        assert!(!p.contains("instructions"));
    }

    #[test]
    fn setup_asks_for_thai_text_and_both_transcripts() {
        let m = setup_message("be brief", None);
        let s = &m["setup"];
        assert_eq!(s["model"], "models/gemini-3.5-live-translate-preview");
        assert_eq!(s["systemInstruction"]["parts"][0]["text"], "be brief");
        assert_eq!(s["generationConfig"]["responseModalities"], json!(["TEXT"]));
        assert_eq!(
            s["generationConfig"]["translationConfig"]["targetLanguageCode"],
            "th"
        );
        assert_eq!(s["inputAudioTranscription"], json!({}));
        assert_eq!(s["outputAudioTranscription"], json!({}));
    }

    #[test]
    fn setup_slides_the_context_window_and_asks_for_handles() {
        let m = setup_message("be brief", None);
        let s = &m["setup"];
        assert_eq!(
            s["contextWindowCompression"],
            json!({ "slidingWindow": {} })
        );
        assert_eq!(s["sessionResumption"], json!({}));
        let again = setup_message("be brief", Some("h-42"));
        assert_eq!(
            again["setup"]["sessionResumption"],
            json!({ "handle": "h-42" })
        );
        assert_eq!(again["setup"]["systemInstruction"], s["systemInstruction"]);
    }

    #[test]
    fn audio_goes_as_base64_pcm_at_16k() {
        let m = audio_message(&[1, 0, 255, 127]);
        assert_eq!(m["realtimeInput"]["audio"]["data"], "AQD/fw==");
        assert_eq!(
            m["realtimeInput"]["audio"]["mimeType"],
            "audio/pcm;rate=16000"
        );
        assert_eq!(
            audio_end_message(),
            json!({ "realtimeInput": { "audioStreamEnd": true } })
        );
        assert_eq!(CHUNK_BYTES, SAMPLE_RATE as usize * 2 / 10);
    }

    #[test]
    fn parses_setup_complete_and_go_away() {
        assert_eq!(parse(r#"{"setupComplete": {}}"#), vec![Event::Ready]);
        assert_eq!(
            parse(r#"{"goAway": {"timeLeft": "50s"}}"#),
            vec![Event::GoAway(Duration::from_secs(50))]
        );
        assert_eq!(
            parse(r#"{"goAway": {"timeLeft": "5.5s"}}"#),
            vec![Event::GoAway(Duration::from_millis(5500))]
        );
        for odd in [r#"{"goAway": {}}"#, r#"{"goAway": {"timeLeft": "-3s"}}"#] {
            assert_eq!(parse(odd), vec![Event::GoAway(Duration::ZERO)], "{odd}");
        }
    }

    #[test]
    fn parses_new_handles_only_when_resumable() {
        let update = |v: Value| parse(&json!({ "sessionResumptionUpdate": v }).to_string());
        assert_eq!(
            update(json!({ "resumable": true, "newHandle": "h-1" })),
            vec![Event::Handle("h-1".into())]
        );
        assert_eq!(
            update(json!({ "resumable": false, "newHandle": "h-2" })),
            vec![]
        );
        assert_eq!(
            update(json!({ "resumable": true, "newHandle": "" })),
            vec![]
        );
        assert_eq!(update(json!({ "resumable": true })), vec![]);
    }

    #[test]
    fn transcript_without_text_is_an_empty_piece() {
        // In silence Live Translate sends both transcripts with only a
        // language code, about once a second: the spike's empty pieces.
        let en = json!({ "serverContent": { "inputTranscription": { "languageCode": "en" } } });
        let th = json!({ "serverContent": { "outputTranscription": { "languageCode": "th" } } });
        assert_eq!(parse(&en.to_string()), vec![Event::En(String::new())]);
        assert_eq!(parse(&th.to_string()), vec![Event::Th(String::new())]);
        let none = json!({ "serverContent": {}, "usageMetadata": { "promptTokenCount": 25 } });
        assert_eq!(
            parse(&none.to_string()),
            vec![Event::Usage(Usage {
                input: 25,
                output: 0
            })]
        );
    }

    #[test]
    fn parses_both_transcripts_english_first() {
        let m = json!({ "serverContent": {
            "outputTranscription": { "text": "ไพมอน" },
            "inputTranscription": { "text": " Paimon" },
        }});
        assert_eq!(
            parse(&m.to_string()),
            vec![Event::En(" Paimon".into()), Event::Th("ไพมอน".into())]
        );
        let empty = json!({ "serverContent": { "inputTranscription": { "text": "" } } });
        assert_eq!(parse(&empty.to_string()), vec![Event::En(String::new())]);
    }

    #[test]
    fn parses_usage_with_the_live_field_names() {
        let m = json!({ "usageMetadata": {
            "promptTokenCount": 50, "responseTokenCount": 12, "totalTokenCount": 62 } });
        assert_eq!(
            parse(&m.to_string()),
            vec![Event::Usage(Usage {
                input: 50,
                output: 12
            })]
        );
    }

    #[test]
    fn unknown_or_broken_messages_have_no_events() {
        assert_eq!(parse("not json"), vec![]);
        assert_eq!(parse(r#"{"sessionResumptionUpdate": {}}"#), vec![]);
        assert_eq!(
            parse(r#"{"serverContent": {"turnComplete": true}}"#),
            vec![]
        );
    }

    #[test]
    fn close_reasons() {
        assert_eq!(close_reason(1000, ""), None);
        assert_eq!(
            close_reason(1007, "API key not valid. Please pass a valid API key."),
            Some("API key ไม่ถูกต้อง ตรวจว่าคัดลอกมาครบแล้วบันทึกใหม่".into())
        );
        assert_eq!(
            close_reason(1011, ""),
            Some("Live Translate ปิดการเชื่อมต่อ (รหัส 1011)".into())
        );
        let long = "x".repeat(500);
        let r = close_reason(1008, &long).unwrap();
        assert!(r.starts_with("Live Translate ปิดการเชื่อมต่อ (รหัส 1008): xxx"));
        assert!(r.chars().count() < 260);
    }
}
