//! The timing rules, kept pure so they are provable on Linux. No CI runner
//! can record a screen, so anything that can be decided without one is
//! decided here: sample durations (including the nonzero guarantee the AAC
//! encoder depends on), timestamp monotonicity, the still-screen repeat
//! cadence that keeps the video track the same length as the capture, the
//! mixing round, and which delivered frames are usable.

use std::time::{Duration, Instant};

/// Media Foundation's time base is 100-nanosecond units, so this — not
/// one nanosecond — is the smallest duration that does not round to
/// ZERO hns on the way to `sink::to_hns`.
///
/// Every sample handed to the AAC Encoder MFT must carry a valid time
/// AND a nonzero duration: a zero duration makes `ProcessInput` succeed
/// and then `ProcessOutput` raise a DIVIDE-BY-ZERO. That is a documented
/// MF implementation bug, it surfaces as a crash with no usable stack,
/// and it is the single most likely way this module fails on a machine
/// no one here can debug. So the floor is applied inside the two
/// duration functions rather than trusted to their callers.
pub const MIN_SAMPLE: Duration = Duration::from_nanos(100);

/// Two samples must never carry the SAME timestamp either — the muxer
/// wants a strictly increasing stream — so a forced bump moves by one
/// representable unit, not by a whole frame, which would shove a
/// legitimately-close pair far out of place.
const MIN_STEP: Duration = MIN_SAMPLE;

/// The mux must not spin when the next frame slot is already overdue; it
/// blocks at least this long per wakeup.
const MIN_WAIT: Duration = Duration::from_millis(1);

/// How long a frame occupies at `fps`. Used as each sample's duration.
pub fn frame_duration(fps: u32) -> Duration {
    let ns = 1_000_000_000u64 / fps.max(1) as u64;
    Duration::from_nanos(ns).max(MIN_SAMPLE)
}

/// Should the mux repeat the previous frame into the next output slot?
///
/// `since_last_sample` is how far the CAPTURE CLOCK has run past the
/// timestamp of the last sample written.
///
/// WGC delivers a frame only when something CHANGES, so a still screen
/// produces no samples at all. Every sample nevertheless claims a
/// nominal `frame_duration(fps)`, and an MP4 track's timeline is the
/// SUM OF ITS SAMPLE DURATIONS — ISO/IEC 14496-12 defines a fragment's
/// `tfdt` baseMediaDecodeTime as the sum of the decode durations of all
/// earlier samples, and `trun` carries a duration per sample; nothing
/// in the container records the time an `IMFSample` was stamped with.
/// So filling the gaps only every so often made the TRACK shorter than
/// the capture: a still 40-second recording wrote ~2 samples a second
/// and played back in under three. Repeating once per elapsed frame
/// slot makes the stream CONSTANT-RATE BY CONSTRUCTION, which is what
/// makes the nominal duration the true one — and it keeps fragments
/// closing far more often than the old half-second cadence, so spec
/// 6.4's crash-resilient prefix gets finer, not coarser.
///
/// TWO slot lengths, not one: the slot immediately after the last
/// sample is still IN PROGRESS and a real frame may yet land in it.
/// Repeating into a live slot would put two samples where one belongs
/// and stretch the timeline instead of shortening it — a busy screen
/// delivering at exactly `fps` would play back at half speed.
pub fn should_repeat(since_last_sample: Duration, frame_dur: Duration) -> bool {
    since_last_sample >= frame_dur.saturating_mul(2)
}

/// Is there enough mixed audio buffered to emit a chunk?
///
/// The AAC encoder works in 1024-sample frames; feeding it a handful of
/// samples per callback multiplies per-sample overhead by orders of
/// magnitude. `min_frames` batches several AAC frames per write.
pub fn audio_chunk_ready(buffered_frames: usize, min_frames: usize) -> bool {
    buffered_frames >= min_frames
}

/// The duration to stamp on an audio sample carrying `frames` frames.
///
/// `None` when there is nothing to write — zero audio devices is legal
/// and must yield a working SILENT capture, not a stream of empty
/// samples whose duration is necessarily zero. See `MIN_SAMPLE` for why
/// a nonzero result is a hard requirement rather than tidiness.
pub fn audio_duration(frames: usize, rate: u32) -> Option<Duration> {
    if frames == 0 {
        return None;
    }
    let ns = frames as u64 * 1_000_000_000 / rate.max(1) as u64;
    Some(Duration::from_nanos(ns).max(MIN_SAMPLE))
}

