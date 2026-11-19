//! Live Translate (FR-AUD-04, FR-GLS-06): the setup message with its system
//! instruction, the audio chunks, and what the server sends back. Pure; the
//! WebSocket lives in `crate::live`.

use super::gemini::Tone;
use super::glossary::Term;
use super::tokens::Usage;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};

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

/// The first message of a session: Thai text only, with both transcripts.
pub fn setup_message(instruction: &str) -> Value {
    json!({ "setup": {
        "model": format!("models/{MODEL}"),
        "systemInstruction": { "parts": [{ "text": instruction }] },
        "inputAudioTranscription": {},
        "outputAudioTranscription": {},
        "generationConfig": {
            "responseModalities": ["TEXT"],
            "translationConfig": { "targetLanguageCode": "th", "echoTargetLanguage": false },
        },
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
    /// A piece of the English transcript; empty in silence.
    En(String),
    /// A piece of the Thai translation; empty in silence.
    Th(String),
    /// Tokens this message reports (FR-TRN-10).
    Usage(Usage),
    /// The server will close the connection soon (handled in #71).
    GoAway,
}

/// The events of one server message; anything unreadable has none.
pub fn parse(message: &str) -> Vec<Event> {
    let v: Value = serde_json::from_str(message).unwrap_or_default();
    let mut out = Vec::new();
    if v.get("setupComplete").is_some() {
        out.push(Event::Ready);
    }
    let content = &v["serverContent"];
    let piece = |key: &str| content[key]["text"].as_str().map(str::to_owned);
    if let Some(en) = piece("inputTranscription") {
        out.push(Event::En(en));
    }
    if let Some(th) = piece("outputTranscription").or_else(|| model_text(content)) {
        out.push(Event::Th(th));
    }
    if let Some(meta) = v.get("usageMetadata") {
        out.push(Event::Usage(Usage::from_metadata(meta)));
    }
    if v.get("goAway").is_some() {
        out.push(Event::GoAway);
    }
    out
}

/// The model's own text parts, thoughts left out: where the Thai may come
/// with TEXT replies when there is no output transcript (the glossary spike
/// scored Thai from either). None when the message has no text part.
fn model_text(content: &Value) -> Option<String> {
    let texts: Vec<&str> = content["modelTurn"]["parts"]
        .as_array()?
        .iter()
        .filter(|p| p["thought"] != Value::Bool(true))
        .filter_map(|p| p["text"].as_str())
        .collect();
    (!texts.is_empty()).then(|| texts.concat())
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
        let m = setup_message("be brief");
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
            vec![Event::GoAway]
        );
    }

    #[test]
    fn thai_as_model_text_counts_when_there_is_no_output_transcript() {
        // With TEXT replies the Thai may come as the model's text parts.
        let m = json!({ "serverContent": { "modelTurn": { "parts": [
            { "text": "คิดก่อน", "thought": true },
            { "text": " ไพมอน" },
            { "text": " รอด้วย" },
        ]}}});
        assert_eq!(
            parse(&m.to_string()),
            vec![Event::Th(" ไพมอน รอด้วย".into())]
        );
        let empty = json!({ "serverContent": { "modelTurn": { "parts": [{ "text": "" }] } } });
        assert_eq!(parse(&empty.to_string()), vec![Event::Th(String::new())]);
        let both = json!({ "serverContent": {
            "outputTranscription": { "text": "ไพมอน" },
            "modelTurn": { "parts": [{ "text": "ไพมอน" }] },
        }});
        assert_eq!(parse(&both.to_string()), vec![Event::Th("ไพมอน".into())]);
        let no_text =
            json!({ "serverContent": { "modelTurn": { "parts": [{ "inlineData": {} }] } } });
        assert_eq!(parse(&no_text.to_string()), vec![]);
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
