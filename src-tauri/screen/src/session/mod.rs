//! The capture session: frames and audio, one clock, one sink.
//!
//! THREE threads, not the two spec 6.1 lists, for a concrete reason.
//! `IMFSample` is a COM interface pointer and is not `Send`, so a sample
//! built on the frame thread cannot be handed anywhere else, and writing one
//! `IMFSinkWriter` from two threads raises apartment questions with a
//! ten-minute feedback loop attached. So the channels carry PLAIN BYTE
//! VECTORS plus a timestamp, and `screen-mux` is the only thread that ever
//! touches the sink. The cost is one memcpy of data already in RAM — the
//! frame was copied out of the GPU staging texture regardless.
//!
//! ONE CLOCK (spec 6.2). Both producers stamp from the same
//! `CaptureClock`, so A/V sync across a pause is structural rather than
//! incidental: there is no second time base to drift from. While paused,
//! `output_ts` returns None and producers DRAIN AND DISCARD (spec 6.3) —
//! the streams stay open, because tearing down and recreating a WGC session
//! on every pause drops frames on resume and can fail outright if the
//! target window changed state.
//!
//! WHAT IS PURE AND WHAT IS NOT. No CI runner can record a screen, so every
//! decision that can be made without one is made in `pacing` or in
//! `apply_control`/`Warnings` below, all of which compile and run on Linux.
//! The `#[cfg(windows)]` arm is deliberately thin: it moves bytes between
//! the pieces and calls those functions. Anything testable that drifts into
//! it is stranded where nothing can reach it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use vault_buddy_capture::session::SourceInput;
use vault_buddy_core::screen_capture_config::ScreenQuality;

use crate::clock::CaptureClock;
use crate::source::ResolvedSource;

#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod mux;
pub mod pacing;
#[cfg(windows)]
mod windows_session;

/// Session control. ONE channel of meaning carries all three, following the
/// audio domain's documented reason: a single interpretation point means no
/// second signalling path can race the stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Stop,
    Pause,
    Resume,
}

/// Apply a control transition. THE one place a transition is interpreted.
///
/// Stop does NOT resume the clock: a capture stopped while paused must keep
/// the paused stretch out of its reported duration, or it claims footage the
/// file does not contain.
pub fn apply_control(
    clock: &mut CaptureClock,
    stopping: &AtomicBool,
    control: Control,
    now: Instant,
) {
    match control {
        Control::Pause => clock.pause(now),
        Control::Resume => clock.resume(now),
        Control::Stop => stopping.store(true, Ordering::Relaxed),
    }
}

/// Live warnings for the shell, and the one kept for the outcome.
///
/// Spec 14: a source vanishing mid-capture is a WARNING that finalizes
/// cleanly, never a failure. The FIRST warning is the one reported, because
/// it is the one that explains what went wrong; a gone receiver is ignored,
/// because an advisory channel must never slow or fail the capture path.
pub struct Warnings {
    inner: Mutex<WarnState>,
}

struct WarnState {
    tx: Option<Sender<String>>,
    first: Option<String>,
}

impl Warnings {
    pub fn new(tx: Option<Sender<String>>) -> Warnings {
        Warnings {
            inner: Mutex::new(WarnState { tx, first: None }),
        }
    }

    pub fn raise(&self, message: String) {
        log::warn!("screen capture: {message}");
        // Poison-tolerant: a panic elsewhere must not turn an advisory
        // warning into a second failure.
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = &state.tx {
            let _ = tx.send(message.clone());
        }
        if state.first.is_none() {
            state.first = Some(message);
        }
    }

    pub fn take(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .first
            .clone()
    }
}

/// Advisory, ~2 Hz, lossy by design (spec 11). A gone receiver must never
/// slow or fail the capture path.
#[derive(Debug, Clone, Copy)]
pub struct FrameStats {
    pub fps: f32,
    pub dropped: u64,
}

