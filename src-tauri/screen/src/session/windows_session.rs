//! The Windows `ScreenSession`: start the three threads, own their handles,
//! and finalize before renaming.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use vault_buddy_capture::session::SourceInput;
use vault_buddy_core::capture_paths;
use vault_buddy_core::screen_capture_config::{bitrate_bps, normalize_fps};
use windows_capture::capture::{
    CaptureControl, GraphicsCaptureApiError, GraphicsCaptureApiHandler,
};
use windows_capture::graphics_capture_api::GraphicsCaptureApi;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::clock::CaptureClock;
use crate::convert;
use crate::frames::{FrameFlags, FrameHandler};
use crate::sink::{AudioFormat, VideoFormat};
use crate::source::SourceHandle;
use crate::ScreenError;

use super::audio::run_audio;
use super::mux::{run_mux, SinkPlan};
use super::{
    apply_control, pacing, Control, Counters, MuxMsg, ScreenOutcome, ScreenSessionParams,
    SharedClock, Warnings, AUDIO_BITRATE_BPS, AUDIO_CHANNELS, AUDIO_RATE, CHANNEL_DEPTH,
};

/// A live screen capture. Task 8's `screen_commands.rs` is the only
/// caller.
pub struct ScreenSession {
    clock: SharedClock,
    stopping: Arc<AtomicBool>,
    counters: Arc<Counters>,
    warnings: Arc<Warnings>,
    control: Option<CaptureControl<FrameHandler, ScreenError>>,
    audio: Option<JoinHandle<()>>,
    mux: Option<JoinHandle<Result<Duration, ScreenError>>>,
    started: Instant,
    part: PathBuf,
    staged: PathBuf,
    width: u32,
    height: u32,
    crop_x: u32,
    crop_y: u32,
}

impl ScreenSession {
    pub fn start(params: ScreenSessionParams) -> Result<ScreenSession, ScreenError> {
        let ScreenSessionParams {
            source,
            part,
            staged,
            fps,
            quality,
            audio,
            warn_tx,
            stats_tx,
        } = params;

        let fps = normalize_fps(fps);
        // Rounded DOWN to even, once, here: NV12 has no way to express an
        // odd dimension, and every frame is judged against this size.
        let (width, height) = convert::even_dims(source.width, source.height);
        if width == 0 || height == 0 {
            log::warn!("screen capture: the source is too small to capture ({width}x{height})");
            return Err(ScreenError::SourceGone);
        }
        // The crop origin is NOT rounded — `even_dims` shrinks a size to
        // something NV12 can express, which is about the output, while the
        // origin is about where in the source that output starts.
        // `clamp_to_frame` has already made a region's size even, so this
        // is a no-op for regions and only ever trims a whole screen or
        // window (whose origin is 0 either way).
        let (crop_x, crop_y) = (source.crop_x, source.crop_y);

        let video = VideoFormat {
            width,
            height,
            fps,
            bitrate_bps: bitrate_bps(quality, width, height, fps),
        };
        // Zero audio sources is legal (spec 6.5): the sink declares no
        // audio stream and `write_audio` is a no-op.
        let audio_format = (!audio.is_empty()).then_some(AudioFormat {
            sample_rate: AUDIO_RATE,
            channels: AUDIO_CHANNELS,
            bitrate_bps: AUDIO_BITRATE_BPS,
        });
        // Validated on THIS thread so a bad format is a synchronous
        // error with a useful message, not a thread-start failure.
        video.validate()?;
        if let Some(a) = audio_format {
            a.validate()?;
        }
        let plan = SinkPlan {
            part: part.clone(),
            video,
            audio: audio_format,
        };

        // ONE instant for the clock and for the outcome's paused
        // accounting: paused_ms is wall time minus the clock's elapsed,
        // so two slightly different origins would bill the gap between
        // them as pause.
        let started = Instant::now();
        let clock: SharedClock = Arc::new(Mutex::new(CaptureClock::new(started)));
        let stopping = Arc::new(AtomicBool::new(false));
        let counters = Arc::new(Counters::default());
        let warnings = Arc::new(Warnings::new(warn_tx));
        let (tx, rx) = mpsc::sync_channel::<MuxMsg>(CHANNEL_DEPTH);

        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), ScreenError>>();
        let mux = std::thread::Builder::new()
            .name("screen-mux".into())
            .spawn({
                let (clock, stopping, counters, warnings) = (
                    Arc::clone(&clock),
                    Arc::clone(&stopping),
                    Arc::clone(&counters),
                    Arc::clone(&warnings),
                );
                move || {
                    run_mux(
                        plan, ready_tx, rx, clock, stopping, fps, counters, stats_tx, warnings,
                    )
                }
            })
            .map_err(|e| {
                log::error!("screen capture: could not start the mux thread: {e}");
                ScreenError::Io(format!("could not start the mux thread: {e}"))
            })?;

