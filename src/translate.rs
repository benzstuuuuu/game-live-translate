//! Translators sit behind one trait so tests can use a fake (#18).

use futures_util::future::BoxFuture;

pub trait Translator: Send + Sync {
    /// Value stored in `lines.engine`.
    fn engine(&self) -> &'static str;
    fn translate<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<String, String>>;
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