/// The timestamp of the audio sample starting at `emitted_frames`.
///
/// Derived from the SAMPLE COUNT, never the wall clock: a wall-clock
/// stamp drifts against the sample rate and desyncs from the video
/// within minutes.
pub fn audio_ts(emitted_frames: u64, rate: u32) -> Duration {
    Duration::from_nanos(emitted_frames * 1_000_000_000 / rate.max(1) as u64)
}

/// Force a timestamp strictly past the one before it. Equal or
/// going-backwards timestamps make a non-monotonic stream, which the
/// muxer rejects outright.
pub fn monotonic_ts(candidate: Duration, last: Option<Duration>, min_step: Duration) -> Duration {
    match last {
        Some(prev) if candidate <= prev => prev + min_step,
        _ => candidate,
    }
}

/// Frames to take from every source this round, or 0 to wait.
///
/// Normally the MINIMUM across the buffers, so the mix stays
/// frame-aligned. But a source that stops delivering WITHOUT reporting
/// `Lost` would pin that minimum at zero forever: all audio stops and
/// every live source's buffer grows without bound. So once any buffer
/// passes `stall_cap` the round proceeds on the LONGEST buffer instead
/// and the short ones mix as silence — `mix_n_to_stereo_i16` already
/// pads a short source with zeroes, so this needs no special casing
/// downstream.
pub fn take_frames(lens: &[usize], min_chunk: usize, stall_cap: usize) -> usize {
    let Some(&min) = lens.iter().min() else {
        return 0; // zero audio sources: a silent capture, forever
    };
    let max = lens.iter().copied().max().unwrap_or(0);
    let take = if max >= stall_cap { max } else { min };
    // Through `audio_chunk_ready`, not a second inline comparison: the
    // batching threshold has exactly one definition.
    if audio_chunk_ready(take, min_chunk) {
        take
    } else {
        0
    }
}

/// How many frames to flush from the buffered audio when a pause begins,
/// so the buffers can be safely cleared afterwards without discarding
/// anything already captured.
///
/// Unlike the ordinary batching round (`take_frames`), this is not gated
/// on `min_chunk` — a pause can land mid-chunk, and that partial audio is
/// still real audio. It takes the LONGEST per-source buffer, the same
/// stall-cap posture `take_frames` falls back to once a source stalls:
/// `mix_n_to_stereo_i16` already pads a shorter source's slice with
/// silence, so nothing needs to wait for every source to agree before the
/// already-captured audio can be written out.
///
/// REGRESSION this exists to prevent: a pause used to clear every buffer
/// outright without emitting them first, silently discarding up to one
/// chunk's worth (~85 ms at the default `AUDIO_CHUNK_FRAMES`) of audio
/// WITHOUT advancing the pacer's `emitted` count — so audio drifted ~85 ms
/// earlier relative to video, and the drift accumulated across every pause.
pub fn pause_flush_frames(lens: &[usize]) -> usize {
    lens.iter().copied().max().unwrap_or(0)
}

/// Advisory frame rate for the `screen:frames` stat (spec 11), lossy by
/// design. Zero rather than an infinity when no time has passed.
pub fn observed_fps(frames: u64, elapsed: Duration) -> f32 {
    let secs = elapsed.as_secs_f32();
    if secs <= 0.0 {
        return 0.0;
    }
    frames as f32 / secs
}

/// The capture's reported duration: the end of the last sample the mux
/// actually WROTE, never the wall clock at `stop()` time.
///
/// The mux can end well before `stop()` runs — a source closing
/// (`frames.rs`'s `on_closed`), or a write failure that still finalizes
/// what came before it — so reporting elapsed-at-stop would claim footage
/// the file does not contain (the same "claims footage the file does not
/// contain" failure `stopping_while_paused_keeps_the_paused_time_out_of_
/// the_duration` guards for the pause case). `written_until ==
/// Duration::ZERO` is the unambiguous "nothing was ever written" case —
/// every real sample carries a nonzero duration, see `MIN_SAMPLE` — so
/// only then does the clock-elapsed fallback apply.
pub fn resolved_duration(written_until: Duration, clock_elapsed: Duration) -> Duration {
    if written_until.is_zero() {
        clock_elapsed
    } else {
        written_until
    }
}

