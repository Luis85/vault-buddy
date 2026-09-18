//! The single source of output timestamps for BOTH the video and audio
//! streams of a screen capture.
//!
//! Sharing one clock is what makes A/V sync across a pause structural rather
//! than incidental: there is no second time base to drift from. Pure and
//! `Instant`-injected, so every pause edge is unit-testable (spec §6.2).

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CaptureClock {
    started: Instant,
    paused_total: Duration,
    paused_since: Option<Instant>,
}

impl CaptureClock {
    pub fn new(started: Instant) -> CaptureClock {
        CaptureClock {
            started,
            paused_total: Duration::ZERO,
            paused_since: None,
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_since.is_some()
    }

    /// Idempotent: a second pause while already paused must not reset the
    /// pause start, or the first pause's elapsed time is lost.
    pub fn pause(&mut self, now: Instant) {
        if self.paused_since.is_none() {
            self.paused_since = Some(now);
        }
    }

    /// Idempotent: a resume with no matching pause is ignored.
    pub fn resume(&mut self, now: Instant) {
        if let Some(since) = self.paused_since.take() {
            self.paused_total += now.saturating_duration_since(since);
        }
    }

    /// Wall-clock time since start, minus all paused time.
    pub fn elapsed(&self, now: Instant) -> Duration {
        let raw = now.saturating_duration_since(self.started);
        let paused = match self.paused_since {
            Some(since) => self.paused_total + now.saturating_duration_since(since),
            None => self.paused_total,
        };
        raw.saturating_sub(paused)
    }

    /// The timestamp to stamp on a frame or audio buffer arriving `now`.
    /// `None` while paused — arriving data is drained and discarded, so
    /// paused wall-clock time never reaches the output timeline.
    pub fn output_ts(&self, now: Instant) -> Option<Duration> {
        if self.is_paused() {
            return None;
        }
        Some(self.elapsed(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn output_time_tracks_wall_clock_while_running() {
        let base = Instant::now();
        let c = CaptureClock::new(base);
        assert_eq!(c.output_ts(at(base, 0)), Some(Duration::from_millis(0)));
        assert_eq!(
            c.output_ts(at(base, 1_500)),
            Some(Duration::from_millis(1_500))
        );
        assert!(!c.is_paused());
    }

    #[test]
    fn output_time_is_none_while_paused() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.pause(at(base, 1_000));
        assert!(c.is_paused());
        assert_eq!(
            c.output_ts(at(base, 2_000)),
            None,
            "nothing is encoded while paused"
        );
    }

    // The invariant the whole pause design rests on: paused wall-clock time
    // never appears in the output timeline. Without this, audio and video
    // drift apart by exactly the pause duration.
    #[test]
    fn paused_time_is_excluded_from_the_output_timeline() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.pause(at(base, 1_000));
        c.resume(at(base, 6_000)); // paused for 5s
        assert_eq!(
            c.output_ts(at(base, 7_000)),
            Some(Duration::from_millis(2_000))
        );
        assert_eq!(c.elapsed(at(base, 7_000)), Duration::from_millis(2_000));
    }

    #[test]
    fn multiple_pause_cycles_accumulate() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.pause(at(base, 1_000));
        c.resume(at(base, 2_000)); // +1s paused
        c.pause(at(base, 3_000));
        c.resume(at(base, 5_000)); // +2s paused, 3s total
        assert_eq!(
            c.output_ts(at(base, 6_000)),
            Some(Duration::from_millis(3_000))
        );
    }

    #[test]
    fn a_second_pause_while_already_paused_is_ignored() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.pause(at(base, 1_000));
        c.pause(at(base, 3_000)); // must not reset the pause start
        c.resume(at(base, 5_000));
        assert_eq!(
            c.output_ts(at(base, 6_000)),
            Some(Duration::from_millis(2_000)),
            "4s paused, so 6s wall = 2s output"
        );
    }

    #[test]
    fn a_resume_without_a_pause_is_ignored() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.resume(at(base, 1_000));
        assert_eq!(
            c.output_ts(at(base, 2_000)),
            Some(Duration::from_millis(2_000))
        );
    }

    // Timestamps handed to the encoder must never go backwards; a
    // non-monotonic stream is rejected outright by the muxer.
    #[test]
    fn output_time_never_goes_backwards_across_a_pause() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        let before = c.output_ts(at(base, 1_000)).unwrap();
        c.pause(at(base, 1_000));
        c.resume(at(base, 9_000));
        let after = c.output_ts(at(base, 9_001)).unwrap();
        assert!(after >= before, "{after:?} must not precede {before:?}");
    }

    #[test]
    fn a_clock_reading_before_the_start_is_zero_not_a_panic() {
        let base = Instant::now() + Duration::from_secs(10);
        let c = CaptureClock::new(base);
        assert_eq!(c.output_ts(Instant::now()), Some(Duration::ZERO));
    }
}