        // Wait for the sink: a capture that cannot be written must fail
        // HERE, while the caller can still show the user why, rather
        // than look like it started and produce nothing.
        match ready_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = mux.join();
                return Err(e);
            }
            Err(_) => {
                let _ = mux.join();
                log::error!("screen capture: the mux thread ended before opening the file");
                return Err(ScreenError::Sink(
                    "the capture writer could not be started".into(),
                ));
            }
        }

        // From here on a failure must tear the mux down rather than leak
        // it, or the .part file stays open for the life of the process.
        let session = ScreenSession {
            clock,
            stopping,
            counters,
            warnings,
            control: None,
            audio: None,
            mux: Some(mux),
            started,
            part,
            staged,
            width,
            height,
            crop_x,
            crop_y,
        };
        session.spawn_producers(source.handle, audio, tx, fps, width, height)
    }

    /// The two producers, spawned after the mux so a failure in either
    /// can unwind through `abandon`.
    #[allow(clippy::too_many_arguments)]
    fn spawn_producers(
        mut self,
        handle: SourceHandle,
        audio: Vec<SourceInput>,
        tx: SyncSender<MuxMsg>,
        fps: u32,
        width: u32,
        height: u32,
    ) -> Result<ScreenSession, ScreenError> {
        let frame_tx = tx.clone();
        let audio_thread = std::thread::Builder::new()
            .name("screen-audio".into())
            .spawn({
                let (clock, stopping, warnings) = (
                    Arc::clone(&self.clock),
                    Arc::clone(&self.stopping),
                    Arc::clone(&self.warnings),
                );
                // `tx` (not a clone) moves here: this thread owning the last
                // sender is what makes its exit disconnect the mux.
                move || run_audio(audio, clock, tx, stopping, warnings)
            });
        let audio_thread = match audio_thread {
            Ok(h) => h,
            Err(e) => {
                // BEFORE abandon: this frame sender is the mux's other
                // half, and holding it here would make the mux wait out
                // its whole stop grace for a disconnect that is sitting
                // on this stack frame.
                drop(frame_tx);
                self.abandon();
                log::error!("screen capture: could not start the audio thread: {e}");
                return Err(ScreenError::Io(format!(
                    "could not start the audio thread: {e}"
                )));
            }
        };
        self.audio = Some(audio_thread);

        let flags = FrameFlags {
            clock: Arc::clone(&self.clock),
            tx: frame_tx,
            crop_x: self.crop_x,
            crop_y: self.crop_y,
            width,
            height,
            counters: Arc::clone(&self.counters),
            stopping: Arc::clone(&self.stopping),
            warnings: Arc::clone(&self.warnings),
        };
        match start_frames(handle, fps, flags) {
            Ok(control) => {
                self.control = Some(control);
                Ok(self)
            }
            Err(e) => {
                self.abandon();
                Err(e)
            }
        }
    }

    /// Last-resort finalize for a session nobody called `stop` on.
    ///
    /// Dropping a live capture would otherwise leak three threads and
    /// leave the `.part` file open for the life of the process, so a
    /// caller that panics between `start` and `stop` would cost the user
    /// the whole recording — the one outcome this module exists to
    /// prevent. It cannot rename (a `Drop` has nowhere to report a
    /// failure), so the `.part` is left for recovery.
    fn drop_guard(&mut self) {
        if self.mux.is_none() {
            return; // stop() already ran; nothing is outstanding
        }
        log::warn!(
            "screen capture: the session was dropped without a stop; finalizing so the \
             capture is not lost"
        );
        self.stopping.store(true, Ordering::Relaxed);
        if let Some(control) = self.control.take() {
            if let Err(e) = control.stop() {
                log::warn!("screen capture: the frame worker did not stop cleanly: {e}");
            }
        }
        self.abandon();
    }

    /// Tear down a half-started session. It still FINALIZES: the
    /// container is fragmented precisely so a partial capture is a
    /// playable prefix rather than an unopenable file.
    fn abandon(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        if let Some(h) = self.audio.take() {
            let _ = h.join();
        }
        if let Some(h) = self.mux.take() {
            if let Ok(Err(e)) = h.join() {
                log::warn!("screen capture: abandoning the capture also failed to finalize: {e}");
            }
        }
    }

    /// The ONE place a control transition is applied, so no second
    /// signalling path can race the stop.
    fn signal(&self, control: Control) {
        let mut clock = self.clock.lock().unwrap_or_else(|e| e.into_inner());
        apply_control(&mut clock, &self.stopping, control, Instant::now());
    }

    pub fn pause(&self) {
        self.signal(Control::Pause);
    }

    pub fn resume(&self) {
        self.signal(Control::Resume);
    }

    pub fn is_running(&self) -> bool {
        !self.stopping.load(Ordering::Relaxed)
            && self.mux.as_ref().is_some_and(|h| !h.is_finished())
    }

    /// Signal, join, FINALIZE, then rename — in that order, always.
    /// Renaming a file the sink still owns is how you get a zero-byte
    /// staged capture.
    pub fn stop(mut self) -> Result<ScreenOutcome, ScreenError> {
        self.signal(Control::Stop);

        // Stop the WGC session and JOIN its thread, so the frame sender
        // is released before anything waits on the mux.
        if let Some(control) = self.control.take() {
            if let Err(e) = control.stop() {
                // Not fatal: the mux's STOP_GRACE bounds the wait, and
                // the footage already written is still finalized.
                log::warn!("screen capture: the frame worker did not stop cleanly: {e}");
            }
        }
        if let Some(h) = self.audio.take() {
            if h.join().is_err() {
                log::error!("screen capture: the audio thread panicked");
            }
        }

        let now = Instant::now();
        let elapsed = self
            .clock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .elapsed(now);
        let paused = now
            .saturating_duration_since(self.started)
            .saturating_sub(elapsed);

        // The mux finalizes as it exits. Finalize is the only drain.
        // A finalize failure or a mux panic both still leave the `.part`
        // on disk holding whatever it had written — the fragmented
        // container degrades to a playable prefix rather than an
        // unopenable file — so both are reported as `Retained` with that
        // path, not a bare Sink error, so a caller can offer it to the
        // user instead of only logging where it went.
        // Whether the retained `.part` is worth anything is NOT a guess:
        // the mux counts every video sample it hands the sink, in a shared
        // atomic that survives even the mux PANICKING. Saying "the file
        // still holds the recording" over a file with no video in it is the
        // untrue message this counter exists to make impossible.
        let holds_footage = || self.counters.video_written.load(Ordering::Relaxed) > 0;
        let written_until = match self.mux.take() {
            Some(h) => match h.join() {
                Ok(Ok(written_until)) => written_until,
                Ok(Err(e)) => {
                    return Err(ScreenError::Retained {
                        path: self.part.clone(),
                        holds_footage: holds_footage(),
                        cause: Box::new(e),
                    })
                }
                Err(_) => {
                    log::error!("screen capture: the mux thread panicked");
                    return Err(ScreenError::Retained {
                        path: self.part.clone(),
                        holds_footage: holds_footage(),
                        cause: Box::new(ScreenError::Sink(
                            "the capture writer stopped unexpectedly".into(),
                        )),
                    });
                }
            },
            None => return Err(ScreenError::Sink("the capture was already stopped".into())),
        };

        // rename_noreplace, never std::fs::rename, which REPLACES on
        // every platform. On failure the .part is deliberately left
        // where it is: it holds the footage, and recovery finds it —
        // and `Retained` carries that path so a caller need not rely on
        // recovery alone to offer it back to the user.
        capture_paths::rename_noreplace(&self.part, &self.staged).map_err(|e| {
            log::error!(
                "screen capture: could not publish {:?} as {:?}: {e}; the .part file is \
                 left in place so the capture is not lost",
                self.part,
                self.staged
            );
            ScreenError::Retained {
                path: self.part.clone(),
                holds_footage: holds_footage(),
                cause: Box::new(ScreenError::Io(format!(
                    "could not finish the capture file: {e}"
                ))),
            }
        })?;

        Ok(ScreenOutcome {
            mp4: self.staged.clone(),
            // The last sample the mux actually wrote, not the wall clock at
            // stop() time (`pacing::resolved_duration`'s doc comment has the
            // full reasoning): the mux can end well before stop() is
            // called, and reporting stop-time would claim footage the file
            // does not contain.
            duration_ms: pacing::resolved_duration(written_until, elapsed).as_millis() as u64,
            paused_ms: paused.as_millis() as u64,
            width: self.width,
            height: self.height,
            dropped: self.counters.dropped.load(Ordering::Relaxed),
            warning: self.warnings.take(),
        })
    }
}