/// Is a delivered frame usable against the size the sink was opened
/// with?
///
/// The output format is fixed at start, but a WINDOW can be resized
/// mid-capture and WGC then delivers a different size. A frame at or
/// above the declared size crops for free — `bgra_to_nv12` reads
/// `width * 4` bytes out of each of the first `height` rows of a
/// stride-pitched buffer. A SMALLER frame has to be dropped and
/// counted: reading the declared height out of it runs past the end of
/// the mapped staging texture, and row padding can make a
/// buffer-length check pass while the dimensions do not.
pub fn usable_frame(got_w: u32, got_h: u32, want_w: u32, want_h: u32) -> bool {
    got_w >= want_w && got_h >= want_h
}

/// Log the first dropped frame of a run, then one in every 300.
///
/// A persistently failing conversion at 60 fps would otherwise write a
/// line every 16 ms and bury every other diagnostic in the log file —
/// but silence is not the alternative: the drop counter is advisory and
/// the log is where a user's bug report gets its evidence.
pub fn should_log_drop(dropped_so_far: u64) -> bool {
    dropped_so_far % 300 == 1
}

/// The video timeline: what the mux stamps, and when it repeats.
///
/// CONSTANT-RATE BY CONSTRUCTION. One sample per frame slot, and every
/// sample claims exactly `frame_duration(fps)` — which is the only way
/// the nominal duration can be the true one, since an MP4 track's length
/// is the sum of its sample durations and nothing else (see
/// `should_repeat`). Real frames fill the slots they land in; the rest
/// are filled with a repeat of the last one.
///
/// `Instant`-injected like `CaptureClock`, so every repeat and pause edge
/// is unit-testable on Linux — the whole point of keeping this out of the
/// Windows arm.
pub struct VideoPacer {
    frame_dur: Duration,
    last_ts: Option<Duration>,
    last_emit: Option<Instant>,
    /// Set by `on_frame`, consumed by the next `on_idle`. A repeat must
    /// never share a mux wakeup with a real frame: when the frame channel
    /// is backed up (`CHANNEL_DEPTH` deep at 4K60), the frame just written
    /// can trail the clock by the whole backlog, and filling that gap with
    /// repeats would double-count slots the queued frames are about to
    /// fill. Deferring costs nothing — the repeat decision is anchored to
    /// the clock, not to when it was last asked — so a skipped check gives
    /// a slot back on the next wakeup instead of losing it.
    frame_since_idle: bool,
}

impl VideoPacer {
    pub fn new(fps: u32) -> VideoPacer {
        VideoPacer {
            frame_dur: frame_duration(fps),
            last_ts: None,
            last_emit: None,
            frame_since_idle: false,
        }
    }

    /// A real frame arrived. Returns the `(timestamp, duration)` to
    /// write.
    pub fn on_frame(&mut self, ts: Duration, now: Instant) -> (Duration, Duration) {
        let ts = monotonic_ts(ts, self.last_ts, MIN_STEP);
        self.last_ts = Some(ts);
        self.last_emit = Some(now);
        self.frame_since_idle = true;
        (ts, self.frame_dur)
    }

