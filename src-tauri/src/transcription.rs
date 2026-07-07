//! Local transcription orchestration: the job queue, the single background
//! worker, model download/prepare, and the panel-facing commands. Extracted
//! from capture_commands.rs — the vault-write and never-clobber contracts are
//! unchanged. The worker yields to a live recording (is_recording) and never
//! holds the queue mutex across download/load/inference.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture_commands::{is_recording, now_ms, toast};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::throttle::EmitThrottle;
use vault_buddy_core::{capture_config, capture_paths, discovery};
use vault_buddy_transcribe::engine::WhisperTranscriber;
use vault_buddy_transcribe::model::{download_model, model_path, ModelTier};
use vault_buddy_transcribe::{
    transcribe_recording, CancelToken, TranscribeError, TranscribeOptions, TranscribeOutcome,
};

#[derive(Clone)]
pub(crate) struct TranscriptionJob {
    pub(crate) mp3: PathBuf,
    pub(crate) vault_id: String,
    pub(crate) force: bool,
}

/// A coarse stage for the job currently being processed, surfaced to the UI
/// (Task 4) via `as_str()`. `Downloading` carries live byte counts — it's the
/// only stage with a percentage of its own before inference's 0-100 progress
/// starts.
#[derive(Clone)]
enum Phase {
    Downloading { received: u64, total: Option<u64> },
    Preparing,
    Transcribing,
}
impl Phase {
    fn as_str(&self) -> &'static str {
        match self {
            Phase::Downloading { .. } => "downloading",
            Phase::Preparing => "preparing",
            Phase::Transcribing => "transcribing",
        }
    }
}

/// The job currently being processed, published under the queue mutex so a
/// future cancel command (Task 4) always has a `CancelToken` to flip.
/// `progress` is written lock-free (`Ordering::Relaxed`) from the whisper
/// progress callback in `process_transcription` — that callback must never
/// take the queue mutex, only this atomic plus `app.emit`.
struct ActiveJob {
    mp3: PathBuf,
    vault_id: String,
    cancel: CancelToken,
    started_at_ms: u64,
    phase: Phase,
    progress: Arc<AtomicU8>, // 0..100 inference %, written lock-free from the callback
}

#[derive(Default)]
struct TranscriptionQueue {
    pending: VecDeque<TranscriptionJob>,
    /// The job the worker is presently on; None between jobs and at idle.
    active: Option<ActiveJob>,
}

/// Outcome of an enqueue attempt, so `enqueue_transcription` knows whether to
/// wake the worker (`Queued` only) and what to log.
#[derive(Debug, PartialEq)]
enum Enqueued {
    Queued,
    UpgradedToForce,
    Duplicate,
}

impl TranscriptionQueue {
    /// Add a job unless it duplicates one already queued or in flight. Dedup
    /// is derived from live state (`pending` + `active`) instead of a side set,
    /// so it can never drift out of sync with the real queue. A `force`
    /// (explicit re-transcribe) is NEVER silently dropped: it promotes a queued
    /// plain job, and it queues even while the same path is actively
    /// transcribing — so a cancel→retry landing in that window re-runs
    /// afterwards instead of vanishing. A plain (auto-scan) enqueue still skips
    /// anything already pending or in flight.
    fn enqueue(&mut self, job: TranscriptionJob) -> Enqueued {
        let active_same = self
            .active
            .as_ref()
            .map(|a| a.mp3 == job.mp3)
            .unwrap_or(false);
        if job.force {
            if let Some(existing) = self.pending.iter_mut().find(|j| j.mp3 == job.mp3) {
                if existing.force {
                    return Enqueued::Duplicate;
                }
                existing.force = true; // promote a queued plain job
                return Enqueued::UpgradedToForce;
            }
            self.pending.push_back(job); // queue even if active_same → re-runs after
            return Enqueued::Queued;
        }
        if active_same || self.pending.iter().any(|j| j.mp3 == job.mp3) {
            return Enqueued::Duplicate; // plain auto-scan: skip if pending or in flight
        }
        self.pending.push_back(job);
        Enqueued::Queued
    }
}

