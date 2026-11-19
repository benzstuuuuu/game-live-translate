//! Claude Haiku (FR-TRN-14): one Messages API call per line, with the same
//! prompt as Gemini (`crate::core::prompt`). Rust has no official Anthropic
//! SDK, so this is plain HTTP through reqwest.
//!
//! The key comes from a `KeyStore` (macOS Keychain in the app) and never
//! leaves this module except in the `x-api-key` header (NFR-07).

use super::gemini::{blocking, KeyTest};
use super::{Translator, Unavailable};
use crate::core::claude::{self as core, MODEL};
use crate::core::gemini::{api_key, redact, Settings};
use crate::core::glossary::{self, Term};
use crate::core::tokens::Usage;
use crate::secret::KeyStore;
use futures_util::future::BoxFuture;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// The key once read from the store: None = not read yet.
    key: Mutex<Option<Option<String>>>,
    /// Tokens of every call since the app opened (FR-TRN-10).
    usage: Mutex<Usage>,
}

impl Claude {
    pub fn new(
        settings: impl Fn() -> Settings + Send + Sync + 'static,
        store: Arc<dyn KeyStore>,
        http: Http,
    ) -> Self {
        Self(Arc::new(Inner {
            settings: Box::new(settings),
            store,
            http,
            key: Mutex::new(None),
            usage: Mutex::default(),
        }))
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
        let started = Instant::now();
        let result = self.call(&key, TEST_LINE, &[], &[]).await;
        let ms = started.elapsed().as_millis() as u64;
        let (ok, message) = match result {
            Ok(thai) => (true, thai),
            Err(reason) => (false, reason),
        };
        Ok(KeyTest {
            ok,
            model: MODEL.into(),
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
        self.call(&key, text, earlier, &terms).await
    }

    /// One Messages call; adds the tokens of every answered call to `usage`
    /// (FR-TRN-10).
    async fn call(
        &self,
        key: &str,
        text: &str,
        earlier: &[String],
        terms: &[Term],
    ) -> Result<String, String> {
        let body = core::request_body(text, earlier, terms, &(self.0.settings)());
        let outcome = match self.0.http.messages(key, &body).await {
            Ok((200, reply)) => {
                let mut usage = self.0.usage.lock().unwrap_or_else(|e| e.into_inner());
                *usage = *usage + core::usage(&reply);
                drop(usage);
                core::parse_reply(&reply)
            }
            Ok((status, reply)) => Err(core::failure_reason(status, &reply)),
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
        Some(MODEL.into())
    }

    fn usage(&self) -> Usage {
        *self.0.usage.lock().unwrap_or_else(|e| e.into_inner())
    }
}
