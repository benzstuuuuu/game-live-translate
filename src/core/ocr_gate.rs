//! Skips OCR while the subtitle picture stays the same (#102).
//!
//! - a frame is shrunk to a small grey thumbnail and compared with the
//!   thumbnail of the last frame that was OCR'd, not of the frame before, so
//!   a slow fade cannot creep past
//! - the picture changed when enough thumbnail cells moved by more than noise
//!   moves them; one new typewriter letter is enough
//! - unchanged: the last OCR result is given again
//! - even unchanged, the picture is OCR'd again every `REFRESH`
//!
//! The numbers are first guesses, to be set with real game frames (#47).

use std::time::{Duration, Instant};

/// Thumbnail columns and rows: 1/8 of OBS's 1440×180 screenshot.
pub const THUMB: (usize, usize) = (180, 22);
/// A thumbnail cell changed when its grey level (0 to 255) moved by more
/// than this. A cell is the mean of about 64 pixels, so JPEG noise moves it
/// by 1 or 2 and a full-stop by 12 or more.
pub const CELL_DELTA: u8 = 12;
/// The picture changed when at least this many cells did.
pub const CHANGED_CELLS: usize = 1;
/// A picture that stays the same is still OCR'd again this often.
pub const REFRESH: Duration = Duration::from_secs(2);

/// Grey picture `luma` (`width` × `height`, one byte per pixel) as `THUMB`
/// cells, each the mean of the pixels it covers.
pub fn thumbnail(luma: &[u8], width: usize, height: usize) -> Vec<u8> {
    let (tw, th) = THUMB;
    let columns: Vec<usize> = (0..width).map(|x| x * tw / width).collect();
    let mut sum = vec![0u32; tw * th];
    let mut count = vec![0u32; tw * th];
    for (y, row) in luma.chunks_exact(width.max(1)).take(height).enumerate() {
        let first = (y * th / height) * tw;
        for (&p, &column) in row.iter().zip(&columns) {
            sum[first + column] += u32::from(p);
            count[first + column] += 1;
        }
    }
    sum.iter()
        .zip(&count)
        .map(|(&s, &n)| (s / n.max(1)) as u8)
        .collect()
}

/// Do two thumbnails show different pictures?
pub fn changed(before: &[u8], now: &[u8]) -> bool {
    let moved = before
        .iter()
        .zip(now)
        .filter(|(a, b)| a.abs_diff(**b) > CELL_DELTA)
        .take(CHANGED_CELLS)
        .count();
    moved >= CHANGED_CELLS
}

/// The last OCR result, given again while the picture stays the same.
pub struct Gate<T> {
    last: Option<Read<T>>,
}

/// The frame that was last OCR'd.
struct Read<T> {
    thumb: Vec<u8>,
    at: Instant,
    result: T,
}

impl<T> Default for Gate<T> {
    fn default() -> Self {
        Self { last: None }
    }
}

