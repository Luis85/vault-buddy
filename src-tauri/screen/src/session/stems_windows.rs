//! The Windows stem writer (Task 53): one named thread, `screen-stems`, the
//! ONLY owner of every stem's audio-only fragmented-MP4 sink — the
//! three-threads rule's reason (`session/mod.rs`): a sink writer is a COM
//! pointer and not `Send`, so the sinks are created, written and finalized
//! on this one thread, fed plain PCM over a bounded channel by the audio
//! thread's `StemTee`.
//!
//! Every decision is made in `stems.rs` (pure, tested on every platform).
//! This file moves bytes between that channel and Media Foundation and
//! EXECUTES IN NO AUTOMATED TEST on any platform — the tutorial editor's
//! Windows checklist rows are its gate.
//!
//! A stem NEVER fails the capture: a sink that cannot be created or written
//! raises one `screen:warning` naming its input, and that stem is dropped —
//! finalized (the only drain; there is no `Flush`) and its part removed, so
//! only COMPLETE stems are ever published. The mixed track holds the same
//! audio, so nothing the user recorded is lost.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread::JoinHandle;

use vault_buddy_core::capture_paths;

use crate::sink::FragmentedSink;

use super::stems::{
    stem_audio_format, stem_failed_warning, StemChunk, StemOutcome, StemParams, StemTarget,
    StemTee, STEM_CHANNEL_DEPTH,
};
use super::Warnings;

/// How one stem ended on the writer thread.
struct Written {
    /// Finalized with every chunk it was handed, and at least one of them.
    complete: bool,
}

/// The stems of one live capture.
pub(crate) struct StemWriter {
    targets: Vec<StemTarget>,
    tee: Option<StemTee>,
    abandoned: Arc<AtomicBool>,
    thread: Option<JoinHandle<Vec<Written>>>,
}

impl StemWriter {
    /// Start the writer. `None` (with a warning) when its thread cannot be
    /// spawned: the capture goes on without stems rather than failing.
    pub(crate) fn start(params: StemParams, warnings: Arc<Warnings>) -> Option<StemWriter> {
        let (tx, rx) = mpsc::sync_channel::<StemChunk>(STEM_CHANNEL_DEPTH);
        let abandoned = Arc::new(AtomicBool::new(false));
        let targets = params.targets;
        let thread = std::thread::Builder::new()
            .name("screen-stems".into())
            .spawn({
                let (targets, warnings) = (targets.clone(), Arc::clone(&warnings));
                move || run_writer(&targets, rx, &warnings)
            });
        match thread {
            Ok(thread) => Some(StemWriter {
                targets,
                tee: Some(StemTee::new(tx, Arc::clone(&abandoned))),
                abandoned,
                thread: Some(thread),
            }),
            Err(e) => {
                log::error!("screen stems: could not start the stem writer: {e}");
                for target in &targets {
                    warnings.raise(stem_failed_warning(&target.input));
                }
                None
            }
        }
    }

    /// The audio thread's end of the channel. Taken once.
    pub(crate) fn take_tee(&mut self) -> Option<StemTee> {
        self.tee.take()
    }

    /// Join the writer (the audio thread has ended, so its channel is
    /// closed), then publish every COMPLETE stem beside the capture and drop
    /// the rest with a warning.
    pub(crate) fn finish(mut self, warnings: &Warnings) -> Vec<StemOutcome> {
        self.tee = None;
        let written = match self.thread.take().map(JoinHandle::join) {
            Some(Ok(written)) => written,
            _ => {
                log::error!("screen stems: the stem writer panicked");
                Vec::new()
            }
        };
        let abandoned = self.abandoned.load(Ordering::Relaxed);
        let mut outcomes = Vec::new();
        for (i, target) in self.targets.iter().enumerate() {
            let complete = !abandoned && written.get(i).is_some_and(|w| w.complete);
            if complete {
                match capture_paths::rename_noreplace(&target.part, &target.staged) {
                    Ok(()) => {
                        outcomes.push(StemOutcome {
                            index: target.index,
                            input: target.input.clone(),
                            file: target.staged.clone(),
                        });
                        continue;
                    }
                    Err(e) => log::warn!("screen stems: could not publish {:?}: {e}", target.part),
                }
            }
            // An incomplete stem is not kept (the module doc): its audio is
            // in the mixed track. The writer already warned for a stem it
            // could not open, write or finalize, and the tee for an abandoned
            // writer; only a failed PUBLISH is still unreported here.
            if complete {
                warnings.raise(stem_failed_warning(&target.input));
            }
            remove_owned_part(&target.part);
        }
        outcomes
    }
}

impl Drop for StemWriter {
    /// A writer dropped without `finish` (an abandoned or failed session)
    /// still finalizes; its parts stay for the recovery sweep.
    fn drop(&mut self) {
        self.tee = None;
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log::error!("screen stems: the stem writer panicked");
            }
        }
    }
}

/// Remove a stem part this capture wrote — a plain file only, never through
/// a link (the no-follow rule every staging delete keeps).
fn remove_owned_part(part: &Path) {
    match std::fs::symlink_metadata(part) {
        Ok(meta) if meta.file_type().is_file() => {
            if let Err(e) = std::fs::remove_file(part) {
                log::warn!("screen stems: could not remove {part:?}: {e}");
            }
        }
        Ok(_) => log::warn!("screen stems: {part:?} is not a plain file; left alone"),
        Err(_) => {} // never created
    }
}

/// The writer thread: create every sink, write each chunk to each stem,
/// finalize them all when the audio thread closes the channel.
fn run_writer(
    targets: &[StemTarget],
    rx: Receiver<StemChunk>,
    warnings: &Warnings,
) -> Vec<Written> {
    let format = stem_audio_format();
    let mut sinks: Vec<Option<FragmentedSink>> = targets
        .iter()
        .map(
            |t| match FragmentedSink::create_audio_only(&t.part, format) {
                Ok(sink) => Some(sink),
                Err(e) => {
                    log::warn!(
                        "screen stems: could not open the stem for {:?}: {e}",
                        t.input
                    );
                    warnings.raise(stem_failed_warning(&t.input));
                    None
                }
            },
        )
        .collect();
    let mut failed: Vec<bool> = sinks.iter().map(Option::is_none).collect();
    let mut chunks = 0u64;
    while let Ok(StemChunk { pcm, ts, dur }) = rx.recv() {
        chunks += 1;
        for (i, samples) in pcm.iter().enumerate() {
            let Some(Some(sink)) = sinks.get_mut(i) else {
                continue;
            };
            if let Err(e) = sink.write_audio(samples, ts, dur) {
                log::warn!(
                    "screen stems: writing the stem for {:?} failed: {e}",
                    targets[i].input
                );
                warnings.raise(stem_failed_warning(&targets[i].input));
                failed[i] = true;
                // Stop writing it, and finalize it now so its file closes:
                // `finish` removes an incomplete stem's part.
                if let Some(Err(e)) = sinks[i].take().map(FragmentedSink::finalize) {
                    log::warn!("screen stems: finalizing a failed stem: {e}");
                }
            }
        }
    }
    sinks
        .into_iter()
        .zip(failed)
        .zip(targets)
        .map(|((sink, failed), target)| {
            let finalized = match sink {
                Some(sink) => match sink.finalize() {
                    Ok(()) => true,
                    Err(e) => {
                        log::warn!("screen stems: finalizing a stem failed: {e}");
                        warnings.raise(stem_failed_warning(&target.input));
                        false
                    }
                },
                None => false,
            };
            Written {
                complete: finalized && !failed && chunks > 0,
            }
        })
        .collect()
}