pub struct ScreenSessionParams {
    pub source: ResolvedSource,
    /// The hidden in-progress file (`.<base>.mp4.part`).
    pub part: PathBuf,
    /// Where `part` is renamed on a clean finalize (`<base>.mp4`).
    pub staged: PathBuf,
    pub fps: u32,
    pub quality: ScreenQuality,
    /// Already-opened audio sources. The cpal `Stream`s stay with the
    /// caller — they are `!Send` and must outlive the session.
    pub audio: Vec<SourceInput>,
    pub warn_tx: Option<Sender<String>>,
    pub stats_tx: Option<Sender<FrameStats>>,
}

pub struct ScreenOutcome {
    pub mp4: PathBuf,
    pub duration_ms: u64,
    pub paused_ms: u64,
    pub width: u32,
    pub height: u32,
    pub dropped: u64,
    pub warning: Option<String>,
}

/// Repeat the last frame after this long with nothing new, so fragments
/// keep closing on a still screen (see `pacing::VideoPacer::on_idle`).
pub const HEARTBEAT: Duration = Duration::from_millis(500);
/// Everything is resampled to this: the rate every Windows AAC encoder MFT
/// is required to accept. The MP3 path's 44 100 is untouched.
pub const AUDIO_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u16 = 2;
pub const AUDIO_BITRATE_BPS: u32 = 128_000;
/// Batch this many stereo frames per AAC write. The encoder works in
/// 1024-sample frames; writing a handful per cpal callback multiplies
/// per-sample overhead by orders of magnitude.
pub const AUDIO_CHUNK_FRAMES: usize = 4096;
/// One second of audio per source. Past this a source counts as stalled and
/// the mixing round stops waiting for it — see `pacing::take_frames`.
pub const AUDIO_STALL_CAP: usize = AUDIO_RATE as usize;

/// The pause-aware time base BOTH producers stamp from (spec 6.2). One
/// clock is what makes A/V sync across a pause structural: there is no
/// second time base to drift from.
pub type SharedClock = std::sync::Arc<Mutex<CaptureClock>>;

/// Read the shared clock. `None` means PAUSED, and a producer that reads
/// `None` drains and discards (spec 6.3).
///
/// Poison-tolerant: a panic on any producer thread must not freeze the
/// capture by poisoning the clock every other thread reads.
pub fn output_ts(clock: &SharedClock) -> Option<Duration> {
    clock
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .output_ts(Instant::now())
}

/// What crosses to the mux thread. Plain data, never an `IMFSample`: COM
/// interface pointers are not `Send`.
///
/// Audio carries its own timestamp and duration because the `AudioPacer`
/// that derives them from the emitted sample count belongs with the thread
/// that does the mixing; video is stamped in the mux, where the
/// `VideoPacer` also owns the still-screen heartbeat.
#[cfg(windows)]
pub(crate) enum MuxMsg {
    Video {
        nv12: Vec<u8>,
        ts: Duration,
    },
    Audio {
        pcm: Vec<i16>,
        ts: Duration,
        dur: Duration,
    },
}

/// Frames in flight between the capture callback and the mux. Deep enough
/// to ride out a fragment write, shallow enough that a genuinely stalled
/// mux shows up as counted drops within a second rather than as unbounded
/// memory growth.
#[cfg(windows)]
pub(crate) const CHANNEL_DEPTH: usize = 16;

/// Off Windows there is nothing to capture. `ScreenSession::start` is in
/// fact statically unreachable here — `source::SourceHandle` is an
/// uninhabited enum, so `ResolvedSource` cannot be constructed — but the
/// shape must still compile, because the Linux build is this crate's
/// compile gate.
#[cfg(not(windows))]
mod stub {
    use super::*;
    use crate::ScreenError;

    pub struct ScreenSession;

    impl ScreenSession {
        pub fn start(_params: ScreenSessionParams) -> Result<ScreenSession, ScreenError> {
            Err(ScreenError::Unsupported)
        }
        pub fn pause(&self) {}
        pub fn resume(&self) {}
        pub fn is_running(&self) -> bool {
            false
        }
        pub fn stop(self) -> Result<ScreenOutcome, ScreenError> {
            Err(ScreenError::Unsupported)
        }
    }
}

#[cfg(not(windows))]
pub use stub::ScreenSession;
#[cfg(windows)]
pub use windows_session::ScreenSession;

