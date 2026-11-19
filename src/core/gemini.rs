//! Gemini translation (FR-TRN-03, 09, 12) and the translation settings of
//! /setup (FR-SET-03): what to send, how to read the reply, and why a call
//! failed. Pure; the HTTP call lives in `crate::translate::gemini`.

use super::glossary::Term;
/// The jobs that can each use their own model (FR-TRN-13).
pub use super::jobs::Job;
use super::prompt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MAX_CONTEXT_LINES: i64 = 10;
pub const MAX_EXTRA_PROMPT_CHARS: usize = 2000;
pub const MAX_MODELS: usize = 12;
const MAX_MODEL_ID: usize = 64;
const MAX_KEY: usize = 256;
/// Thinking level that sends no `thinkingConfig`: the model's own default.
/// Offered only for model IDs we do not know.
pub const MODEL_DEFAULT: &str = "default";

/// Thai register of the translation; default is spoken Thai (FR-SET-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    #[default]
    Casual,
    Neutral,
    Formal,
}

impl Tone {
    pub fn instruction(self) -> &'static str {
        match self {
            Tone::Casual => {
                "casual spoken Thai, the way friends talk; drop polite particles \
                 (ครับ/ค่ะ) unless the speaker is clearly being polite"
            }
            Tone::Neutral => {
                "plain everyday Thai, polite but not stiff, like subtitles on a \
                 streaming service"
            }
            Tone::Formal => "formal written Thai with full polite forms and no slang",
        }
    }
}

/// One Gemini model the user can pick, with its own thinking level (FR-TRN-12).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub thinking: String,
}

/// Translation settings in settings.json (SRS 7.4).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    pub context_lines: u8,
    pub tone: Tone,
    pub extra_prompt: String,
    pub gemini: GeminiSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GeminiSettings {
    pub models: Vec<Model>,
    /// The model that translates live lines.
    pub live_model: String,
    /// The model for "แปลใหม่" on /reader; empty = the live model (FR-TRN-13).
    pub quality_model: String,
}

impl Default for Settings {
    fn default() -> Self {
        let model = |id: &str, thinking: &str| Model {
            id: id.into(),
            thinking: thinking.into(),
        };
        Self {
            context_lines: 3,
            tone: Tone::Casual,
            extra_prompt: String::new(),
            gemini: GeminiSettings {
                // `low` keeps subtitle lines fast and cheap (research/gemini-pricing).
                models: vec![
                    model("gemini-3.8-flash", "low"),
                    model("gemini-4-hydrogen", "low"),
                    model("gemini-4-carbon", "medium"),
                ],
                live_model: "gemini-3.8-flash".into(),
                quality_model: String::new(),
            },
        }
    }
}

/// A change from /setup: only the keys present are changed (FR-SET-06).
#[derive(Debug, Default, Deserialize)]
pub struct Patch {
    pub context_lines: Option<i64>,
    pub tone: Option<Tone>,
    pub extra_prompt: Option<String>,
    pub gemini: Option<GeminiPatch>,
}

#[derive(Debug, Default, Deserialize)]
pub struct GeminiPatch {
    pub models: Option<Vec<Model>>,
    pub live_model: Option<String>,
    /// Empty or blank sets it back to the live model.
    pub quality_model: Option<String>,
}

