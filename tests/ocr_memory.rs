//! OCR the same frame for 60 s; memory must not grow (autorelease pool).
//! Run on a Mac: cargo test --release --test ocr_memory -- --ignored --nocapture

use std::path::PathBuf;
use std::time::{Duration, Instant};

fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        .unwrap()
        / 1024.0
}

#[test]
#[ignore = "takes 60 s; run by hand"]
fn ocr_same_frame_for_a_minute_does_not_leak() {
    let frame = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames/05.png");
    let warm = Instant::now();
    while warm.elapsed() < Duration::from_secs(5) {
        yokoten::ocr::read(&frame).unwrap();
    }
    let before = rss_mb();
    let (start, mut n) = (Instant::now(), 0);
    while start.elapsed() < Duration::from_secs(60) {
        assert!(!yokoten::ocr::read(&frame).unwrap().is_empty());
        n += 1;
    }
    let after = rss_mb();
    println!("{n} OCR calls, RSS {before:.1} MB -> {after:.1} MB");
    assert!(after - before < 20.0, "RSS grew {:.1} MB", after - before);
}