impl<T: Clone> Gate<T> {
    /// What the picture with thumbnail `thumb` reads at `now`: from `ocr`
    /// when it changed since the last OCR or that was `REFRESH` ago,
    /// otherwise the last result again. After a failed OCR the next frame is
    /// read again.
    pub fn read<E>(
        &mut self,
        thumb: Vec<u8>,
        now: Instant,
        ocr: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        if let Some(last) = &self.last {
            if now.duration_since(last.at) < REFRESH && !changed(&last.thumb, &thumb) {
                return Ok(last.result.clone());
            }
        }
        self.last = None;
        let result = ocr()?;
        self.last = Some(Read {
            thumb,
            at: now,
            result: result.clone(),
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 1440;
    const H: usize = 180;
    const DARK: u8 = 20;
    const LIT: u8 = 235;

    /// A dark dialogue box with lit rectangles (x, y, width, height) on it.
    fn picture(marks: &[(usize, usize, usize, usize)]) -> Vec<u8> {
        let mut img = vec![DARK; W * H];
        for &(x0, y0, w, h) in marks {
            for y in y0..y0 + h {
                img[y * W + x0..y * W + x0 + w].fill(LIT);
            }
        }
        img
    }

    /// `n` letter-sized marks in a row, like a line of subtitle text.
    fn text(n: usize) -> Vec<(usize, usize, usize, usize)> {
        (0..n).map(|i| (400 + i * 26, 70, 16, 30)).collect()
    }

    /// The picture with every pixel moved by up to `by`, like JPEG noise.
    fn noisy(img: &[u8], by: i16) -> Vec<u8> {
        let mut seed = 0x2545_f491u32;
        img.iter()
            .map(|&p| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let d = (seed >> 16) as i16 % (2 * by + 1) - by;
                (i16::from(p) + d).clamp(0, 255) as u8
            })
            .collect()
    }

    fn thumb(img: &[u8]) -> Vec<u8> {
        thumbnail(img, W, H)
    }

    #[test]
    fn thumbnail_is_the_mean_of_each_cell() {
        // Left half dark, right half lit.
        let img = picture(&[(W / 2, 0, W / 2, H)]);
        let t = thumb(&img);
        assert_eq!(t.len(), THUMB.0 * THUMB.1);
        for row in t.chunks_exact(THUMB.0) {
            assert!(row[..THUMB.0 / 2].iter().all(|&c| c == DARK));
            assert!(row[THUMB.0 / 2..].iter().all(|&c| c == LIT));
        }
        // 1440×176 is 8×8 pixels a cell: one lit pixel moves its cell by 3.
        let mut dot = vec![0u8; W * 176];
        dot[0] = 255;
        let t = thumbnail(&dot, W, 176);
        assert_eq!(t[0], 255 / 64);
        assert!(t[1..].iter().all(|&c| c == 0));
    }

    #[test]
    fn thumbnail_of_any_size_has_the_same_cells() {
        for (w, h) in [(1200, 220), (982, 180), (64, 8), (1, 1)] {
            assert_eq!(
                thumbnail(&vec![77; w * h], w, h).len(),
                THUMB.0 * THUMB.1,
                "{w}x{h}"
            );
        }
        assert_eq!(thumbnail(&[], 0, 0).len(), THUMB.0 * THUMB.1);
        // A picture as large as the thumbnail keeps every pixel.
        let small: Vec<u8> = (0..THUMB.0 * THUMB.1).map(|i| (i % 251) as u8).collect();
        assert_eq!(thumbnail(&small, THUMB.0, THUMB.1), small);
    }

    #[test]
    fn same_picture_has_not_changed() {
        let img = picture(&text(20));
        assert!(!changed(&thumb(&img), &thumb(&img)));
        let empty = picture(&[]);
        assert!(!changed(&thumb(&empty), &thumb(&empty)));
    }

    #[test]
    fn slight_noise_has_not_changed() {
        let img = picture(&text(20));
        assert!(!changed(&thumb(&img), &thumb(&noisy(&img, 6))));
        assert!(!changed(&thumb(&noisy(&img, 6)), &thumb(&img)));
        let empty = picture(&[]);
        assert!(!changed(&thumb(&empty), &thumb(&noisy(&empty, 6))));
    }

    #[test]
    fn one_more_letter_has_changed() {
        let before = picture(&text(20));
        let after = picture(&text(21));
        assert!(changed(&thumb(&before), &thumb(&after)));
        // Also through noise, and when the letter goes away.
        assert!(changed(&thumb(&before), &thumb(&noisy(&after, 6))));
        assert!(changed(&thumb(&after), &thumb(&before)));
    }

    #[test]
    fn a_thin_letter_or_a_full_stop_has_changed() {
        let before = picture(&text(20));
        let mut with_l = text(20);
        with_l.push((400 + 20 * 26, 70, 3, 30));
        assert!(changed(&thumb(&before), &thumb(&picture(&with_l))));
        // A 5×5 dot wherever it falls on the thumbnail's grid.
        for shift in 0..9 {
            let mut with_dot = text(20);
            with_dot.push((920 + shift, 92 + shift, 5, 5));
            assert!(
                changed(&thumb(&before), &thumb(&picture(&with_dot))),
                "dot at +{shift}"
            );
        }
    }

    #[test]
    fn another_line_has_changed() {
        let before = picture(&text(20));
        let after: Vec<_> = (0..12).map(|i| (300 + i * 31, 40, 20, 34)).collect();
        assert!(changed(&thumb(&before), &thumb(&picture(&after))));
        assert!(changed(&thumb(&before), &thumb(&picture(&[]))));
    }

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    /// Feeds `img` to the gate at `now`; the OCR counts its calls in `calls`
    /// and reads "read N".
    fn feed(gate: &mut Gate<String>, img: &[u8], now: Instant, calls: &mut u32) -> String {
        gate.read(thumb(img), now, || {
            *calls += 1;
            Ok::<_, String>(format!("read {calls}"))
        })
        .unwrap()
    }

    #[test]
    fn unchanged_picture_is_read_once_then_again_after_two_seconds() {
        let t0 = Instant::now();
        let img = picture(&text(20));
        let mut gate = Gate::default();
        let mut calls = 0;
        assert_eq!(feed(&mut gate, &img, t0, &mut calls), "read 1");
        // 4 frames a second, with noise: the first reading is given again.
        for ms in (250..2000).step_by(250) {
            let frame = noisy(&img, 4);
            assert_eq!(feed(&mut gate, &frame, at(t0, ms), &mut calls), "read 1");
        }
        assert_eq!(calls, 1);
        // Two seconds after the reading: read again, and the wait restarts.
        assert_eq!(feed(&mut gate, &img, at(t0, 2000), &mut calls), "read 2");
        assert_eq!(feed(&mut gate, &img, at(t0, 3750), &mut calls), "read 2");
        assert_eq!(feed(&mut gate, &img, at(t0, 4000), &mut calls), "read 3");
        assert_eq!(calls, 3);
    }

    #[test]
    fn changed_picture_is_read_at_once() {
        let t0 = Instant::now();
        let mut gate = Gate::default();
        let mut calls = 0;
        // A typewriter line: every new letter is read.
        for n in 1..=5u32 {
            let img = picture(&text(n as usize));
            let now = at(t0, u64::from(n) * 250);
            assert_eq!(feed(&mut gate, &img, now, &mut calls), format!("read {n}"));
        }
        // Then it stands still.
        let img = picture(&text(5));
        assert_eq!(feed(&mut gate, &img, at(t0, 1500), &mut calls), "read 5");
        assert_eq!(calls, 5);
    }

    #[test]
    fn slow_fade_is_read_again_once_it_adds_up() {
        let t0 = Instant::now();
        let mut gate = Gate::default();
        let mut calls = 0;
        // Each frame is 3 grey levels brighter than the one before: never a
        // change from frame to frame, but one from the frame that was read.
        for step in 0..12u8 {
            let img = vec![DARK + step * 3; W * H];
            feed(&mut gate, &img, at(t0, u64::from(step) * 100), &mut calls);
        }
        assert!((2..=4).contains(&calls), "{calls}");
    }

    #[test]
    fn failed_ocr_is_tried_again_on_the_next_frame() {
        let t0 = Instant::now();
        let img = picture(&text(20));
        let mut gate = Gate::default();
        let mut calls = 0;
        assert_eq!(feed(&mut gate, &img, t0, &mut calls), "read 1");
        // The picture changes and its OCR fails: nothing to give again.
        let next = picture(&text(8));
        let failed = gate.read(thumb(&next), at(t0, 250), || Err::<String, _>("no"));
        assert_eq!(failed, Err("no"));
        assert_eq!(feed(&mut gate, &next, at(t0, 500), &mut calls), "read 2");
        assert_eq!(feed(&mut gate, &next, at(t0, 750), &mut calls), "read 2");
    }
}