#[cfg(test)]
mod tests {
    use super::pacing::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_frame_lasts_its_share_of_a_second() {
        assert_eq!(frame_duration(30), Duration::from_nanos(33_333_333));
        assert_eq!(frame_duration(60), Duration::from_nanos(16_666_666));
    }

    #[test]
    fn frame_duration_never_divides_by_zero() {
        // fps reaches here from config. normalize_fps guards the config
        // path, but a division by zero in the capture hot path is a panic
        // across the WebView2 FFI boundary — an abort with no crash record.
        assert_eq!(frame_duration(0), frame_duration(1));
    }

    #[test]
    fn a_still_screen_repeats_its_last_frame_once_the_heartbeat_elapses() {
        let hb = Duration::from_millis(500);
        assert!(!should_repeat(Duration::from_millis(499), hb));
        assert!(should_repeat(Duration::from_millis(500), hb));
        assert!(should_repeat(Duration::from_secs(30), hb));
    }

    #[test]
    fn audio_is_batched_rather_than_written_per_callback() {
        assert!(!audio_chunk_ready(1023, 1024));
        assert!(audio_chunk_ready(1024, 1024));
        assert!(audio_chunk_ready(9999, 1024));
    }

    // --- the nonzero-duration guarantee ---------------------------------
    // Every sample handed to the AAC encoder MFT must carry a NONZERO
    // duration. A zero duration makes ProcessInput succeed and then
    // ProcessOutput raise a divide-by-zero (a documented MF implementation
    // bug) — an unexplainable crash with no usable stack. MF's time base is
    // 100 ns units, so "nonzero" means >= 100 ns, not >= 1 ns: a 99 ns
    // duration rounds to 0 hns in sink::to_hns and trips exactly the same
    // bug while looking nonzero here.

    #[test]
    fn an_empty_audio_chunk_is_not_written_at_all() {
        // Zero audio devices is legal and must produce a working silent
        // capture. The failure this guards: emitting a zero-length sample,
        // whose duration is necessarily 0 and whose ProcessOutput then
        // divides by zero.
        assert_eq!(audio_duration(0, 48_000), None);
    }

    #[test]
    fn an_audio_sample_duration_matches_its_frame_count() {
        // 4096 frames at 48 kHz: 4096 * 1e9 / 48000 = 85_333_333 ns
        // (derived by hand, not by running the implementation).
        assert_eq!(
            audio_duration(4096, 48_000),
            Some(Duration::from_nanos(85_333_333))
        );
        // 1024 frames (one AAC frame): 1024 * 1e9 / 48000 = 21_333_333 ns.
        assert_eq!(
            audio_duration(1024, 48_000),
            Some(Duration::from_nanos(21_333_333))
        );
    }

    #[test]
    fn an_audio_sample_duration_never_rounds_to_zero_hundred_nanosecond_units() {
        // A rate high enough that frames * 1e9 / rate truncates to 0 ns.
        // Unreachable through AUDIO_RATE today, but the floor is what makes
        // the guarantee a property of the function rather than of its one
        // current caller.
        let d = audio_duration(1, u32::MAX).expect("one frame is still a sample");
        assert!(d.as_nanos() >= 100, "{d:?} rounds to 0 hns");
    }

    #[test]
    fn a_frame_duration_never_rounds_to_zero_hundred_nanosecond_units() {
        // Same guarantee on the video side: frame_duration is public and a
        // caller that skipped normalize_fps could hand it a huge fps.
        assert!(frame_duration(u32::MAX).as_nanos() >= 100);
    }

    // --- audio timestamps ------------------------------------------------

    #[test]
    fn audio_timestamps_come_from_the_sample_count_not_the_wall_clock() {
        // A wall-clock stamp drifts against the sample rate and desyncs from
        // the video within minutes.
        assert_eq!(audio_ts(0, 48_000), Duration::ZERO);
        // 48000 frames = exactly one second.
        assert_eq!(audio_ts(48_000, 48_000), Duration::from_secs(1));
    }

