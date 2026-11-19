//! App-wide settings in `settings.json` (SRS 7.4): `translator` and
//! `retranslator` here, the translation keys in `crate::core::gemini`. Each
//! writer replaces only its own keys; other keys already in the file are kept
//! as they are.

use crate::app::{App, AppState};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::sync::watch;

/// The saved translator engine, if the file has one.
pub fn translator(path: &Path) -> Option<String> {
    read(path).get("translator")?.as_str().map(str::to_owned)
}

/// The saved engine for "แปลใหม่" (FR-TRN-16); None when the file has no
/// such key (files from before it) or keeps it null (= the live one).
pub fn retranslator(path: &Path) -> Option<String> {
    read(path).get("retranslator")?.as_str().map(str::to_owned)
}

/// Puts the saved picks back on start. Engines this build lacks are skipped:
/// the live pick stays the default, "แปลใหม่" follows the live one.
pub fn restore(app: &App, path: &Path) {
    if let Some(saved) = translator(path) {
        app.set_translator(&saved);
    }
    if !app.set_retranslator(retranslator(path).as_deref()) {
        app.set_retranslator(None);
    }
}

/// Writes `translator` into the file, keeping every other key.
pub fn save_translator(path: &Path, engine: &str) -> std::io::Result<()> {
    save_values(
        path,
        Map::from_iter([("translator".into(), Value::from(engine))]),
    )
}

/// Writes these top-level keys into the file, keeping every other key.
pub fn save_values(path: &Path, values: Map<String, Value>) -> std::io::Result<()> {
    // One writer at a time, so two savers cannot drop each other's keys.
    static WRITING: Mutex<()> = Mutex::new(());
    let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = read(path);
    all.extend(values);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&all)?)?;
    std::fs::rename(tmp, path)
}

/// Saves both translator picks now and each time one changes, until the
/// state sender is gone.
pub async fn keep_translator(mut state: watch::Receiver<AppState>, path: PathBuf) {
    let mut saved = None;
    loop {
        let now = {
            let s = state.borrow_and_update();
            (s.translator, s.retranslator)
        };
        if saved != Some(now) {
            let values = Map::from_iter([
                ("translator".into(), Value::from(now.0)),
                ("retranslator".into(), Value::from(now.1)),
            ]);
            if let Err(e) = save_values(&path, values) {
                eprintln!("save settings: {e}");
            }
            saved = Some(now);
        }
        if state.changed().await.is_err() {
            return;
        }
    }
}

/// The file as a JSON object; missing or broken files read as empty.
pub fn read(path: &Path) -> Map<String, Value> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file() -> PathBuf {
        std::env::temp_dir().join(format!("yokoten-settings-{}.json", uuid::Uuid::new_v4()))
    }

    #[test]
    fn missing_file_has_no_translator() {
        assert_eq!(translator(&temp_file()), None);
    }

    #[test]
    fn broken_file_has_no_translator() {
        let path = temp_file();
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(translator(&path), None);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn save_then_read_back_keeping_other_keys() {
        let path = temp_file();
        std::fs::write(&path, r#"{"context_lines": 3, "translator": "fake"}"#).unwrap();
        save_translator(&path, "apple").unwrap();
        assert_eq!(translator(&path).as_deref(), Some("apple"));
        let all: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(all["context_lines"], 3);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn old_or_null_retranslator_reads_as_none() {
        let path = temp_file();
        std::fs::write(&path, r#"{"translator": "apple"}"#).unwrap();
        assert_eq!(retranslator(&path), None);
        std::fs::write(&path, r#"{"translator": "apple", "retranslator": null}"#).unwrap();
        assert_eq!(retranslator(&path), None);
        std::fs::write(&path, r#"{"retranslator": "gemini"}"#).unwrap();
        assert_eq!(retranslator(&path).as_deref(), Some("gemini"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn save_values_replaces_only_its_keys() {
        let path = temp_file();
        std::fs::write(&path, r#"{"translator": "apple", "tone": "formal"}"#).unwrap();
        let values = Map::from_iter([("tone".to_owned(), Value::from("casual"))]);
        save_values(&path, values).unwrap();
        let all = read(&path);
        assert_eq!(all["tone"], "casual");
        assert_eq!(all["translator"], "apple");
        std::fs::remove_file(path).unwrap();
    }
}
