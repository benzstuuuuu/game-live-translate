//! Game profiles (FR-SET-04, FR-AUD-02): what /setup may save as a name and
//! as the source of text.

/// Longest name, in characters; the /reader header has one line for it.
pub const NAME_MAX: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    Taken,
}

impl NameError {
    /// What to tell the player, in Thai.
    pub fn message(self) -> String {
        match self {
            Self::Empty => "ตั้งชื่อโปรไฟล์ก่อน".into(),
            Self::TooLong => format!("ชื่อยาวเกิน {NAME_MAX} ตัวอักษร"),
            Self::Taken => "มีโปรไฟล์ชื่อนี้แล้ว".into(),
        }
    }
}

/// `raw` with outer spaces trimmed and inner runs of spaces made one, or why
/// it cannot be a profile name. `others` are the names of the other
/// profiles; case and spacing do not make a name new.
pub fn clean_name(raw: &str, others: &[String]) -> Result<String, NameError> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.chars().count() > NAME_MAX {
        return Err(NameError::TooLong);
    }
    let key = name.to_lowercase();
    let taken = others.iter().any(|o| {
        o.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
            == key
    });
    if taken {
        return Err(NameError::Taken);
    }
    Ok(name)
}

/// Where a profile's text comes from: the subtitle box or the game's audio.
pub const SOURCES: [&str; 2] = ["subtitle", "audio"];

/// `raw` as one of `SOURCES`, or None.
pub fn source(raw: &str) -> Option<&'static str> {
    SOURCES.into_iter().find(|s| *s == raw.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn others(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn trims_and_collapses_spaces() {
        assert_eq!(
            clean_name("  Genshin   Impact \t", &[]),
            Ok("Genshin Impact".into())
        );
        assert_eq!(clean_name("เกมแรก", &[]), Ok("เกมแรก".into()));
    }

    #[test]
    fn blank_is_empty() {
        assert_eq!(clean_name("", &[]), Err(NameError::Empty));
        assert_eq!(clean_name("   \n", &[]), Err(NameError::Empty));
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        let thai = "ก".repeat(NAME_MAX);
        assert_eq!(clean_name(&thai, &[]), Ok(thai.clone()));
        assert_eq!(
            clean_name(&format!("{thai}ข"), &[]),
            Err(NameError::TooLong)
        );
    }

    #[test]
    fn same_name_ignoring_case_and_spaces_is_taken() {
        let o = others(&["Final Fantasy  XVI", "เกมแรก"]);
        assert_eq!(clean_name("final fantasy xvi", &o), Err(NameError::Taken));
        assert_eq!(clean_name(" เกมแรก ", &o), Err(NameError::Taken));
        assert_eq!(
            clean_name("Final Fantasy XV", &o),
            Ok("Final Fantasy XV".into())
        );
    }

    #[test]
    fn source_is_subtitle_or_audio() {
        assert_eq!(source("subtitle"), Some("subtitle"));
        assert_eq!(source(" audio "), Some("audio"));
        assert_eq!(source("Audio"), None);
        assert_eq!(source("ocr"), None);
        assert_eq!(source(""), None);
    }

    #[test]
    fn every_error_has_a_thai_message() {
        for e in [NameError::Empty, NameError::TooLong, NameError::Taken] {
            assert!(!e.message().is_empty());
        }
        assert!(NameError::TooLong.message().contains("40"));
    }
}