impl Settings {
    /// These settings with `patch` applied, or a Thai reason it is refused.
    pub fn apply(&self, patch: Patch) -> Result<Settings, String> {
        let context_lines = match patch.context_lines {
            Some(n) if (0..=MAX_CONTEXT_LINES).contains(&n) => n as u8,
            Some(_) => return Err(format!("บรรทัดบริบทต้องอยู่ระหว่าง 0 ถึง {MAX_CONTEXT_LINES}")),
            None => self.context_lines,
        };
        let extra_prompt = match patch.extra_prompt {
            Some(p) if p.trim().chars().count() > MAX_EXTRA_PROMPT_CHARS => {
                return Err(format!(
                    "prompt เพิ่มเติมยาวได้ไม่เกิน {MAX_EXTRA_PROMPT_CHARS} ตัวอักษร"
                ))
            }
            Some(p) => p.trim().to_owned(),
            None => self.extra_prompt.clone(),
        };
        let gemini = patch.gemini.unwrap_or_default();
        let models = match gemini.models {
            Some(list) => models(list)?,
            None => self.gemini.models.clone(),
        };
        let live_model = match gemini.live_model {
            Some(raw) => listed(&models, &raw)?,
            None if models.iter().any(|m| m.id == self.gemini.live_model) => {
                self.gemini.live_model.clone()
            }
            None => models[0].id.clone(),
        };
        let quality_model = match gemini.quality_model {
            Some(raw) if raw.trim().is_empty() => String::new(),
            Some(raw) => listed(&models, &raw)?,
            None if models.iter().any(|m| m.id == self.gemini.quality_model) => {
                self.gemini.quality_model.clone()
            }
            None => String::new(),
        };
        Ok(Settings {
            context_lines,
            tone: patch.tone.unwrap_or(self.tone),
            extra_prompt,
            gemini: GeminiSettings {
                models,
                live_model,
                quality_model,
            },
        })
    }

    /// Settings as stored; anything invalid falls back to the defaults.
    pub fn from_stored(value: &Value) -> Settings {
        serde_json::from_value::<Patch>(value.clone())
            .ok()
            .and_then(|p| Settings::default().apply(p).ok())
            .unwrap_or_default()
    }

    /// The model for live lines.
    pub fn live(&self) -> &Model {
        self.gemini
            .models
            .iter()
            .find(|m| m.id == self.gemini.live_model)
            .unwrap_or(&self.gemini.models[0])
    }

    /// The model that does `job`; retranslating uses the live model unless
    /// another one is picked.
    pub fn model(&self, job: Job) -> &Model {
        let quality = &self.gemini.quality_model;
        let picked = self.gemini.models.iter().find(|m| &m.id == quality);
        match (job, picked) {
            (Job::Retranslate, Some(m)) => m,
            _ => self.live(),
        }
    }

    /// What GET /api/settings returns: the settings, the thinking levels each
    /// model accepts, and whether a key is saved. Never the key itself.
    pub fn view(&self, key_saved: bool) -> Value {
        let models: Vec<Value> = self
            .gemini
            .models
            .iter()
            .map(
                |m| json!({ "id": m.id, "thinking": m.thinking, "levels": thinking_levels(&m.id) }),
            )
            .collect();
        json!({
            "context_lines": self.context_lines,
            "tone": self.tone,
            "extra_prompt": self.extra_prompt,
            "gemini": {
                "models": models,
                "live_model": self.gemini.live_model,
                "quality_model": self.gemini.quality_model,
                "key_saved": key_saved,
            },
        })
    }
}

/// Checks a whole model list: 1 to MAX_MODELS entries, path-safe unique IDs,
/// and a thinking level each model accepts. An empty level means the model's
/// first one, so /setup can add a model before it knows the levels.
fn models(list: Vec<Model>) -> Result<Vec<Model>, String> {
    if list.is_empty() {
        return Err("ต้องมี model อย่างน้อยหนึ่งตัว".into());
    }
    if list.len() > MAX_MODELS {
        return Err(format!("เก็บ model ได้ไม่เกิน {MAX_MODELS} ตัว"));
    }
    let mut out: Vec<Model> = Vec::with_capacity(list.len());
    for m in list {
        let id = model_id(&m.id)?;
        if out.iter().any(|o| o.id == id) {
            return Err(format!("{id} อยู่ในรายการแล้ว"));
        }
        let levels = thinking_levels(&id);
        let thinking = match m.thinking.trim().to_ascii_lowercase() {
            t if t.is_empty() => levels[0].to_owned(),
            t => t,
        };
        if !levels.contains(&thinking.as_str()) {
            return Err(format!("{id} ใช้ thinking ระดับ {thinking} ไม่ได้"));
        }
        out.push(Model { id, thinking });
    }
    Ok(out)
}

