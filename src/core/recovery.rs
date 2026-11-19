//! Getting back to translating by itself once a problem is fixed (FR-ERR-04
//! to 06): how soon to try again for each cause, and when the cause card
//! changes or goes away. No I/O; `crate::recovery` runs it.

use super::error::{AppError, ErrorCode};
use std::time::Duration;

/// OBS closed, WebSocket Server off, card or scene missing: open OBS again
/// this often (FR-ERR-05).
pub const RECONNECT_EVERY: Duration = Duration::from_secs(5);
/// Card black: ask the open source for a frame again this often, so the
/// picture coming back is seen within about a second (FR-ERR-06).
pub const RECHECK_BLACK_EVERY: Duration = Duration::from_secs(1);

/// What to do after the frame source failed with an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// Drop the source and open OBS again after this long.
    Reopen(Duration),
    /// Keep the source and ask it for the next frame again after this long.
    Recheck(Duration),
    /// Only the player can fix it; wait for "ลองอีกครั้ง".
    Never,
}

impl Retry {
    pub fn delay(self) -> Option<Duration> {
        match self {
            Self::Reopen(d) | Self::Recheck(d) => Some(d),
            Self::Never => None,
        }
    }
}

/// How the session gets past `code`.
pub fn retry_for(code: ErrorCode) -> Retry {
    match code {
        ErrorCode::ObsUnreachable | ErrorCode::CardNotFound => Retry::Reopen(RECONNECT_EVERY),
        ErrorCode::CardBlack => Retry::Recheck(RECHECK_BLACK_EVERY),
        // Reconnecting Live Translate is #71. Microphone permission is
        // turned on by the player in System Settings.
        ErrorCode::TranslatorUnavailable
        | ErrorCode::NoRegion
        | ErrorCode::LiveTranslateFailed
        | ErrorCode::MicPermissionDenied => Retry::Never,
    }
}

/// Whether a session is stopped by a problem, and which one the pages show.
#[derive(Debug, Default)]
pub struct Outage {
    shown: Option<AppError>,
}

impl Outage {
    /// The source failed with `error`. Returns the error to show when it
    /// differs from the one shown, so the pages are not told the same thing
    /// on every try.
    pub fn fail(&mut self, error: AppError) -> Option<AppError> {
        if self.shown.as_ref() == Some(&error) {
            return None;
        }
        self.shown = Some(error.clone());
        Some(error)
    }

    /// A frame came. True when that ends an outage: back to translating.
    pub fn frame(&mut self) -> bool {
        self.shown.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obs_and_card_problems_reconnect_every_five_seconds() {
        assert_eq!(
            retry_for(ErrorCode::ObsUnreachable),
            Retry::Reopen(Duration::from_secs(5))
        );
        assert_eq!(
            retry_for(ErrorCode::CardNotFound),
            Retry::Reopen(RECONNECT_EVERY)
        );
    }

    #[test]
    fn black_card_keeps_the_source_and_looks_again_soon() {
        assert_eq!(
            retry_for(ErrorCode::CardBlack),
            Retry::Recheck(RECHECK_BLACK_EVERY)
        );
        assert!(RECHECK_BLACK_EVERY < RECONNECT_EVERY);
    }

    #[test]
    fn problems_only_the_player_can_fix_are_not_retried() {
        assert_eq!(retry_for(ErrorCode::TranslatorUnavailable), Retry::Never);
        assert_eq!(retry_for(ErrorCode::NoRegion), Retry::Never);
        assert_eq!(retry_for(ErrorCode::LiveTranslateFailed), Retry::Never);
        assert_eq!(retry_for(ErrorCode::MicPermissionDenied), Retry::Never);
    }

    #[test]
    fn card_audio_unplugged_is_tried_again_like_the_card() {
        let code = AppError::card_audio_missing("UGREEN 15390").code;
        assert_eq!(retry_for(code), Retry::Reopen(RECONNECT_EVERY));
    }

    #[test]
    fn delay_is_how_long_to_wait_or_none() {
        assert_eq!(
            Retry::Reopen(RECONNECT_EVERY).delay(),
            Some(RECONNECT_EVERY)
        );
        assert_eq!(
            Retry::Recheck(RECHECK_BLACK_EVERY).delay(),
            Some(RECHECK_BLACK_EVERY)
        );
        assert_eq!(Retry::Never.delay(), None);
    }

    #[test]
    fn a_frame_without_an_outage_changes_nothing() {
        let mut o = Outage::default();
        assert!(!o.frame());
        assert!(!o.frame());
    }

    #[test]
    fn first_failure_is_shown_and_the_same_one_again_is_not() {
        let mut o = Outage::default();
        assert_eq!(
            o.fail(AppError::obs_unreachable()),
            Some(AppError::obs_unreachable())
        );
        assert_eq!(o.fail(AppError::obs_unreachable()), None);
        assert_eq!(o.fail(AppError::obs_unreachable()), None);
    }

    #[test]
    fn a_different_cause_replaces_the_card() {
        let mut o = Outage::default();
        o.fail(AppError::obs_unreachable());
        // OBS is back but the card is not plugged in yet.
        let next = AppError::card_not_found("UGREEN 15390");
        assert_eq!(o.fail(next.clone()), Some(next));
    }

    #[test]
    fn a_frame_after_a_failure_ends_the_outage_once() {
        let mut o = Outage::default();
        o.fail(AppError::card_black());
        assert!(o.frame(), "picture is back: translate again");
        assert!(!o.frame());
        // A later failure is new again, even with the same cause.
        assert_eq!(o.fail(AppError::card_black()), Some(AppError::card_black()));
    }
}
