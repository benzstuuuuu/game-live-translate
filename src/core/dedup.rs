//! Turns OCR'd frames into line events (FR-CAP-03 to 05).
//!
//! - a frame counts once it reads the same twice in a row
//! - a speaker-name line on top is dropped before comparing; a frame left with
//!   only the name is ignored
//! - a line ≥85% similar to the last one sent is OCR jitter, not a new line
//! - text that grows from the last line (typewriter subtitles) updates it
//! - text inside the last line, or the last line inside it, is the same line
//!   with credit names or a menu tab beside it
//! - a line ≥85% similar to one of the last 5 sent, seen in the last minute, is
//!   a menu flipping back; seeing it again restarts its minute
//! - a line whose every word has a digit ("L2", "4B7 J26") is a code, not text
//! - an empty box, read twice, means the subtitle is gone

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const SIMILAR: f64 = 0.85;
/// Shortest text that counts as the same line when inside another.
const CONTAINED_MIN: usize = 8;
const RECENT: usize = 5;
const RECENT_FOR: Duration = Duration::from_secs(60);

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
    /// Lines sent lately, oldest first, with when each was last on screen.
    /// The newest one is `last`.
    recent: VecDeque<(String, Instant)>,
}

impl Dedup {
    /// Feed one frame's OCR lines, top to bottom.
    pub fn push(&mut self, ocr_lines: &[String], now: Instant) -> Option<Change> {
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
            Some(last) if last != text && text.starts_with(last) => {
                self.recent.pop_back();
                Change::Update(text.clone())
            }
            Some(last) if same_line(last, &text) => {
                if let Some((_, seen)) = self.recent.back_mut() {
                    *seen = now;
                }
                return None;
            }
            _ if seen_lately(&mut self.recent, &text, now) => return None,
            _ => Change::New(text.clone()),
        };
        self.recent.push_back((text.clone(), now));
        if self.recent.len() > RECENT {
            self.recent.pop_front();
        }
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

/// Same text, or one inside the other with something beside it.
fn same_line(a: &str, b: &str) -> bool {
    if similarity(a, b) >= SIMILAR {
        return true;
    }
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let long_enough = |s: &str| s.chars().count() >= CONTAINED_MIN;
    long_enough(&a) && long_enough(&b) && (a.contains(&b) || b.contains(&a))
}

/// Whether `text` was sent in the last minute; if so it is seen again now.
fn seen_lately(recent: &mut VecDeque<(String, Instant)>, text: &str, now: Instant) -> bool {
    let hit = recent.iter_mut().find(|(sent, seen)| {
        now.duration_since(*seen) < RECENT_FOR && similarity(sent, text) >= SIMILAR
    });
    match hit {
        Some((_, seen)) => {
            *seen = now;
            true
        }
        None => false,
    }
}

/// Dialogue text of a frame: "" for an empty box, None for a name-only frame.
fn dialogue(lines: &[String]) -> Option<String> {
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.chars().any(char::is_alphabetic) && !is_ui_label(l) && !is_code(l))
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
    let line = line.to_lowercase();
    UI_LABELS.contains(&line.as_str()) || is_level(&line)
}