/// Background transcription queue. One worker (see `run_transcription`)
/// drains it, yielding to any active recording so inference never steals
/// CPU from live capture.
#[derive(Default)]
pub struct TranscriptionState {
    inner: Mutex<TranscriptionQueue>,
    cv: Condvar,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ActiveJobDto {
    mp3: String,
    vault_id: String,
    phase: String,
    progress: u8,
    received: Option<u64>,
    total: Option<u64>,
    started_at_ms: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct QueuedDto {
    mp3: String,
    vault_id: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionQueueDto {
    active: Option<ActiveJobDto>,
    queued: Vec<QueuedDto>,
    waiting_for_recording: bool,
}

pub(crate) fn enqueue_transcription(app: &AppHandle, job: TranscriptionJob) {
    let state = app.state::<TranscriptionState>();
    let mut guard = lock_ignoring_poison(&state.inner);
    // Keep the path for logging — `enqueue` consumes the job.
    let mp3 = job.mp3.clone();
    match guard.enqueue(job) {
        Enqueued::Queued => {
            log::info!("transcribe: queued {}", mp3.display());
            state.cv.notify_all();
        }
        // Already pending as a plain job — the worker will reach it, so no
        // notify is needed; just record the promotion.
        Enqueued::UpgradedToForce => {
            log::info!("transcribe: upgraded queued job to force {}", mp3.display());
        }
        Enqueued::Duplicate => {}
    }
}

/// Enqueue every capture recording still needing a transcript, across all
/// vaults that opted in. Same root discipline as `run_recovery`.
fn scan_and_enqueue(app: &AppHandle) {
    let cfg = capture_config::load_config();
    for vault in discovery::discover_vaults() {
        let v = capture_config::vault_config(&cfg, &vault.id);
        if !v.transcribe {
            continue;
        }
        for folder in v.recording_roots() {
            let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), folder)
            else {
                continue;
            };
            if !root.is_dir() {
                continue;
            }
            if capture_paths::assert_root_inside_vault(Path::new(&vault.path), &root).is_err() {
                continue;
            }
            for mp3 in vault_buddy_core::transcript::pending_transcriptions(&root) {
                enqueue_transcription(
                    app,
                    TranscriptionJob {
                        mp3,
                        vault_id: vault.id.clone(),
                        force: false,
                    },
                );
            }
        }
    }
}

/// Lock `TranscriptionState` just long enough to set the active job's phase
/// (a no-op if the queue is idle, e.g. a cancel raced this) — the ~3×
/// repeated lock/set/unlock block behind `Phase::Downloading`/`Preparing`/
/// `Transcribing`. Dedupe only: the phase-change event itself is still
/// emitted by each caller, right after this returns, never under the lock.
fn set_phase(app: &AppHandle, phase: Phase) {
    let state = app.state::<TranscriptionState>();
    let mut guard = lock_ignoring_poison(&state.inner);
    if let Some(active) = guard.active.as_mut() {
        active.phase = phase;
    }
}

fn process_transcription(
    app: &AppHandle,
    job: &TranscriptionJob,
    loaded: &mut Option<(ModelTier, WhisperTranscriber)>,
) {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &job.vault_id);
    // A forced (explicit) re-transcribe ignores the vault's auto-transcribe
    // setting; the automatic path still bails when disabled.
    if !cfg.transcribe && !job.force {
        return;
    }
    let tier = ModelTier::from_str(&cfg.transcription_model);

