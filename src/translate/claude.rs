//! Claude API (FR-TRN-14): one Messages API call per line, with the same
//! prompt as Gemini (`crate::core::prompt`) and the model and effort picked
//! on /setup. Rust has no official Anthropic SDK, so this is plain HTTP
//! through reqwest.
//!
//! The key comes from a `KeyStore` (macOS Keychain in the app) and never
//! leaves this module except in the `x-api-key` header (NFR-07).

use super::gemini::{blocking, KeyTest};
use super::{Translator, Unavailable};
use crate::core::claude::{self as core, Pick, EFFORTS, MODELS};
use crate::core::claude_cli::check_reply_of;
use crate::core::gemini::{api_key, redact, Settings};
use crate::core::glossary::{self, Term};
use crate::core::tokens::Usage;
use crate::secret::KeyStore;
use crate::settings;
use futures_util::future::BoxFuture;
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// The key in settings.json that holds `{ "model": …, "effort": … }`.
const SETTINGS_KEY: &str = "claude";
const BASE: &str = "https://api.anthropic.com";
/// The API version every request names (curl examples in the Claude API
/// docs). The body sends none of the fields it refuses (sampling params).
const VERSION: &str = "2026-10-21";
/// What the key test translates.
const TEST_LINE: &str = "Where are we going?";

/// The Messages API over HTTP.
pub struct Http {
    client: reqwest::Client,
    base: String,
}

impl Http {
    pub fn new() -> Result<Self, reqwest::Error> {
        Self::with_base(BASE)
    }

    /// Talks to `base` instead of api.anthropic.com; tests run a fake there.
    pub fn with_base(base: &str) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            client,
            base: base.trim_end_matches('/').to_owned(),
        })
    }

    /// POSTs `body` to /v1/messages; the reply's status and body, or a Thai
    /// reason the call did not get through.
    async fn messages(&self, key: &str, body: &Value) -> Result<(u16, Vec<u8>), String> {
        // The key goes in this header only, never in the URL (NFR-07).
        let res = self
            .client
            .post(format!("{}/v1/messages", self.base))
            .header("x-api-key", key)
            .header("anthropic-version", VERSION)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| network_reason(&e))?;
        let status = res.status().as_u16();
        let bytes = res.bytes().await.map_err(|e| network_reason(&e))?;
        Ok((status, bytes.to_vec()))
    }
}

fn network_reason(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "Claude ตอบช้าเกินไป ลองอีกครั้ง".into()
    } else {
        "ต่อ Claude ไม่ได้ ตรวจการเชื่อมต่ออินเทอร์เน็ต".into()
    }
}

/// A cheap handle: the translator in `Config.translators` and the /setup
/// API (`crate::translation_api`) share one.
#[derive(Clone)]
pub struct Claude(Arc<Inner>);

struct Inner {
    /// Tone, extra prompt and context lines from /setup, read per line.
    settings: Box<dyn Fn() -> Settings + Send + Sync>,
    store: Arc<dyn KeyStore>,
    http: Http,
    /// settings.json; None in tests that do not care.
    file: Option<PathBuf>,
    /// The model and effort picked on /setup, read per line.
    pick: RwLock<Pick>,
    /// The key once read from the store: None = not read yet.
    key: Mutex<Option<Option<String>>>,
    /// Tokens of every call since the app opened (FR-TRN-10).
    usage: Mutex<Usage>,
}

impl Claude {
    /// The model and effort come from `file` (settings.json), Sonnet `low`
    /// when it has none or a bad one.
    pub fn new(
        settings: impl Fn() -> Settings + Send + Sync + 'static,
        store: Arc<dyn KeyStore>,
        http: Http,
        file: Option<PathBuf>,
    ) -> Self {
        let stored = file.as_deref().map(settings::read);
        let pick = Pick::from_stored(stored.as_ref().and_then(|all| all.get(SETTINGS_KEY)));
        Self(Arc::new(Inner {
            settings: Box::new(settings),
            store,
            http,
            file,
            pick: RwLock::new(pick),
            key: Mutex::new(None),
            usage: Mutex::default(),
        }))
    }

