//! Turns OCR'd frames into line events (FR-CAP-03 to 05).
//!
//! - a frame counts once it reads the same twice in a row
//! - a speaker-name line on top is dropped before comparing; a frame left with
//!   only the name is ignored
//! - a line ≥85% similar to the last one sent is OCR jitter, not a new line
//! - text that grows from the last line (typewriter subtitles) updates it
//! - an empty box, read twice, means the subtitle is gone

pub const SIMILAR: f64 = 0.85;

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    New(String),
    Update(String),
    Gone,
}

#[derive(Debug, Default)]
pub struct Dedup {
    pending: Option<String>,
    last: Option<String>,
    gone: bool,
}

impl Dedup {
    /// Feed one frame's OCR lines, top to bottom.
    pub fn push(&mut self, ocr_lines: &[String]) -> Option<Change> {
        let text = dialogue(ocr_lines)?;
        let stable = self.pending.as_deref() == Some(text.as_str());
        self.pending = Some(text.clone());
        if !stable {
            return None;
        }
        if text.is_empty() {
            return self.disappear();
        }
        let change = match self.last.as_deref() {
            None => Change::New(text.clone()),
            Some(last) if last == text => return None,
            Some(last) if text.starts_with(last) => Change::Update(text.clone()),
            Some(last) if similarity(last, &text) >= SIMILAR => return None,
            Some(_) => Change::New(text.clone()),
        };
        self.last = Some(text);
        self.gone = false;
        Some(change)
    }

    fn disappear(&mut self) -> Option<Change> {
        if self.gone || self.last.is_none() {
            return None;
        }
        self.gone = true;
        Some(Change::Gone)
    }
}

/// Dialogue text of a frame: "" for an empty box, None for a name-only frame.
fn dialogue(lines: &[String]) -> Option<String> {
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.chars().any(char::is_alphabetic) && !is_ui_label(l))
        .collect();
    let body = match lines.as_slice() {
        [only] if is_speaker(only) => return None,
        [first, rest @ ..] if is_speaker(first) => rest,
        all => all,
    };
    Some(body.join(" "))
}

/// Game UI labels that sit inside a tall subtitle box (Genshin's auto-play
/// marker shows "Playing"/"Auto" beside cutscene lines).
/// ponytail: fixed list; make it per-profile when a second game needs its own.
const UI_LABELS: &[&str] = &["playing", "auto"];

fn is_ui_label(line: &str) -> bool {
    UI_LABELS.contains(&line.to_lowercase().as_str())
}

/// Short, capitalised, unpunctuated: "Mara", "Captain Rhee".
fn is_speaker(line: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().collect();
    line.chars().count() <= 24
        && words.len() <= 3
        && words
            .iter()
            .all(|w| w.chars().next().is_some_and(char::is_uppercase))
        && !line.chars().any(|c| ".,!?…:;\"“”".contains(c))
}

