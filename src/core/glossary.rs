//! Glossary (FR-GLS-01, 04, 05): which terms a line uses and how they reach
//! the translator. Gemini gets the terms in its prompt; Apple Translation gets
//! placeholders `[1]`, `[2]` … that are swapped back to Thai afterwards (#27).
//!
//! Matching ignores case, takes whole words only ("Warden" matches
//! "Warden's" but not "Wardenship") and tries longer terms first ("Captain
//! Rhee" before "Rhee").

use serde::Serialize;

/// Longest source or target, in characters.
pub const MAX_TERM_CHARS: usize = 100;

/// One glossary entry: the English term and the Thai it always becomes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Term {
    pub source: String,
    pub target: String,
}

impl Term {
    pub fn new(source: &str, target: &str) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermError {
    EmptySource,
    EmptyTarget,
    TooLong,
    /// The source has no letter or digit, so it can never match a word.
    NoLetters,
}

impl TermError {
    /// What to tell the player, in Thai.
    pub fn message(self) -> String {
        match self {
            Self::EmptySource => "ใส่คำต้นฉบับก่อน".into(),
            Self::EmptyTarget => "ใส่คำแปลก่อน".into(),
            Self::TooLong => format!("คำยาวได้ไม่เกิน {MAX_TERM_CHARS} ตัวอักษร"),
            Self::NoLetters => "คำต้นฉบับต้องมีตัวอักษรหรือตัวเลข".into(),
        }
    }
}

/// `source` and `target` with outer spaces trimmed and inner runs of spaces
/// made one, or why they cannot be a term.
pub fn clean(source: &str, target: &str) -> Result<Term, TermError> {
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let (source, target) = (squash(source), squash(target));
    if source.is_empty() {
        return Err(TermError::EmptySource);
    }
    if target.is_empty() {
        return Err(TermError::EmptyTarget);
    }
    if source.chars().count() > MAX_TERM_CHARS || target.chars().count() > MAX_TERM_CHARS {
        return Err(TermError::TooLong);
    }
    if !source.chars().any(char::is_alphanumeric) {
        return Err(TermError::NoLetters);
    }
    Ok(Term { source, target })
}

/// The glossary of one game (FR-GLS-01): the shared terms plus the game's
/// own, where a game term replaces a shared one with the same source.
pub fn merge(shared: Vec<Term>, game: Vec<Term>) -> Vec<Term> {
    let mut out: Vec<Term> = shared
        .into_iter()
        .filter(|s| !game.iter().any(|g| same_word(&g.source, &s.source)))
        .collect();
    out.extend(game);
    out
}

fn same_word(a: &str, b: &str) -> bool {
    a.chars().count() == b.chars().count() && a.chars().zip(b.chars()).all(|(x, y)| eq_ci(x, y))
}

fn eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// A letter or digit that would make a match part of a longer word. Thai
/// letters do not count: Thai has no spaces, so an English name left in a
/// Thai sentence sits right against them.
fn joins(c: char) -> bool {
    (c.is_alphanumeric() || c == '_') && !('\u{0E00}'..='\u{0E7F}').contains(&c)
}

/// One match: byte range in the text and the index of the term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub start: usize,
    pub end: usize,
    pub term: usize,
}

/// Every match of `terms` in `text`, left to right, without overlaps.
pub fn find(text: &str, terms: &[Term]) -> Vec<Hit> {
    let mut order: Vec<usize> = (0..terms.len())
        .filter(|&i| !terms[i].source.is_empty())
        .collect();
    order.sort_by_key(|&i| std::cmp::Reverse(terms[i].source.chars().count()));
    let mut hits = Vec::new();
    let mut prev: Option<char> = None;
    let mut pos = 0;
    while let Some(c) = text[pos..].chars().next() {
        let found = order
            .iter()
            .find_map(|&i| match_at(text, pos, prev, &terms[i].source).map(|end| (i, end)));
        match found {
            Some((term, end)) => {
                hits.push(Hit {
                    start: pos,
                    end,
                    term,
                });
                prev = text[..end].chars().next_back();
                pos = end;
            }
            None => {
                prev = Some(c);
                pos += c.len_utf8();
            }
        }
    }
    hits
}

