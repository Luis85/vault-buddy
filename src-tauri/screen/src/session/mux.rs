//! The mux thread: the ONLY owner of the `FragmentedSink`.
//!
//! `IMFSinkWriter` is a COM interface pointer and is NOT `Send`, which is
//! why the sink is created on this thread rather than handed to it, and why
//! the channel carries plain byte vectors instead of `IMFSample`s.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::sink::{AudioFormat, FragmentedSink, VideoFormat};
use crate::ScreenError;

use super::{output_ts, pacing, FrameStats, MuxMsg, SharedClock, Warnings, HEARTBEAT};

/// How often the advisory `FrameStats` go out (~2 Hz, spec 11).
const STATS_EVERY: Duration = Duration::from_millis(500);
/// After a stop is signalled the mux normally ends on DISCONNECT, which is
/// lossless: it only fires once every producer has dropped its sender, and
/// the channel is drained first. This is the bound for the abnormal case —
/// a `CaptureControl::stop` that could not post its WM_QUIT leaves the
/// frame thread alive and its sender held, and waiting on a disconnect that
/// will never come would hang the app on Stop with no crash record.
const STOP_GRACE: Duration = Duration::from_secs(2);

/// Everything the mux thread needs to build its own sink. The sink
/// itself cannot be handed over: it holds a COM interface pointer.
pub(super) struct SinkPlan {
    pub part: PathBuf,
    pub video: VideoFormat,
    pub audio: Option<AudioFormat>,
}

/// The mux thread: the ONLY owner of the sink.
///
/// It also carries the still-screen heartbeat. WGC emits a frame only
/// when something changes, so an untouched screen closes no fragments
/// and a crash would lose the whole idle stretch — exactly what spec
/// 6.4's container choice exists to prevent. The repeat decision, its
/// timestamp and the wait that lets it fire while audio keeps arriving
/// all live in `pacing::VideoPacer`, which Linux can prove.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_mux(
    sink: SinkPlan,
    ready_tx: mpsc::Sender<Result<(), ScreenError>>,
    rx: Receiver<MuxMsg>,
    clock: SharedClock,
    stopping: Arc<AtomicBool>,
    fps: u32,
    dropped: Arc<AtomicU64>,
    stats_tx: Option<Sender<FrameStats>>,
    warnings: Arc<Warnings>,
) -> Result<(), ScreenError> {
    // The sink is CREATED HERE, on the thread that owns it for its whole
    // life, because `IMFSinkWriter` is a COM interface pointer and is
    // NOT `Send` — the same reason samples never cross a thread. It also
    // keeps `MFStartup`/`MFShutdown` paired on one thread. `start` waits
    // on `ready_tx` for this result, so a creation failure is still
    // reported synchronously from `ScreenSession::start`.
    let mut sink = match FragmentedSink::create(&sink.part, sink.video, sink.audio) {
        Ok(s) => {
            if ready_tx.send(Ok(())).is_err() {
                // Nobody is waiting any more; there is nothing to write
                // for. Finalize the (empty) file rather than leak it.
                return s.finalize();
            }
            s
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e.clone()));
            return Err(e);
        }
    };
    drop(ready_tx);

    let mut pacer = pacing::VideoPacer::new(fps, HEARTBEAT);
    let mut last_frame: Option<Vec<u8>> = None;
    let mut stop_seen: Option<Instant> = None;
    let mut stats_at = Instant::now();
    let mut stats_frames: u64 = 0;

    let result = loop {
        let wait = pacer.wait(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(MuxMsg::Video { nv12, ts }) => {
                let (ts, dur) = pacer.on_frame(ts, Instant::now());
                if let Err(e) = sink.write_video(&nv12, ts, dur) {
                    break Err(e);
                }
                last_frame = Some(nv12);
                stats_frames += 1;
            }
            Ok(MuxMsg::Audio { pcm, ts, dur }) => {
                if let Err(e) = sink.write_audio(&pcm, ts, dur) {
                    break Err(e);
                }
            }
            // Every producer dropped its sender: the capture is over and
            // the queue is already drained. This is the normal exit.
            Err(RecvTimeoutError::Disconnected) => break Ok(()),
            Err(RecvTimeoutError::Timeout) => {
                if stopping.load(Ordering::Relaxed) {
                    let since = *stop_seen.get_or_insert_with(Instant::now);
                    if since.elapsed() >= STOP_GRACE {
                        log::warn!(
                            "screen capture: a producer never released the mux channel; \
                             finalizing anyway so the capture is not lost"
                        );
                        break Ok(());
                    }
                }
            }
        }

        // Checked on EVERY wakeup, not only on a timeout: audio chunks
        // arrive every ~85 ms, so a heartbeat that only ran in the
        // timeout arm would never fire on a still screen recorded with a
        // microphone.
        if let Some(frame) = &last_frame {
            if let Some((ts, dur)) = pacer.on_idle(output_ts(&clock), Instant::now()) {
                if let Err(e) = sink.write_video(frame, ts, dur) {
                    break Err(e);
                }
                // Counted like any other frame: the stat describes what
                // the FILE contains, and a repeat is in the file. The
                // alternative — reporting 0 fps while fragments are
                // being written — would read as a stalled capture.
                stats_frames += 1;
            }
        }

        if let Some(tx) = &stats_tx {
            let elapsed = stats_at.elapsed();
            if elapsed >= STATS_EVERY {
                // Lossy by design: a gone receiver must never slow or
                // fail the capture path.
                let _ = tx.send(FrameStats {
                    fps: pacing::observed_fps(stats_frames, elapsed),
                    dropped: dropped.load(Ordering::Relaxed),
                });
                stats_at = Instant::now();
                stats_frames = 0;
            }
        }
    };

    if let Err(e) = &result {
        // The capture is still worth finalizing: a fragmented MP4
        // degrades to a playable prefix, so a write failure ten minutes
        // in must not throw away the ten minutes.
        warnings.raise(format!("the capture stopped writing early: {e}"));
    }
    log::info!(
        "screen capture: finalizing after {} dropped frame(s)",
        dropped.load(Ordering::Relaxed)
    );
    // Finalize is the ONLY drain. Never call Flush here — it DISCARDS
    // pending samples (see sink.rs's module docs).
    let finalized = sink.finalize();
    // A write failure that still finalized is a warning, not a failure:
    // the file plays. Only a finalize failure is fatal.
    finalized
}
