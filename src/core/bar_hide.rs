//! When the stream bar of the audio path goes (FR-AUD-06): it shows as soon
//! as speech starts and follows it; once the line closes, it goes if no new
//! speech starts within `HIDE_AFTER`. Time is passed in, so tests need no
//! clock.

use std::time::{Duration, Instant};

/// How long a closed audio line stays on the stream with no new speech.
pub const HIDE_AFTER: Duration = Duration::from_secs(3);

/// The wait for hiding the bar after the last audio line closed.
#[derive(Debug, Default)]
pub struct HideTimer {
    due: Option<Instant>,
}

impl HideTimer {
    /// An audio line was pushed at `now`: an open one (`done` false) is
    /// speech going on, so nothing is due; a closed one starts the wait.
    pub fn line(&mut self, done: bool, now: Instant) {
        self.due = done.then(|| now + HIDE_AFTER);
    }

    /// When the bar should go, if a closed line is waiting.
    pub fn due(&self) -> Option<Instant> {
        self.due
    }

    /// True once, when the wait is over at `now`: hide the bar.
    pub fn fire(&mut self, now: Instant) -> bool {
        match self.due {
            Some(due) if now >= due => {
                self.due = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    #[test]
    fn hide_after_is_three_seconds() {
        assert_eq!(HIDE_AFTER, Duration::from_secs(3));
    }

    #[test]
    fn nothing_is_due_before_any_line() {
        let mut t = HideTimer::default();
        assert_eq!(t.due(), None);
        assert!(!t.fire(Instant::now()));
    }

    #[test]
    fn speech_going_on_never_hides() {
        let t0 = Instant::now();
        let mut t = HideTimer::default();
        t.line(false, t0);
        assert_eq!(t.due(), None);
        assert!(!t.fire(t0 + secs(10.0)));
    }

    #[test]
    fn closed_line_hides_three_seconds_later_once() {
        let t0 = Instant::now();
        let mut t = HideTimer::default();
        t.line(false, t0);
        t.line(true, t0 + secs(1.0));
        assert_eq!(t.due(), Some(t0 + secs(4.0)));
        assert!(!t.fire(t0 + secs(3.9)));
        assert!(t.fire(t0 + secs(4.0)));
        assert_eq!(t.due(), None);
        assert!(!t.fire(t0 + secs(5.0)), "fires only once");
    }

    #[test]
    fn speech_before_three_seconds_keeps_the_bar() {
        let t0 = Instant::now();
        let mut t = HideTimer::default();
        t.line(true, t0);
        t.line(false, t0 + secs(2.9));
        assert_eq!(t.due(), None);
        assert!(!t.fire(t0 + secs(3.0)));
        assert!(!t.fire(t0 + secs(10.0)));
    }

    #[test]
    fn the_next_closed_line_starts_the_wait_again() {
        let t0 = Instant::now();
        let mut t = HideTimer::default();
        t.line(true, t0);
        t.line(false, t0 + secs(2.0));
        t.line(true, t0 + secs(5.0));
        assert!(!t.fire(t0 + secs(7.9)));
        assert!(t.fire(t0 + secs(8.0)));
    }

    #[test]
    fn late_check_still_hides() {
        let t0 = Instant::now();
        let mut t = HideTimer::default();
        t.line(true, t0);
        assert!(t.fire(t0 + secs(60.0)));
    }
}