    // Publish the active job BEFORE any observable work starts, so a future
    // cancel command (Task 4) always has a token to flip. The mutex hold is
    // brief — just the insert — never across the download/model-load/
    // inference that follows. `cancel`/`progress` are kept as locals too
    // (not re-read from `active` later) — they share state with the clones
    // stored below via CancelToken's/Arc's Clone, so either handle works.
    let started_at_ms = now_ms();
    let cancel = CancelToken::new();
    let progress = Arc::new(AtomicU8::new(0));
    {
        let state = app.state::<TranscriptionState>();
        let mut guard = lock_ignoring_poison(&state.inner);
        guard.active = Some(ActiveJob {
            mp3: job.mp3.clone(),
            vault_id: job.vault_id.clone(),
            cancel: cancel.clone(),
            started_at_ms,
            phase: Phase::Preparing,
            progress: progress.clone(),
        });
    }
    let _ = app.emit(
        "capture:transcribing",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy(), "vaultId": job.vault_id }),
    );
    if job.force {
        // Reflect the in-flight regeneration in the note embed by swapping our
        // own regenerable sidecar for the "transcribing…" placeholder — but
        // NEVER overwrite a Complete/hand-edited transcript up-front. If this
        // forced job then fails, the original must survive: fail_transcription
        // writes via replace_if_ours, which skips a non-regenerable sidecar, so
        // leaving it untouched means a failed re-transcribe can't destroy it.
        // On success, transcribe_recording's force_write_sidecar swaps the
        // finished transcript for the freshly generated one.
        if vault_buddy_core::transcript::transcript_status(&job.mp3)
            != vault_buddy_core::transcript::TranscriptStatus::Complete
        {
            let name = job
                .mp3
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Err(e) = vault_buddy_core::transcript::force_write_sidecar(
                &vault_buddy_core::transcript::transcript_path(&job.mp3),
                &vault_buddy_core::transcript::render_placeholder(&name),
            ) {
                log::warn!(
                    "transcribe: writing placeholder for {} failed: {e}",
                    job.mp3.display()
                );
            }
        }
    } else if let Err(e) = vault_buddy_core::transcript::write_placeholder(&job.mp3) {
        log::warn!(
            "transcribe: writing placeholder for {} failed: {e}",
            job.mp3.display()
        );
    }

    let model = match ensure_model(app, tier, &job.mp3, &cancel) {
        Ok(p) => p,
        Err(e) => {
            // A cancel during download returns Err too — the token says which.
            // A user cancel is a cancellation, not a failure.
            if cancel.is_cancelled() {
                return emit_cancelled(app, &job.mp3);
            }
            return fail_transcription(app, &job.mp3, &format!("model unavailable: {e}"));
        }
    };
    // Handover: the model is on disk now (just downloaded, or already
    // present) — replace the download row with "preparing" BEFORE the
    // model-load gap below, so a download UI can never stick at 100%.
    set_phase(app, Phase::Preparing);
    let _ = app.emit(
        "capture:modelReady",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy() }),
    );

    // A cancel that landed during download/prepare: honor it before the
    // (uninterruptible, multi-second) model load rather than after it.
    if cancel.is_cancelled() {
        return emit_cancelled(app, &job.mp3);
    }
    if loaded.as_ref().map(|(t, _)| *t) != Some(tier) {
        match WhisperTranscriber::load(&model) {
            Ok(w) => *loaded = Some((tier, w)),
            Err(e) => {
                // A model that downloaded but won't load is corrupt on disk;
                // ensure_model returns early on dest.exists(), so leaving it
                // means every future job reloads the same broken file and
                // fails identically. Discard it so the next attempt
                // re-downloads. A removal failure only costs the self-heal
                // (the load error is still surfaced), but it must not be
                // swallowed silently.
                if let Err(rm) = vault_buddy_transcribe::model::remove_model(tier) {
                    log::warn!(
                        "failed to remove corrupt {} model after load error: {rm}",
                        tier.label()
                    );
                }
                return fail_transcription(app, &job.mp3, &e);
            }
        }
    }
    let transcriber = &loaded.as_ref().unwrap().1;
    let opts = TranscribeOptions {
        language: cfg.transcription_language.clone(),
        timestamps: cfg.transcript_timestamps,
        model_label: tier.label(),
    };
    let generated_at = chrono::Local::now().to_rfc3339();

    set_phase(app, Phase::Transcribing);
    let _ = app.emit(
        "capture:transcribeProgress",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy(), "progress": 0 }),
    );
    let app_cb = app.clone();
    let mp3_cb = job.mp3.clone();
    let mut emit_throttle = EmitThrottle::new(5);
    let mut log_throttle = EmitThrottle::new(25);
    let on_progress: Box<dyn FnMut(i32) + Send> = Box::new(move |p| {
        let p = p.clamp(0, 100);
        progress.store(p as u8, Ordering::Relaxed); // lock-free, no queue mutex
        let terminal = p >= 100;
        if emit_throttle.should_emit(p as u64, terminal) {
            let _ = app_cb.emit(
                "capture:transcribeProgress",
                serde_json::json!({ "mp3": mp3_cb.to_string_lossy(), "progress": p }),
            );
        }
        if log_throttle.should_emit(p as u64, terminal) {
            log::info!("transcribe: {} inference {}%", mp3_cb.display(), p);
        }
    });
    // (inference start/elapsed with audio length is logged inside
    // transcribe_recording — Task 2 — which owns the samples.)
    let result = transcribe_recording(
        &job.mp3,
        transcriber,
        &opts,
        &generated_at,
        job.force,
        &cancel,
        on_progress,
    );
    match result {
        Ok(TranscribeOutcome::Written(path)) => {
            log::info!("transcribe: wrote {}", path.display());
            let _ = app.emit(
                "capture:transcribed",
                serde_json::json!({
                    "mp3": job.mp3.to_string_lossy(),
                    "transcript": path.to_string_lossy(),
                }),
            );
        }
        Ok(TranscribeOutcome::SkippedForeign(_)) => {
            // Decode + inference succeeded, but transcribe_recording's
            // replace_if_ours refused to clobber a complete/hand-edited
            // sidecar — a distinct honest signal, not the same "success" as
            // capture:transcribed, so the UI can tell the two apart.
            // (transcribe_recording already logs this fact via log::warn! —
            // no need to log it again here.)
            let _ = app.emit(
                "capture:transcribeSkipped",
                serde_json::json!({
                    "mp3": job.mp3.to_string_lossy(),
                    "message": "kept your existing transcript — not overwritten",
                }),
            );
        }
        Err(TranscribeError::Failed(e)) => fail_transcription(app, &job.mp3, &e),
        Err(TranscribeError::Cancelled) => emit_cancelled(app, &job.mp3),
    }
}

