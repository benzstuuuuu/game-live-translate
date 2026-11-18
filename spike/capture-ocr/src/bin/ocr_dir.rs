//! OCR every PNG in a directory (sorted by name) and print one TSV row per file:
//! `file \t ocr_ms \t text` (lines joined with " | ", top to bottom).
//! usage: ocr_dir <dir> [fast]
use apple_vision::prelude::*;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().ok_or("usage: ocr_dir <dir> [fast]")?;
    let level = match args.next().as_deref() {
        Some("fast") => RecognitionLevel::Fast,
        _ => RecognitionLevel::Accurate,
    };
    let rec = TextRecognizer::new()
        .with_recognition_level(level)
        .with_language_correction(true);

    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();

    for path in files {
        let t = Instant::now();
        let mut obs = rec.recognize_in_path(&path)?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        // Vision's y origin is bottom-left: higher y = higher on screen.
        obs.sort_by(|a, b| b.bounding_box.y.total_cmp(&a.bounding_box.y));
        let text = obs.iter().map(|o| o.text.as_str()).collect::<Vec<_>>().join(" | ");
        println!("{}\t{:.1}\t{}", path.file_name().unwrap().to_string_lossy(), ms, text);
    }
    Ok(())
}
