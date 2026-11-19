//! Lines of the audio path (FR-AUD-06): Live Translate sends the English
//! transcript and the Thai translation as pieces about once a second and
//! never says where a sentence ends, so pieces pile into the open line until
//! one of two things closes it (the rule spike #13 found closest to real
//! subtitles):
//!
//! - the first empty Thai piece: in silence the model sends an empty English
//!   piece, then an empty Thai one; Thai lags English, so its empty piece is
//!   the safe one;
//! - once the English holds a sentence end (`.`, `?` or `!` at the end or
//!   before a space) with 40 or more characters up to it, the next English
//!   piece that is not empty: the line closes at that sentence end, and the
//!   words after it start the new line with the piece. Pieces are cut
//!   anywhere, so the end is often inside one. Thai pieces that land before
//!   the next English piece still belong to the old line.

/// English and Thai of one line so far, trimmed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Text {
    pub en: String,
    pub th: String,
}

/// What a piece did to the lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The open line now reads this; the first one opens a new line.
    Open(Text),
    /// The open line is done and reads this.
    Close(Text),
}

/// English long enough to close on a sentence end, in characters.
pub const MIN_SENTENCE_CHARS: usize = 40;

/// The open line, built from pieces as they come.
#[derive(Debug, Default)]
pub struct Splitter {
    en: String,
    th: String,
}

impl Splitter {
    /// A piece of the English transcript.
    pub fn en(&mut self, piece: &str) -> Vec<Step> {
        if piece.trim().is_empty() {
            return Vec::new();
        }
        let mut steps = Vec::new();
        if let Some(end) = self.sentence_end() {
            let rest = self.en.split_off(end);
            steps.extend(self.close());
            self.en = rest;
        }
        self.en.push_str(piece);
        steps.push(Step::Open(self.text()));
        steps
    }

    /// A piece of the Thai translation.
    pub fn th(&mut self, piece: &str) -> Vec<Step> {
        if piece.trim().is_empty() {
            return self.close().into_iter().collect();
        }
        self.th.push_str(piece);
        vec![Step::Open(self.text())]
    }

    /// Closes the open line, if any (the session is ending).
    pub fn close(&mut self) -> Option<Step> {
        if self.en.trim().is_empty() && self.th.trim().is_empty() {
            return None;
        }
        let text = self.text();
        self.en.clear();
        self.th.clear();
        Some(Step::Close(text))
    }

    /// Byte index just after the last sentence end with at least
    /// `MIN_SENTENCE_CHARS` before it, if any.
    fn sentence_end(&self) -> Option<usize> {
        let en = &self.en;
        en.char_indices()
            .filter(|&(i, c)| {
                matches!(c, '.' | '?' | '!')
                    && en[i + 1..].chars().next().is_none_or(char::is_whitespace)
            })
            .map(|(i, _)| i + 1)
            .rfind(|&end| en[..end].trim().chars().count() >= MIN_SENTENCE_CHARS)
    }

