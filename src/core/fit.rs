//! How the LCD board's line is fitted into its image (#85): the largest
//! font size that fits, and the text cut short with `…` when even the
//! smallest does not. Whether something fits is asked of the caller
//! (CoreText).

/// Largest whole size in `min..=max` that `fits`, assuming that when a size
/// fits every smaller one does too. None when not even `min` fits.
pub fn largest(min: u32, max: u32, fits: impl Fn(u32) -> bool) -> Option<u32> {
    if min > max || !fits(min) {
        return None;
    }
    let (mut lo, mut hi) = (min, max);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    Some(lo)
}

/// The longest start of `text` that still `fits` with `…` after it, cut
/// where no Thai vowel or tone mark is torn from its consonant; just `…`
/// when nothing does.
pub fn ellipsize(text: &str, fits: impl Fn(&str) -> bool) -> String {
    let cuts = cuts(text);
    let cut = |i: usize| format!("{}…", text[..cuts[i]].trim_end());
    let (mut lo, mut hi) = (0, cuts.len() - 1);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if fits(&cut(mid)) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    cut(lo)
}

/// Byte offsets where `text` may be cut, from 0 up to (not including) its
/// end.
fn cuts(text: &str) -> Vec<usize> {
    let mut out = vec![0];
    let mut prev = None;
    for (i, c) in text.char_indices() {
        if i > 0 && !follows(c) && !prev.is_some_and(leads) {
            out.push(i);
        }
        prev = Some(c);
    }
    out
}

/// Thai vowels and marks written after or over the consonant before them.
fn follows(c: char) -> bool {
    matches!(c, '\u{0E30}'..='\u{0E3A}' | '\u{0E45}'..='\u{0E4E}')
}

/// Thai vowels written before the consonant they belong to (เ แ โ ใ ไ).
fn leads(c: char) -> bool {
    matches!(c, '\u{0E40}'..='\u{0E44}')
}

#[cfg(test)]
mod tests {
    use super::{ellipsize, largest};

    #[test]
    fn largest_finds_the_biggest_fitting_size() {
        assert_eq!(largest(10, 100, |s| s <= 37), Some(37));
        assert_eq!(largest(10, 100, |_| true), Some(100));
        assert_eq!(largest(10, 10, |_| true), Some(10));
    }

    #[test]
    fn largest_is_none_when_min_does_not_fit() {
        assert_eq!(largest(10, 100, |_| false), None);
        assert_eq!(largest(20, 10, |_| true), None);
    }

    #[test]
    fn ellipsize_keeps_the_longest_fitting_start() {
        let fits = |s: &str| s.chars().count() <= 6;
        assert_eq!(ellipsize("abcdefghij", fits), "abcde…");
    }

    #[test]
    fn ellipsize_drops_spaces_before_the_ellipsis() {
        let fits = |s: &str| s.chars().count() <= 5;
        assert_eq!(ellipsize("abc   defgh", fits), "abc…");
    }

    #[test]
    fn ellipsize_gives_ellipsis_alone_when_nothing_fits() {
        assert_eq!(ellipsize("abc", |_| false), "…");
        assert_eq!(ellipsize("", |_| true), "…");
    }

    #[test]
    fn ellipsize_never_tears_marks_from_their_consonant() {
        // ที่ = ท + sara ii + mai ek; cutting inside leaves dangling marks.
        let text = "ที่นี่";
        for max in 1..=7 {
            let out = ellipsize(text, |s| s.chars().count() <= max);
            let kept = out.trim_end_matches('…');
            assert!(["", "ที่", "ที่นี่"].contains(&kept), "{max}: {out}");
        }
    }

    #[test]
    fn ellipsize_never_leaves_a_leading_vowel_alone() {
        // เก: cutting after เ would show เ with no consonant.
        let out = ellipsize("กเก", |s| s.chars().count() <= 3);
        assert_eq!(out, "ก…");
    }

    #[test]
    fn ellipsize_keeps_sara_am_with_its_consonant() {
        let out = ellipsize("กำลัง", |s| s.chars().count() <= 2);
        assert_eq!(out, "…");
        let out = ellipsize("กำลัง", |s| s.chars().count() <= 3);
        assert_eq!(out, "กำ…");
    }
}