    /// The model and effort in use.
    pub fn pick(&self) -> Pick {
        self.0
            .pick
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Changes the model and/or effort (from /setup) and saves them; the
    /// next line uses them.
    pub fn set_pick(&self, model: Option<&str>, effort: Option<&str>) -> Result<Pick, String> {
        let mut current = self.0.pick.write().unwrap_or_else(|e| e.into_inner());
        let next = current.with(model, effort)?;
        if let Some(path) = &self.0.file {
            let values = Map::from_iter([(SETTINGS_KEY.into(), json!(next))]);
            settings::save_values(path, values).map_err(|e| format!("บันทึกค่าตั้งไม่ได้: {e}"))?;
        }
        *current = next.clone();
        Ok(next)
    }

    /// What GET /api/claude shows: the pick, the choices, and whether a key
    /// is saved (never the key).
    pub async fn view(&self) -> Value {
        let pick = self.pick();
        json!({
            "model": pick.model,
            "effort": pick.effort,
            "models": MODELS,
            "efforts": EFFORTS,
            "key_saved": self.key_saved().await,
        })
    }

    /// Whether a key is saved; never the key.
    pub async fn key_saved(&self) -> bool {
        matches!(self.key().await, Ok(Some(_)))
    }

    /// Saves a pasted key to the store (FR-SET-07).
    pub async fn save_key(&self, raw: &str) -> Result<(), String> {
        let key = api_key(raw)?;
        let store = self.0.store.clone();
        let saved = key.clone();
        blocking(move || store.set(&saved)).await?;
        *self.key_slot() = Some(Some(key));
        Ok(())
    }

    /// Translates a short line using `raw` if given or else the saved key,
    /// and says whether that worked.
    pub async fn test_key(&self, raw: Option<&str>) -> Result<KeyTest, String> {
        let key = match raw.map(str::trim).filter(|k| !k.is_empty()) {
            Some(k) => api_key(k)?,
            None => self.key().await?.ok_or("ยังไม่ได้ใส่ API key")?,
        };
        let pick = self.pick();
        let started = Instant::now();
        let result = self.call(&key, TEST_LINE, &[], &[], &pick).await;
        let ms = started.elapsed().as_millis() as u64;
        let (ok, message) = match result {
            Ok(thai) => (true, thai),
            Err(reason) => (false, reason),
        };
        Ok(KeyTest {
            ok,
            model: pick.model,
            message,
            ms,
        })
    }

    /// Only the glossary terms the line uses go in the prompt (FR-GLS-04).
    async fn translate_line(
        &self,
        text: &str,
        earlier: &[String],
        glossary: &[Term],
    ) -> Result<String, String> {
        let key = self.key().await?.ok_or_else(|| no_key().message)?;
        let terms = glossary::used(text, glossary);
        self.call(&key, text, earlier, &terms, &self.pick()).await
    }

    /// One Messages call with `pick`'s model and effort; adds the tokens of
    /// every answered call to `usage` (FR-TRN-10). A reply that is more than
    /// a translation fails.
    async fn call(
        &self,
        key: &str,
        text: &str,
        earlier: &[String],
        terms: &[Term],
        pick: &Pick,
    ) -> Result<String, String> {
        let body = core::request_body(text, earlier, terms, &(self.0.settings)(), pick);
        let outcome = match self.0.http.messages(key, &body).await {
            Ok((200, reply)) => {
                let mut usage = self.0.usage.lock().unwrap_or_else(|e| e.into_inner());
                *usage = *usage + core::usage(&reply);
                drop(usage);
                core::parse_reply(&reply).and_then(|thai| check_reply_of("Claude", text, &thai))
            }
            Ok((status, reply)) => Err(core::failure_reason(status, &reply, &pick.model)),
            Err(reason) => Err(reason),
        };
        outcome.map_err(|e| redact(&e, key))
    }

    /// The saved key, read from the store once.
    async fn key(&self) -> Result<Option<String>, String> {
        let cached = self.key_slot().clone();
        if let Some(known) = cached {
            return Ok(known);
        }
        let store = self.0.store.clone();
        let key = blocking(move || store.get()).await?;
        *self.key_slot() = Some(key.clone());
        Ok(key)
    }

    fn key_slot(&self) -> std::sync::MutexGuard<'_, Option<Option<String>>> {
        self.0.key.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn no_key() -> Unavailable {
    Unavailable {
        message: "ยังไม่ได้ใส่ Claude API key".into(),
        fix: "เปิด /setup บน Mac ใส่ Claude API key ในส่วนแปลภาษาแล้วกดบันทึก หรือเลือกตัวแปลอื่น".into(),
    }
}

impl Translator for Claude {
    fn engine(&self) -> &'static str {
        "claude"
    }

    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(self.translate_line(text, &[], &[]))
    }

    fn translate_in_context<'a>(
        &'a self,
        text: &'a str,
        earlier: &'a [String],
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(self.translate_line(text, earlier, &[]))
    }

    fn translate_with_glossary<'a>(
        &'a self,
        text: &'a str,
        earlier: &'a [String],
        glossary: &'a [Term],
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(self.translate_line(text, earlier, glossary))
    }

    /// Ready when a key is saved; whether it works shows on the first line or
    /// with "ทดสอบ key".
    fn check(&self) -> BoxFuture<'_, Result<(), Unavailable>> {
        Box::pin(async move {
            match self.key().await {
                Ok(Some(_)) => Ok(()),
                Ok(None) => Err(no_key()),
                Err(message) => Err(Unavailable {
                    message,
                    fix: "เปิด Keychain Access แล้วอนุญาตให้ Yokoten อ่าน key หรือบันทึก key ใหม่ใน /setup"
                        .into(),
                }),
            }
        })
    }

    fn model_id(&self) -> Option<String> {
        Some(self.pick().model)
    }

    fn usage(&self) -> Usage {
        *self.0.usage.lock().unwrap_or_else(|e| e.into_inner())
    }
}