impl Drop for ScreenSession {
    fn drop(&mut self) {
        self.drop_guard();
    }
}

/// Start the WGC session. Two arms because `Settings` is generic over
/// the item type, and a `Monitor` and a `Window` are different types.
///
/// The capture worker thread is spawned INSIDE `windows-capture`
/// (`capture.rs`'s `start_free_threaded` uses a bare `thread::spawn`),
/// so it is the one thread reached from this file that cannot be named.
/// The blocking `start` would let us own and name it, but it offers no
/// way to stop a session that is receiving no frames — a still screen
/// would hang the app on Stop — so the unnamed thread is the lesser
/// cost. Recorded here because the diagnostics invariant ("every
/// spawned thread is named") is otherwise silently broken.
fn start_frames(
    handle: SourceHandle,
    fps: u32,
    flags: FrameFlags,
) -> Result<CaptureControl<FrameHandler, ScreenError>, ScreenError> {
    let started = match handle {
        SourceHandle::Screen(m) => FrameHandler::start_free_threaded(settings(m, fps, flags)),
        SourceHandle::Window(w) => FrameHandler::start_free_threaded(settings(w, fps, flags)),
    };
    started.map_err(|e| {
        log::error!("screen capture: could not start the frame worker: {e}");
        match e {
            // The picked source stopped being capturable between
            // `source::resolve` and here.
            GraphicsCaptureApiError::ItemConvertFailed => ScreenError::SourceGone,
            other => ScreenError::Sink(format!("could not start the frame worker: {other}")),
        }
    })
}