    /// The mux woke and the slot after the last sample may have gone by
    /// unfilled. Returns the repeat to write, or `None`.
    ///
    /// Call it in a LOOP: each call fills at most one slot, so a mux that
    /// blocked through several of them catches back up rather than
    /// staying permanently short. It terminates — every repeat advances
    /// the timeline by a whole frame towards the `clock_ts` it was handed.
    ///
    /// `clock_ts` is the shared `CaptureClock`'s `output_ts`, so `None`
    /// means PAUSED and there is nothing to repeat. Repeating while
    /// paused would advance the output timeline by paused wall-clock
    /// time — precisely what the clock exists to keep out of it — and
    /// would then make the first frame after resume stamp EARLIER than
    /// the repeats. Driving the repeats off that same clock is also what
    /// makes a long pause cost nothing on resume: paused time is not in
    /// it, so there is no gap to catch up on.
    pub fn on_idle(
        &mut self,
        clock_ts: Option<Duration>,
        now: Instant,
    ) -> Option<(Duration, Duration)> {
        if std::mem::take(&mut self.frame_since_idle) {
            return None;
        }
        let clock_ts = clock_ts?;
        // Nothing to repeat before the first frame.
        let last_ts = self.last_ts?;
        if !should_repeat(clock_ts.saturating_sub(last_ts), self.frame_dur) {
            return None;
        }
        // The NEXT slot, never the clock's current reading: a repeat that
        // jumped to the clock would advance the timeline by one frame per
        // two elapsed, and the track would come out half as long as the
        // capture. Being the slot after `last_ts`, it is strictly
        // increasing without needing `monotonic_ts` to force it.
        let ts = last_ts + self.frame_dur;
        self.last_ts = Some(ts);
        self.last_emit = Some(now);
        Some((ts, self.frame_dur))
    }

    /// How long to block waiting for the next message.
    ///
    /// It SHRINKS as the next slot approaches, and never blocks longer
    /// than one frame period: blocking longer means the timeout arm never
    /// fires while audio chunks keep arriving (every ~85 ms), so a still
    /// screen recorded with a microphone would repeat only as often as
    /// the mux happened to wake — the slots in between would be lost and
    /// the track would run short of the capture again.
    pub fn wait(&self, now: Instant) -> Duration {
        let Some(last) = self.last_emit else {
            return self.frame_dur.max(MIN_WAIT);
        };
        let since = now.saturating_duration_since(last);
        self.frame_dur.saturating_sub(since).max(MIN_WAIT)
    }
}

/// The audio timeline: contiguous, sample-count-derived, never zero.
pub struct AudioPacer {
    rate: u32,
    emitted: u64,
}

impl AudioPacer {
    pub fn new(rate: u32) -> AudioPacer {
        AudioPacer { rate, emitted: 0 }
    }

    /// Claim `frames` frames. Returns the `(timestamp, duration)` for
    /// the sample, or `None` when there is nothing to write.
    pub fn take(&mut self, frames: usize) -> Option<(Duration, Duration)> {
        let dur = audio_duration(frames, self.rate)?;
        let ts = audio_ts(self.emitted, self.rate);
        self.emitted += frames as u64;
        Some((ts, dur))
    }
}

#[cfg(test)]
#[cfg(test)]
mod tests {
    //! The VIDEO timeline's own tests: the frame-slot cadence that keeps
    //! the track the same length as the capture, and the pacer that runs
    //! it. They sit beside the rules rather than in `session/mod.rs`,
    //! where they grew up — which is also what keeps that file under the
    //! LOC cap. The audio round, the stats and the frame-usability rules
    //! keep their tests there.
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_still_screen_repeats_its_last_frame_once_a_whole_slot_has_gone_by() {
        // TWO frame periods, not one: the slot right after the last sample
        // is still in progress and a real frame may yet land in it. The
        // threshold is on the CAPTURE CLOCK's distance from that sample,
        // which is what keeps one sample per slot and no more.
        let fd = frame_duration(30); // 33_333_333 ns, hand-derived: 1e9/30
        assert!(!should_repeat(Duration::from_nanos(66_666_665), fd));
        assert!(should_repeat(Duration::from_nanos(66_666_666), fd));
        assert!(should_repeat(Duration::from_secs(30), fd));
        // A busy screen delivering at exactly fps never repeats at all.
        assert!(!should_repeat(fd, fd));
    }

    // --- the video pacer, which owns the repeat cadence ------------------

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn a_pacer_with_no_frame_yet_has_nothing_to_repeat() {
        // Repeating before the first frame would write an empty sample.
        let mut p = VideoPacer::new(30);
        let base = Instant::now();
        assert!(p
            .on_idle(Some(Duration::from_secs(9)), at(base, 9_000))
            .is_none());
    }