/// Ensure the tier's model is on disk, downloading with progress if not.
/// `mp3` identifies the job in the download-progress event and in the
/// `active` phase this sets while a download is running (Task 4 reads
/// both) — it names nothing else here.
fn ensure_model(
    app: &AppHandle,
    tier: ModelTier,
    mp3: &Path,
    cancel: &CancelToken,
) -> Result<PathBuf, String> {
    if let Some(p) = model_path(tier) {
        if p.exists() {
            return Ok(p);
        }
    }
    log::info!("transcribe: downloading model {}", tier.as_str());
    let app = app.clone();
    let mp3 = mp3.to_path_buf();
    let mut throttle = EmitThrottle::new(4_000_000);
    download_model(tier, cancel, &mut |received, total| {
        if throttle.should_emit(received, Some(received) == total) {
            // Phase update is brief (one field write, via set_phase); the
            // emit itself happens after its guard drops internally.
            set_phase(&app, Phase::Downloading { received, total });
            let _ = app.emit(
                "capture:modelDownload",
                serde_json::json!({
                    "mp3": mp3.to_string_lossy(),
                    "model": tier.as_str(),
                    "received": received,
                    "total": total,
                }),
            );
        }
    })
}

/// Terminal bookkeeping for a cancelled transcription — shared by the
/// worker's `TranscribeError::Cancelled` arm and the pre-inference cancel
/// checks (a cancel during download or model-prepare). Replaces only OUR own
/// regenerable sidecar (`replace_if_ours` never clobbers a complete/hand-
/// edited transcript) with a `cancelled` note, and emits the terminal event.
fn emit_cancelled(app: &AppHandle, mp3: &Path) {
    let name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Err(e) = vault_buddy_core::transcript::replace_if_ours(
        &vault_buddy_core::transcript::transcript_path(mp3),
        &vault_buddy_core::transcript::render_cancelled(&name),
    ) {
        log::warn!(
            "transcribe: writing cancelled sidecar for {} failed: {e}",
            mp3.display()
        );
    }
    let _ = app.emit(
        "capture:transcribeCancelled",
        serde_json::json!({ "mp3": mp3.to_string_lossy() }),
    );
    log::info!("transcribe: cancelled {}", mp3.display());
}

