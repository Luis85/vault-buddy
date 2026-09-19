//! The mux thread: the ONLY owner of the `FragmentedSink`.
//!
//! `IMFSinkWriter` is a COM interface pointer and is NOT `Send`, which is
//! why the sink is created on this thread rather than handed to it, and why
//! the channel carries plain byte vectors instead of `IMFSample`s.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::diagnose;
use crate::sink::{AudioFormat, FragmentedSink, VideoFormat};
use crate::ScreenError;

use super::{output_ts, pacing, Counters, FrameStats, MuxMsg, SharedClock, Warnings, HEARTBEAT};

/// How often the advisory `FrameStats` go out (~2 Hz, spec 11).
const STATS_EVERY: Duration = Duration::from_millis(500);
/// After a stop is signalled the mux normally ends on DISCONNECT, which is
/// lossless: it only fires once every producer has dropped its sender, and
/// the channel is drained first. In practice THAT disconnect is what bounds
/// stop, and it happens almost immediately: the frame callback's own
/// `stopping` check (`frames.rs`'s `on_frame_arrived`) ends the WGC session
/// on its very next delivery once `Control::Stop` is signalled, dropping
/// the frame sender straight away.
///
/// `STOP_GRACE` is NOT that bound — it is evaluated only in
/// `recv_timeout`'s `Timeout` arm below, and `pacer.wait()` resets to
/// roughly the heartbeat (~500 ms) after every frame, so a
/// steadily-delivering producer never lets that arm run at all; this
/// constant sits unreached on the ordinary stop path. What it covers is
/// the abnormal, much rarer case where the Timeout arm genuinely keeps
/// firing while stopping is set: a `CaptureControl::stop` whose WM_QUIT
/// could not be posted, or a session that has gone fully idle (no video,
/// no audio) right as Stop is signalled. Either would otherwise wait on a
/// disconnect that never comes and hang the app on Stop with no crash
/// record — `STOP_GRACE` is the bound for THAT wait, not for stop in
/// general.
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
///
/// Returns the end timestamp of the last sample actually written on a
/// clean finalize — `ScreenSession::stop` reports THIS as the capture's
/// duration, not the wall clock, since the mux can end well before `stop`
/// is called.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_mux(
    sink: SinkPlan,
    ready_tx: mpsc::Sender<Result<(), ScreenError>>,
    rx: Receiver<MuxMsg>,
    clock: SharedClock,
    stopping: Arc<AtomicBool>,
    fps: u32,
    counters: Arc<Counters>,
    stats_tx: Option<Sender<FrameStats>>,
    warnings: Arc<Warnings>,
) -> Result<Duration, ScreenError> {
    // The sink is CREATED HERE, on the thread that owns it for its whole
    // life, because `IMFSinkWriter` is a COM interface pointer and is
    // NOT `Send` — the same reason samples never cross a thread. It also
    // keeps `MFStartup`/`MFShutdown` paired on one thread. `start` waits
    // on `ready_tx` for this result, so a creation failure is still
    // reported synchronously from `ScreenSession::start`.
    //
    // `declared` is read BEFORE `sink` is consumed: the size the file was
    // opened at is half of what a zero-video capture has to tell the user.
    let declared = (sink.video.width, sink.video.height);
    let mut sink = match FragmentedSink::create(&sink.part, sink.video, sink.audio) {
        Ok(s) => {
            if ready_tx.send(Ok(())).is_err() {
                // Nobody is waiting any more; there is nothing to write
                // for. Finalize the (empty) file rather than leak it.
                // Nothing was ever written, so ZERO is the correct
                // written-until value here too.
                return s.finalize().map(|()| Duration::ZERO);
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
    // The end (ts + dur) of the last sample actually handed to the sink,
    // video or audio, whichever is later. This — NOT the wall clock at
    // `stop()` time — is what the session reports as the capture's
    // duration: the mux can end long before `stop()` is called (a source
    // closing, a write failure that still finalizes what came before it),
    // and reporting stop-time would claim footage the file does not
    // contain. `Duration::ZERO` doubles as "nothing was written": every
    // real sample carries a nonzero duration (`pacing::MIN_SAMPLE`), so
    // zero can never be a legitimate written-until value.
    let mut written_until = Duration::ZERO;

    let result = loop {
        let wait = pacer.wait(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(MuxMsg::Video { nv12, ts }) => {
                let (ts, dur) = pacer.on_frame(ts, Instant::now());
                if let Err(e) = sink.write_video(&nv12, ts, dur) {
                    break Err(e);
                }
                written_until = written_until.max(ts + dur);
                counters.video_written.fetch_add(1, Ordering::Relaxed);
                last_frame = Some(nv12);
                stats_frames += 1;
            }
            Ok(MuxMsg::Audio { pcm, ts, dur }) => {
                if let Err(e) = sink.write_audio(&pcm, ts, dur) {
                    break Err(e);
                }
                written_until = written_until.max(ts + dur);
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
                written_until = written_until.max(ts + dur);
                counters.video_written.fetch_add(1, Ordering::Relaxed);
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
                    dropped: counters.dropped.load(Ordering::Relaxed),
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
        counters.dropped.load(Ordering::Relaxed)
    );
    // Finalize is the ONLY drain. Never call Flush here — it DISCARDS
    // pending samples (see sink.rs's module docs).
    let finalized = sink.finalize();

    // A capture that handed the sink NO video sample cannot be described
    // by whatever HRESULT finalize happens to raise — Media Foundation's
    // own answer is `MF_E_SINK_NO_SAMPLES_PROCESSED`, which reaches the
    // user as a raw, untranslatable OS string. The real cause is knowable
    // here: the frames were the wrong size for the file, and both sizes
    // are in hand. Replacing the HRESULT is deliberate, not hiding it —
    // the raw text goes to the log for a bug report.
    if let Some(msg) = diagnose::zero_video_diagnosis(
        counters.video_written.load(Ordering::Relaxed),
        declared,
        diagnose::unpack_dims(counters.undersized.load(Ordering::Relaxed)),
    ) {
        match &finalized {
            Ok(()) => log::error!("screen capture: {msg}"),
            Err(e) => log::error!("screen capture: {msg}; finalize also failed: {e}"),
        }
        return Err(ScreenError::Sink(msg));
    }
    // A write failure that still finalized is a warning, not a failure:
    // the file plays. Only a finalize failure is fatal. On success the
    // caller gets `written_until` — what was actually written — never
    // the wall clock, which it does not have access to from here anyway.
    finalized.map(|()| written_until)
}