fn settings<T: TryInto<GraphicsCaptureItemType>>(
    item: T,
    fps: u32,
    flags: FrameFlags,
) -> Settings<FrameFlags, T> {
    Settings::new(
        item,
        // Every OS-policy knob is left at Default deliberately: none of
        // them has a setting behind it yet, and picking one here would
        // be inventing product behaviour in the capture engine.
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        // An OS-side throttle, not a frame rate: WGC otherwise delivers
        // at the monitor's refresh, so a 30 fps capture on a 144 Hz
        // panel would push four frames through the bounded channel for
        // every one the mux wants and report the rest to the user as
        // DROPPED frames. Capping eligibility at one frame period makes
        // that counter mean what it says.
        minimum_update_interval_settings(fps),
        DirtyRegionSettings::Default,
        // BGRA8, NOT this crate's Rgba8 DEFAULT: `convert::bgra_to_nv12`
        // reads B, G, R, A per pixel, so the default would swap red and
        // blue in every captured frame — a bug that looks like a colour
        // grade, not a format mistake.
        ColorFormat::Bgra8,
        flags,
    )
}

/// Whether to throttle WGC delivery to the frame rate, or leave it at
/// `Default` (deliver at the monitor's refresh).
///
/// `MinimumUpdateIntervalSettings::Custom` is a HARD GATE in the vendored
/// `windows-capture` crate, not a hint: `GraphicsCaptureApi::new` checks
/// it BEFORE the session is created (`graphics_capture_api.rs` ~150-153)
/// and `GraphicsCaptureApi::start` checks it again when applying the
/// settings (~349-358) — both refuse the WHOLE session with
/// `MinimumUpdateIntervalUnsupported` whenever this setting is anything
/// but `Default` and `is_minimum_update_interval_supported()` does not
/// return `Ok(true)`. That probe queries a Windows 11-era WinRT property
/// (`GraphicsCaptureSession.MinUpdateInterval`).
///
/// The app targets Windows 11, where the probe returns `Ok(true)` and
/// `Custom` is what actually gets used — so the fallback arms below are
/// not the expected path. They stay anyway, and deliberately: this is a
/// CAPABILITY query, not a version check. A Windows 11 build that has
/// not yet shipped the property, a future runtime that moves it, or a
/// probe that simply errors would otherwise take the WHOLE session down
/// with `MinimumUpdateIntervalUnsupported` — an optional throttling knob
/// must never be the reason a capture cannot start at all. Deleting the
/// fallback to "simplify" re-opens exactly that failure, which is why it
/// is called out here rather than left to look like dead code.
fn minimum_update_interval_settings(fps: u32) -> MinimumUpdateIntervalSettings {
    match GraphicsCaptureApi::is_minimum_update_interval_supported() {
        Ok(true) => {
            log::debug!(
                "screen capture: MinUpdateInterval is supported; throttling WGC delivery to \
                 the frame rate"
            );
            MinimumUpdateIntervalSettings::Custom(pacing::frame_duration(fps))
        }
        Ok(false) => {
            log::debug!(
                "screen capture: MinUpdateInterval is not supported on this Windows build; \
                 capturing at the monitor's full refresh rate instead"
            );
            MinimumUpdateIntervalSettings::Default
        }
        Err(e) => {
            log::debug!(
                "screen capture: could not probe MinUpdateInterval support ({e}); capturing \
                 at the monitor's full refresh rate instead"
            );
            MinimumUpdateIntervalSettings::Default
        }
    }
}
