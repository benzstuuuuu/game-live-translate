//! Which translator does which job (FR-TRN-02, 16). Live lines use the
//! app-wide pick; "แปลใหม่" on /reader has a pick of its own, which by
//! default follows the live one. Both picks name engines from the same list
//! (`app::Config.translators`), so an engine added there is offered for both.

/// The jobs a translator does; each may use its own engine (FR-TRN-16) and
/// its own model (FR-TRN-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// Lines as they appear on screen.
    Live,
    /// A line translated again from /reader (FR-TRN-08).
    Retranslate,
}

/// The engine that does `job`: the retranslate pick when there is one, else
/// the live engine.
pub fn engine_for(job: Job, live: &'static str, retranslate: Option<&'static str>) -> &'static str {
    match (job, retranslate) {
        (Job::Retranslate, Some(engine)) => engine,
        _ => live,
    }
}

/// A retranslate pick read from /setup or settings.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// No name, or a blank one: retranslate with the live engine.
    SameAsLive,
    /// One of the engines this build has.
    Engine(&'static str),
    /// A name this build has no engine for.
    Unknown,
}

/// Reads `name` against the engines this build has.
pub fn pick(name: Option<&str>, engines: &[&'static str]) -> Pick {
    let Some(name) = name.map(str::trim).filter(|n| !n.is_empty()) else {
        return Pick::SameAsLive;
    };
    engines
        .iter()
        .find(|e| **e == name)
        .map_or(Pick::Unknown, |e| Pick::Engine(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGINES: &[&str] = &["apple", "gemini", "fake"];

    #[test]
    fn live_lines_use_the_live_engine_whatever_retranslate_picked() {
        assert_eq!(engine_for(Job::Live, "apple", None), "apple");
        assert_eq!(engine_for(Job::Live, "apple", Some("gemini")), "apple");
    }

    #[test]
    fn retranslate_uses_its_own_pick() {
        assert_eq!(
            engine_for(Job::Retranslate, "apple", Some("gemini")),
            "gemini"
        );
    }

    #[test]
    fn retranslate_follows_live_without_a_pick() {
        assert_eq!(engine_for(Job::Retranslate, "apple", None), "apple");
        assert_eq!(engine_for(Job::Retranslate, "gemini", None), "gemini");
    }

    #[test]
    fn missing_or_blank_name_means_same_as_live() {
        assert_eq!(pick(None, ENGINES), Pick::SameAsLive);
        assert_eq!(pick(Some(""), ENGINES), Pick::SameAsLive);
        assert_eq!(pick(Some("  "), ENGINES), Pick::SameAsLive);
    }

    #[test]
    fn a_known_engine_is_picked_by_its_name() {
        assert_eq!(pick(Some("gemini"), ENGINES), Pick::Engine("gemini"));
        assert_eq!(pick(Some(" fake "), ENGINES), Pick::Engine("fake"));
    }

    #[test]
    fn an_engine_this_build_lacks_is_unknown() {
        assert_eq!(pick(Some("claude"), ENGINES), Pick::Unknown);
        assert_eq!(pick(Some("Gemini"), ENGINES), Pick::Unknown);
        assert_eq!(pick(Some("apple"), &["gemini"]), Pick::Unknown);
    }
}