/// Best-effort failure: leave the audio + note untouched, replace the
/// sidecar with a retryable `failed` note, and surface it.
fn fail_transcription(app: &AppHandle, mp3: &Path, message: &str) {
    log::warn!("transcribe: {} failed: {message}", mp3.display());
    let name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = vault_buddy_core::transcript::transcript_path(mp3);
    let content = vault_buddy_core::transcript::render_error(&name, message);
    if let Err(e) = vault_buddy_core::transcript::replace_if_ours(&path, &content) {
        log::warn!(
            "transcribe: writing failed sidecar for {} failed: {e}",
            mp3.display()
        );
    }
    let _ = app.emit(
        "capture:transcribeFailed",
        serde_json::json!({ "mp3": mp3.to_string_lossy(), "message": message }),
    );
    toast(app, "Transcription failed", message);
}

/// The vault whose folder contains `mp3` (for the retry command).
fn owning_vault_id(mp3: &Path) -> Option<String> {
    discovery::discover_vaults()
        .into_iter()
        .find(|v| mp3.starts_with(&v.path))
        .map(|v| v.id)
}

/// Run one job body, converting a panic into a `false` so the worker loop can
/// fail just that job and keep going. Mirrors the `catch_unwind` guard lib.rs
/// uses around the metronome tick; the build is not `panic=abort` and the
/// panic hook only logs, so the unwind is catchable on this worker thread.
fn catch_job<F: FnOnce()>(f: F) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_ok()
}

/// Startup + on-demand worker: drains the transcription queue, postponing
/// while a recording is active. The loaded whisper model is cached across
/// jobs of the same tier. Mirrors `run_recovery`'s shape (own thread, coarse
/// is-recording gate).
pub fn run_transcription(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("transcribe-worker".into())
        .spawn(move || {
            // Route whisper.cpp/ggml native logs into our log files before any
            // model is loaded — they default to stderr, which a windowed build
            // discards, so this is where an inference failure's real detail was
            // being lost. Once, up front; re-installing the hook is harmless.
            vault_buddy_transcribe::engine::install_logging_hooks();
            // Backfill: transcribe anything already on disk missing a transcript
            // (previous-session saves, crash-recovered captures, freshly enabled
            // vaults).
            scan_and_enqueue(&app);
            let mut loaded: Option<(ModelTier, WhisperTranscriber)> = None;
            loop {
                // Wait until a job is present, but only PEEK: the recording gate
                // below may leave it queued, and popping before that gate would
                // drop the job (or a force upgrade that lands between the peek
                // and the pop). We claim it only once we've decided to run it.
                {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = lock_ignoring_poison(&state.inner);
                    while guard.pending.is_empty() {
                        // The Condvar guard is poisonable too — recover it the
                        // same way `lock_ignoring_poison` recovers the mutex, so
                        // a panic elsewhere can't wedge the worker permanently on
                        // a poisoned wait.
                        guard = state.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
                    }
                }
                // Never contend with a live recording for CPU — re-check soon.
                if is_recording(&app) {
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                }
                // Claim the front under the lock and process THAT value — a
                // force upgrade that landed after the peek is already reflected
                // in the job we pop here (dedup derives from live pending +
                // active, so it can't drift). `None` means the queue emptied in
                // the unlocked gap between the peek above and this pop — only
                // `cancel_transcription`'s `pending.retain` can do that (this is
                // the sole popper), e.g. cancelling the last queued job — so
                // just loop back.
                let job = {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = lock_ignoring_poison(&state.inner);
                    guard.pending.pop_front()
                };
                let Some(job) = job else { continue };
                let completed = catch_job(|| process_transcription(&app, &job, &mut loaded));
                if !completed {
                    // The job panicked. Fail just this recording, drop the
                    // cached model (it may be mid-load / inconsistent), and let
                    // the loop continue — one bad job must not stop the worker.
                    log::error!("transcribe: worker caught a panic on {}", job.mp3.display());
                    loaded = None;
                    fail_transcription(&app, &job.mp3, "internal error during transcription");
                }
                // Clear the active-job slot after every `process_transcription`
                // return path (success, failure, or an early-return on a bad
                // model/load) — it's the one place that needs to. Dedup now
                // derives from live state (`pending` + `active`): success leaves
                // a `complete` sidecar (won't rescan), failure a `failed` one (a
                // later launch's scan or a manual retry re-queues it), so
                // dropping `active` here is all the cleanup the dedup needs.
                {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = lock_ignoring_poison(&state.inner);
                    guard.active = None;
                }
            }
        })
        .expect("failed to spawn transcribe-worker thread");
}

