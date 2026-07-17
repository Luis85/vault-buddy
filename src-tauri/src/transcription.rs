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
use vault_buddy_transcribe::model::{
    download_model, download_vad_model, model_path, vad_model_path, ModelTier,
};
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
    /// Set when an explicit force re-transcribe was requested for this same
    /// path while it was already running (`enqueue`'s `WillRerunAfterActive`
    /// case, instead of pushing a second `pending` entry for the same mp3).
    /// `finish_active` requeues it once this run ends; `cancel` clears it, so
    /// cancelling truly stops all pending work on the path rather than
    /// leaving a duplicate to silently restart right after (the bug a Codex
    /// review caught: cancelling the active job left an already-queued
    /// duplicate for the same path untouched).
    rerun_force: bool,
}

#[derive(Default)]
struct TranscriptionQueue {
    pending: VecDeque<TranscriptionJob>,
    /// The job the worker is presently on; None between jobs and at idle.
    active: Option<ActiveJob>,
    /// A one-shot request (artifact id) for the worker to drop its cached
    /// transcriber before the delete command unlinks the model file —
    /// whisper.cpp mmaps the model, and Windows refuses to delete a mapped
    /// file, so an idle worker's cache would otherwise block deletion
    /// forever. Latest-wins on overwrite (see the test).
    pending_purge: Option<String>,
}

/// Outcome of an enqueue attempt, so `enqueue_transcription` knows whether to
/// wake the worker (`Queued` only) and what to log.
#[derive(Debug, PartialEq)]
enum Enqueued {
    Queued,
    UpgradedToForce,
    /// A force re-transcribe was requested for a path already running — no
    /// new entry was queued; the active job was marked to rerun itself once
    /// it finishes (see `finish_active`).
    WillRerunAfterActive,
    Duplicate,
}

/// Outcome of a cancel request against the queue, so `cancel_transcription`
/// knows which sidecar write (if any) is its own responsibility.
#[derive(Debug, PartialEq)]
enum CancelOutcome {
    /// The active job for this path had its token flipped (and any pending
    /// rerun request cleared) — the worker's own `TranscribeError::Cancelled`
    /// arm owns the sidecar write, not the caller.
    CancelledActive,
    /// A pending (not yet started) job for this path was dropped — the
    /// caller owns writing the cancelled sidecar.
    RemovedPending,
    NotFound,
}

