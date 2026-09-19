//! The timing rules, kept pure so they are provable on Linux. No CI runner
//! can record a screen, so anything that can be decided without one is
//! decided here: sample durations (including the nonzero guarantee the AAC
//! encoder depends on), timestamp monotonicity, the still-screen heartbeat,
//! the mixing round, and which delivered frames are usable.

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

/// The mux must not spin when the heartbeat is already overdue; it
/// blocks at least this long per wakeup.
const MIN_WAIT: Duration = Duration::from_millis(1);

/// How long a frame occupies at `fps`. Used as each sample's duration.
pub fn frame_duration(fps: u32) -> Duration {
    let ns = 1_000_000_000u64 / fps.max(1) as u64;
    Duration::from_nanos(ns).max(MIN_SAMPLE)
}

/// Should the mux repeat the previous frame?
///
/// WGC delivers a frame only when something CHANGES. A screen left
/// untouched produces no samples, so no fragment closes, so a crash
/// loses the whole idle stretch — which defeats the reason spec 6.4
/// chose fragmented MP4. Repeating the last frame keeps fragments
/// closing and keeps the file playable at a steady rate.
pub fn should_repeat(since_last_frame: Duration, heartbeat: Duration) -> bool {
    since_last_frame >= heartbeat
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
/// `Instant`-injected like `CaptureClock`, so every heartbeat and pause
/// edge is unit-testable on Linux — the whole point of keeping this out
/// of the Windows arm.
pub struct VideoPacer {
    frame_dur: Duration,
    heartbeat: Duration,
    last_ts: Option<Duration>,
    last_emit: Option<Instant>,
}

impl VideoPacer {
    pub fn new(fps: u32, heartbeat: Duration) -> VideoPacer {
        VideoPacer {
            frame_dur: frame_duration(fps),
            heartbeat,
            last_ts: None,
            last_emit: None,
        }
    }

    /// A real frame arrived. Returns the `(timestamp, duration)` to
    /// write.
    pub fn on_frame(&mut self, ts: Duration, now: Instant) -> (Duration, Duration) {
        let ts = monotonic_ts(ts, self.last_ts, MIN_STEP);
        self.last_ts = Some(ts);
        self.last_emit = Some(now);
        (ts, self.frame_dur)
    }

    /// The mux woke with no new frame. Returns the repeat to write, or
    /// `None`.
    ///
    /// `clock_ts` is the shared `CaptureClock`'s `output_ts`, so `None`
    /// means PAUSED and there is nothing to repeat. Repeating while
    /// paused would advance the output timeline by paused wall-clock
    /// time — precisely what the clock exists to keep out of it — and
    /// would then make the first frame after resume stamp EARLIER than
    /// the repeats.
    pub fn on_idle(
        &mut self,
        clock_ts: Option<Duration>,
        now: Instant,
    ) -> Option<(Duration, Duration)> {
        let clock_ts = clock_ts?;
        // Nothing to repeat before the first frame.
        self.last_ts?;
        let since = now.saturating_duration_since(self.last_emit?);
        if !should_repeat(since, self.heartbeat) {
            return None;
        }
        let ts = monotonic_ts(clock_ts, self.last_ts, MIN_STEP);
        self.last_ts = Some(ts);
        self.last_emit = Some(now);
        Some((ts, self.frame_dur))
    }

    /// How long to block waiting for the next message.
    ///
    /// It SHRINKS as the heartbeat approaches. Blocking a full heartbeat
    /// every wakeup means the timeout never fires while audio chunks
    /// keep arriving (every ~85 ms), so a still screen recorded with a
    /// microphone would never repeat a frame and would close no
    /// fragments at all.
    pub fn wait(&self, now: Instant) -> Duration {
        let Some(last) = self.last_emit else {
            return self.heartbeat;
        };
        let since = now.saturating_duration_since(last);
        self.heartbeat.saturating_sub(since).max(MIN_WAIT)
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
