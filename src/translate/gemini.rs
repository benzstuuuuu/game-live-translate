//! Gemini (FR-TRN-03, 09, 12): one generateContent call per line, with the
//! earlier lines, the tone and the extra prompt from /setup (FR-SET-03).
//!
//! `Gemini` is a cheap handle: the translator in `Config.translators` and the
//! /setup API (`crate::translation_api`) share one. The key comes from a
//! `KeyStore` (macOS Keychain in the app) and never leaves this module except
//! in the `x-goog-api-key` header (NFR-07).

use super::{Translator, Unavailable};
use crate::core::gemini::{self as core, Job, Patch, Settings};
use crate::core::glossary::{self, Term};
use crate::core::tokens::Usage;
use crate::secret::KeyStore;
use crate::settings as file;
use futures_util::future::BoxFuture;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

const ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/models";
/// What the key test translates.
const TEST_LINE: &str = "Where are we going?";

/// The Gemini HTTP API, behind a trait so tests answer with canned replies.
pub trait Api: Send + Sync {
    /// POSTs `body` to `models/{model}:generateContent`; the reply's status and
    /// body, or a Thai reason the call did not get through.
    fn generate<'a>(
        &'a self,
        key: &'a str,
        model: &'a str,
        body: Value,
    ) -> BoxFuture<'a, Result<(u16, Vec<u8>), String>>;
}

/// The real API over HTTPS.
pub struct Http(reqwest::Client);

impl Http {
    pub fn new() -> Result<Self, reqwest::Error> {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()
            .map(Self)
    }
}

impl Api for Http {
    fn generate<'a>(
        &'a self,
        key: &'a str,
        model: &'a str,
        body: Value,
    ) -> BoxFuture<'a, Result<(u16, Vec<u8>), String>> {
        Box::pin(async move {
            // The key goes in a header only, never in the URL (NFR-07).
            let res = self
                .0
                .post(format!("{ENDPOINT}/{model}:generateContent"))
                .header("x-goog-api-key", key)
                .header("content-type", "application/json")
                .body(body.to_string())
                .send()
                .await
                .map_err(|e| network_reason(&e))?;
            let status = res.status().as_u16();
            let bytes = res.bytes().await.map_err(|e| network_reason(&e))?;
            Ok((status, bytes.to_vec()))
        })
    }
}

fn network_reason(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "Gemini ตอบช้าเกินไป ลองอีกครั้ง".into()
    } else {
        "ต่อ Gemini ไม่ได้ ตรวจการเชื่อมต่ออินเทอร์เน็ต".into()
    }
}

/// Result of the "ทดสอบ key" button.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyTest {
    pub ok: bool,
    pub model: String,
    /// Thai: the sample translation when ok, else why the key failed.
    pub message: String,
    pub ms: u64,
}

/// The handle translates with the model of its `Job` (FR-TRN-13): `new` gives
/// the live one, `Translator::retranslator` the one for /reader.
#[derive(Clone)]
pub struct Gemini(Arc<Inner>, Job);

struct Inner {
    settings: RwLock<Settings>,
    store: Arc<dyn KeyStore>,
    api: Arc<dyn Api>,
    /// settings.json; None in tests that do not care.
    file: Option<PathBuf>,
    /// The key once read from the store: None = not read yet.
    key: Mutex<Option<Option<String>>>,
    /// Tokens of every call since the app opened (FR-TRN-10).
    usage: Mutex<Usage>,
}

impl Gemini {
    pub fn new(
        settings: Settings,
        store: Arc<dyn KeyStore>,
        api: Arc<dyn Api>,
        file: Option<PathBuf>,
    ) -> Self {
        let inner = Inner {
            settings: RwLock::new(settings),
            store,
            api,
            file,
            key: Mutex::new(None),
            usage: Mutex::default(),
        };
        Self(Arc::new(inner), Job::Live)
    }