/// Retry / on-demand transcription of a specific recording.
#[tauri::command]
pub fn transcribe_recording_now(app: AppHandle, path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    if !mp3.is_file() {
        return Err("Recording not found.".to_string());
    }
    let vault_id = owning_vault_id(&mp3).ok_or("Recording is not inside a known vault.")?;
    enqueue_transcription(
        &app,
        TranscriptionJob {
            mp3,
            vault_id,
            force: false,
        },
    );
    Ok(())
}

/// Explicit, forced re-transcription of a specific recording: regenerates even
/// a finished transcript and ignores the vault's auto-transcribe setting.
#[tauri::command]
pub fn retranscribe(app: AppHandle, path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    if !mp3.is_file() {
        return Err("Recording not found.".to_string());
    }
    let vault_id = owning_vault_id(&mp3).ok_or("Recording is not inside a known vault.")?;
    enqueue_transcription(
        &app,
        TranscriptionJob {
            mp3,
            vault_id,
            force: true,
        },
    );
    Ok(())
}

/// Cancel a queued or in-flight transcription. The queue mutex is held only
/// for bookkeeping (flip the active job's `CancelToken`, or drop a pending
/// job) — NEVER across the sidecar write below, which does a temp+fsync+
/// rename (`replace_if_ours`) and would otherwise stall every other
/// command that needs the same mutex (enqueue, status, a concurrent cancel)
/// for the duration of a disk flush.
///
/// The active job's sidecar is deliberately NOT written here: cancelling it
/// only flips the token, and the worker's `TranscribeError::Cancelled` arm
/// (in `process_transcription`) owns that write via `replace_if_ours`, which
/// already refuses to clobber a finished/hand-edited transcript. A pending
/// job's sidecar is NOT guaranteed to be our own `pending` placeholder or
/// absent: `retranscribe` pushes straight into the queue via
/// `enqueue_transcription` with no up-front placeholder write, so a queued
/// forced re-transcribe of an already-`Complete` (or hand-edited) recording
/// still has that original on disk while pending. The write below therefore
/// uses the same never-clobber `replace_if_ours` as the worker's arm above,
/// not the unguarded `force_write_sidecar` — and duplicating the active
/// job's write here (instead of leaving it to the worker) would still be
/// wrong, since it would race the worker's own write to the same path.
#[tauri::command]
pub fn cancel_transcription(app: AppHandle, path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    // Phase 1: fast bookkeeping under the mutex; decide what to write after.
    let write_cancelled = {
        let state = app.state::<TranscriptionState>();
        let mut guard = lock_ignoring_poison(&state.inner);
        if guard.active.as_ref().map(|a| a.mp3 == mp3).unwrap_or(false) {
            guard.active.as_ref().unwrap().cancel.cancel(); // aborts inference; the worker writes the cancelled sidecar
            return Ok(()); // worker owns the terminal bookkeeping for the active job
        }
        // Pending job: drop it now; write its sidecar AFTER releasing the lock.
        let before = guard.pending.len();
        guard.pending.retain(|j| j.mp3 != mp3);
        if guard.pending.len() == before {
            return Err("No such transcription in the queue.".into());
        }
        // `pending.retain` above already dropped the job; dedup derives from
        // live state now, so there is no separate set to prune.
        true
    }; // <-- mutex released here
    if write_cancelled {
        let name = mp3
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Err(e) = vault_buddy_core::transcript::replace_if_ours(
            &vault_buddy_core::transcript::transcript_path(&mp3),
            &vault_buddy_core::transcript::render_cancelled(&name),
        ) {
            log::warn!(
                "transcribe: writing cancelled sidecar for {} failed: {e}",
                mp3.display()
            );
        }
        let _ = app.emit(
            "capture:transcribeCancelled",
            serde_json::json!({ "mp3": mp3.to_string_lossy() }),
        );
    }
    Ok(())
}

