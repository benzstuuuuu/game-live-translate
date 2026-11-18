//! Apple Vision OCR. Every call runs inside an autorelease pool: without it
//! the spike leaked 1.4 GB in 11 minutes.

use apple_vision::prelude::*;
use std::path::Path;

/// Text lines of an image, top to bottom.
pub fn read(path: &Path) -> Result<Vec<String>, String> {
    let rec = TextRecognizer::new()
        .with_recognition_level(RecognitionLevel::Accurate)
        .with_language_correction(true);
    let mut obs = objc2::rc::autoreleasepool(|_| rec.recognize_in_path(path))
        .map_err(|e| format!("OCR {}: {e:?}", path.display()))?;
    // Vision's y origin is bottom-left: higher y is higher on screen.
    obs.sort_by(|a, b| b.bounding_box.y.total_cmp(&a.bounding_box.y));
    Ok(obs.into_iter().map(|o| o.text).collect())
}