    #[test]
    fn a_still_screen_repeats_the_last_frame_into_the_next_slot() {
        let fd = frame_duration(30); // 33_333_333 ns, hand-derived: 1e9/30
        let mut p = VideoPacer::new(30);
        let base = Instant::now();
        p.on_frame(Duration::from_millis(100), at(base, 100));
        // The check right after a real frame is always declined, so the
        // repeat can never be written over frames still queued behind it.
        assert!(p
            .on_idle(Some(Duration::from_millis(100)), at(base, 100))
            .is_none());
        // 100 ms + one slot: that slot is still in progress, nothing to fill.
        assert!(p
            .on_idle(Some(Duration::from_millis(133)), at(base, 133))
            .is_none());
        let (ts, dur) = p
            .on_idle(Some(Duration::from_millis(167)), at(base, 167))
            .expect("a whole slot has elapsed unfilled");
        assert_eq!(
            ts,
            Duration::from_millis(100) + fd,
            "the repeat fills the NEXT slot, never the clock's own reading — \
             jumping to the clock advances one frame per two elapsed and \
             halves the track"
        );
        assert_eq!(dur, fd);
    }

    // THE regression this module exists to prevent (a user-confirmed
    // production bug): a 40-second capture played back in 3-4 seconds while
    // its own metadata claimed 40. An MP4 track's timeline is the SUM OF ITS
    // SAMPLE DURATIONS (ISO/IEC 14496-12: a fragment's `tfdt`
    // baseMediaDecodeTime is defined as the sum of the decode durations of
    // all earlier samples, and within a fragment `trun` carries a duration
    // per sample) - never the value handed to `IMFSample::SetSampleTime`.
    // WGC delivers a frame only when the screen CHANGES, so a still screen
    // that repeated only every 500 ms wrote ~2 samples a second, each
    // claiming a nominal 1/30 s: 40 real seconds came to ~2.7 s of track.
    //
    // Every number below is hand-derived from fps and elapsed time, never
    // from running the implementation:
    //   frame_duration(30) = 1e9/30 truncated  = 33_333_333 ns
    //   a repeat is due once the slot AFTER the last sample has elapsed in
    //   full, so repeat j is emitted while (j-1)*fd <= 40s - 2*fd, i.e.
    //   j <= 1199, and the last sample's ts is 1199*fd = 39_966_666_267 ns
    //   samples = 1 real frame + 1199 repeats = 1200
    //   summed duration = 1200 * 33_333_333 = 39_999_999_600 ns
    // The 400 ns shortfall is frame_duration's own integer truncation
    // (1/3 ns per frame x 1200), not a timeline error.
    #[test]
    fn forty_seconds_of_a_still_screen_write_forty_seconds_of_sample_duration() {
        let fps = 30;
        let fd = frame_duration(fps);
        let elapsed = Duration::from_secs(40);
        let mut p = VideoPacer::new(fps);
        let base = Instant::now();

        let mut total = Duration::ZERO;
        let mut samples = 0u32;
        // `written_until` exactly as mux.rs keeps it: the end of the last
        // sample handed to the sink, which is what `resolved_duration`
        // reports to the user as the capture's length.
        let mut written_until = Duration::ZERO;
        let (ts, dur) = p.on_frame(Duration::ZERO, base);
        total += dur;
        samples += 1;
        written_until = written_until.max(ts + dur);

        // 10 ms polls: FINER than the mux's own wakeup cadence (`wait()` is
        // at most one frame period), so this models a mux that never misses
        // a slot. The catch-up loop is what the mux runs.
        let mut ms = 10;
        while ms <= 40_000 {
            while let Some((ts, dur)) = p.on_idle(Some(Duration::from_millis(ms)), at(base, ms)) {
                total += dur;
                samples += 1;
                written_until = written_until.max(ts + dur);
            }
            ms += 10;
        }

        // THE agreement that was broken: `written_until` rode the capture
        // clock while the track rode the sample durations, so the sidecar
        // said 40 s about a file that held 3. On a constant-rate timeline
        // they are the same number by construction.
        assert_eq!(
            written_until, total,
            "the duration reported to the user must be the duration the \
             track actually holds"
        );

        assert_eq!(samples, 1200, "one sample per elapsed frame slot");
        assert_eq!(total, Duration::from_nanos(39_999_999_600));
        // The property, independent of the poll cadence above: the track
        // never claims more than the wall clock and is never short by more
        // than the one slot still in progress.
        assert!(total <= elapsed, "{total:?} overruns {elapsed:?}");
        assert!(
            total > elapsed - fd,
            "{total:?} is more than one frame short of {elapsed:?}"
        );
    }

