//! Pure logic: no network, no OBS, no disk. Unit-tested, ≥80% line coverage in CI.

pub mod access;
pub mod audio_lines;
pub mod bar;
pub mod bar_hide;
pub mod checklist;
pub mod dedup;
pub mod error;
pub mod fallback;
pub mod gemini;
pub mod glossary;
pub mod live;
pub mod obs;
pub mod obs_stream;
pub mod profile;
pub mod reconnect;
pub mod recovery;
pub mod region;
pub mod thai;
pub mod tokens;
pub mod wav;