    #[test]
    fn consecutive_audio_chunks_are_contiguous_and_strictly_increasing() {
        // The failure mode: a chunk stamped at the END of its own span (or a
        // ts computed after the counter advanced) leaves a gap the encoder
        // renders as a click, and overlapping chunks desync A/V.
        let mut p = AudioPacer::new(48_000);
        let (t0, d0) = p.take(4096).expect("a full chunk is written");
        let (t1, d1) = p.take(4096).expect("a full chunk is written");
        assert_eq!(t0, Duration::ZERO);
        assert_eq!(t1, t0 + d0, "chunk 2 must start where chunk 1 ended");
        assert_eq!(d0, d1);
        assert!(t1 > t0);
    }

    #[test]
    fn an_empty_audio_round_emits_nothing_and_does_not_advance_the_timeline() {
        let mut p = AudioPacer::new(48_000);
        assert!(p.take(0).is_none());
        let (t, _) = p.take(4096).expect("a full chunk is written");
        assert_eq!(
            t,
            Duration::ZERO,
            "an empty round must not shift the timeline"
        );
    }

    // --- monotonicity ----------------------------------------------------

    #[test]
    fn a_timestamp_is_forced_strictly_past_the_previous_one() {
        // Two frames sampled in the same instant would otherwise carry equal
        // timestamps, and a repeat after a pause could carry an EARLIER one.
        // A non-monotonic stream is rejected outright by the muxer.
        let step = Duration::from_nanos(100);
        assert_eq!(
            monotonic_ts(Duration::from_millis(5), None, step),
            Duration::from_millis(5)
        );
        assert_eq!(
            monotonic_ts(
                Duration::from_millis(5),
                Some(Duration::from_millis(5)),
                step
            ),
            Duration::from_millis(5) + step
        );
        assert_eq!(
            monotonic_ts(
                Duration::from_millis(1),
                Some(Duration::from_millis(9)),
                step
            ),
            Duration::from_millis(9) + step
        );
        assert_eq!(
            monotonic_ts(
                Duration::from_millis(9),
                Some(Duration::from_millis(5)),
                step
            ),
            Duration::from_millis(9),
            "a value already ahead is left exactly as it is"
        );
    }

    // --- the video pacer, which owns the heartbeat -----------------------

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn a_pacer_with_no_frame_yet_has_nothing_to_repeat() {
        // Repeating before the first frame would write an empty sample.
        let mut p = VideoPacer::new(30, Duration::from_millis(500));
        let base = Instant::now();
        assert!(p
            .on_idle(Some(Duration::from_secs(9)), at(base, 9_000))
            .is_none());
    }

    #[test]
    fn a_still_screen_repeats_the_last_frame_on_the_capture_clocks_timeline() {
        let mut p = VideoPacer::new(30, Duration::from_millis(500));
        let base = Instant::now();
        p.on_frame(Duration::from_millis(100), at(base, 100));
        // Not yet: only 499 ms of stillness.
        assert!(p
            .on_idle(Some(Duration::from_millis(599)), at(base, 599))
            .is_none());
        let (ts, dur) = p
            .on_idle(Some(Duration::from_millis(600)), at(base, 600))
            .expect("half a second of stillness closes no fragment otherwise");
        assert_eq!(
            ts,
            Duration::from_millis(600),
            "the repeat rides the capture clock"
        );
        assert_eq!(dur, frame_duration(30));
    }

