//! One audio session over many Live Translate connections (FR-AUD-07). A
//! connection lasts about ten minutes: `goAway` comes about 50 s before the
//! server closes it. The session goes on over a new connection that resumes
//! with the latest handle from `sessionResumptionUpdate`:
//!
//! - after `goAway`, at the next silence (an empty Thai piece, so no line is
//!   cut), or when 5 s are left if no silence comes;
//! - at once when the connection drops by itself;
//! - a try that fails waits 1 s before the next; after 3 failed tries in a
//!   row the session shows `live_translate_failed` and tries again on the
//!   schedule of `recovery` (#43).
//!
//! Audio heard while there is no connection waits in a `Backlog`. Stopping
//! sends 2 s of silence before `audioStreamEnd`, or the last sentence is
//! never translated, then waits a little for that sentence (`Ending`).
//!
//! No I/O and no clock: the time comes in as `now`. `crate::live` runs it.

use super::live::CHUNK_BYTES;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// After `goAway`, reconnect when this little time is left even without a
/// silence.
pub const RECONNECT_WHEN_LEFT: Duration = Duration::from_secs(5);
/// Failed tries in a row before the cause card shows.
pub const TRIES: u32 = 3;
/// Pause after a failed try before the next one.
pub const RETRY_GAP: Duration = Duration::from_secs(1);
/// Opening a connection that takes longer than this is a failed try.
pub const CONNECT_LIMIT: Duration = Duration::from_secs(10);
/// Silence sent before `audioStreamEnd`: 2 s of 100 ms chunks.
pub const SILENCE_CHUNKS: usize = 20;
/// How long to wait for the last sentence after `audioStreamEnd`.
pub const LAST_LINE_WAIT: Duration = Duration::from_secs(5);
/// Audio kept while there is no connection: the latest 10 s. Older audio is
/// dropped so a long outage does not come back as stale lines.
pub const BACKLOG_CHUNKS: usize = 100;

/// What to do with the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Keep it.
    Stay,
    /// Open a new one now, resuming with the latest handle.
    Reconnect,
    /// Wait this long, then open a new one.
    RetryIn(Duration),
    /// Show `live_translate_failed`, then try again as `recovery` says.
    Fail,
}

/// The latest handle, the `goAway` deadline and the failed tries.
#[derive(Debug, Default)]
pub struct Reconnect {
    handle: Option<String>,
    /// When the server will close the connection, once `goAway` came.
    closes_at: Option<Instant>,
    failures: u32,
}

impl Reconnect {
    /// The handle to resume with; None starts a new session on the server.
    pub fn handle(&self) -> Option<&str> {
        self.handle.as_deref()
    }

    /// `sessionResumptionUpdate` said the session can resume with `handle`.
    pub fn new_handle(&mut self, handle: String) {
        self.handle = Some(handle);
    }

    /// The server accepted the setup: the connection works.
    pub fn ready(&mut self) {
        self.failures = 0;
        self.closes_at = None;
    }

    /// `goAway` came at `now` with `left` until the server closes.
    pub fn go_away(&mut self, now: Instant, left: Duration) -> Next {
        self.closes_at = Some(now + left);
        self.tick(now)
    }

    /// An empty Thai piece came: the line closed, a good moment to move.
    pub fn silence(&mut self) -> Next {
        match self.closes_at.take() {
            Some(_) => Next::Reconnect,
            None => Next::Stay,
        }
    }

    /// Time passed with no silence. Moving is decided once: the new
    /// connection is not left for another before it is ready.
    pub fn tick(&mut self, now: Instant) -> Next {
        match self.closes_at {
            Some(at) if at.saturating_duration_since(now) <= RECONNECT_WHEN_LEFT => {
                self.closes_at = None;
                Next::Reconnect
            }
            _ => Next::Stay,
        }
    }

    /// When `tick` has something to say: the moment 5 s are left.
    pub fn wake_at(&self) -> Option<Instant> {
        self.closes_at
            .map(|at| at.checked_sub(RECONNECT_WHEN_LEFT).unwrap_or(at))
    }

    /// The connection ended without Yokoten closing it; `ready` says if it
    /// had worked. Losing a working one reconnects at once; a try that never
    /// worked counts as a failure.
    pub fn lost(&mut self, ready: bool) -> Next {
        self.closes_at = None;
        if ready {
            return Next::Reconnect;
        }
        self.failures += 1;
        if self.failures < TRIES {
            return Next::RetryIn(RETRY_GAP);
        }
        // The handle may be what the server refuses: start afresh.
        self.handle = None;
        Next::Fail
    }
}