    fn text(&self) -> Text {
        Text {
            en: self.en.trim().to_owned(),
            th: self.th.trim().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A piece as Live Translate sends it: English or Thai.
    enum P {
        En(&'static str),
        Th(&'static str),
    }

    fn feed(pieces: &[P]) -> Vec<Step> {
        let mut s = Splitter::default();
        pieces
            .iter()
            .flat_map(|p| match p {
                P::En(t) => s.en(t),
                P::Th(t) => s.th(t),
            })
            .collect()
    }

    fn text(en: &str, th: &str) -> Text {
        Text {
            en: en.into(),
            th: th.into(),
        }
    }

    fn closed(steps: &[Step]) -> Vec<Text> {
        steps
            .iter()
            .filter_map(|s| match s {
                Step::Close(t) => Some(t.clone()),
                Step::Open(_) => None,
            })
            .collect()
    }

    #[test]
    fn pieces_grow_the_open_line() {
        let steps = feed(&[
            P::En("Keep your"),
            P::Th("เก็บ"),
            P::En(" lantern low"),
            P::Th("ตะเกียงไว้ต่ำ ๆ"),
        ]);
        assert_eq!(
            steps,
            vec![
                Step::Open(text("Keep your", "")),
                Step::Open(text("Keep your", "เก็บ")),
                Step::Open(text("Keep your lantern low", "เก็บ")),
                Step::Open(text("Keep your lantern low", "เก็บตะเกียงไว้ต่ำ ๆ")),
            ]
        );
    }

    #[test]
    fn first_empty_thai_piece_closes_the_line() {
        // Silence: an empty English piece first, then an empty Thai one.
        let steps = feed(&[
            P::En(" Keep your lantern low."),
            P::Th("เก็บตะเกียงไว้ต่ำ ๆ"),
            P::En(""),
            P::Th(""),
            P::En(""),
            P::Th(""),
        ]);
        assert_eq!(
            closed(&steps),
            vec![text("Keep your lantern low.", "เก็บตะเกียงไว้ต่ำ ๆ")]
        );
        assert_eq!(steps.last(), Some(&Step::Close(closed(&steps)[0].clone())));
    }

    #[test]
    fn empty_english_alone_closes_nothing() {
        let steps = feed(&[P::En("Wait"), P::En(""), P::En("  "), P::En(" for me")]);
        assert!(closed(&steps).is_empty());
        assert_eq!(steps.last(), Some(&Step::Open(text("Wait for me", ""))));
    }

    #[test]
    fn long_sentence_closes_when_the_next_english_arrives() {
        // 46 characters ending with a full stop; the Thai tail that lands
        // before the next English piece still belongs to it.
        let steps = feed(&[
            P::En("The bridge is out, so we will swim"),
            P::Th("สะพานพัง"),
            P::En(" across the river."),
            P::Th(" เราเลยต้องว่ายข้ามแม่น้ำ"),
            P::En(" Stay close"),
            P::Th("อยู่ใกล้ ๆ"),
        ]);
        assert_eq!(
            closed(&steps),
            vec![text(
                "The bridge is out, so we will swim across the river.",
                "สะพานพัง เราเลยต้องว่ายข้ามแม่น้ำ"
            )]
        );
        assert_eq!(
            steps.last(),
            Some(&Step::Open(text("Stay close", "อยู่ใกล้ ๆ")))
        );
    }

    #[test]
    fn closing_and_opening_come_from_one_piece_in_order() {
        let mut s = Splitter::default();
        s.en("Where are we going after the storm passes?");
        assert_eq!(
            s.en(" Home."),
            vec![
                Step::Close(text("Where are we going after the storm passes?", "")),
                Step::Open(text("Home.", "")),
            ]
        );
    }

    #[test]
    fn short_sentence_keeps_going() {
        // "No. But" ends with a full stop at "No." but is far below 40.
        let steps = feed(&[
            P::En("No."),
            P::En(" But it's the only road left!"),
            P::En(" Move."),
        ]);
        assert!(closed(&steps).is_empty());
        assert_eq!(
            steps.last(),
            Some(&Step::Open(text(
                "No. But it's the only road left! Move.",
                ""
            )))
        );
    }

    #[test]
    fn long_english_without_a_sentence_end_stays_open() {
        let steps = feed(&[
            P::En("We have been walking along this road since morning"),
            P::En(" and the town is still"),
        ]);
        assert!(closed(&steps).is_empty());
    }

    #[test]
    fn sentence_end_counts_with_trailing_space_and_question_or_bang() {
        for end in ["?", "!", ". "] {
            let mut s = Splitter::default();
            s.en(&format!(
                "Did you really think the gate would stay open{end}"
            ));
            let steps = s.en("No");
            assert!(matches!(steps[0], Step::Close(_)), "{end:?}");
        }
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        // 39 characters of English with a sentence end is not enough.
        let mut s = Splitter::default();
        s.en(&format!("{}.", "a".repeat(MIN_SENTENCE_CHARS - 2)));
        assert!(closed(&s.en("b")).is_empty());
    }

    #[test]
    fn thai_first_opens_a_line_too() {
        let steps = feed(&[P::Th("ไปกัน"), P::Th("")]);
        assert_eq!(
            steps,
            vec![Step::Open(text("", "ไปกัน")), Step::Close(text("", "ไปกัน"))]
        );
    }

    #[test]
    fn sentence_end_inside_a_piece_splits_at_the_next_english_piece() {
        // The real service cuts pieces anywhere: the full stop lands in the
        // middle of one. The words after it start the next line once more
        // English comes; the Thai that came before stays with the old line.
        let steps = feed(&[
            P::En("Mara, hold the rope while I climb over"),
            P::Th("มาร่า จับเชือกไว้"),
            P::En(" the old wall. The"),
            P::Th(" ระหว่างที่ฉันปีนข้ามกำแพงเก่า"),
            P::En(" guards are coming"),
            P::Th("ทหารยามกำลังมา"),
        ]);
        assert_eq!(
            closed(&steps),
            vec![text(
                "Mara, hold the rope while I climb over the old wall.",
                "มาร่า จับเชือกไว้ ระหว่างที่ฉันปีนข้ามกำแพงเก่า"
            )]
        );
        assert_eq!(
            steps.last(),
            Some(&Step::Open(text("The guards are coming", "ทหารยามกำลังมา")))
        );
    }

    #[test]
    fn split_takes_the_last_sentence_end_past_forty_characters() {
        let mut s = Splitter::default();
        s.en("Stop. We have walked this road since early morning! Rest");
        let steps = s.en(" here");
        assert_eq!(
            steps,
            vec![
                Step::Close(text(
                    "Stop. We have walked this road since early morning!",
                    ""
                )),
                Step::Open(text("Rest here", "")),
            ]
        );
    }

    #[test]
    fn a_dot_inside_a_word_or_number_is_no_sentence_end() {
        let mut s = Splitter::default();
        s.en("The new patch version is 3.5 and the map file is world.dat");
        assert!(closed(&s.en(" today")).is_empty());
    }

    #[test]
    fn close_on_stop_only_when_a_line_is_open() {
        let mut s = Splitter::default();
        assert_eq!(s.close(), None);
        s.en("Run");
        assert_eq!(s.close(), Some(Step::Close(text("Run", ""))));
        assert_eq!(s.close(), None);
    }

    #[test]
    fn many_lines_in_a_row() {
        // The shape spike #13 saw: pieces about a second apart, Thai a bit
        // behind English, silence between lines (made-up dialogue).
        let steps = feed(&[
            P::En("Captain,"),
            P::Th("กัปตัน"),
            P::En(" the ship is ready to sail."),
            P::Th(" เรือพร้อมออกแล้ว"),
            P::En(""),
            P::Th(""),
            P::En("Then raise the anchor and set a course for the islands."),
            P::Th("งั้นถอนสมอแล้วตั้งเข็มไปที่หมู่เกาะ"),
            P::En(" Nobody stays behind."),
            P::Th("ไม่มีใครถูกทิ้งไว้"),
            P::En(""),
            P::Th(""),
        ]);
        assert_eq!(
            closed(&steps),
            vec![
                text("Captain, the ship is ready to sail.", "กัปตัน เรือพร้อมออกแล้ว"),
                text(
                    "Then raise the anchor and set a course for the islands.",
                    "งั้นถอนสมอแล้วตั้งเข็มไปที่หมู่เกาะ"
                ),
                text("Nobody stays behind.", "ไม่มีใครถูกทิ้งไว้"),
            ]
        );
    }
}