/// Live snapshot of the transcription queue for the Recordings panel: the
/// job currently running (phase/progress, plus download byte counts while
/// `Phase::Downloading`), everything still waiting, and whether the queue is
/// stalled behind a live recording (`is_recording` — the same coarse gate
/// `run_transcription`'s worker loop yields to). Read-only: the queue mutex
/// is held only long enough to clone/copy fields out of it.
#[tauri::command]
pub fn transcription_queue_status(app: AppHandle) -> TranscriptionQueueDto {
    let state = app.state::<TranscriptionState>();
    let guard = lock_ignoring_poison(&state.inner);
    let active = guard.active.as_ref().map(|a| {
        let (received, total) = match a.phase {
            Phase::Downloading { received, total } => (Some(received), total),
            _ => (None, None),
        };
        ActiveJobDto {
            mp3: a.mp3.to_string_lossy().into_owned(),
            vault_id: a.vault_id.clone(),
            phase: a.phase.as_str().to_string(),
            progress: a.progress.load(Ordering::Relaxed),
            received,
            total,
            started_at_ms: a.started_at_ms,
        }
    });
    let queued = guard
        .pending
        .iter()
        .map(|j| QueuedDto {
            mp3: j.mp3.to_string_lossy().into_owned(),
            vault_id: j.vault_id.clone(),
        })
        .collect();
    // "waiting" = there is work but nothing active because a recording is
    // live. Snapshot that before dropping the guard: is_recording() locks
    // CaptureState, and TranscriptionState must never be held across another
    // domain's lock (mirrors run_recovery's discipline for CaptureState vs
    // the log write below it).
    let stalled = active.is_none() && !guard.pending.is_empty();
    drop(guard);
    let waiting_for_recording = stalled && is_recording(&app);
    TranscriptionQueueDto {
        active,
        queued,
        waiting_for_recording,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(path: &str, force: bool) -> TranscriptionJob {
        TranscriptionJob {
            mp3: PathBuf::from(path),
            vault_id: "v".to_string(),
            force,
        }
    }

    // Minimal in-flight job for the dedup tests — only `mp3` is read by
    // `enqueue`; the rest are inert placeholders the struct requires.
    fn active_job(path: &str) -> ActiveJob {
        ActiveJob {
            mp3: PathBuf::from(path),
            vault_id: "v".to_string(),
            cancel: CancelToken::new(),
            started_at_ms: 0,
            phase: Phase::Preparing,
            progress: Arc::new(AtomicU8::new(0)),
        }
    }

    #[test]
    fn catch_job_survives_a_panicking_job() {
        // Regression: a panic in process_transcription must NOT propagate out of
        // the worker loop (that silently stops all future transcriptions). The
        // seam reports the panic so the loop can fail just that job and continue.
        assert!(super::catch_job(|| {}), "a normal job reports completed");
        assert!(
            !super::catch_job(|| panic!("boom")),
            "a panicking job is caught and reported, not propagated"
        );
    }

    #[test]
    fn plain_enqueue_dedupes_when_already_pending() {
        let mut q = TranscriptionQueue::default();
        assert_eq!(q.enqueue(job("X", false)), Enqueued::Queued);
        assert_eq!(q.pending.len(), 1);
        assert_eq!(q.pending[0].mp3, PathBuf::from("X"));
        assert_eq!(q.enqueue(job("X", false)), Enqueued::Duplicate);
        assert_eq!(q.pending.len(), 1);
    }

    #[test]
    fn force_upgrades_a_pending_plain_job() {
        let mut q = TranscriptionQueue::default();
        assert_eq!(q.enqueue(job("X", false)), Enqueued::Queued);
        assert_eq!(q.enqueue(job("X", true)), Enqueued::UpgradedToForce);
        assert_eq!(q.pending.len(), 1);
        assert!(q.pending[0].force);
    }

    #[test]
    fn force_is_queued_even_while_path_is_actively_transcribing() {
        let mut q = TranscriptionQueue {
            active: Some(active_job("X")),
            ..Default::default()
        };
        assert_eq!(q.enqueue(job("X", true)), Enqueued::Queued);
        assert_eq!(q.pending.len(), 1);
        assert!(q.pending[0].force);
        assert_eq!(q.pending[0].mp3, PathBuf::from("X"));
    }

    #[test]
    fn plain_enqueue_is_dropped_while_path_is_active() {
        let mut q = TranscriptionQueue {
            active: Some(active_job("X")),
            ..Default::default()
        };
        assert_eq!(q.enqueue(job("X", false)), Enqueued::Duplicate);
        assert!(q.pending.is_empty());
    }
}