impl TranscriptionQueue {
    /// Add a job unless it duplicates one already queued or in flight. Dedup
    /// is derived from live state (`pending` + `active`) instead of a side set,
    /// so it can never drift out of sync with the real queue. A `force`
    /// (explicit re-transcribe) is NEVER silently dropped: it promotes a queued
    /// plain job; if the same path is already running, it marks the ACTIVE job
    /// to rerun itself once it finishes (`WillRerunAfterActive`) instead of
    /// pushing a second `pending` entry for that path — `transcription_queue_status`
    /// would otherwise report one path as both `active` and `queued` (the
    /// frontend store keys jobs by mp3, so the queued seed silently overwrote
    /// the active phase), and `cancel_transcription`'s active-first check would
    /// never even look at that duplicate, so a cancel wouldn't stop it (a
    /// Codex review caught exactly this). A plain (auto-scan) enqueue still
    /// skips anything already pending or in flight.
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
            if active_same {
                if let Some(active) = self.active.as_mut() {
                    active.rerun_force = true;
                }
                return Enqueued::WillRerunAfterActive;
            }
            self.pending.push_back(job);
            return Enqueued::Queued;
        }
        if active_same || self.pending.iter().any(|j| j.mp3 == job.mp3) {
            return Enqueued::Duplicate; // plain auto-scan: skip if pending or in flight
        }
        self.pending.push_back(job);
        Enqueued::Queued
    }

    /// True when `mp3` is the job the worker is currently processing. The
    /// rename command uses this to refuse renaming a file mid-transcription
    /// (Codex PR #46): the worker holds this exact path open for decode and
    /// its terminal write (sidecar) targets it, so a rename underneath it
    /// would strand that write under the old name.
    fn is_active(&self, mp3: &Path) -> bool {
        self.active.as_ref().map(|a| a.mp3.as_path()) == Some(mp3)
    }

    /// Retarget a still-PENDING (not yet started) job's mp3 path in place —
    /// used after a successful `rename_capture` moves the file out from
    /// under a queued job, so the worker eventually writes its terminal
    /// sidecar under the name the renamed note actually embeds (Codex PR
    /// #46: leaving the queue keyed to the old path meant the worker
    /// completed into a sidecar the note no longer pointed at, and the
    /// renamed note's embedded placeholder stayed pending forever). Returns
    /// whether a pending job matched `old` — a miss (already started,
    /// already finished, or never queued) is not an error, just nothing to
    /// do. Never touches `active`: an in-flight job must be refused by the
    /// caller via `is_active` before this runs, not silently retargeted
    /// mid-decode.
    fn retarget_pending(&mut self, old: &Path, new: PathBuf) -> bool {
        // At most one pending entry per path by construction (`enqueue`
        // dedups against live pending+active), so the first match is THE
        // match — renaming in place can never leave a stale duplicate
        // behind or collide with another pending entry for `new`.
        if let Some(job) = self.pending.iter_mut().find(|j| j.mp3 == old) {
            job.mp3 = new;
            true
        } else {
            false
        }
    }

    /// Clear the active slot after a job finishes (success/fail/panic) — the
    /// worker's one call, right after `process_transcription` returns.
    /// Requeues the path as a fresh `force` job when a rerun was requested
    /// while it ran (`enqueue`'s `WillRerunAfterActive`), returning that job so
    /// the caller can log it; `None` for a normal finish, an idle queue, or a
    /// rerun request that `cancel` cleared in the meantime.
    fn finish_active(&mut self) -> Option<TranscriptionJob> {
        let active = self.active.take()?;
        if !active.rerun_force {
            return None;
        }
        let job = TranscriptionJob {
            mp3: active.mp3,
            vault_id: active.vault_id,
            force: true,
        };
        self.pending.push_back(job.clone());
        Some(job)
    }

    /// Cancel whatever is queued or running for `mp3`. Checks `active` first:
    /// if it's the same path, flip its token AND clear `rerun_force` — a
    /// cancel must stop ALL pending work on the path, not just the current
    /// run, or a force re-transcribe requested while it ran would silently
    /// restart right after `finish_active` (the Codex-caught bug). Otherwise
    /// drop a matching pending job.
    fn cancel(&mut self, mp3: &Path) -> CancelOutcome {
        if let Some(active) = self.active.as_mut() {
            if active.mp3 == mp3 {
                active.cancel.cancel(); // aborts inference; the worker writes the cancelled sidecar
                active.rerun_force = false;
                return CancelOutcome::CancelledActive;
            }
        }
        let before = self.pending.len();
        self.pending.retain(|j| j.mp3 != mp3);
        if self.pending.len() != before {
            CancelOutcome::RemovedPending
        } else {
            CancelOutcome::NotFound
        }
    }

    fn request_purge(&mut self, id: &str) {
        self.pending_purge = Some(id.to_string());
    }
    fn take_purge(&mut self) -> Option<String> {
        self.pending_purge.take()
    }
    /// Whether the worker is presently on a job — the delete command's
    /// refusal gate (see model_commands.rs).
    fn any_active(&self) -> bool {
        self.active.is_some()
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
        // Nothing new in `pending` to wake the worker for — it's already
        // busy with this exact path and will requeue it via `finish_active`.
        Enqueued::WillRerunAfterActive => {
            log::info!(
                "transcribe: {} is already running — will re-run once it finishes",
                mp3.display()
            );
        }
        Enqueued::Duplicate => {}
    }
}

/// Post a one-shot cache-purge request and wake the worker — the delete
/// command's first half (see model_commands.rs for the second).
pub(crate) fn request_model_purge(app: &AppHandle, id: &str) {
    let state = app.state::<TranscriptionState>();
    let mut guard = lock_ignoring_poison(&state.inner);
    guard.request_purge(id);
    state.cv.notify_all();
}

/// Whether ANY transcription job is currently in flight — the delete
/// command refuses while one is (its terminal write may target the model
/// being deleted, and mid-inference the mmap is guaranteed live).
pub(crate) fn is_any_transcription_active(app: &AppHandle) -> bool {
    let state = app.state::<TranscriptionState>();
    let guard = lock_ignoring_poison(&state.inner);
    guard.any_active()
}

