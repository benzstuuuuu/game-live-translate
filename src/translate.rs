//! Translators sit behind one trait so tests can use a fake (#18).

#[cfg(feature = "apple-translation")]
mod apple;
pub mod claude;
pub mod claude_cli;
mod claude_proc;
pub mod gemini;

#[cfg(feature = "apple-translation")]
pub use apple::Apple;
pub use claude::Claude;
pub use claude_cli::ClaudeCli;
pub use gemini::Gemini;

use crate::core::glossary::Term;
use crate::core::tokens::Usage;
use futures_util::future::BoxFuture;
use std::sync::Arc;

pub trait Translator: Send + Sync {
    /// Engine name: the value of `AppState.translator`, `PUT /api/translator`
    /// and `lines.engine`.
    fn engine(&self) -> &'static str;
    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>>;

    /// Translates `text` given the session's earlier lines, oldest first.
    /// Engines that cannot use context translate the line alone.
    fn translate_in_context<'a>(
        &'a self,
        text: &'a str,
        _earlier: &'a [String],
    ) -> BoxFuture<'a, Result<String, String>> {
        self.translate(text)
    }

    /// Translates `text` in context, keeping the game's `glossary` (all its
    /// terms; each engine picks the ones the line uses). By default the terms
    /// become placeholders `[1]`, `[2]` … and come back in Thai (FR-GLS-05);
    /// engines that take a prompt put them there instead (FR-GLS-04).
    fn translate_with_glossary<'a>(
        &'a self,
        text: &'a str,
        earlier: &'a [String],
        glossary: &'a [Term],
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(crate::glossary::with_placeholders(
            self, text, earlier, glossary,
        ))
    }

    /// Can this engine translate right now? Called before a session starts and
    /// after a switch (FR-TRN-11). Engines with nothing to check are ready.
    fn check(&self) -> BoxFuture<'_, Result<(), Unavailable>> {
        Box::pin(async { Ok(()) })
    }

    /// The model translating now, for `lines.model_id`; None for engines
    /// without models.
    fn model_id(&self) -> Option<String> {
        None
    }

    /// The engine that translates a line again from /reader, when that job
    /// has its own model (FR-TRN-13); None = this engine as it is.
    fn retranslator(&self) -> Option<Arc<dyn Translator>> {
        None
    }

    /// Tokens this engine has used since the app opened (FR-TRN-10); zero
    /// for engines that do not count them.
    fn usage(&self) -> Usage {
        Usage::default()
    }

    /// A session that may use this engine started: get ready so its first
    /// line does not wait (FR-TRN-15). Most engines have nothing to do.
    fn warm(&self) {}

    /// Translating stopped or the app is closing: let go of anything kept
    /// open, so no child process outlives the session (FR-TRN-15).
    fn stop(&self) {}
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