/// Audio heard while no connection is ready, oldest first.
#[derive(Debug, Default)]
pub struct Backlog {
    chunks: VecDeque<Vec<u8>>,
}

impl Backlog {
    /// Keeps `chunk`, dropping the oldest past `BACKLOG_CHUNKS`.
    pub fn push(&mut self, chunk: Vec<u8>) {
        if self.chunks.len() == BACKLOG_CHUNKS {
            self.chunks.pop_front();
        }
        self.chunks.push_back(chunk);
    }

    /// The oldest chunk, to send next.
    pub fn front(&self) -> Option<&[u8]> {
        self.chunks.front().map(Vec::as_slice)
    }

    /// Forgets the oldest chunk once it is sent.
    pub fn pop(&mut self) {
        self.chunks.pop_front();
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }
}

/// The end of a session: silence, `audioStreamEnd`, then the last sentence.
#[derive(Debug)]
pub struct Ending {
    silence_left: usize,
    ended_at: Option<Instant>,
}

impl Default for Ending {
    fn default() -> Self {
        Self {
            silence_left: SILENCE_CHUNKS,
            ended_at: None,
        }
    }
}

impl Ending {
    /// The next 100 ms of silence to send, or None once 2 s went: then send
    /// `audioStreamEnd` and call `ended`.
    pub fn silence(&mut self) -> Option<Vec<u8>> {
        if self.silence_left == 0 {
            return None;
        }
        self.silence_left -= 1;
        Some(vec![0; CHUNK_BYTES])
    }

    /// True until `audioStreamEnd` is sent.
    pub fn sending(&self) -> bool {
        self.ended_at.is_none()
    }

    /// `audioStreamEnd` went out at `now`.
    pub fn ended(&mut self, now: Instant) {
        self.ended_at = Some(now);
    }

    /// When to stop waiting for the last sentence; None before the end.
    pub fn give_up_at(&self) -> Option<Instant> {
        self.ended_at.map(|at| at + LAST_LINE_WAIT)
    }

