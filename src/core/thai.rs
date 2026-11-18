//! Thai text clean-up applied to every translator's output (FR-TRN-01).

/// Turns nikhahit + sara aa (U+0E4D U+0E32) into sara am (U+0E33). Both look
/// the same on screen, but only sara am is correct for search and copy.
pub fn fix_am(s: &str) -> String {
    s.replace("\u{0E4D}\u{0E32}", "\u{0E33}")
}

#[cfg(test)]
mod tests {
    use super::fix_am;

    const NIKHAHIT_AA: &str = "\u{0E4D}\u{0E32}";
    const AM: &str = "\u{0E33}";

    #[test]
    fn nikhahit_then_aa_becomes_am() {
        assert_eq!(fix_am(&format!("ท{NIKHAHIT_AA}")), format!("ท{AM}"));
    }

    #[test]
    fn every_occurrence_is_fixed() {
        let input = format!("น{NIKHAHIT_AA}ท{NIKHAHIT_AA}ง{NIKHAHIT_AA}น");
        assert_eq!(fix_am(&input), format!("น{AM}ท{AM}ง{AM}น"));
    }

    #[test]
    fn tone_mark_between_consonant_and_am_is_kept() {
        // น้ำ written as น + mai tho + nikhahit + sara aa.
        let input = format!("น\u{0E49}{NIKHAHIT_AA}");
        assert_eq!(fix_am(&input), format!("น\u{0E49}{AM}"));
    }

    #[test]
    fn correct_am_and_other_text_unchanged() {
        for s in ["น้ำ", "สวัสดี Mara", "", "ทำ 123"] {
            assert_eq!(fix_am(s), s);
        }
    }

    #[test]
    fn lone_nikhahit_or_aa_unchanged() {
        let input = "\u{0E4D}ก\u{0E32}";
        assert_eq!(fix_am(input), input);
    }
}
