//! Translates one line with the translator in use, or with its backup when
//! that one fails or is too slow (FR-TRN-04, 05). The decisions are in
//! `core::fallback`; this file only runs them.
//!
//! Without a backup (another engine is in use, or the build has no Apple
//! Translation) a line waits as long as the engine takes, and a failed line is
//! logged and dropped as before.

use crate::app::App;
use crate::core::fallback::{Failure, Fallback, LIMIT};
use crate::core::thai;
use crate::glossary;
use crate::translate::Translator;
use std::time::Instant;

pub struct Translated {
    pub engine: &'static str,
    pub model_id: Option<String>,
    pub text: String,
    /// True when the backup translated it (`lines.is_fallback`).
    pub is_fallback: bool,
}

/// `text` in Thai, given the session's earlier lines and the glossary of the
/// profile in use; Err with the reason when no translator could do it.
pub async fn translate(app: &App, text: &str, earlier: &[String]) -> Result<Translated, String> {
    let terms = glossary::current(app);
    let selected = app.translator();
    let Some(backup) = app.backup() else {
        let t = selected
            .translate_with_glossary(text, earlier, &terms)
            .await?;
        return Ok(done(&*selected, &t, false));
    };
    if app.with_fallback(|f| f.try_selected(Instant::now())) {
        let attempt = selected.translate_with_glossary(text, earlier, &terms);
        let failure = match tokio::time::timeout(LIMIT, attempt).await {
            Ok(Ok(t)) => {
                app.with_fallback(Fallback::reset);
                return Ok(done(&*selected, &t, false));
            }
            Ok(Err(reason)) => Failure::Failed(reason),
            Err(_) => Failure::Slow,
        };
        let engine = selected.engine();
        eprintln!("{engine}: {}", failure.reason(engine));
        app.with_fallback(|f| f.failed(&failure, engine, Instant::now()));
    }
    let t = backup
        .translate_with_glossary(text, earlier, &terms)
        .await?;
    Ok(done(&*backup, &t, true))
}

/// What a line keeps of `engine`'s translation `text`.
pub fn done(engine: &dyn Translator, text: &str, is_fallback: bool) -> Translated {
    Translated {
        engine: engine.engine(),
        model_id: engine.model_id(),
        text: thai::fix_am(text),
        is_fallback,
    }
}
