//! The translation prompt every prompt-taking translator sends (FR-TRN-03):
//! the tone and the extra prompt from /setup (FR-SET-03) as the system
//! prompt, and the earlier lines, the glossary terms the line uses
//! (FR-GLS-04) and the line itself as the user turn.

use super::gemini::Settings;
use super::glossary::Term;

/// What the translator is and how to answer: the tone and the extra prompt.
pub fn system(s: &Settings) -> String {
    let mut p = format!(
        "You translate English video game subtitles into Thai for a player reading along live.\n\
         Reply with the Thai translation of the line after \"Line:\" only: no quotes, notes, \
         romanization or English.\n\
         Keep names of people and places as they are unless they have a well-known Thai form.\n\
         Tone: {}.\n\
         The line was read off the screen by OCR, so it may be cut off, misspelled or garbled. \
         Translate your best reading of it and reply with that one translation only: never ask \
         for the line again, never comment on it, never show a draft or a correction. If it \
         cannot be read at all, reply with the line unchanged.",
        s.tone.instruction()
    );
    if !s.extra_prompt.is_empty() {
        p.push_str("\n\nThe player's own instructions:\n");
        p.push_str(&s.extra_prompt);
    }
    p
}

/// The line, after up to `context_lines` earlier lines and the `glossary`
/// terms it uses.
pub fn user(text: &str, context: &[String], glossary: &[Term], s: &Settings) -> String {
    let keep = context.len().min(usize::from(s.context_lines));
    let earlier = &context[context.len() - keep..];
    let mut p = String::new();
    if !earlier.is_empty() {
        p.push_str("Earlier lines, for context only (do not translate them):\n");
        for line in earlier {
            p.push_str("- ");
            p.push_str(line);
            p.push('\n');
        }
        p.push('\n');
    }
    if !glossary.is_empty() {
        p.push_str("Glossary: translate these terms exactly as given:\n");
        for t in glossary {
            p.push_str(&format!("- {} → {}\n", t.source, t.target));
        }
        p.push('\n');
    }
    p.push_str("Line:\n");
    p.push_str(text);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    const OCR: &str = "The line was read off the screen by OCR, so it may be cut off, \
        misspelled or garbled. Translate your best reading of it and reply with that one \
        translation only: never ask for the line again, never comment on it, never show a \
        draft or a correction. If it cannot be read at all, reply with the line unchanged.";

    #[test]
    fn the_ocr_sentence_comes_right_after_the_tone() {
        let s = Settings::default();
        let tone = format!("Tone: {}.", s.tone.instruction());
        assert!(
            system(&s).ends_with(&format!("{tone}\n{OCR}")),
            "{}",
            system(&s)
        );
    }

    #[test]
    fn the_players_own_instructions_come_after_the_ocr_sentence() {
        let s = Settings {
            extra_prompt: "Mara is a woman.".into(),
            ..Settings::default()
        };
        let own = format!("{OCR}\n\nThe player's own instructions:\nMara is a woman.");
        assert!(system(&s).ends_with(&own), "{}", system(&s));
    }
}