    #[test]
    fn a_paused_capture_never_repeats_a_frame() {
        // REGRESSION: repeating while paused advances the output timeline by
        // PAUSED WALL-CLOCK TIME — exactly what CaptureClock exists to keep
        // out of the output — and then makes the first frame after resume
        // stamp EARLIER than the repeats, i.e. a non-monotonic stream.
        let mut p = VideoPacer::new(30, Duration::from_millis(500));
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
    fn the_wait_shrinks_so_the_heartbeat_fires_even_while_audio_keeps_arriving() {
        // REGRESSION: blocking a full heartbeat on every wakeup means the
        // timeout arm never runs while audio chunks arrive every ~85 ms, so
        // a still screen recorded WITH a microphone never repeats a frame
        // and closes no fragments — the heartbeat silently does nothing in
        // the common case.
        let mut p = VideoPacer::new(30, Duration::from_millis(500));
        let base = Instant::now();
        p.on_frame(Duration::from_millis(0), base);
        assert_eq!(p.wait(at(base, 100)), Duration::from_millis(400));
        // Never zero: a zero wait busy-spins the mux thread.
        assert!(p.wait(at(base, 5_000)) > Duration::ZERO);
    }

    #[test]
    fn a_repeat_is_forced_past_the_previous_timestamp_when_the_clock_has_not_moved() {
        let mut p = VideoPacer::new(30, Duration::from_millis(500));
        let base = Instant::now();
        p.on_frame(Duration::from_millis(100), at(base, 100));
        let (ts, _) = p
            .on_idle(Some(Duration::from_millis(100)), at(base, 700))
            .expect("the heartbeat elapsed");
        assert!(ts > Duration::from_millis(100));
    }

    // --- the mixing round ------------------------------------------------

    #[test]
    fn zero_audio_sources_never_produce_a_chunk() {
        // Zero devices is legal and must yield a working SILENT capture, not
        // a stream of empty samples.
        assert_eq!(take_frames(&[], 4096, 48_000), 0);
    }

    #[test]
    fn a_round_takes_the_frame_aligned_minimum_across_sources() {
        assert_eq!(take_frames(&[8192, 4096], 4096, 48_000), 4096);
        assert_eq!(take_frames(&[4095], 4096, 48_000), 0);
        assert_eq!(take_frames(&[4096], 4096, 48_000), 4096);
    }

    #[test]
    fn a_stalled_source_stops_silencing_every_other_source() {
        // REGRESSION: taking the minimum means one source that stops
        // delivering without reporting Lost pins the round at zero forever —
        // all audio stops and the live sources' buffers grow without bound.
        // Past the cap the round proceeds on the longest buffer and the
        // short ones mix as silence (mix_n_to_stereo_i16 pads).
        assert_eq!(take_frames(&[0, 48_000], 4096, 48_000), 48_000);
        assert_eq!(take_frames(&[0, 47_999], 4096, 48_000), 0);
    }

    // --- the pause flush (never silently drop already-captured audio) ----

    #[test]
    fn a_pause_flushes_every_buffered_frame_instead_of_dropping_it() {
        // REGRESSION: a pause used to clear the pre-pause partial buffers
        // outright, discarding up to one chunk's worth (~85 ms) of already
        // captured audio. Unlike `take_frames`, there is no `min_chunk`
        // gate here — a pause can land mid-chunk, and that partial audio
        // is still real.
        assert_eq!(pause_flush_frames(&[4095]), 4095);
        assert_eq!(pause_flush_frames(&[1]), 1);
    }

    #[test]
    fn a_pause_flush_takes_the_longest_source_so_a_lagging_one_is_padded_not_dropped() {
        // Two sources at different buffer lengths (normal jitter between
        // independently-delivering streams): flushing the shorter one's
        // length would silently drop the longer source's tail.
        assert_eq!(pause_flush_frames(&[2000, 4095]), 4095);
    }

    #[test]
    fn a_pause_flush_with_nothing_buffered_is_a_no_op() {
        assert_eq!(pause_flush_frames(&[]), 0);
        assert_eq!(pause_flush_frames(&[0, 0]), 0);
    }

    #[test]
    fn flushing_a_partial_buffer_on_pause_advances_emitted_like_any_other_chunk() {
        // Pinning that the pause flush rides the SAME `AudioPacer::take` as
        // the ordinary batching path, so `emitted` — and every timestamp
        // derived from it afterward — stays consistent with what was
        // actually written, instead of silently falling behind by
        // whatever a bare `buffers.clear()` would have dropped.
        let mut p = AudioPacer::new(48_000);
        let flush = pause_flush_frames(&[2048]);
        let (ts, dur) = p.take(flush).expect("a nonzero flush is written");
        assert_eq!(ts, Duration::ZERO);
        assert_eq!(dur, audio_duration(2048, 48_000).unwrap());
        // The next chunk (post-resume) must start exactly where the flush
        // left off, not back at zero — which is what "clear without
        // advancing the pacer" produced.
        let (ts2, _) = p.take(1024).expect("a nonzero chunk is written");
        assert_eq!(ts2, ts + dur);
    }

    // --- advisory stats ---------------------------------------------------

    #[test]
    fn observed_fps_is_frames_over_elapsed_and_never_divides_by_zero() {
        assert_eq!(observed_fps(30, Duration::from_secs(1)), 30.0);
        assert_eq!(observed_fps(15, Duration::from_millis(500)), 30.0);
        assert_eq!(observed_fps(5, Duration::ZERO), 0.0);
    }

    // --- reported duration: what the mux wrote, never the wall clock -----

    #[test]
    fn duration_reports_the_last_written_samples_end_not_the_stop_time() {
        // REGRESSION: the mux can end (a source closing, a write failure)
        // long before `stop()` is called; reporting the clock's
        // elapsed-at-stop instead of what was actually written claims
        // footage the file does not contain — a monitor unplugged at 60 s
        // with stop() called at 300 s must report 60 s, not 300 s.
        let written_until = Duration::from_secs(60);
        let stop_time_elapsed = Duration::from_secs(300);
        assert_eq!(
            resolved_duration(written_until, stop_time_elapsed),
            written_until
        );
    }

    #[test]
    fn duration_falls_back_to_the_clock_only_when_nothing_was_written() {
        // Duration::ZERO is the unambiguous "the mux never wrote a
        // sample" case (every real sample carries a nonzero duration —
        // MIN_SAMPLE), so only then does the clock-elapsed fallback apply.
        let stop_time_elapsed = Duration::from_secs(42);
        assert_eq!(
            resolved_duration(Duration::ZERO, stop_time_elapsed),
            stop_time_elapsed
        );
    }

    // --- the control state machine ---------------------------------------
    // Pause/resume/stop accounting is pure logic over the shared clock and
    // one flag, so it lives here where Linux can prove it rather than
    // inside the Windows arm where nothing can reach it.

    use super::{apply_control, Control, Warnings};
    use crate::clock::CaptureClock;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn pause_and_resume_move_only_the_clock() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        let stopping = AtomicBool::new(false);
        apply_control(&mut c, &stopping, Control::Pause, at(base, 1_000));
        assert!(c.is_paused());
        assert!(!stopping.load(Ordering::Relaxed), "a pause is not a stop");
        apply_control(&mut c, &stopping, Control::Resume, at(base, 6_000));
        assert!(!c.is_paused());
        assert_eq!(c.elapsed(at(base, 7_000)), Duration::from_millis(2_000));
    }