    // The other direction, and the one a careless fix breaks: a BUSY screen
    // delivering a real frame every slot must produce NO repeats at all. A
    // repeat threshold of one slot instead of two would fire in the moment
    // between a slot boundary and the frame for that slot arriving, writing
    // two samples where one belongs — 40 real seconds would then play back
    // over 80, which is no better than playing back over 4.
    #[test]
    fn a_busy_screen_delivering_every_slot_is_never_padded_with_repeats() {
        let fps = 30;
        let fd = frame_duration(fps);
        let mut p = VideoPacer::new(fps);
        let base = Instant::now();

        let mut total = Duration::ZERO;
        let mut samples = 0u32;
        // 300 frames delivered one slot apart = 10 s at 30 fps. Between
        // each pair the mux wakes SEVERAL times — audio chunks land every
        // ~85 ms and `wait()` shrinks — and the last of those wakeups falls
        // exactly on the slot boundary, a hair before the frame for that
        // slot arrives. That is the race a one-slot threshold loses.
        for i in 0..300u32 {
            let ts = fd * i;
            let now = base + ts;
            let (_, dur) = p.on_frame(ts, now);
            total += dur;
            samples += 1;
            for third in 1..=3u32 {
                let clock = ts + fd * third / 3;
                while let Some((_, dur)) = p.on_idle(Some(clock), base + clock) {
                    total += dur;
                    samples += 1;
                }
            }
        }
        assert_eq!(samples, 300, "one sample per delivered frame, no padding");
        assert_eq!(total, fd * 300);
    }

    // REGRESSION: the frame channel is `CHANNEL_DEPTH` deep, so at 4K60 the
    // mux can be draining a backlog whose OLDEST frame trails the clock by
    // the whole queue. Catching that gap up with repeats would write a
    // second sample for every slot the queued frames are about to fill.
    // A repeat therefore never shares a wakeup with a real frame.
    #[test]
    fn a_backed_up_frame_channel_is_not_caught_up_over_the_frames_still_queued() {
        let fps = 30;
        let fd = frame_duration(fps);
        let mut p = VideoPacer::new(fps);
        let base = Instant::now();
        // The clock is 16 slots ahead of the frame being written: the
        // queue holds 16 frames and this is the oldest.
        let clock = fd * 16;
        let now = base + clock;
        p.on_frame(Duration::ZERO, now);
        assert!(
            p.on_idle(Some(clock), now).is_none(),
            "the 15 slots between this frame and the clock belong to the \
             frames still in the queue, not to repeats"
        );
        // Once the queue really is empty — the next wakeup with no frame —
        // the catch-up runs as normal.
        assert!(p.on_idle(Some(clock), now).is_some());
    }

    // REGRESSION: a repeat cadence driven by elapsed WALL CLOCK would owe
    // the timeline one frame for every paused slot and would pay them all
    // back in a burst the instant the capture resumed — minutes of repeats,
    // written over audio that never stopped being sample-count-accurate.
    // Driving it off the capture clock, which excludes paused time, means
    // there is nothing to catch up on.
    #[test]
    fn a_long_pause_costs_nothing_to_catch_up_on_resume() {
        let fps = 30;
        let fd = frame_duration(fps);
        let mut p = VideoPacer::new(fps);
        let base = Instant::now();
        p.on_frame(Duration::ZERO, base);
        // Ten minutes of wall clock, all of it paused: the clock reads None
        // throughout and never advances.
        for ms in (1_000..=600_000).step_by(1_000) {
            assert!(p.on_idle(None, at(base, ms)).is_none());
        }
        // Resumed. The capture clock has not moved for ten minutes, so it
        // reads two slots past the one frame written: one of those slots
        // has elapsed in full and one is still in progress, so exactly ONE
        // repeat is owed — not six hundred seconds of them.
        let mut repeats = 0u32;
        while p.on_idle(Some(fd * 2), at(base, 600_001)).is_some() {
            repeats += 1;
            assert!(repeats < 10, "a pause must not owe the timeline a burst");
        }
        assert_eq!(repeats, 1, "only the slots the CAPTURE clock passed");
    }