/// End of `source` when it matches as a whole word at `pos`.
fn match_at(text: &str, pos: usize, prev: Option<char>, source: &str) -> Option<usize> {
    let first = source.chars().next()?;
    if joins(first) && prev.is_some_and(joins) {
        return None;
    }
    let mut rest = text[pos..].char_indices();
    for want in source.chars() {
        let (_, got) = rest.next()?;
        if !eq_ci(want, got) {
            return None;
        }
    }
    let end = pos + rest.offset();
    let last = source.chars().next_back()?;
    let next = text[end..].chars().next();
    if joins(last) && next.is_some_and(joins) {
        return None;
    }
    Some(end)
}

/// The terms `text` uses, each once, in order of first appearance. Gemini
/// gets only these in its prompt (FR-GLS-04).
pub fn used(text: &str, terms: &[Term]) -> Vec<Term> {
    let mut seen: Vec<usize> = Vec::new();
    for h in find(text, terms) {
        if !seen.contains(&h.term) {
            seen.push(h.term);
        }
    }
    seen.into_iter().map(|i| terms[i].clone()).collect()
}

/// `text` with every term replaced by its Thai (fixing English terms the
/// translator left in place).
pub fn replace_terms(text: &str, terms: &[Term]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for h in find(text, terms) {
        out.push_str(&text[pos..h.start]);
        out.push_str(&terms[h.term].target);
        pos = h.end;
    }
    out.push_str(&text[pos..]);
    out
}

/// A line with its glossary terms swapped for `[1]`, `[2]` … (FR-GLS-05).
/// The same term gets the same number every time it appears.
#[derive(Debug, Clone, PartialEq)]
pub struct Protected {
    pub text: String,
    /// Term `n` is placeholder `[n+1]`.
    terms: Vec<Term>,
    /// How many times each placeholder is in `text`.
    counts: Vec<usize>,
}

pub fn protect(text: &str, terms: &[Term]) -> Protected {
    let mut slots: Vec<usize> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for h in find(text, terms) {
        let n = match slots.iter().position(|&t| t == h.term) {
            Some(n) => n,
            None => {
                slots.push(h.term);
                counts.push(0);
                slots.len() - 1
            }
        };
        counts[n] += 1;
        out.push_str(&text[pos..h.start]);
        out.push_str(&format!("[{}]", n + 1));
        pos = h.end;
    }
    out.push_str(&text[pos..]);
    Protected {
        text: out,
        terms: slots.into_iter().map(|i| terms[i].clone()).collect(),
        counts,
    }
}

impl Protected {
    /// The terms behind the placeholders, `[1]` first.
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    /// True when the line has no glossary term.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// `translated` with each placeholder swapped for its Thai term; the
    /// spaces the translator put around them stay. None when a placeholder
    /// went missing or came back too often.
    pub fn restore(&self, translated: &str) -> Option<String> {
        let tokens = placeholders(translated);
        let mut counts = vec![0; self.terms.len()];
        for &(_, _, n) in &tokens {
            if let Some(c) = n.checked_sub(1).and_then(|i| counts.get_mut(i)) {
                *c += 1;
            }
        }
        if counts != self.counts {
            return None;
        }
        let mut out = String::with_capacity(translated.len());
        let mut pos = 0;
        for (start, end, n) in tokens {
            let Some(term) = n.checked_sub(1).and_then(|i| self.terms.get(i)) else {
                continue;
            };
            out.push_str(&translated[pos..start]);
            out.push_str(&term.target);
            pos = end;
        }
        out.push_str(&translated[pos..]);
        Some(out)
    }
}

