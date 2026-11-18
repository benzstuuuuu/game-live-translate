//! Translators sit behind one trait so tests can use a fake (#18).

mod apple;

pub use apple::Apple;

use futures_util::future::BoxFuture;

pub trait Translator: Send + Sync {
    /// Engine name: the value of `AppState.translator`, `PUT /api/translator`
    /// and `lines.engine`.
    fn engine(&self) -> &'static str;
    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>>;

    /// Can this engine translate right now? Called before a session starts and
    /// after a switch (FR-TRN-11). Engines with nothing to check are ready.
    fn check(&self) -> BoxFuture<'_, Result<(), Unavailable>> {
        Box::pin(async { Ok(()) })
    }
}

/// Why an engine cannot translate, for `AppState.error` (code
/// `translator_unavailable`). Both texts are Thai and shown to the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Unavailable {
    pub message: String,
    pub fix: String,
}

/// Marks the text instead of translating it.
pub struct Fake;

impl Translator for Fake {
    fn engine(&self) -> &'static str {
        "fake"
    }

    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("(แปล) {text}")) })
    }
}