    /// The session can close: the end went out and the last line closed, or
    /// it waited long enough.
    pub fn done(&self, now: Instant, line_open: bool) -> bool {
        self.give_up_at().is_some_and(|at| !line_open || now >= at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    #[test]
    fn a_new_session_has_no_handle_and_stays() {
        let mut r = Reconnect::default();
        assert_eq!(r.handle(), None);
        assert_eq!(r.silence(), Next::Stay);
        assert_eq!(r.tick(Instant::now()), Next::Stay);
        assert_eq!(r.wake_at(), None);
    }

    #[test]
    fn keeps_the_latest_handle() {
        let mut r = Reconnect::default();
        r.new_handle("h1".into());
        r.new_handle("h2".into());
        assert_eq!(r.handle(), Some("h2"));
        r.ready();
        assert_eq!(r.handle(), Some("h2"), "a new connection keeps it");
    }

    #[test]
    fn go_away_waits_for_silence() {
        let t0 = Instant::now();
        let mut r = Reconnect::default();
        assert_eq!(r.go_away(t0, secs(50.0)), Next::Stay);
        assert_eq!(r.tick(t0 + secs(30.0)), Next::Stay);
        assert_eq!(r.silence(), Next::Reconnect);
    }

    #[test]
    fn go_away_without_silence_reconnects_when_five_seconds_are_left() {
        let t0 = Instant::now();
        let mut r = Reconnect::default();
        r.go_away(t0, secs(50.0));
        assert_eq!(r.wake_at(), Some(t0 + secs(45.0)));
        assert_eq!(r.tick(t0 + secs(44.9)), Next::Stay);
        assert_eq!(r.tick(t0 + secs(45.0)), Next::Reconnect);
    }

    #[test]
    fn go_away_with_little_time_left_reconnects_now() {
        let t0 = Instant::now();
        let mut r = Reconnect::default();
        assert_eq!(r.go_away(t0, secs(4.0)), Next::Reconnect);
        assert_eq!(r.wake_at(), None);
        let mut r = Reconnect::default();
        assert_eq!(r.go_away(t0, Duration::ZERO), Next::Reconnect);
    }

    #[test]
    fn moving_is_decided_once_while_the_new_connection_opens() {
        let t0 = Instant::now();
        let mut r = Reconnect::default();
        r.go_away(t0, secs(50.0));
        assert_eq!(r.silence(), Next::Reconnect);
        assert_eq!(r.silence(), Next::Stay);
        assert_eq!(r.wake_at(), None);
        let mut r = Reconnect::default();
        r.go_away(t0, secs(50.0));
        assert_eq!(r.tick(t0 + secs(46.0)), Next::Reconnect);
        assert_eq!(r.tick(t0 + secs(47.0)), Next::Stay);
        assert_eq!(r.wake_at(), None);
    }

    #[test]
    fn a_working_connection_forgets_the_go_away() {
        let t0 = Instant::now();
        let mut r = Reconnect::default();
        r.go_away(t0, secs(50.0));
        r.ready();
        assert_eq!(r.silence(), Next::Stay);
        assert_eq!(r.tick(t0 + secs(49.0)), Next::Stay);
    }

    #[test]
    fn a_dropped_connection_reconnects_at_once() {
        let mut r = Reconnect::default();
        r.new_handle("h".into());
        r.ready();
        assert_eq!(r.lost(true), Next::Reconnect);
        assert_eq!(r.handle(), Some("h"));
        // Dropping again and again after working is not a failure.
        r.ready();
        assert_eq!(r.lost(true), Next::Reconnect);
    }

    #[test]
    fn three_failed_tries_in_a_row_fail_and_drop_the_handle() {
        let mut r = Reconnect::default();
        r.new_handle("h".into());
        r.ready();
        assert_eq!(r.lost(true), Next::Reconnect);
        assert_eq!(r.lost(false), Next::RetryIn(RETRY_GAP));
        assert_eq!(r.lost(false), Next::RetryIn(RETRY_GAP));
        assert_eq!(r.handle(), Some("h"));
        assert_eq!(r.lost(false), Next::Fail);
        assert_eq!(r.handle(), None);
        // Every later try that fails keeps the card up.
        assert_eq!(r.lost(false), Next::Fail);
    }

    #[test]
    fn a_working_try_resets_the_count() {
        let mut r = Reconnect::default();
        r.lost(false);
        r.lost(false);
        r.ready();
        assert_eq!(r.lost(false), Next::RetryIn(RETRY_GAP));
        assert_eq!(r.lost(false), Next::RetryIn(RETRY_GAP));
        assert_eq!(r.lost(false), Next::Fail);
    }

    #[test]
    fn losing_the_connection_clears_the_go_away() {
        let t0 = Instant::now();
        let mut r = Reconnect::default();
        r.go_away(t0, secs(50.0));
        r.lost(true);
        assert_eq!(r.wake_at(), None);
        assert_eq!(r.silence(), Next::Stay);
    }

    #[test]
    fn backlog_keeps_order_and_only_the_latest_ten_seconds() {
        let mut b = Backlog::default();
        assert!(b.is_empty());
        assert_eq!(b.front(), None);
        for i in 0..BACKLOG_CHUNKS + 5 {
            b.push(vec![i as u8]);
        }
        let mut sent = Vec::new();
        while let Some(c) = b.front() {
            sent.push(c[0] as usize);
            b.pop();
        }
        assert_eq!(sent, (5..BACKLOG_CHUNKS + 5).collect::<Vec<_>>());
        assert!(b.is_empty());
    }

    #[test]
    fn ending_sends_two_seconds_of_silence_then_waits_for_the_last_line() {
        let t0 = Instant::now();
        let mut e = Ending::default();
        assert!(e.sending());
        assert!(!e.done(t0, false), "not before audioStreamEnd");
        let mut n = 0;
        while let Some(chunk) = e.silence() {
            assert_eq!(chunk, vec![0; CHUNK_BYTES]);
            n += 1;
        }
        assert_eq!(n, SILENCE_CHUNKS);
        assert_eq!(n * CHUNK_BYTES, 2 * 32_000, "2 s of 16 kHz s16le");
        assert_eq!(e.give_up_at(), None);
        e.ended(t0);
        assert!(!e.sending());
        assert_eq!(e.give_up_at(), Some(t0 + LAST_LINE_WAIT));
        assert!(!e.done(t0 + secs(1.0), true), "the last line is still open");
        assert!(e.done(t0 + secs(1.0), false), "the last line closed");
        assert!(e.done(t0 + LAST_LINE_WAIT, true), "waited long enough");
    }
}
