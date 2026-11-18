//! The 20 lines and glossary of the #27 spike (`spike/translate`,
//! `src/bin/glossary.rs`), shared by the ignored real-engine tests.

use yokoten::core::glossary::{self, Term};

pub const GLOSSARY: &[(&str, &str)] = &[
    ("Ember Crown", "มงกุฎเอมเบอร์"),
    ("Hollow Order", "ภาคีฮอลโลว์"),
    ("Captain Rhee", "กัปตันรี"),
    ("Warden", "วอร์เดน"),
    ("Paimon", "ไพมอน"),
    ("Mondstadt", "มอนด์สตัดท์"),
];

pub const LINES: &[&str] = &[
    "The Ember Crown was stolen from the vault three days ago.",
    "Your reputation precedes you, Warden.",
    "The Hollow Order doesn't forgive deserters.",
    "Captain Rhee wants a word with you. Now.",
    "The Ember Crown belongs to the people, not to the throne.",
    "The Warden will not bargain with thieves.",
    "The Hollow Order has eyes in every port.",
    "Captain Rhee was right about you.",
    "Welcome back, Warden. We kept your seat warm.",
    "Paimon is hungry again!",
    "Let's head back to Mondstadt before the storm.",
    "Did the Warden really steal the Ember Crown from the Hollow Order?",
    "Tell Captain Rhee that Paimon found the Ember Crown.",
    "Warden, the Hollow Order is waiting for you in Mondstadt.",
    "Nobody trusts the Hollow Order anymore.",
    "Is that the Ember Crown? It's smaller than I imagined.",
    "The Warden's sword is broken.",
    "Paimon's map says Mondstadt is close.",
    "Ember Crown, Ember Crown, always the Ember Crown!",
    "We met the Warden. The Warden smiled.",
];

pub fn terms() -> Vec<Term> {
    GLOSSARY.iter().map(|(s, t)| Term::new(s, t)).collect()
}

/// Thai terms the line should carry but `thai` lacks; empty when every
/// term the line uses came through.
pub fn missing(line: &str, thai: &str) -> Vec<String> {
    glossary::used(line, &terms())
        .into_iter()
        .filter(|t| !thai.contains(&t.target))
        .map(|t| t.target)
        .collect()
}