    // A mux that blocked through several slots (a fragment write, a slow
    // disk) must be able to catch back up, or the track stays permanently
    // short of the capture by however long the stall lasted.
    #[test]
    fn a_mux_that_blocked_through_several_slots_catches_back_up() {
        let fps = 30;
        let fd = frame_duration(fps);
        let mut p = VideoPacer::new(fps);
        let base = Instant::now();
        p.on_frame(Duration::ZERO, base);
        p.on_idle(Some(Duration::ZERO), base); // consume the post-frame skip

        // 300 ms of stall = 9 whole slots elapsed (300ms / 33.333ms = 9.0),
        // of which the ninth is still in progress, so 8 are owed.
        let clock = Duration::from_millis(300);
        let now = at(base, 300);
        let mut last = Duration::ZERO;
        let mut repeats = 0u32;
        while let Some((ts, dur)) = p.on_idle(Some(clock), now) {
            assert!(ts > last, "{ts:?} must follow {last:?}");
            assert!(dur >= MIN_SAMPLE, "a zero duration divides by zero in MF");
            last = ts;
            repeats += 1;
        }
        assert_eq!(repeats, 8);
        assert_eq!(last, fd * 8);
    }

    #[test]
    fn a_paused_capture_never_repeats_a_frame() {
        // REGRESSION: repeating while paused advances the output timeline by
        // PAUSED WALL-CLOCK TIME — exactly what CaptureClock exists to keep
        // out of the output — and then makes the first frame after resume
        // stamp EARLIER than the repeats, i.e. a non-monotonic stream.
        let mut p = VideoPacer::new(30);
        let base = Instant::now();
        p.on_frame(Duration::from_millis(100), at(base, 100));
        // Paused: the clock reports None for as long as the pause lasts.
        assert!(p.on_idle(None, at(base, 5_000)).is_none());
        assert!(p.on_idle(None, at(base, 60_000)).is_none());
        // Resumed. The clock's output time has NOT advanced by the pause.
        let (ts, _) = p.on_frame(Duration::from_millis(150), at(base, 60_100));
        assert_eq!(ts, Duration::from_millis(150));
    }

    #[test]
    fn the_wait_never_blocks_past_a_frame_slot_and_shrinks_as_one_approaches() {
        // REGRESSION: blocking longer than a slot means the mux wakes less
        // often than slots elapse, so the repeats that keep the track the
        // same length as the capture are written late or not at all — and
        // the timeout arm never runs while audio chunks arrive every
        // ~85 ms, so a still screen recorded WITH a microphone was the
        // common case for both failures.
        let fd = frame_duration(30); // 33_333_333 ns, hand-derived: 1e9/30
        let mut p = VideoPacer::new(30);
        let base = Instant::now();
        assert_eq!(p.wait(base), fd, "with nothing written yet, one slot");
        p.on_frame(Duration::ZERO, base);
        assert_eq!(
            p.wait(at(base, 10)),
            fd - Duration::from_millis(10),
            "10 ms into the slot, the rest of it is left"
        );
        assert!(p.wait(at(base, 100)) <= fd);
        // Never zero: a zero wait busy-spins the mux thread.
        assert!(p.wait(at(base, 5_000)) > Duration::ZERO);
    }

    #[test]
    fn a_repeat_is_never_written_for_a_clock_that_has_not_moved() {
        // The repeat is a SLOT, not a wall-clock reading: a clock that has
        // not moved has no elapsed slot to fill, however long the mux has
        // been awake. (The old heartbeat fired on Instant-elapsed alone and
        // needed `monotonic_ts` to shove the stamp past its predecessor;
        // now the stamp is the next slot and is strictly increasing by
        // construction.)
        let mut p = VideoPacer::new(30);
        let base = Instant::now();
        p.on_frame(Duration::from_millis(100), at(base, 100));
        assert!(p
            .on_idle(Some(Duration::from_millis(100)), at(base, 700))
            .is_none());
        assert!(p
            .on_idle(Some(Duration::from_millis(100)), at(base, 9_000))
            .is_none());
    }
}