/// Thinking levels a model accepts (research/gemini-pricing): `minimal` fails
/// on all three known models and `xhigh` is Hydrogen only.
pub fn thinking_levels(model_id: &str) -> &'static [&'static str] {
    match model_id {
        "gemini-3.8-flash" | "gemini-4-carbon" => &["low", "medium", "high"],
        "gemini-4-hydrogen" => &["low", "medium", "high", "xhigh"],
        _ => &[MODEL_DEFAULT, "minimal", "low", "medium", "high"],
    }
}

/// `raw` as the ID of a model in `models`, or why it is not one.
fn listed(models: &[Model], raw: &str) -> Result<String, String> {
    let id = model_id(raw)?;
    if !models.iter().any(|m| m.id == id) {
        return Err(format!("ยังไม่มี {id} ในรายการ model"));
    }
    Ok(id)
}

/// A model ID safe to put in the request path; `models/` is dropped.
pub fn model_id(raw: &str) -> Result<String, String> {
    let id = raw.trim();
    let id = id.strip_prefix("models/").unwrap_or(id);
    let ok_char = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || ".-_".contains(c);
    let first_ok = id
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if first_ok && id.len() <= MAX_MODEL_ID && id.chars().all(ok_char) {
        Ok(id.to_owned())
    } else {
        Err("model ID ใช้ได้เฉพาะ a-z, 0-9, จุด, ขีด และขีดล่าง เช่น gemini-3.8-flash".into())
    }
}

/// A pasted API key, trimmed, or why it cannot be one.
pub fn api_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("ยังไม่ได้ใส่ API key".into());
    }
    if key.len() > MAX_KEY || !key.chars().all(|c| c.is_ascii_graphic()) {
        return Err("API key ต้องเป็นตัวอักษรอังกฤษและตัวเลขต่อกัน ไม่มีช่องว่าง".into());
    }
    Ok(key.to_owned())
}

/// Body of `models/{id}:generateContent` for one line (FR-TRN-03): the line,
/// the earlier lines as context, the glossary terms the line uses
/// (FR-GLS-04), the tone and the extra prompt, with the thinking level of
/// the model that does `job`.
pub fn request_body(
    text: &str,
    context: &[String],
    glossary: &[Term],
    settings: &Settings,
    job: Job,
) -> Value {
    let user = prompt::user(text, context, glossary, settings);
    let mut body = json!({
        "systemInstruction": { "parts": [{ "text": prompt::system(settings) }] },
        "contents": [{ "role": "user", "parts": [{ "text": user }] }],
    });
    let thinking = &settings.model(job).thinking;
    if thinking != MODEL_DEFAULT {
        body["generationConfig"] = json!({ "thinkingConfig": { "thinkingLevel": thinking } });
    }
    body
}