/// True when `mp3` is the transcription job currently running — the shell's
/// rename guard (Codex PR #46): renaming a file the worker is mid-decode on
/// would leave it completing its terminal write into the old, now-orphaned
/// path while the renamed note embeds a placeholder that never resolves.
pub(crate) fn is_active_transcription(app: &AppHandle, mp3: &Path) -> bool {
    let state = app.state::<TranscriptionState>();
    let guard = lock_ignoring_poison(&state.inner);
    guard.is_active(mp3)
}

/// Retarget a still-queued transcription job from `old` to `new` after a
/// successful rename (Codex PR #46), so the worker eventually writes its
/// sidecar under the name the renamed note actually embeds instead of the
/// stale pre-rename path. Pure bookkeeping under the queue mutex — no I/O —
/// so a short hold here is fine per this file's never-hold-across-I/O rule.
pub(crate) fn retarget_pending_transcription(app: &AppHandle, old: &Path, new: PathBuf) -> bool {
    let state = app.state::<TranscriptionState>();
    let mut guard = lock_ignoring_poison(&state.inner);
    guard.retarget_pending(old, new)
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

/// The composed `initial_prompt` for a job: the recording's current title
/// (its stem minus the `YYYY-MM-DD HHmm ` capture prefix) plus the vault's
/// custom vocabulary. Pure so it's unit-testable on Linux;
/// `process_transcription` feeds it the live config.
fn initial_prompt_for(mp3: &Path, vocabulary: Option<&str>) -> Option<String> {
    let stem = mp3
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = vault_buddy_core::capture_paths::capture_title(&stem);
    vault_buddy_transcribe::compose_initial_prompt(title, vocabulary)
}

/// How long one failed silero download suppresses re-attempts. Without a
/// backoff, an offline/firewalled setup with the MAIN model already cached
/// stalled EVERY job through the full network timeout before degrading to
/// no-VAD (Codex PR #61) — a default-on optional accelerant repeatedly
/// delaying the thing the user actually asked for. Ten minutes keeps the
/// stall a rare event while still picking a returning network up without
/// an app restart.
const VAD_DOWNLOAD_BACKOFF_MS: u64 = 10 * 60 * 1000;

/// Clock-free backoff decision state (unit-tested; `ensure_vad_model`
/// feeds it `now_ms()` under the process-wide `VAD_BACKOFF` mutex).
#[derive(Default)]
struct VadDownloadBackoff {
    retry_at_ms: Option<u64>,
}

impl VadDownloadBackoff {
    fn should_attempt(&self, now_ms: u64) -> bool {
        self.retry_at_ms.is_none_or(|t| now_ms >= t)
    }
    fn record_failure(&mut self, now_ms: u64) {
        self.retry_at_ms = Some(now_ms + VAD_DOWNLOAD_BACKOFF_MS);
    }
    fn record_success(&mut self) {
        self.retry_at_ms = None;
    }
}

static VAD_BACKOFF: Mutex<VadDownloadBackoff> =
    Mutex::new(VadDownloadBackoff { retry_at_ms: None });

/// Ensure the Silero VAD model is on disk, downloading (progress rides the
/// existing `capture:modelDownload` event with model:"vad") if missing.
/// Failure is NOT a job failure — the caller degrades to a no-VAD run: the
/// user's intent is "transcribe my meeting", and a ~1 MB optional accelerant
/// must never block that. A cancel mid-download is still a real cancel (the
/// caller consults the token, exactly like the main-model path). A recent
/// download FAILURE short-circuits to the same degrade without touching the
/// network (`VAD_BACKOFF` above) — but a cancel never arms the backoff: the
/// user aborting a job says nothing about the network.
fn ensure_vad_model(app: &AppHandle, mp3: &Path, cancel: &CancelToken) -> Result<PathBuf, String> {
    if let Some(p) = vad_model_path() {
        if p.exists() {
            return Ok(p);
        }
    }
    if !lock_ignoring_poison(&VAD_BACKOFF).should_attempt(now_ms()) {
        return Err(format!(
            "a recent download attempt failed; next retry after {} min of backoff",
            VAD_DOWNLOAD_BACKOFF_MS / 60_000
        ));
    }
    log::info!("transcribe: downloading the silero VAD model");
    let app = app.clone();
    let mp3 = mp3.to_path_buf();
    // ~885 KB file: a 200 KB emit step yields a handful of updates.
    let mut throttle = EmitThrottle::new(200_000);
    let result = download_vad_model(cancel, &mut |received, total| {
        if throttle.should_emit(received, Some(received) == total) {
            set_phase(&app, Phase::Downloading { received, total });
            let _ = app.emit(
                "capture:modelDownload",
                serde_json::json!({
                    "mp3": mp3.to_string_lossy(),
                    "model": "vad",
                    "received": received,
                    "total": total,
                }),
            );
        }
    });
    match &result {
        Ok(_) => lock_ignoring_poison(&VAD_BACKOFF).record_success(),
        // A cancelled download is the user's choice, not the network's
        // verdict — the next job should attempt normally.
        Err(_) if cancel.is_cancelled() => {}
        Err(_) => lock_ignoring_poison(&VAD_BACKOFF).record_failure(now_ms()),
    }
    result
}

/// Reuse the cached transcriber only when BOTH cache-key elements match.
/// Pure so the (tier, use_gpu) contract is unit-tested; the worker loop
/// applies it below.
fn needs_reload(cached: Option<(ModelTier, bool)>, tier: ModelTier, use_gpu: bool) -> bool {
    cached != Some((tier, use_gpu))
}

fn process_transcription(
    app: &AppHandle,
    job: &TranscriptionJob,
    loaded: &mut Option<(ModelTier, bool, WhisperTranscriber)>,
) {
    let app_cfg = capture_config::load_config();
    let cfg = capture_config::vault_config(&app_cfg, &job.vault_id);
    // GPU is an app-global (not per-vault) knob, read from the SAME
    // config.json load as the per-vault settings above — one read serves
    // both, exactly as fresh: load_config reads the whole file in one shot,
    // so a second call a moment later isn't a fresher torn-window read,
    // it's just a second file read.
    let use_gpu = app_cfg.transcription.use_gpu;
    // A forced (explicit) re-transcribe ignores the vault's auto-transcribe
    // setting; the automatic path still bails when disabled.
    if !cfg.transcribe && !job.force {
        return;
    }
    let tier = ModelTier::from_str(&cfg.transcription_model);

    // The worker loop already published this job as `active` in the SAME
    // lock acquisition it popped it under, before calling this function —
    // so there is no gap where this path is neither `pending` nor `active`.
    // (There used to be: this used to construct and publish `ActiveJob` here,
    // after the `load_config` call above already did synchronous I/O, which
    // left a same-path enqueue briefly undetectable as a duplicate.)
    let (cancel, progress) = {
        let state = app.state::<TranscriptionState>();
        let guard = lock_ignoring_poison(&state.inner);
        let active = guard
            .active
            .as_ref()
            .expect("the worker loop publishes `active` before calling process_transcription");
        (active.cancel.clone(), active.progress.clone())
    };
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
    // Resolve the Silero model only for VAD-enabled vaults. A download
    // failure DEGRADES — the job still transcribes, just without silence
    // skipping (the warning below and the stats row's "off" are the traces) —
    // unless the error was actually our own cancel.
    let vad_model = if cfg.transcription_vad {
        match ensure_vad_model(app, &job.mp3, &cancel) {
            Ok(p) => Some(p),
            Err(e) => {
                if cancel.is_cancelled() {
                    return emit_cancelled(app, &job.mp3);
                }
                log::warn!(
                    "transcribe: VAD model unavailable, transcribing {} without silence skipping: {e}",
                    job.mp3.display()
                );
                None
            }
        }
    } else {
        None
    };
    // Handover: both the main model AND — for a VAD-enabled vault — the
    // silero model are on disk now (just downloaded, or already present)
    // — replace the download row with "preparing" BEFORE the model-load
    // gap below, so a download UI can never stick at 100%.
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
    if needs_reload(loaded.as_ref().map(|(t, g, _)| (*t, *g)), tier, use_gpu) {
        match WhisperTranscriber::load(&model, use_gpu) {
            Ok(w) => *loaded = Some((tier, use_gpu, w)),
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
    let transcriber = &loaded.as_ref().unwrap().2;
    let opts = TranscribeOptions {
        language: cfg.transcription_language.clone(),
        timestamps: cfg.transcript_timestamps,
        model_label: tier.label(),
        initial_prompt: initial_prompt_for(&job.mp3, cfg.transcription_vocabulary.as_deref()),
        vad_model,
    };
    let generated_at = chrono::Local::now().to_rfc3339();

    set_phase(app, Phase::Transcribing);
    let _ = app.emit(
        "capture:transcribeProgress",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy(), "progress": 0 }),
    );
    let app_cb = app.clone();
    let mp3_cb = job.mp3.clone();
    // Seeded at 0, matching the "progress": 0 emit just above — an unseeded
    // throttle's own first call always fires regardless of value, which
    // re-announced/re-logged that same 0% a second time.
    let mut emit_throttle = EmitThrottle::new_seeded(5, 0);
    let mut log_throttle = EmitThrottle::new_seeded(25, 0);
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

/// The vault whose folder contains `mp3` (for the retry/force commands),
/// matched on CANONICAL paths — `Path::starts_with` on raw components
/// accepted `<vault>\..\anywhere` escapes and symlinks (GAP-01). None when
/// the path cannot be resolved or no registered vault contains it.
fn owning_vault_id(mp3: &Path) -> Option<String> {
    let vaults = discovery::discover_vaults();
    capture_paths::vault_owning_path(&vaults, mp3).map(|owned| owned.vault.id.clone())
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
/// jobs of the same tier AND the same GPU flag (`needs_reload`) — a toggle
/// flip takes effect on the very next job, no restart. Mirrors
/// `run_recovery`'s shape (own thread, coarse is-recording gate).
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
            let mut loaded: Option<(ModelTier, bool, WhisperTranscriber)> = None;
            loop {
                // Wait until a job is present, but only PEEK: the recording gate
                // below may leave it queued, and popping before that gate would
                // drop the job (or a force upgrade that lands between the peek
                // and the pop). We claim it only once we've decided to run it.
                let purge = {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = lock_ignoring_poison(&state.inner);
                    while guard.pending.is_empty() && guard.pending_purge.is_none() {
                        // The Condvar guard is poisonable too — recover it the
                        // same way `lock_ignoring_poison` recovers the mutex, so
                        // a panic elsewhere can't wedge the worker permanently on
                        // a poisoned wait.
                        guard = state.cv.wait(guard).unwrap_or_else(|e| e.into_inner());
                    }
                    guard.take_purge()
                };
                // Drop the cached transcriber BEFORE any delete attempt can
                // race the mmap (the requesting command retries the unlink
                // while we get here). "vad" is accepted as a no-op for
                // symmetry — the worker never caches the silero model.
                if let Some(id) = purge {
                    if loaded.as_ref().map(|(t, _, _)| t.as_str()) == Some(id.as_str()) {
                        log::info!("transcribe: dropping cached {id} model for deletion");
                        loaded = None;
                    }
                    // A purge with no pending work: loop back to the wait
                    // rather than falling through to the recording gate.
                    let state = app.state::<TranscriptionState>();
                    let guard = lock_ignoring_poison(&state.inner);
                    if guard.pending.is_empty() {
                        continue;
                    }
                }
                // Never contend with a live recording for CPU — re-check soon.
                if is_recording(&app) {
                    std::thread::sleep(Duration::from_secs(30));
                    continue;
                }
                // Claim the front AND publish `active` in the SAME lock
                // acquisition — a force upgrade that landed after the peek is
                // already reflected in the job we pop here (dedup derives from
                // live pending + active, so it can't drift), and publishing
                // `active` here (rather than inside `process_transcription`,
                // after that function's own I/O) closes the gap where this
                // path used to be neither `pending` nor `active` for a moment.
                // `None` means the queue emptied in the unlocked gap between
                // the peek above and this pop — only `cancel_transcription`'s
                // `pending.retain` can do that (this is the sole popper), e.g.
                // cancelling the last queued job — so just loop back.
                let job = {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = lock_ignoring_poison(&state.inner);
                    let job = guard.pending.pop_front();
                    if let Some(job) = &job {
                        guard.active = Some(ActiveJob {
                            mp3: job.mp3.clone(),
                            vault_id: job.vault_id.clone(),
                            cancel: CancelToken::new(),
                            started_at_ms: now_ms(),
                            phase: Phase::Preparing,
                            progress: Arc::new(AtomicU8::new(0)),
                            rerun_force: false,
                        });
                    }
                    job
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
                // `finish_active` (which also requeues an explicit rerun
                // request marked while this job ran) is all the cleanup needed.
                {
                    let state = app.state::<TranscriptionState>();
                    let mut guard = lock_ignoring_poison(&state.inner);
                    if let Some(rerun) = guard.finish_active() {
                        log::info!(
                            "transcribe: re-queued {} for a force re-transcribe requested while it was running",
                            rerun.mp3.display()
                        );
                    }
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
    if !capture_paths::is_capture_mp3(&mp3) {
        return Err("Not a Vault Buddy capture file.".to_string());
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
    if !capture_paths::is_capture_mp3(&mp3) {
        return Err("Not a Vault Buddy capture file.".to_string());
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
/// for bookkeeping (`TranscriptionQueue::cancel`: flip the active job's
/// `CancelToken` and clear its `rerun_force`, or drop a pending job) — NEVER
/// across the sidecar write below, which does a temp+fsync+rename
/// (`replace_if_ours`) and would otherwise stall every other command that
/// needs the same mutex (enqueue, status, a concurrent cancel) for the
/// duration of a disk flush.
///
/// The active job's sidecar is deliberately NOT written here: cancelling it
/// only flips the token (and clears any pending rerun — see `cancel`'s doc),
/// and the worker's `TranscribeError::Cancelled` arm (in
/// `process_transcription`) owns that write via `replace_if_ours`, which
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
    let outcome = {
        let state = app.state::<TranscriptionState>();
        let mut guard = lock_ignoring_poison(&state.inner);
        guard.cancel(&mp3)
    }; // <-- mutex released here
    match outcome {
        CancelOutcome::CancelledActive => Ok(()), // worker owns the terminal bookkeeping
        CancelOutcome::NotFound => Err("No such transcription in the queue.".into()),
        CancelOutcome::RemovedPending => {
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
            Ok(())
        }
    }
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

    // Regression (Codex PR #61): with Skip silence on and the silero file
    // missing, EVERY job re-attempted the download — in an offline setup
    // each transcription stalled through the full network timeout before
    // degrading to no-VAD, because nothing recorded that the download just
    // failed. The backoff arms on failure, expires on its own (a fixed
    // window, so a network that comes back is picked up without a
    // restart), and clears on success.
    #[test]
    fn vad_download_backoff_arms_on_failure_and_expires() {
        let mut b = VadDownloadBackoff::default();
        assert!(b.should_attempt(1_000), "nothing failed yet: attempt");
        b.record_failure(1_000);
        assert!(
            !b.should_attempt(1_000 + VAD_DOWNLOAD_BACKOFF_MS - 1),
            "inside the window: skip without touching the network"
        );
        assert!(
            b.should_attempt(1_000 + VAD_DOWNLOAD_BACKOFF_MS),
            "window elapsed: try again"
        );
        // Success clears an armed backoff outright — a transient blip must
        // not suppress attempts once a download has actually worked.
        b.record_failure(50_000);
        b.record_success();
        assert!(b.should_attempt(50_001));
    }

    fn job(path: &str, force: bool) -> TranscriptionJob {
        TranscriptionJob {
            mp3: PathBuf::from(path),
            vault_id: "v".to_string(),
            force,
        }
    }

    // Minimal in-flight job for the dedup tests — only `mp3` (and, in the
    // rerun tests, `rerun_force`) is read; the rest are inert placeholders
    // the struct requires.
    fn active_job(path: &str) -> ActiveJob {
        ActiveJob {
            mp3: PathBuf::from(path),
            vault_id: "v".to_string(),
            cancel: CancelToken::new(),
            started_at_ms: 0,
            phase: Phase::Preparing,
            progress: Arc::new(AtomicU8::new(0)),
            rerun_force: false,
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
    fn force_marks_the_active_job_to_rerun_instead_of_queueing_a_duplicate() {
        // Regression (Codex P2): a force re-transcribe of an ALREADY-active
        // path used to push a second `pending` entry for the same mp3, so
        // `transcription_queue_status` reported one path as both `active`
        // and `queued` — the frontend store (keyed by mp3) then let the
        // queued seed silently overwrite the active phase — and
        // `cancel_transcription`'s active-first check returned before ever
        // looking at that duplicate, so cancelling appeared to work but the
        // duplicate ran right after. It must instead mark the running job.
        let mut q = TranscriptionQueue {
            active: Some(active_job("X")),
            ..Default::default()
        };
        assert_eq!(q.enqueue(job("X", true)), Enqueued::WillRerunAfterActive);
        assert!(
            q.pending.is_empty(),
            "no duplicate entry for the active path"
        );
        assert!(q.active.as_ref().unwrap().rerun_force);
    }

    #[test]
    fn finish_active_requeues_a_forced_rerun_request() {
        let mut q = TranscriptionQueue {
            active: Some(active_job("X")),
            ..Default::default()
        };
        q.active.as_mut().unwrap().rerun_force = true;
        let requeued = q.finish_active();
        assert!(q.active.is_none());
        assert_eq!(q.pending.len(), 1);
        assert!(q.pending[0].force);
        assert_eq!(requeued.map(|j| j.mp3), Some(PathBuf::from("X")));
    }

    #[test]
    fn finish_active_does_not_requeue_without_a_rerun_request() {
        let mut q = TranscriptionQueue {
            active: Some(active_job("X")),
            ..Default::default()
        };
        assert!(q.finish_active().is_none());
        assert!(q.active.is_none());
        assert!(q.pending.is_empty());
    }

    #[test]
    fn finish_active_on_an_idle_queue_is_a_no_op() {
        let mut q = TranscriptionQueue::default();
        assert!(q.finish_active().is_none());
    }

    #[test]
    fn cancel_active_clears_the_token_and_any_pending_rerun_request() {
        // Regression (Codex P2): cancelling the active job must also cancel
        // a rerun requested while it ran, or the cancel appears to stop the
        // work but the rerun silently starts right after `finish_active`.
        let mut q = TranscriptionQueue {
            active: Some(active_job("X")),
            ..Default::default()
        };
        q.active.as_mut().unwrap().rerun_force = true;
        assert_eq!(q.cancel(Path::new("X")), CancelOutcome::CancelledActive);
        let active = q.active.as_ref().unwrap();
        assert!(active.cancel.is_cancelled());
        assert!(!active.rerun_force);
        assert!(
            q.finish_active().is_none(),
            "a cleared rerun request must not requeue"
        );
    }

    #[test]
    fn cancel_removes_a_pending_job() {
        let mut q = TranscriptionQueue::default();
        q.enqueue(job("X", false));
        assert_eq!(q.cancel(Path::new("X")), CancelOutcome::RemovedPending);
        assert!(q.pending.is_empty());
    }

    #[test]
    fn cancel_reports_not_found_for_an_unknown_path() {
        let mut q = TranscriptionQueue::default();
        assert_eq!(q.cancel(Path::new("nope")), CancelOutcome::NotFound);
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

    // --- Codex PR #46: rename desyncs the transcription queue ------------
    //
    // A capture rename that lands while the mp3's transcription job is still
    // QUEUED or ACTIVE used to leave the queue keyed to the OLD path: the
    // worker later wrote its terminal sidecar under the old name while the
    // renamed note embeds the (moved) pending placeholder, which then never
    // resolves until the next launch's backfill re-runs a multi-minute
    // whisper inference. `is_active` lets the rename command refuse an
    // in-flight job outright; `retarget_pending` lets it fix up a queued one
    // in place instead.

    #[test]
    fn is_active_true_only_for_the_running_path() {
        let q = TranscriptionQueue {
            active: Some(active_job("X")),
            ..Default::default()
        };
        assert!(q.is_active(Path::new("X")));
        assert!(!q.is_active(Path::new("Y")));
    }

    #[test]
    fn is_active_false_on_an_idle_queue() {
        let q = TranscriptionQueue::default();
        assert!(!q.is_active(Path::new("X")));
    }

    #[test]
    fn retarget_pending_renames_the_matching_queued_job() {
        let mut q = TranscriptionQueue::default();
        q.enqueue(job("old.mp3", false));
        q.enqueue(job("other.mp3", false));
        assert!(q.retarget_pending(Path::new("old.mp3"), PathBuf::from("new.mp3")));
        let mp3s: Vec<_> = q.pending.iter().map(|j| j.mp3.clone()).collect();
        assert_eq!(
            mp3s,
            vec![PathBuf::from("new.mp3"), PathBuf::from("other.mp3")],
            "the matching entry is renamed in place; the other is untouched"
        );
    }

    #[test]
    fn retarget_pending_returns_false_when_nothing_matches() {
        let mut q = TranscriptionQueue::default();
        q.enqueue(job("other.mp3", false));
        assert!(!q.retarget_pending(Path::new("old.mp3"), PathBuf::from("new.mp3")));
        assert_eq!(q.pending[0].mp3, PathBuf::from("other.mp3"));
    }

    #[test]
    fn retarget_pending_never_touches_the_active_job() {
        // Behavioral pin against a naive implementation that retargets
        // whichever job (pending OR active) matches `old`: this must be a
        // pending-only operation. The shell refuses to call it for an
        // active path (via `is_active`), but the queue method itself must
        // not rely on that — it must be structurally incapable of mutating
        // `active`.
        let mut q = TranscriptionQueue {
            active: Some(active_job("X")),
            ..Default::default()
        };
        assert!(!q.retarget_pending(Path::new("X"), PathBuf::from("X2")));
        assert_eq!(q.active.as_ref().unwrap().mp3, PathBuf::from("X"));
    }

    #[test]
    fn initial_prompt_for_composes_title_and_vocabulary() {
        let mp3 = Path::new("/v/Meetings/2026/07/2026-07-16 0930 Budget review.mp3");
        assert_eq!(
            initial_prompt_for(mp3, Some("Kubernetes, rmcp")),
            Some("Budget review. Kubernetes, rmcp".to_string())
        );
        assert_eq!(
            initial_prompt_for(mp3, None),
            Some("Budget review".to_string())
        );
    }

    #[test]
    fn initial_prompt_for_is_none_when_there_is_nothing_to_prime_with() {
        // A non-capture stem passes through capture_title unchanged and still
        // primes (harmless), but an empty stem + no vocabulary must be None so
        // whisper runs exactly as before this feature.
        assert_eq!(initial_prompt_for(Path::new(""), None), None);
        assert_eq!(
            initial_prompt_for(Path::new("/x/download.mp3"), None),
            Some("download".to_string())
        );
    }

    #[test]
    fn retarget_pending_keeps_one_entry_per_path() {
        // There is at most one pending entry per path by construction
        // (`enqueue`'s dedup) — retarget must rename in place, never append,
        // so it can't create a duplicate for the new path either.
        let mut q = TranscriptionQueue::default();
        q.enqueue(job("old.mp3", false));
        q.enqueue(job("other.mp3", false));
        assert!(q.retarget_pending(Path::new("old.mp3"), PathBuf::from("new.mp3")));
        assert_eq!(
            q.pending.len(),
            2,
            "retarget renames, it never adds an entry"
        );
        let unique: std::collections::HashSet<_> = q.pending.iter().map(|j| &j.mp3).collect();
        assert_eq!(
            unique.len(),
            2,
            "no duplicate mp3 in the pending list after retarget"
        );
    }

    #[test]
    fn model_reloads_when_tier_or_gpu_changes() {
        // The cached transcriber must be reused ONLY when both the tier
        // and the GPU flag match — a toggle flip takes effect on the next
        // job without a restart (spec: cache key (tier, use_gpu)).
        assert!(!needs_reload(
            Some((ModelTier::Small, true)),
            ModelTier::Small,
            true
        ));
        assert!(needs_reload(
            Some((ModelTier::Small, true)),
            ModelTier::Small,
            false
        ));
        assert!(needs_reload(
            Some((ModelTier::Small, true)),
            ModelTier::Turbo,
            true
        ));
        assert!(needs_reload(None, ModelTier::Small, true));
    }

    #[test]
    fn purge_request_round_trips_and_is_one_shot() {
        let mut q = TranscriptionQueue::default();
        assert_eq!(q.take_purge(), None);
        q.request_purge("small");
        assert_eq!(q.take_purge(), Some("small".to_string()));
        assert_eq!(q.take_purge(), None, "one-shot: taken means gone");
        // A second request before the worker wakes overwrites — deleting
        // two models back-to-back must not strand the first request as a
        // stale drop of the wrong tier later.
        q.request_purge("base");
        q.request_purge("turbo");
        assert_eq!(q.take_purge(), Some("turbo".to_string()));
    }

    #[test]
    fn any_active_reflects_the_active_slot() {
        // The delete command's refusal gate, at the queue-logic level:
        // deleting a model out from under a running job would race its
        // guaranteed-live mmap (and possibly its terminal write's tier).
        let mut q = TranscriptionQueue::default();
        assert!(!q.any_active());
        q.active = Some(active_job("X")); // existing test helper
        assert!(q.any_active());
    }
}