/// Every `[n]` in `s`: (start, end, n).
fn placeholders(s: &str) -> Vec<(usize, usize, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'[' {
            let digits = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            let close = i + 1 + digits;
            if digits > 0 && b.get(close) == Some(&b']') {
                if let Ok(n) = s[i + 1..close].parse() {
                    out.push((i, close + 1, n));
                }
                i = close + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The glossary of the #27 spike.
    fn spike() -> Vec<Term> {
        vec![
            Term::new("Ember Crown", "มงกุฎเอมเบอร์"),
            Term::new("Hollow Order", "ภาคีฮอลโลว์"),
            Term::new("Captain Rhee", "กัปตันรี"),
            Term::new("Warden", "วอร์เดน"),
            Term::new("Paimon", "ไพมอน"),
            Term::new("Mondstadt", "มอนด์สตัดท์"),
        ]
    }

    fn sources(text: &str, terms: &[Term]) -> Vec<String> {
        find(text, terms)
            .into_iter()
            .map(|h| text[h.start..h.end].to_owned())
            .collect()
    }

    #[test]
    fn matching_ignores_case() {
        let t = spike();
        assert_eq!(
            sources("the WARDEN and the warden", &t),
            ["WARDEN", "warden"]
        );
    }

    #[test]
    fn possessive_matches_but_longer_word_does_not() {
        let t = spike();
        assert_eq!(sources("The Warden's sword is broken.", &t), ["Warden"]);
        assert_eq!(sources("The Warden’s sword.", &t), ["Warden"]);
        assert!(find("Wardenship is a burden.", &t).is_empty());
        assert!(find("The SubWarden left.", &t).is_empty());
        assert!(find("Warden2 left.", &t).is_empty());
    }

    #[test]
    fn longer_term_wins() {
        let t = vec![Term::new("Rhee", "รี"), Term::new("Captain Rhee", "กัปตันรี")];
        let hits = find("Captain Rhee met Rhee.", &t);
        assert_eq!(hits.len(), 2);
        assert_eq!(t[hits[0].term].source, "Captain Rhee");
        assert_eq!(t[hits[1].term].source, "Rhee");
    }

    #[test]
    fn terms_at_both_ends_and_next_to_punctuation() {
        let t = spike();
        assert_eq!(sources("Paimon, Mondstadt!", &t), ["Paimon", "Mondstadt"]);
        assert_eq!(sources("(Warden)", &t), ["Warden"]);
    }

    #[test]
    fn term_ending_in_punctuation_matches_before_a_letter() {
        let t = vec![Term::new("Dr.", "ดร.")];
        assert_eq!(sources("Ask Dr.Mara", &t), ["Dr."]);
        assert!(find("Ask Mdr. Mara", &t).is_empty());
    }

    #[test]
    fn non_ascii_text_and_terms() {
        let t = vec![Term::new("Élan", "เอลัน")];
        assert_eq!(sources("— élan, ÉLAN —", &t), ["élan", "ÉLAN"]);
    }

    #[test]
    fn empty_sources_and_lines_match_nothing() {
        assert!(find("anything", &[Term::new("", "x")]).is_empty());
        assert!(find("", &spike()).is_empty());
    }

    #[test]
    fn english_term_against_thai_letters_still_matches() {
        let t = spike();
        assert_eq!(replace_terms("ผู้คุมWardenยิ้ม", &t), "ผู้คุมวอร์เดนยิ้ม");
    }

    #[test]
    fn used_lists_each_term_once_in_order() {
        let t = spike();
        let u = used(
            "Tell Captain Rhee that Paimon found the Ember Crown. Paimon!",
            &t,
        );
        let names: Vec<&str> = u.iter().map(|t| t.source.as_str()).collect();
        assert_eq!(names, ["Captain Rhee", "Paimon", "Ember Crown"]);
        assert!(used("Nothing here.", &t).is_empty());
    }

    #[test]
    fn game_terms_override_shared_ones() {
        let shared = vec![Term::new("Warden", "ผู้คุม"), Term::new("Paimon", "ไพมอน")];
        let game = vec![Term::new("warden", "วอร์เดน")];
        let all = merge(shared, game);
        assert_eq!(
            all,
            vec![Term::new("Paimon", "ไพมอน"), Term::new("warden", "วอร์เดน")]
        );
        assert_eq!(used("The Warden.", &all)[0].target, "วอร์เดน");
    }

    #[test]
    fn protect_numbers_terms_by_first_appearance() {
        let t = spike();
        let p = protect(
            "Did the Warden really steal the Ember Crown from the Hollow Order?",
            &t,
        );
        assert_eq!(p.text, "Did the [1] really steal the [2] from the [3]?");
        let names: Vec<&str> = p.terms().iter().map(|t| t.source.as_str()).collect();
        assert_eq!(names, ["Warden", "Ember Crown", "Hollow Order"]);
    }

    #[test]
    fn repeated_term_keeps_its_number() {
        let p = protect(
            "Ember Crown, Ember Crown, always the Ember Crown!",
            &spike(),
        );
        assert_eq!(p.text, "[1], [1], always the [1]!");
        assert_eq!(
            p.restore("[1], [1], [1] เสมอ!").unwrap(),
            "มงกุฎเอมเบอร์, มงกุฎเอมเบอร์, มงกุฎเอมเบอร์ เสมอ!"
        );
    }

    #[test]
    fn possessive_keeps_its_suffix() {
        let p = protect("The Warden's sword is broken.", &spike());
        assert_eq!(p.text, "The [1]'s sword is broken.");
    }

    #[test]
    fn restore_keeps_spaces_the_translator_added() {
        let p = protect("The Ember Crown was stolen.", &spike());
        assert_eq!(p.restore("[1] ถูกขโมย").unwrap(), "มงกุฎเอมเบอร์ ถูกขโมย");
    }

    #[test]
    fn restore_fails_when_a_placeholder_is_missing() {
        let p = protect("We met the Warden. The Warden smiled.", &spike());
        assert_eq!(p.text, "We met the [1]. The [1] smiled.");
        assert_eq!(p.restore("เราพบ [1] แล้วเขายิ้ม"), None);
        assert_eq!(p.restore("เราพบผู้คุม"), None);
    }

    #[test]
    fn restore_fails_when_a_placeholder_comes_back_too_often() {
        let p = protect("Warden!", &spike());
        assert_eq!(p.restore("[1] [1]!"), None);
    }

    #[test]
    fn restore_leaves_other_brackets_alone() {
        let p = protect("Paimon is hungry.", &spike());
        assert_eq!(
            p.restore("[1] หิว [9] [x] [] [").unwrap(),
            "ไพมอน หิว [9] [x] [] ["
        );
        assert_eq!(p.restore("[0] หิว"), None);
    }

    #[test]
    fn restore_does_not_reread_thai_that_looks_like_a_placeholder() {
        let t = vec![Term::new("Alpha", "[2]"), Term::new("Beta", "บีตา")];
        let p = protect("Alpha and Beta", &t);
        assert_eq!(p.restore("[1] กับ [2]").unwrap(), "[2] กับ บีตา");
    }

    #[test]
    fn line_without_terms_is_unchanged() {
        let p = protect("Nobody here.", &spike());
        assert!(p.is_empty());
        assert_eq!(p.text, "Nobody here.");
        assert_eq!(p.restore("ไม่มีใคร").unwrap(), "ไม่มีใคร");
    }

    #[test]
    fn replace_terms_swaps_leftover_english() {
        let t = spike();
        assert_eq!(
            replace_terms("กัปตัน Rhee และ Captain Rhee กับ warden's", &t),
            "กัปตัน Rhee และ กัปตันรี กับ วอร์เดน's"
        );
    }

    #[test]
    fn clean_trims_and_checks() {
        assert_eq!(
            clean("  Captain   Rhee ", " กัปตัน รี "),
            Ok(Term::new("Captain Rhee", "กัปตัน รี"))
        );
        assert_eq!(clean(" ", "ก"), Err(TermError::EmptySource));
        assert_eq!(clean("A", "\t"), Err(TermError::EmptyTarget));
        assert_eq!(clean("--", "ก"), Err(TermError::NoLetters));
        let long = "a".repeat(MAX_TERM_CHARS + 1);
        assert_eq!(clean(&long, "ก"), Err(TermError::TooLong));
        assert_eq!(clean("a", &long), Err(TermError::TooLong));
        for e in [
            TermError::EmptySource,
            TermError::EmptyTarget,
            TermError::TooLong,
            TermError::NoLetters,
        ] {
            assert!(!e.message().is_empty());
        }
    }
}