/// The Thai text of a generateContent reply, or a Thai reason there is none.
pub fn parse_reply(body: &[u8]) -> Result<String, String> {
    let v: Value =
        serde_json::from_slice(body).map_err(|_| "อ่านคำตอบของ Gemini ไม่ได้".to_owned())?;
    let candidate = &v["candidates"][0];
    let text: String = candidate["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["thought"] != Value::Bool(true))
        .filter_map(|p| p["text"].as_str())
        .collect();
    let text = text.trim();
    if !text.is_empty() {
        return Ok(text.to_owned());
    }
    if let Some(reason) = v["promptFeedback"]["blockReason"].as_str() {
        return Err(format!("Gemini ไม่ยอมแปลบรรทัดนี้ ({reason})"));
    }
    match candidate["finishReason"].as_str() {
        Some(r) if r != "STOP" => Err(format!("Gemini ไม่ส่งคำแปลกลับมา ({r})")),
        _ => Err("Gemini ไม่ส่งคำแปลกลับมา".into()),
    }
}

/// Thai reason for a failed call, for the key test and the log (FR-TRN-09).
pub fn failure_reason(status: u16, body: &[u8], model: &str) -> String {
    let v: Value = serde_json::from_slice(body).unwrap_or_default();
    let message: String = v["error"]["message"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect();
    let invalid_key = message.contains("API key not valid")
        || v["error"]["details"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|d| d["reason"] == "API_KEY_INVALID");
    match status {
        _ if invalid_key => "API key ไม่ถูกต้อง ตรวจว่าคัดลอกมาครบแล้วบันทึกใหม่".into(),
        401 | 403 => "key นี้ไม่มีสิทธิ์ใช้ Gemini API ตรวจใน Google AI Studio".into(),
        404 => format!("ไม่พบ model {model} ตรวจ model ID ในรายการ"),
        429 => "ใช้ Gemini เกินโควตาหรือถี่เกินไป รอสักครู่หรือตรวจ billing".into(),
        400 => format!("Gemini ไม่รับคำขอ: {message}"),
        500..=599 => "Gemini มีปัญหาชั่วคราว ลองอีกครั้ง".into(),
        _ => format!("Gemini ตอบกลับ HTTP {status}"),
    }
}

/// `text` with every copy of `key` blanked out (NFR-07).
pub fn redact(text: &str, key: &str) -> String {
    if key.is_empty() {
        text.to_owned()
    } else {
        text.replace(key, "[key]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(v: Value) -> Patch {
        serde_json::from_value(v).unwrap()
    }

    fn text_of(body: &Value, path: &str) -> String {
        body.pointer(path)
            .and_then(Value::as_str)
            .unwrap()
            .to_owned()
    }

    // ---- defaults (FR-SET-03, FR-TRN-12) ----

    #[test]
    fn defaults_follow_the_srs() {
        let s = Settings::default();
        assert_eq!(s.context_lines, 3);
        assert_eq!(s.tone, Tone::Casual);
        assert_eq!(s.extra_prompt, "");
        let ids: Vec<_> = s.gemini.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            ["gemini-3.8-flash", "gemini-4-hydrogen", "gemini-4-carbon"]
        );
        assert_eq!(s.live().id, "gemini-3.8-flash");
        for m in &s.gemini.models {
            assert!(thinking_levels(&m.id).contains(&m.thinking.as_str()));
        }
    }

    #[test]
    fn thinking_levels_depend_on_the_model() {
        assert_eq!(
            thinking_levels("gemini-3.8-flash"),
            ["low", "medium", "high"]
        );
        assert_eq!(
            thinking_levels("gemini-4-hydrogen"),
            ["low", "medium", "high", "xhigh"]
        );
        assert_eq!(
            thinking_levels("gemini-4-carbon"),
            ["low", "medium", "high"]
        );
        let custom = thinking_levels("gemini-3.5-flash-lite");
        assert_eq!(custom[0], MODEL_DEFAULT);
        assert!(custom.contains(&"minimal"));
    }

    // ---- patches ----

    #[test]
    fn patch_changes_only_given_keys() {
        let s = Settings::default()
            .apply(patch(json!({ "tone": "formal", "context_lines": 0 })))
            .unwrap();
        assert_eq!(s.tone, Tone::Formal);
        assert_eq!(s.context_lines, 0);
        assert_eq!(s.gemini, Settings::default().gemini);
    }

    #[test]
    fn apply_leaves_the_original_alone() {
        let before = Settings::default();
        let _ = before.apply(patch(json!({ "tone": "neutral" }))).unwrap();
        assert_eq!(before, Settings::default());
    }

    #[test]
    fn context_lines_stay_in_range() {
        let s = Settings::default();
        assert!(s.apply(patch(json!({ "context_lines": 10 }))).is_ok());
        assert!(s.apply(patch(json!({ "context_lines": 11 }))).is_err());
        assert!(s.apply(patch(json!({ "context_lines": -1 }))).is_err());
    }

    #[test]
    fn extra_prompt_is_trimmed_and_capped() {
        let s = Settings::default()
            .apply(patch(
                json!({ "extra_prompt": "  เรียกตัวเอกว่า นักเดินทาง \n" }),
            ))
            .unwrap();
        assert_eq!(s.extra_prompt, "เรียกตัวเอกว่า นักเดินทาง");
        let long = "ก".repeat(MAX_EXTRA_PROMPT_CHARS + 1);
        assert!(s.apply(patch(json!({ "extra_prompt": long }))).is_err());
    }

    #[test]
    fn custom_model_can_be_added_and_used() {
        let mut models = Settings::default().gemini.models;
        models.push(Model {
            id: "models/gemini-3.5-flash-lite".into(),
            thinking: "minimal".into(),
        });
        let s = Settings::default()
            .apply(patch(json!({ "gemini": {
                "models": models, "live_model": "gemini-3.5-flash-lite" } })))
            .unwrap();
        assert_eq!(s.gemini.models.len(), 4);
        assert_eq!(s.live().id, "gemini-3.5-flash-lite");
        assert_eq!(s.live().thinking, "minimal");
    }

    #[test]
    fn thinking_level_must_suit_the_model() {
        let s = Settings::default();
        let bad = json!({ "gemini": { "models": [
            { "id": "gemini-3.8-flash", "thinking": "xhigh" } ] } });
        let err = s.apply(patch(bad)).unwrap_err();
        assert!(err.contains("gemini-3.8-flash"), "{err}");
        let ok = json!({ "gemini": { "models": [
            { "id": "gemini-4-hydrogen", "thinking": "XHigh " } ] } });
        let s = s.apply(patch(ok)).unwrap();
        assert_eq!(s.gemini.models[0].thinking, "xhigh");
    }

    #[test]
    fn empty_thinking_takes_the_models_first_level() {
        let s = Settings::default()
            .apply(patch(json!({ "gemini": { "models": [
                { "id": "gemini-4-carbon", "thinking": "" },
                { "id": "gemini-3.5-flash-lite", "thinking": " " } ] } })))
            .unwrap();
        assert_eq!(s.gemini.models[0].thinking, "low");
        assert_eq!(s.gemini.models[1].thinking, MODEL_DEFAULT);
    }

    #[test]
    fn model_list_rules() {
        let s = Settings::default();
        let m = |id: &str| json!({ "id": id, "thinking": "low" });
        assert!(s
            .apply(patch(json!({ "gemini": { "models": [] } })))
            .is_err());
        let dup = json!({ "gemini": { "models": [m("gemini-3.8-flash"), m("models/gemini-3.8-flash")] } });
        assert!(s.apply(patch(dup)).is_err());
        let many: Vec<_> = (0..=MAX_MODELS)
            .map(|i| m(&format!("gemini-x{i}")))
            .collect();
        assert!(s
            .apply(patch(json!({ "gemini": { "models": many } })))
            .is_err());
    }

    #[test]
    fn live_model_must_be_in_the_list() {
        let s = Settings::default();
        let err = s
            .apply(patch(json!({ "gemini": { "live_model": "gemini-9" } })))
            .unwrap_err();
        assert!(err.contains("gemini-9"), "{err}");
        let s = s
            .apply(patch(
                json!({ "gemini": { "live_model": "gemini-4-hydrogen" } }),
            ))
            .unwrap();
        assert_eq!(s.live().id, "gemini-4-hydrogen");
    }

    #[test]
    fn removing_the_live_model_falls_back_to_the_first() {
        let s = Settings::default()
            .apply(patch(
                json!({ "gemini": { "live_model": "gemini-4-carbon" } }),
            ))
            .unwrap()
            .apply(patch(json!({ "gemini": { "models": [
                { "id": "gemini-4-hydrogen", "thinking": "low" } ] } })))
            .unwrap();
        assert_eq!(s.gemini.live_model, "gemini-4-hydrogen");
    }

    // ---- model per job (FR-TRN-13) ----

    #[test]
    fn retranslate_uses_the_live_model_until_one_is_picked() {
        let s = Settings::default();
        assert_eq!(s.gemini.quality_model, "");
        assert_eq!(s.model(Job::Retranslate), s.live());
        assert_eq!(s.model(Job::Live), s.live());
    }

    #[test]
    fn each_job_gets_its_own_model() {
        let s = Settings::default()
            .apply(patch(
                json!({ "gemini": { "quality_model": " models/gemini-4-carbon " } }),
            ))
            .unwrap();
        assert_eq!(s.gemini.quality_model, "gemini-4-carbon");
        assert_eq!(s.model(Job::Live).id, "gemini-3.8-flash");
        assert_eq!(s.model(Job::Retranslate).id, "gemini-4-carbon");
        assert_eq!(s.model(Job::Retranslate).thinking, "medium");
        // Changing the live model leaves the other job alone.
        let s = s
            .apply(patch(
                json!({ "gemini": { "live_model": "gemini-4-hydrogen" } }),
            ))
            .unwrap();
        assert_eq!(s.model(Job::Retranslate).id, "gemini-4-carbon");
    }

    #[test]
    fn quality_model_must_be_in_the_list_or_empty() {
        let s = Settings::default();
        let err = s
            .apply(patch(json!({ "gemini": { "quality_model": "gemini-9" } })))
            .unwrap_err();
        assert!(err.contains("gemini-9"), "{err}");
        let s = s
            .apply(patch(
                json!({ "gemini": { "quality_model": "gemini-4-carbon" } }),
            ))
            .unwrap()
            .apply(patch(json!({ "gemini": { "quality_model": "  " } })))
            .unwrap();
        assert_eq!(s.gemini.quality_model, "");
        assert_eq!(s.model(Job::Retranslate), s.live());
    }

    #[test]
    fn removing_the_quality_model_goes_back_to_the_live_one() {
        let s = Settings::default()
            .apply(patch(
                json!({ "gemini": { "quality_model": "gemini-4-carbon" } }),
            ))
            .unwrap()
            .apply(patch(json!({ "gemini": { "models": [
                { "id": "gemini-3.8-flash", "thinking": "low" } ] } })))
            .unwrap();
        assert_eq!(s.gemini.quality_model, "");
        assert_eq!(s.model(Job::Retranslate).id, "gemini-3.8-flash");
    }

    #[test]
    fn request_uses_the_thinking_level_of_its_job() {
        let s = Settings::default()
            .apply(patch(
                json!({ "gemini": { "quality_model": "gemini-4-carbon" } }),
            ))
            .unwrap();
        let level = |job| {
            request_body("Hi.", &[], &[], &s, job)["generationConfig"]["thinkingConfig"]
                ["thinkingLevel"]
                .clone()
        };
        assert_eq!(level(Job::Live), "low");
        assert_eq!(level(Job::Retranslate), "medium");
    }

    // ---- stored settings ----

    #[test]
    fn stored_settings_round_trip() {
        let s = Settings::default()
            .apply(patch(json!({ "tone": "neutral", "extra_prompt": "x",
                "gemini": { "quality_model": "gemini-4-carbon" } })))
            .unwrap();
        let stored = serde_json::to_value(&s).unwrap();
        assert_eq!(Settings::from_stored(&stored), s);
    }

    #[test]
    fn broken_stored_settings_fall_back_to_defaults() {
        assert_eq!(Settings::from_stored(&json!({})), Settings::default());
        assert_eq!(Settings::from_stored(&json!("nope")), Settings::default());
        let bad = json!({ "context_lines": 99, "tone": "shouty" });
        assert_eq!(Settings::from_stored(&bad), Settings::default());
    }

    // ---- view ----

    #[test]
    fn view_lists_levels_and_key_state_but_no_key() {
        let v = Settings::default().view(true);
        assert_eq!(v["gemini"]["key_saved"], true);
        assert_eq!(v["gemini"]["models"][1]["levels"][3], "xhigh");
        assert_eq!(v["tone"], "casual");
        assert_eq!(v["context_lines"], 3);
        assert_eq!(v["gemini"]["quality_model"], "");
        assert_eq!(
            Settings::default().view(false)["gemini"]["key_saved"],
            false
        );
    }

    // ---- ids and keys ----

    #[test]
    fn model_ids_are_path_safe() {
        assert_eq!(
            model_id(" models/gemini-4-carbon ").unwrap(),
            "gemini-4-carbon"
        );
        for bad in [
            "",
            "gemini/../x",
            "gemini 4",
            "gemini?key=x",
            "-x",
            "Gemini-4",
        ] {
            assert!(model_id(bad).is_err(), "{bad}");
        }
        assert!(model_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn api_keys_are_trimmed_and_checked() {
        assert_eq!(
            api_key("  AIzaExample_123-x \n").unwrap(),
            "AIzaExample_123-x"
        );
        assert!(api_key("   ").is_err());
        assert!(api_key("has space").is_err());
        assert!(api_key("ไทย").is_err());
        assert!(api_key(&"a".repeat(257)).is_err());
    }

    #[test]
    fn redact_blanks_the_key() {
        assert_eq!(
            redact("bad key AIzaX in url", "AIzaX"),
            "bad key [key] in url"
        );
        assert_eq!(redact("nothing", ""), "nothing");
    }

    // ---- request (FR-TRN-03) ----

    #[test]
    fn request_carries_line_context_tone_and_extra_prompt() {
        let s = Settings::default()
            .apply(patch(
                json!({ "tone": "formal", "extra_prompt": "Call the hero Traveler." }),
            ))
            .unwrap();
        let context = vec!["Where is the gate?".to_owned(), "North of here.".to_owned()];
        let body = request_body("Then we go north.", &context, &[], &s, Job::Live);

        let system = text_of(&body, "/systemInstruction/parts/0/text");
        assert!(system.contains("Thai"), "{system}");
        assert!(system.contains(Tone::Formal.instruction()), "{system}");
        assert!(system.contains("Call the hero Traveler."), "{system}");

        let user = text_of(&body, "/contents/0/parts/0/text");
        assert_eq!(body["contents"][0]["role"], "user");
        let gate = user.find("Where is the gate?").unwrap();
        let north = user.find("North of here.").unwrap();
        let line = user.find("Then we go north.").unwrap();
        assert!(
            gate < north && north < line,
            "context in order, line last: {user}"
        );

        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "low"
        );
    }

    #[test]
    fn context_is_cut_to_the_setting() {
        let s = Settings::default()
            .apply(patch(json!({ "context_lines": 1 })))
            .unwrap();
        let context = vec!["old".to_owned(), "older".to_owned(), "newest".to_owned()];
        let user = text_of(
            &request_body("now", &context, &[], &s, Job::Live),
            "/contents/0/parts/0/text",
        );
        assert!(user.contains("newest"));
        assert!(!user.contains("old\n") && !user.contains("older"), "{user}");
    }

    #[test]
    fn no_context_and_no_extra_prompt() {
        let s = Settings::default()
            .apply(patch(json!({ "context_lines": 0 })))
            .unwrap();
        let body = request_body("Hello.", &["before".to_owned()], &[], &s, Job::Live);
        let user = text_of(&body, "/contents/0/parts/0/text");
        assert!(!user.contains("before"), "{user}");
        assert!(user.contains("Hello."));
        let system = text_of(&body, "/systemInstruction/parts/0/text");
        assert!(system.contains(Tone::Casual.instruction()));
        assert!(!system.contains("player's own instructions"), "{system}");
    }

    #[test]
    fn glossary_terms_go_before_the_line() {
        let s = Settings::default();
        let terms = [Term::new("Warden", "วอร์เดน"), Term::new("Paimon", "ไพมอน")];
        let body = request_body("Warden, Paimon!", &[], &terms, &s, Job::Live);
        let user = text_of(&body, "/contents/0/parts/0/text");
        let warden = user.find("- Warden → วอร์เดน").unwrap();
        let paimon = user.find("- Paimon → ไพมอน").unwrap();
        let line = user.find("Line:").unwrap();
        assert!(warden < paimon && paimon < line, "{user}");
        let plain = request_body("Hi.", &[], &[], &s, Job::Live);
        assert!(!text_of(&plain, "/contents/0/parts/0/text").contains("Glossary"));
    }

    #[test]
    fn model_default_thinking_sends_no_thinking_config() {
        let s = Settings::default()
            .apply(patch(json!({ "gemini": {
                "models": [{ "id": "gemini-2.9-mini", "thinking": "default" }] } })))
            .unwrap();
        let body = request_body("Hi.", &[], &[], &s, Job::Live);
        assert!(body.get("generationConfig").is_none(), "{body}");
    }

    // ---- reply ----

    #[test]
    fn reply_text_skips_thoughts_and_trims() {
        let body = json!({ "candidates": [{ "content": { "parts": [
            { "text": "thinking...", "thought": true },
            { "text": " ไปทาง" },
            { "text": "เหนือกัน\n" }
        ] }, "finishReason": "STOP" }] });
        assert_eq!(
            parse_reply(body.to_string().as_bytes()).unwrap(),
            "ไปทางเหนือกัน"
        );
    }

    #[test]
    fn blocked_or_empty_reply_says_why() {
        let blocked = json!({ "promptFeedback": { "blockReason": "SAFETY" } });
        let err = parse_reply(blocked.to_string().as_bytes()).unwrap_err();
        assert!(err.contains("SAFETY"), "{err}");

        let cut =
            json!({ "candidates": [{ "content": { "parts": [] }, "finishReason": "MAX_TOKENS" }] });
        let err = parse_reply(cut.to_string().as_bytes()).unwrap_err();
        assert!(err.contains("MAX_TOKENS"), "{err}");

        assert!(parse_reply(b"{}").is_err());
        assert!(parse_reply(b"<html>").is_err());
    }

    // ---- failures (FR-TRN-09) ----

    fn google_error(code: u16, status: &str, message: &str, reason: Option<&str>) -> Vec<u8> {
        let details = reason.map_or(json!([]), |r| json!([{ "reason": r }]));
        json!({ "error": { "code": code, "status": status, "message": message, "details": details } })
            .to_string()
            .into_bytes()
    }

    #[test]
    fn invalid_key_is_named() {
        let body = google_error(
            400,
            "INVALID_ARGUMENT",
            "API key not valid. Please pass a valid API key.",
            Some("API_KEY_INVALID"),
        );
        assert!(failure_reason(400, &body, "m").contains("API key ไม่ถูกต้อง"));
        let body = google_error(400, "INVALID_ARGUMENT", "API key not valid.", None);
        assert!(failure_reason(400, &body, "m").contains("API key ไม่ถูกต้อง"));
    }

    #[test]
    fn each_failure_has_its_own_reason() {
        let denied = google_error(403, "PERMISSION_DENIED", "denied", None);
        assert!(failure_reason(403, &denied, "m").contains("ไม่มีสิทธิ์"));
        let missing = google_error(404, "NOT_FOUND", "models/x is not found", None);
        assert!(failure_reason(404, &missing, "gemini-x").contains("gemini-x"));
        let quota = google_error(429, "RESOURCE_EXHAUSTED", "quota", None);
        assert!(failure_reason(429, &quota, "m").contains("โควตา"));
        let bad = google_error(
            400,
            "INVALID_ARGUMENT",
            "Thinking level xhigh is not supported",
            None,
        );
        assert!(failure_reason(400, &bad, "m").contains("Thinking level xhigh"));
        assert!(failure_reason(503, b"", "m").contains("ชั่วคราว"));
        assert!(failure_reason(418, b"", "m").contains("418"));
    }

    #[test]
    fn long_google_messages_are_cut() {
        let body = google_error(400, "INVALID_ARGUMENT", &"x".repeat(1000), None);
        assert!(failure_reason(400, &body, "m").chars().count() < 300);
    }
}