    #[test]
    fn stopping_while_paused_keeps_the_paused_time_out_of_the_duration() {
        // REGRESSION: resuming the clock on the way out (or stamping the
        // outcome from wall time) would bill the whole pause as recorded
        // duration, so a capture paused for ten minutes would report ten
        // extra minutes of footage that is not in the file.
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        let stopping = AtomicBool::new(false);
        apply_control(&mut c, &stopping, Control::Pause, at(base, 1_000));
        apply_control(&mut c, &stopping, Control::Stop, at(base, 601_000));
        assert!(stopping.load(Ordering::Relaxed));
        assert!(
            c.is_paused(),
            "stopping must not quietly resume the clock on the way out"
        );
        assert_eq!(c.elapsed(at(base, 601_000)), Duration::from_millis(1_000));
    }

    #[test]
    fn a_second_pause_does_not_restart_the_pause_window() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        let stopping = AtomicBool::new(false);
        apply_control(&mut c, &stopping, Control::Pause, at(base, 1_000));
        apply_control(&mut c, &stopping, Control::Pause, at(base, 3_000));
        apply_control(&mut c, &stopping, Control::Resume, at(base, 5_000));
        assert_eq!(c.elapsed(at(base, 6_000)), Duration::from_millis(2_000));
    }

    // --- warnings ---------------------------------------------------------

    #[test]
    fn a_warning_reaches_the_shell_live_and_is_kept_for_the_outcome() {
        let (tx, rx) = std::sync::mpsc::channel();
        let w = Warnings::new(Some(tx));
        w.raise("the capture source closed".into());
        // try_recv, never recv: `raise` sends synchronously, so a blocking
        // read turns a MISSING send into a hang instead of a failure — which
        // is exactly what it did when this test was mutation-checked.
        assert_eq!(rx.try_recv().unwrap(), "the capture source closed");
        assert_eq!(w.take(), Some("the capture source closed".to_string()));
    }

    #[test]
    fn the_first_warning_is_the_one_reported_and_a_gone_receiver_is_harmless() {
        // A gone receiver must never slow or fail the capture path, and a
        // later warning must not overwrite the one that explains why the
        // capture ended.
        let (tx, rx) = std::sync::mpsc::channel();
        drop(rx);
        let w = Warnings::new(Some(tx));
        w.raise("first".into());
        w.raise("second".into());
        assert_eq!(w.take(), Some("first".to_string()));
    }

    #[test]
    fn a_session_with_no_warning_channel_still_records_one() {
        let w = Warnings::new(None);
        assert_eq!(w.take(), None);
        w.raise("audio source lost".into());
        assert_eq!(w.take(), Some("audio source lost".to_string()));
    }

    // --- the non-Windows degradation --------------------------------------

    #[cfg(not(windows))]
    #[test]
    fn a_capture_cannot_even_be_constructed_off_windows() {
        // `source::SourceHandle` is an uninhabited enum on non-Windows, so
        // `ResolvedSource` is unconstructible, so `ScreenSessionParams`
        // cannot be built and `ScreenSession::start` is statically
        // unreachable here. That is a stronger guarantee than a runtime
        // Unsupported, and it is why this file has no runtime stub test.
        //
        // This replaces the phase-1 `engine::start_capture` stub test that
        // engine.rs's deletion removes. If a future change gives
        // SourceHandle a non-Windows variant, the `match h {}` below stops
        // compiling — which is the alarm.
        fn _assert_uninhabited(h: crate::source::SourceHandle) -> ! {
            match h {}
        }
        assert!(matches!(
            crate::source::resolve(&crate::source::SourceId::Screen(0)),
            Err(crate::ScreenError::Unsupported)
        ));
    }

    // --- which frames are usable ------------------------------------------

    #[test]
    fn a_frame_at_or_above_the_declared_size_is_kept_and_cropped() {
        // The sink's format is fixed at start. A window enlarged mid-capture
        // delivers BIGGER frames; bgra_to_nv12 reads width*4 bytes of each
        // of the first `height` rows out of a stride-pitched buffer, so a
        // bigger frame crops to the declared size for free.
        assert!(usable_frame(1920, 1080, 1920, 1080));
        assert!(usable_frame(2560, 1440, 1920, 1080));
    }

    #[test]
    fn a_frame_smaller_than_the_declared_size_is_dropped_not_read_past() {
        // A window SHRUNK mid-capture delivers frames with fewer rows. Reading
        // the declared height out of them runs past the end of the mapped
        // staging texture; padding can make the length check pass, so the
        // check has to be on the dimensions, not on the buffer length.
        assert!(!usable_frame(1920, 1079, 1920, 1080));
        assert!(!usable_frame(1919, 1080, 1920, 1080));
    }

    #[test]
    fn a_dropped_frame_is_logged_at_the_start_of_a_run_then_rate_limited() {
        // At 60 fps a persistently failing conversion would write a log line
        // every 16 ms and bury every other diagnostic in the file.
        assert!(should_log_drop(1));
        assert!(!should_log_drop(2));
        assert!(!should_log_drop(299));
        assert!(should_log_drop(301));
    }

    #[test]
    fn the_shared_clock_reads_none_while_paused_and_survives_a_poisoned_lock() {
        // Both producers read this helper every frame. A panic on one thread
        // poisoning the clock would otherwise freeze the whole capture.
        use super::{output_ts, SharedClock};
        use std::sync::Arc;
        let clock: SharedClock = Arc::new(std::sync::Mutex::new(CaptureClock::new(Instant::now())));
        assert!(output_ts(&clock).is_some());
        clock.lock().unwrap().pause(Instant::now());
        assert!(output_ts(&clock).is_none(), "paused: drain and discard");

        let poison = Arc::clone(&clock);
        let _ = std::thread::spawn(move || {
            let _guard = poison.lock().unwrap();
            panic!("poison the clock");
        })
        .join();
        clock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resume(Instant::now());
        assert!(output_ts(&clock).is_some(), "a poisoned clock still reads");
    }
}
