//! The WGC frame callback: BGRA out of the GPU staging texture, NV12 into
//! the mux channel. Windows-only.
//!
//! DELIBERATELY THIN. Every decision this file makes — is the frame usable,
//! should the drop be logged, what timestamp does it carry — is made by a
//! pure function in `session::pacing` or by the shared `CaptureClock`, so
//! it is provable on Linux. No CI runner can record a screen, so logic that
//! settles here is logic nothing can reach.
//!
//! A PANIC HERE IS FATAL. This runs as a callback across the WinRT/COM
//! boundary, where an unwind aborts the process with no crash record. There
//! is no indexing, no unwrap and no division in the body below for exactly
//! that reason, and a conversion failure is a counted drop rather than an
//! error return (an error return also ends the capture — see
//! `graphics_capture_api.rs`, which halts the session on `result.is_err()`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;

use crate::session::{output_ts, pacing, Counters, MuxMsg, SharedClock, Warnings};
use crate::ScreenError;
use crate::{convert, diagnose};

/// Everything the callback needs, handed in through `Settings`.
pub(crate) struct FrameFlags {
    pub clock: SharedClock,
    pub tx: SyncSender<MuxMsg>,
    /// Where in each delivered frame the output starts. `(0, 0)` for a
    /// whole screen or window; a region's origin inside its monitor.
    pub crop_x: u32,
    pub crop_y: u32,
    /// The size the sink was opened with — already rounded to even by
    /// `convert::even_dims`. A resized window's frames are judged against
    /// this, never the other way round: the output format is fixed at start.
    pub width: u32,
    pub height: u32,
    pub counters: Arc<Counters>,
    pub stopping: Arc<AtomicBool>,
    pub warnings: Arc<Warnings>,
}

pub(crate) struct FrameHandler {
    flags: FrameFlags,
    /// Reused across frames: a per-frame NV12 allocation at 4K60 is
    /// megabytes per second of allocator churn in the hot path.
    nv12: Vec<u8>,
}

impl FrameHandler {
    fn drop_frame(&self, reason: &str) {
        // fetch_add returns the PREVIOUS value, so +1 is this drop's ordinal
        // and the first drop of a run is 1 — which is what should_log_drop
        // keys on.
        let n = self.flags.counters.dropped.fetch_add(1, Ordering::Relaxed) + 1;
        if pacing::should_log_drop(n) {
            log::warn!("screen capture: dropped a frame ({reason}); {n} so far");
        }
    }
}

impl GraphicsCaptureApiHandler for FrameHandler {
    type Flags = FrameFlags;
    type Error = ScreenError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(FrameHandler {
            flags: ctx.flags,
            nv12: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.flags.stopping.load(Ordering::Relaxed) {
            capture_control.stop();
            return Ok(());
        }
        // Paused: drain and discard (spec 6.3). The WGC session stays open,
        // because tearing it down and recreating it on every pause drops
        // frames on resume and can fail outright if the target window
        // changed state.
        let Some(ts) = output_ts(&self.flags.clock) else {
            return Ok(());
        };

        if !pacing::usable_frame(
            frame.width(),
            frame.height(),
            self.flags.crop_x,
            self.flags.crop_y,
            self.flags.width,
            self.flags.height,
        ) {
            // RECORDED, not just logged: if every frame lands here the file
            // ends up with no video at all, and `mux` needs the observed
            // size to tell the user WHY rather than surfacing a bare
            // MF_E_SINK_NO_SAMPLES_PROCESSED. Latest-wins is enough — the
            // sizes in a run are all the same one.
            self.flags.counters.undersized.store(
                diagnose::pack_dims(frame.width(), frame.height()),
                Ordering::Relaxed,
            );
            // Both sizes in the line, never just "the source shrank": a
            // wrongly DECLARED size and a user resizing the window produce
            // the same drop, and only the numbers tell them apart.
            self.drop_frame(&diagnose::undersized_drop_reason(
                frame.width(),
                frame.height(),
                self.flags.width,
                self.flags.height,
            ));
            return Ok(());
        }

        let mut buffer = match frame.buffer() {
            Ok(b) => b,
            Err(e) => {
                // One unmappable frame must not end a recording that is
                // otherwise fine.
                self.drop_frame(&format!("the frame could not be mapped: {e}"));
                return Ok(());
            }
        };
        // The PADDED buffer plus its row pitch, deliberately — NOT
        // `as_nopadding_buffer`, which copies the whole frame to strip
        // padding that `convert::bgra_to_nv12` already skips. That stride
        // parameter exists precisely so this copy is unnecessary.
        let stride = buffer.row_pitch() as usize;
        let (crop_x, crop_y) = (self.flags.crop_x, self.flags.crop_y);
        let (width, height) = (self.flags.width, self.flags.height);
        let bytes = buffer.as_raw_buffer();

        if let Err(e) =
            convert::bgra_crop_to_nv12(bytes, stride, crop_x, crop_y, width, height, &mut self.nv12)
        {
            self.drop_frame(&e.to_string());
            return Ok(());
        }

        // A bounded channel, and a full one counts a drop instead of
        // blocking. Blocking here would stall the WGC callback and drop
        // frames at the SOURCE with no counter; an unbounded channel would
        // trade a visible drop for invisible memory growth. The counter is
        // what `screen:frames` reports (spec 17.3).
        match self.flags.tx.try_send(MuxMsg::Video {
            nv12: self.nv12.clone(),
            ts,
        }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.drop_frame("the muxer is behind"),
            Err(TrySendError::Disconnected(_)) => {
                // The mux is gone; there is nothing left to write to.
                capture_control.stop();
            }
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // Spec 14: the source vanishing during capture is a WARNING and the
        // capture finalizes cleanly with what it has. A closed window must
        // not lose the recording that preceded it, so this signals a stop
        // rather than returning an error (which the capture API turns into
        // a failed session).
        self.flags
            .warnings
            .raise("the capture source closed — the recording was ended early".into());
        self.flags.stopping.store(true, Ordering::Relaxed);
        Ok(())
    }
}