    pub fn settings(&self) -> Settings {
        self.0
            .settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// What GET /api/settings shows: settings and whether a key is saved.
    pub async fn view(&self) -> Value {
        let saved = matches!(self.key().await, Ok(Some(_)));
        self.settings().view(saved)
    }

    /// Applies a change from /setup and writes it to settings.json.
    pub fn update(&self, patch: Patch) -> Result<Settings, String> {
        let mut current = self.0.settings.write().unwrap_or_else(|e| e.into_inner());
        let next = current.apply(patch)?;
        if let Some(path) = &self.0.file {
            let Ok(Value::Object(values)) = serde_json::to_value(&next) else {
                return Err("บันทึกค่าตั้งไม่ได้".into());
            };
            file::save_values(path, values).map_err(|e| format!("บันทึกค่าตั้งไม่ได้: {e}"))?;
        }
        *current = next.clone();
        Ok(next)
    }

    /// Saves a pasted key to the store (FR-SET-07).
    pub async fn save_key(&self, raw: &str) -> Result<(), String> {
        let key = core::api_key(raw)?;
        let store = self.0.store.clone();
        let saved = key.clone();
        blocking(move || store.set(&saved)).await?;
        *self.key_slot() = Some(Some(key));
        Ok(())
    }

    /// Translates a short line with the live model, using `raw` if given or
    /// else the saved key, and says whether that worked.
    pub async fn test_key(&self, raw: Option<&str>) -> Result<KeyTest, String> {
        let key = match raw.map(str::trim).filter(|k| !k.is_empty()) {
            Some(k) => core::api_key(k)?,
            None => self.key().await?.ok_or("ยังไม่ได้ใส่ API key")?,
        };
        let settings = self.settings();
        let model = settings.live().id.clone();
        let started = Instant::now();
        let result = self
            .call(&key, TEST_LINE, &[], &[], &settings, Job::Live)
            .await;
        let ms = started.elapsed().as_millis() as u64;
        let (ok, message) = match result {
            Ok(thai) => (true, thai),
            Err(reason) => (false, reason),
        };
        Ok(KeyTest {
            ok,
            model,
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
        self.call(&key, text, earlier, &terms, &self.settings(), self.1)
            .await
    }

    /// One generateContent call with the model of `job`; adds the tokens of
    /// every answered call to `usage` (FR-TRN-10).
    async fn call(
        &self,
        key: &str,
        text: &str,
        earlier: &[String],
        terms: &[Term],
        settings: &Settings,
        job: Job,
    ) -> Result<String, String> {
        let model = &settings.model(job).id;
        let body = core::request_body(text, earlier, terms, settings, job);
        let outcome = match self.0.api.generate(key, model, body).await {
            Ok((200, reply)) => {
                let mut usage = self.0.usage.lock().unwrap_or_else(|e| e.into_inner());
                *usage = *usage + Usage::from_reply(&reply);
                drop(usage);
                core::parse_reply(&reply)
            }
            Ok((status, reply)) => Err(core::failure_reason(status, &reply, model)),
            Err(reason) => Err(reason),
        };
        outcome.map_err(|e| core::redact(&e, key))
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

/// Keychain calls block (and may wait on a macOS prompt), so they run off the
/// async threads.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|_| Err("อ่าน Keychain ไม่ได้".into()))
}

fn no_key() -> Unavailable {
    Unavailable {
        message: "ยังไม่ได้ใส่ Gemini API key".into(),
        fix: "เปิด /setup บน Mac ใส่ API key ในส่วนแปลภาษาแล้วกดบันทึก หรือเลือกตัวแปลอื่น".into(),
    }
}

impl Translator for Gemini {
    fn engine(&self) -> &'static str {
        "gemini"
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
        Some(self.settings().model(self.1).id.clone())
    }

    fn retranslator(&self) -> Option<Arc<dyn Translator>> {
        Some(Arc::new(Self(self.0.clone(), Job::Retranslate)))
    }

    fn usage(&self) -> Usage {
        *self.0.usage.lock().unwrap_or_else(|e| e.into_inner())
    }
}