/// Character level beside the HP bar: "Lv. 5", "Lv.90", "LV 12".
fn is_level(lower: &str) -> bool {
    let Some(rest) = lower.strip_prefix("lv") else {
        return false;
    };
    let digits = rest.trim_start_matches('.').trim();
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// Button or map codes: every word has a digit ("L2", "4B7 J26").
fn is_code(line: &str) -> bool {
    line.split_whitespace()
        .all(|w| w.chars().any(|c| c.is_ascii_digit()))
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

    /// Feed frames one second apart, collect every change.
    fn run(frames: &[&[&str]]) -> Vec<Change> {
        let timed: Vec<(u64, &[&str])> = (0..).zip(frames.iter().copied()).collect();
        run_at(&timed)
    }

    /// Feed frames at the given second, collect every change.
    fn run_at(frames: &[(u64, &[&str])]) -> Vec<Change> {
        let start = Instant::now();
        let mut d = Dedup::default();
        frames
            .iter()
            .filter_map(|(s, f)| d.push(&frame(f), start + Duration::from_secs(*s)))
            .collect()
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
    fn level_label_is_ignored() {
        for lv in ["Lv. 5", "Lv.90", "LV 12", "lv. 5"] {
            let f: &[&str] = &["Paimon doesn't.", lv];
            assert_eq!(run(&[f, f]), vec![new("Paimon doesn't.")], "{lv}");
        }
    }

    #[test]
    fn words_starting_with_lv_are_kept() {
        let f: &[&str] = &["Lv. 5 is too low for this."];
        assert_eq!(run(&[f, f]), vec![new("Lv. 5 is too low for this.")]);
        assert!(!is_ui_label("Lvl"));
        assert!(!is_ui_label("Lv."));
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
    fn line_without_its_credit_names_is_the_same_line() {
        let a: &[&str] = &["ollie brand mia stone Grab the rope, quick."];
        let b: &[&str] = &["Grab the rope, quick."];
        assert_eq!(
            run(&[a, a, b, b]),
            vec![new("ollie brand mia stone Grab the rope, quick.")]
        );
    }

    #[test]
    fn credit_names_added_to_the_line_are_the_same_line() {
        let a: &[&str] = &["We left the cart by the well."];
        let b: &[&str] = &["oIlie brAnd We left the cart by the well."];
        assert_eq!(
            run(&[a, a, b, b]),
            vec![new("We left the cart by the well.")]
        );
    }

    #[test]
    fn room_code_before_a_place_name_is_the_same_line() {
        let a: &[&str] = &["K4 Boiler room"];
        let b: &[&str] = &["Boiler room"];
        assert_eq!(run(&[a, a, b, b]), vec![new("K4 Boiler room")]);
    }

    #[test]
    fn menu_with_an_extra_tab_is_the_same_line() {
        let a: &[&str] = &["Saved games Settings ext"];
        let b: &[&str] = &["Profile Saved games Settings ext"];
        assert_eq!(
            run(&[a, a, b, b, a, a, b, b]),
            vec![new("Saved games Settings ext")]
        );
    }

    #[test]
    fn short_text_inside_a_line_is_not_the_same_line() {
        let a: &[&str] = &["Not here, not now."];
        let b: &[&str] = &["Not now"];
        assert_eq!(
            run(&[a, a, b, b]),
            vec![new("Not here, not now."), new("Not now")]
        );
    }

    #[test]
    fn two_menus_flipping_send_once_each() {
        let a: &[&str] = &["Map journal quests"];
        let b: &[&str] = &["Crafting recipes upgrade"];
        assert_eq!(
            run(&[a, a, b, b, a, a, b, b, a, a, b, b]),
            vec![new("Map journal quests"), new("Crafting recipes upgrade")]
        );
    }

    #[test]
    fn menus_flipping_past_a_minute_are_not_resent() {
        let a: &[&str] = &["Map journal quests"];
        let b: &[&str] = &["Crafting recipes upgrade"];
        let frames: Vec<(u64, &[&str])> = (0..200)
            .map(|s| (s * 2, if (s / 2) % 2 == 0 { a } else { b }))
            .collect();
        assert_eq!(
            run_at(&frames),
            vec![new("Map journal quests"), new("Crafting recipes upgrade")]
        );
    }

    #[test]
    fn line_not_seen_for_a_minute_is_sent_again() {
        let a: &[&str] = &["Map journal quests"];
        let b: &[&str] = &["Crafting recipes upgrade"];
        assert_eq!(
            run_at(&[(0, a), (1, a), (2, b), (3, b), (70, a), (71, a)]),
            vec![
                new("Map journal quests"),
                new("Crafting recipes upgrade"),
                new("Map journal quests")
            ]
        );
    }

    #[test]
    fn sixth_line_pushes_out_the_oldest() {
        let lines: [&[&str]; 6] = [
            &["Stay behind me."],
            &["Don't touch the stones."],
            &["We rest here tonight."],
            &["Grab the rope, quick."],
            &["Who left the gate open?"],
            &["Bring more lamp oil."],
        ];
        let mut frames: Vec<&[&str]> = lines.iter().flat_map(|l| [*l, *l]).collect();
        frames.extend([lines[1], lines[1], lines[0], lines[0]]);
        let mut sent: Vec<Change> = lines.iter().map(|l| new(l[0])).collect();
        sent.push(new("Stay behind me."));
        assert_eq!(run(&frames), sent);
    }

    #[test]
    fn line_sharing_words_with_the_last_is_new() {
        let a: &[&str] = &["We left the cart by the well."];
        let b: &[&str] = &["We left the cart and ran."];
        assert_eq!(
            run(&[a, a, b, b]),
            vec![
                new("We left the cart by the well."),
                new("We left the cart and ran.")
            ]
        );
    }

    #[test]
    fn code_only_box_counts_as_empty() {
        for code in ["L2", "4B7", "J26", "D1D", "4B7 J26"] {
            let a: &[&str] = &["Hold still."];
            let c: &[&str] = &[code];
            assert_eq!(
                run(&[a, a, c, c]),
                vec![new("Hold still."), Change::Gone],
                "{code}"
            );
        }
    }

    #[test]
    fn code_beside_a_line_is_dropped() {
        let f: &[&str] = &["Hold still.", "4B7 J26"];
        assert_eq!(run(&[f, f]), vec![new("Hold still.")]);
    }

    #[test]
    fn short_words_are_not_codes() {
        for word in ["No.", "Hi.", "Go!", "LRR", "Brian", "Lv. 5 is too low."] {
            assert!(!is_code(word), "{word}");
        }
        let f: &[&str] = &["No."];
        assert_eq!(run(&[f, f]), vec![new("No.")]);
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