/// 1 − edit distance / longer length, case-insensitive.
pub fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let longest = a.len().max(b.len());
    if longest == 0 {
        return 1.0;
    }
    1.0 - levenshtein(&a, &b) as f64 / longest as f64
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    /// Feed frames, collect every change.
    fn run(frames: &[&[&str]]) -> Vec<Change> {
        let mut d = Dedup::default();
        frames.iter().filter_map(|f| d.push(&frame(f))).collect()
    }

    fn new(s: &str) -> Change {
        Change::New(s.into())
    }

    #[test]
    fn needs_two_identical_frames() {
        assert_eq!(run(&[&["Hold still."]]), vec![]);
        assert_eq!(
            run(&[&["Hold still."], &["Hold still."]]),
            vec![new("Hold still.")]
        );
    }

    #[test]
    fn same_line_held_on_screen_sends_once() {
        let f: &[&str] = &["Hold still."];
        assert_eq!(run(&[f, f, f, f, f]), vec![new("Hold still.")]);
    }

    #[test]
    fn flicker_between_frames_is_not_stable() {
        assert_eq!(
            run(&[&["Hold still."], &["Hold stiII."], &["Hold still."]]),
            vec![]
        );
    }

    #[test]
    fn speaker_name_is_dropped() {
        let f: &[&str] = &["Mara", "Keep your lantern low."];
        assert_eq!(run(&[f, f]), vec![new("Keep your lantern low.")]);
    }

    #[test]
    fn two_word_speaker_is_dropped() {
        let f: &[&str] = &["Captain Rhee", "Raise the sails."];
        assert_eq!(run(&[f, f]), vec![new("Raise the sails.")]);
    }

    #[test]
    fn name_only_frame_is_ignored_not_gone() {
        let line: &[&str] = &["Mara", "Keep your lantern low."];
        let name: &[&str] = &["Mara"];
        assert_eq!(
            run(&[line, line, name, name, line, line]),
            vec![new("Keep your lantern low.")]
        );
    }

    #[test]
    fn name_only_frame_does_not_break_stability() {
        let line: &[&str] = &["Mara", "Keep your lantern low."];
        assert_eq!(
            run(&[line, &["Mara"], line]),
            vec![new("Keep your lantern low.")]
        );
    }

    #[test]
    fn two_line_dialogue_joins_with_space() {
        let f: &[&str] = &["Ren", "You sure about this,", "Mara?"];
        assert_eq!(run(&[f, f]), vec![new("You sure about this, Mara?")]);
    }

    #[test]
    fn sentence_on_top_is_not_a_speaker() {
        let f: &[&str] = &["Wait.", "Something moved."];
        assert_eq!(run(&[f, f]), vec![new("Wait. Something moved.")]);
    }

    #[test]
    fn ocr_jitter_on_same_line_is_not_new() {
        let a: &[&str] = &["No. But it's the only road left."];
        let b: &[&str] = &["No. But it's the only road left"];
        let c: &[&str] = &["No. But its the only road 1eft."];
        assert_eq!(
            run(&[a, a, b, b, c, c]),
            vec![new("No. But it's the only road left.")]
        );
    }

    #[test]
    fn different_line_after_line_is_new() {
        let a: &[&str] = &["Stay behind me."];
        let b: &[&str] = &["Don't touch the stones."];
        assert_eq!(
            run(&[a, a, b, b]),
            vec![new("Stay behind me."), new("Don't touch the stones.")]
        );
    }

    #[test]
    fn typewriter_updates_the_line() {
        let a: &[&str] = &["No. But it's the"];
        let b: &[&str] = &["No. But it's the only road left."];
        assert_eq!(
            run(&[a, a, b, b]),
            vec![
                new("No. But it's the"),
                Change::Update("No. But it's the only road left.".into())
            ]
        );
    }

    #[test]
    fn typewriter_still_typing_sends_nothing_until_it_settles() {
        let frames: &[&[&str]] = &[
            &["No."],
            &["No. But"],
            &["No. But it's"],
            &["No. But it's the end."],
            &["No. But it's the end."],
        ];
        assert_eq!(run(frames), vec![new("No. But it's the end.")]);
    }

    #[test]
    fn empty_box_twice_is_gone_once() {
        let a: &[&str] = &["Hold still."];
        let e: &[&str] = &[];
        assert_eq!(
            run(&[a, a, e, e, e, e]),
            vec![new("Hold still."), Change::Gone]
        );
    }

    #[test]
    fn lines_without_letters_are_ignored() {
        // HP bar "1213 / 1213" sits inside a tall box on Genshin.
        let f: &[&str] = &["Paimon doesn't.", "1213 / 1213"];
        assert_eq!(run(&[f, f]), vec![new("Paimon doesn't.")]);
    }

    #[test]
    fn numbers_alone_count_as_empty() {
        let a: &[&str] = &["Hold still."];
        let hp: &[&str] = &["1213 / 1213"];
        assert_eq!(run(&[a, a, hp, hp]), vec![new("Hold still."), Change::Gone]);
    }

    #[test]
    fn auto_play_label_is_ignored() {
        let f: &[&str] = &["Knights of Favonius, time is against us.", "Playing"];
        assert_eq!(
            run(&[f, f]),
            vec![new("Knights of Favonius, time is against us.")]
        );
        let g: &[&str] = &["AUTO", "Hold still."];
        assert_eq!(run(&[g, g]), vec![new("Hold still.")]);
    }

    #[test]
    fn label_word_inside_a_sentence_is_kept() {
        let f: &[&str] = &["Keep playing."];
        assert_eq!(run(&[f, f]), vec![new("Keep playing.")]);
    }

    #[test]
    fn whitespace_only_lines_count_as_empty() {
        let a: &[&str] = &["Hold still."];
        let e: &[&str] = &["  ", ""];
        assert_eq!(run(&[a, a, e, e]), vec![new("Hold still."), Change::Gone]);
    }

    #[test]
    fn empty_before_any_line_is_not_gone() {
        let e: &[&str] = &[];
        assert_eq!(run(&[e, e, e]), vec![]);
    }

    #[test]
    fn gone_then_new_line_then_gone_again() {
        let a: &[&str] = &["Hold still."];
        let b: &[&str] = &["We rest here tonight."];
        let e: &[&str] = &[];
        assert_eq!(
            run(&[a, a, e, e, b, b, e, e]),
            vec![
                new("Hold still."),
                Change::Gone,
                new("We rest here tonight."),
                Change::Gone
            ]
        );
    }

    #[test]
    fn same_line_after_gone_is_not_resent() {
        let a: &[&str] = &["Hold still."];
        let e: &[&str] = &[];
        assert_eq!(
            run(&[a, a, e, e, a, a]),
            vec![new("Hold still."), Change::Gone]
        );
    }

    #[test]
    fn similarity_bounds() {
        assert_eq!(similarity("", ""), 1.0);
        assert_eq!(similarity("abc", "abc"), 1.0);
        assert_eq!(similarity("ABC", "abc"), 1.0);
        assert_eq!(similarity("abc", "xyz"), 0.0);
        assert!(similarity("kitten", "sitting") < SIMILAR);
    }

    #[test]
    fn speaker_rules() {
        assert!(is_speaker("Mara"));
        assert!(is_speaker("Captain Rhee"));
        assert!(!is_speaker("Hold still."));
        assert!(!is_speaker("you there"));
        assert!(!is_speaker("The Very Long Name Of Someone"));
        assert!(!is_speaker("Mara:"));
    }
}
