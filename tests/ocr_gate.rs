//! The OCR gate (#102) on the hand-made fixture frames: which of them count
//! as a new picture, and how many of 100 frames still need OCR. No Vision
//! here: the OCR only counts its calls.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use yokoten::core::ocr_gate::{changed, thumbnail, Gate};

/// Thumbnail of each fixture frame, in file-name order (01.png first).
fn thumbs() -> Vec<Vec<u8>> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "png"))
        .collect();
    files.sort();
    files
        .iter()
        .map(|p| {
            let grey = image::open(p).unwrap().into_luma8();
            let (w, h) = (grey.width() as usize, grey.height() as usize);
            thumbnail(grey.as_raw(), w, h)
        })
        .collect()
}

#[test]
fn only_fixture_frames_with_other_text_count_as_changed() {
    let thumbs = thumbs();
    // See tests/fixtures/make_frames.py: frames 1-2, 5-6, 7-8, 9-10, 11-12
    // and 13-14 show the same text; every other step changes it, down to a
    // full stop going away (12 to 13).
    let same = [1, 5, 7, 9, 11, 13];
    for n in 1..thumbs.len() {
        assert_eq!(
            changed(&thumbs[n - 1], &thumbs[n]),
            !same.contains(&n),
            "frame {n} to {}",
            n + 1
        );
    }
}

/// Feeds the gate one thumbnail every 250 ms, the app's 4 frames a second.
/// How many of them it had OCR'd; without the gate that is all of them.
fn readings<'a>(frames: impl Iterator<Item = &'a Vec<u8>>) -> u32 {
    let t0 = Instant::now();
    let mut gate = Gate::default();
    let mut calls = 0;
    for (tick, thumb) in frames.enumerate() {
        let now = t0 + Duration::from_millis(250) * tick as u32;
        let read = gate.read(thumb.clone(), now, || {
            calls += 1;
            Ok::<_, ()>(())
        });
        read.unwrap();
    }
    calls
}

#[test]
fn fixture_frames_held_three_ticks_need_a_fifth_of_the_ocr() {
    let thumbs = thumbs();
    // Each frame stays for 3 ticks, as when #102 was measured.
    let frames = (0..100).map(|tick| &thumbs[tick / 3 % thumbs.len()]);
    assert_eq!(readings(frames), 20);
}

#[test]
fn a_line_that_stays_is_read_once_every_two_seconds() {
    let thumbs = thumbs();
    // 25 s of one picture: 13 readings, not 100.
    assert_eq!(readings(std::iter::repeat_n(&thumbs[0], 100)), 13);
}
