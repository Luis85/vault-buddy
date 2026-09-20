//! The audio thread: drain every opened source, downmix, resample, mix,
//! and hand batched chunks to the mux.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::time::Duration;

use vault_buddy_capture::mixer;
use vault_buddy_capture::session::{SourceInput, SourceMsg};

use super::{
    output_ts, pacing, MuxMsg, SharedClock, Warnings, AUDIO_CHUNK_FRAMES, AUDIO_RATE,
    AUDIO_STALL_CAP,
};

/// Report a source's loss exactly once, however it was noticed.
fn lose(alive: &mut [bool], i: usize, name: &str, warnings: &Warnings) {
    if alive.get(i).copied().unwrap_or(false) {
        alive[i] = false;
        warnings.raise(format!(
            "the audio source \"{name}\" was lost — the rest of the recording continues \
             without it"
        ));
    }
}

/// Drain every source, downmix to mono, resample to `AUDIO_RATE`, mix to
/// stereo, and hand batched chunks to the mux.
///
/// While paused the drained samples are DISCARDED (spec 6.3): the
/// streams stay open so resume is instant, but paused wall-clock time
/// never appears in the output.
///
/// It runs even with ZERO sources — `take_frames` then never produces a
/// round, so the capture is silently silent — because this thread owns
/// the last `MuxMsg` sender and its exit is what disconnects the mux.
pub(super) fn run_audio(
    sources: Vec<SourceInput>,
    clock: SharedClock,
    tx: SyncSender<MuxMsg>,
    stopping: Arc<AtomicBool>,
    warnings: Arc<Warnings>,
) {
    // One growing buffer per source; they arrive at different rates and
    // must be mixed frame-aligned.
    let mut buffers: Vec<Vec<f32>> = vec![Vec::new(); sources.len()];
    let mut alive: Vec<bool> = vec![true; sources.len()];
    let mut pacer = pacing::AudioPacer::new(AUDIO_RATE);
    // The pause EDGE latch. Without it every paused iteration flushed, so
    // audio captured DURING the pause was written and the pause did not
    // pause the audio track — see `pacing::pause_flush_take`.
    let mut was_paused = false;

    while !stopping.load(Ordering::Relaxed) {
        let mut got_anything = false;
        for (i, src) in sources.iter().enumerate() {
            loop {
                match src.rx.try_recv() {
                    Ok(SourceMsg::Samples(raw)) => {
                        got_anything = true;
                        let mono = mixer::downmix_to_mono(&raw, src.channels);
                        let at_rate = mixer::resample_linear(&mono, src.rate, AUDIO_RATE);
                        buffers[i].extend_from_slice(&at_rate);
                    }
                    // The audio domain's posture: one device dropping out
                    // never stops the capture (spec 14).
                    Ok(SourceMsg::Lost) => {
                        got_anything = true;
                        lose(&mut alive, i, &src.name, &warnings);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    // A stream torn down WITHOUT sending Lost is the same
                    // outcome for the user — silence from that device for
                    // the rest of the recording — so it must not be the
                    // one path that says nothing.
                    Err(mpsc::TryRecvError::Disconnected) => {
                        lose(&mut alive, i, &src.name, &warnings);
                        break;
                    }
                }
            }
        }

        // While paused, throw away whatever arrives DURING the pause
        // rather than buffering it: buffered paused audio would surface as
        // a burst on resume, and its frames would push the sample-count
        // timeline past the video's. But anything captured BEFORE the
        // pause began is real audio that already happened, and must be
        // written out — see `pacing::pause_flush_frames`'s doc comment for
        // the ~85 ms-per-pause regression this flush prevents. It goes
        // through the SAME `AudioPacer::take` as the ordinary batching
        // path below, so `emitted` — and every timestamp after it —
        // stays consistent with what was actually written.
        //
        // That flush is EDGE-TRIGGERED (`pause_flush_take`, which owns the
        // latch and its own regression note): flushing on every paused
        // iteration wrote the paused audio it exists to discard.
        let paused = output_ts(&clock).is_none();
        let mut idle_paused = false;
        if paused {
            let lens: Vec<usize> = buffers.iter().map(|b| b.len()).collect();
            // The EDGE only. A later paused iteration takes 0 — its buffers
            // hold audio captured DURING the pause, which is discarded.
            let take = pacing::pause_flush_take(paused, was_paused, &lens);
            // Nothing to write and nothing to wait for: without this the
            // drain kept reporting work, `got_anything` stayed true, and the
            // thread busy-spun for the whole pause instead of sleeping.
            idle_paused = take == 0;
            if take > 0 {
                let slices: Vec<&[f32]> = buffers.iter().map(|b| &b[..take.min(b.len())]).collect();
                let stereo = mixer::mix_n_to_stereo_i16(&slices);
                if let Some((ts, dur)) = pacer.take(take) {
                    if tx
                        .send(MuxMsg::Audio {
                            pcm: stereo,
                            ts,
                            dur,
                        })
                        .is_err()
                    {
                        break; // the mux is gone; nothing left to write to
                    }
                }
            }
            for b in &mut buffers {
                b.clear();
            }
        } else {
            let lens: Vec<usize> = buffers.iter().map(|b| b.len()).collect();
            let take = pacing::take_frames(&lens, AUDIO_CHUNK_FRAMES, AUDIO_STALL_CAP);
            if take > 0 {
                let slices: Vec<&[f32]> = buffers.iter().map(|b| &b[..take.min(b.len())]).collect();
                // No per-source gain normalisation (spec 6.5): dividing
                // by N would change existing two-source meeting levels.
                let stereo = mixer::mix_n_to_stereo_i16(&slices);
                for b in &mut buffers {
                    b.drain(..take.min(b.len()));
                }
                // take is the FRAME count; `stereo` is interleaved, so
                // the pacer is advanced by frames, never by sample count.
                if let Some((ts, dur)) = pacer.take(take) {
                    if tx
                        .send(MuxMsg::Audio {
                            pcm: stereo,
                            ts,
                            dur,
                        })
                        .is_err()
                    {
                        break; // the mux is gone; nothing left to write to
                    }
                }
            }
        }

        was_paused = paused;

        if !got_anything || idle_paused {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
