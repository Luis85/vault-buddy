//! Publish a rendered product into a vault (Task 48; F-43; ADR R13; A21):
//! `editor_publish_product` -- **the TENTH sanctioned vault write**, on the
//! ninth's rails.
//!
//! **Order, and what each step may leave behind.** Every refusal the
//! request itself earns -- no such product, its file gone, no vault, a
//! folder that escapes the vault -- answers before anything is created or a
//! job registered. Then the job (`kind: "publish"`, `JobRegistry`) is
//! registered BEFORE a byte is written, so `editor_get_jobs`, the close
//! guard and the shutdown gate all see it (F19). Then, in the vault:
//! `editor::vault_dir::prepare_export_dir` (containment asserted
//! before AND after `create_dir_all`), the free-space check, the journal's
//! `reserved` step, the COPY (`core::editor::publish_io::copy_into_vault`:
//! an owned hidden temp beside the target, `rename_noreplace` with the
//! ` (N)` retry, pairwise with the note), the journal's `video` step, the
//! note -- rendered with the name the video LANDED under (A21) -- and
//! `complete`. Any failure before the video lands rolls the created
//! directories back (`rollback_export_dir`) and leaves no temp; a note
//! failure after it is a WARNING and the video stays -- the ninth write's
//! posture exactly.
//!
//! **Nothing in the project changes.** The product is opened read-only and
//! never moved (R13: it must stay playable in its project); the ledger,
//! `project.json` and the session are only read.
//!
//! **The journal** (`jobs\<jobId>\publish.json`, `{step, video, note}`)
//! exists for a CRASH: a publish that returns removes its own job
//! directory, whatever the outcome, so only an interrupted one leaves a
//! journal behind -- which `recovery::interrupted_publishes` reports on the
//! next start and never deletes or retries (docs/Gaps.md: no resume).
//!
//! **Shutdown.** `blocks_shutdown` is the gate's publish term -- by KIND,
//! never the render term's set (counting every kind in one term re-opens
//! GAP-190's loop). The quit workers `cancel_all_bounded` it: a copy
//! answers its cancel between chunks and removes its temp, so a quit never
//! strands a half-written file in a vault; once the video has landed the
//! note is written regardless (it is the only thing still missing). After
//! the bound expires the gate stops counting publishes
//! (`PUBLISHES_ABANDONED`), the render term's latch. A DISCARD cancels and
//! waits for it too (`media_derive::stop_session_derivations`): the product
//! it reads and the journal it writes live in the directory a discard
//! removes.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{NaiveDateTime, Timelike};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewWindow};
use vault_buddy_core::capture_config;
use vault_buddy_core::capture_note::{write_atomic_replacing, write_note_collision_safe};
use vault_buddy_core::capture_paths::{base_name, capture_dir, safe_recording_root};
use vault_buddy_core::editor::note::{render_tutorial_note, TutorialNoteMeta};
use vault_buddy_core::editor::publish_io::{copy_cancellable, copy_into_vault};
use vault_buddy_core::editor::render_plan::chapters_for;
use vault_buddy_core::editor::{EditorError, EditorErrorCode, Product};
use vault_buddy_core::screen_capture_config::export_space_shortfall;
use vault_buddy_core::screen_capture_paths::reserve_final_screen;
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::vault_config::VaultCaptureConfig;
use vault_buddy_screen::staging_title::sanitize_title;

use super::authz::require_editor_window;
use super::media_jobs::{start_job_in, JobKind, JobPhase, JobReporter, JobTerminal, NoSubscriber};
use super::prefs_commands::{blocking, local_data, project_id_for};
use super::project_store::project_dir;
use super::render_jobs::{read_ledger, JOBS_DIR, PRODUCTS_DIR};
use super::vault_dir::{prepare_export_dir, rollback_export_dir};
use super::EditorState;

/// The journal's file name inside `jobs\<jobId>\`.
pub(crate) const PUBLISH_JOURNAL: &str = "publish.json";
/// How often a quit's cancel re-reads the registry.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// Set once a quit's bounded publish cancel expired (the render term's
/// `RENDERS_ABANDONED`, for the same Alt+F4 re-close loop).
static PUBLISHES_ABANDONED: AtomicBool = AtomicBool::new(false);

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn unavailable(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::DestinationUnavailable, message)
}

// ---- the wire ----

/// Contract reference `PublishDestination`: where the product goes.
/// `folder` blank means the vault's screen-capture folder.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishDestination {
    pub vault_id: String,
    pub folder: String,
    pub dated: bool,
    pub create_note: bool,
}

/// Contract reference `PublishReceipt`. `notePath` and `warning` are
/// present-even-when-null.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishReceipt {
    pub video_path: String,
    pub note_path: Option<String>,
    pub vault_id: String,
    pub vault_name: String,
    pub warning: Option<String>,
}

/// How far a publish got (`publish.json`'s `step`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PublishStep {
    Reserved,
    Video,
    Note,
    Complete,
}

/// `jobs\<jobId>\publish.json`: the step reached and the two names,
/// vault-relative (`/`-separated) -- the RESERVED names at `reserved`, the
/// LANDED ones from `video` on. `note` is null when no note was asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PublishJournal {
    pub step: PublishStep,
    pub video: String,
    pub note: Option<String>,
}

// ---- the world outside the project store ----

/// What a publish reads and writes beyond the project store. Production is
/// `LiveEnv` (Obsidian's registry, `config.json`, the real volume); the
/// tests hand in a tempdir vault and observe or fail the copy and the note.
pub(crate) trait PublishEnv {
    /// The vault's folder and display name, or `None` when it is not in
    /// Obsidian's registry.
    fn vault(&self, vault_id: &str) -> Option<(PathBuf, String)>;
    fn config(&self, vault_id: &str) -> VaultCaptureConfig;
    /// Free bytes on `dir`'s volume; `None` is UNKNOWN and never refuses.
    fn free_bytes(&self, dir: &Path) -> Option<u64> {
        vault_buddy_screen::disk::free_bytes(dir)
    }
    /// Move the product's bytes into the owned temp.
    fn stream(
        &self,
        source: &mut File,
        dest: &mut File,
        total: u64,
        cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(f64),
    ) -> io::Result<()> {
        copy_cancellable(source, dest, total, cancel, on_progress)
    }
    /// Write the note at `target` (never replacing); where it landed.
    fn write_note(&self, target: &Path, content: &str) -> io::Result<PathBuf> {
        write_note_collision_safe(target, content)
    }
}

struct LiveEnv;

impl PublishEnv for LiveEnv {
    fn vault(&self, vault_id: &str) -> Option<(PathBuf, String)> {
        crate::commands::find_vault(vault_id)
            .ok()
            .map(|v| (PathBuf::from(v.path), v.name))
    }

    fn config(&self, vault_id: &str) -> VaultCaptureConfig {
        capture_config::vault_config(&capture_config::load_config(), vault_id)
    }
}

// ---- planning: every refusal before anything exists ----

/// Everything resolved before a job exists.
struct PublishPlan {
    product: Product,
    source: PathBuf,
    size: u64,
    vault_path: PathBuf,
    vault_id: String,
    vault_name: String,
    dir: PathBuf,
    base: String,
    note: Option<TutorialNoteMeta>,
    jobs_dir: PathBuf,
}

/// The wall-clock time a ledger `createdAt` states (its own offset, so the
/// name does not move with the machine's timezone); now when unreadable.
fn rendered_at(created_at: &str) -> NaiveDateTime {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|at| at.naive_local())
        .unwrap_or_else(|_| chrono::Local::now().naive_local())
}

/// `YYYY-MM-DD HHmm <name>` -- the capture naming, with the product's name
/// through the staging title rules (no separator, no trailing dot, bounded
/// length), so it is safe as a file name on every volume a vault lives on.
pub(crate) fn publish_base(product: &Product) -> String {
    let at = rendered_at(&product.created_at);
    base_name(
        at.date(),
        at.hour(),
        at.minute(),
        &sanitize_title(&product.name),
    )
}

fn note_meta(product: &Product, cfg: &VaultCaptureConfig) -> TutorialNoteMeta {
    let range = product
        .render_range
        .as_ref()
        .map(|r| (r.start_ms, r.end_ms));
    let chapters = match product.snapshot.as_deref() {
        Some(project) => chapters_for(project, range).unwrap_or_else(|e| {
            log::warn!("editor publish: no chapters for the note: {}", e.message);
            Vec::new()
        }),
        None => Vec::new(),
    };
    TutorialNoteMeta {
        recorded_at: product.created_at.clone(),
        duration_ms: product.duration_ms,
        product: product.name.clone(),
        revision: product.revision,
        range,
        chapters,
        extra_frontmatter: cfg.screen_extra_frontmatter.clone(),
        body_template: cfg.screen_body_template.clone(),
    }
}

fn plan_publish(
    state: &EditorState,
    root: &Path,
    env: &dyn PublishEnv,
    session_id: &str,
    product_id: &str,
    destination: &PublishDestination,
) -> Result<PublishPlan, EditorError> {
    let project_id = project_id_for(state, session_id)?;
    let project = project_dir(root, &project_id)
        .ok_or_else(|| err(EditorErrorCode::Internal, "Not a valid project id."))?;
    let product = read_ledger(root, &project_id)?
        .into_iter()
        .find(|p| p.id == product_id)
        .ok_or_else(|| {
            err(
                EditorErrorCode::InvalidRequest,
                "There is no such product in this project.",
            )
        })?;
    let source = project.join(PRODUCTS_DIR).join(&product.filename);
    let size = std::fs::symlink_metadata(&source)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .ok_or_else(|| {
            err(
                EditorErrorCode::SourceMissing,
                "The rendered file is no longer on disk, so it cannot be published.",
            )
        })?;
    let vault_id = destination.vault_id.trim();
    if vault_id.is_empty() {
        return Err(unavailable("Choose a vault to publish into."));
    }
    let (vault_path, vault_name) = env.vault(vault_id).ok_or_else(|| {
        unavailable("That vault is no longer in Obsidian. Open it in Obsidian and try again.")
    })?;
    if !vault_path.is_dir() {
        return Err(unavailable(
            "Vault folder not found — was it moved or deleted?",
        ));
    }
    let cfg = env.config(vault_id);
    let folder = match destination.folder.trim() {
        "" => cfg.screen_capture_root(),
        chosen => chosen,
    };
    let folder_root = safe_recording_root(&vault_path, folder).map_err(unavailable)?;
    let dir = capture_dir(
        &folder_root,
        rendered_at(&product.created_at).date(),
        destination.dated,
    );
    Ok(PublishPlan {
        base: publish_base(&product),
        note: destination.create_note.then(|| note_meta(&product, &cfg)),
        source,
        size,
        vault_path,
        vault_id: vault_id.to_string(),
        vault_name,
        dir,
        jobs_dir: project.join(JOBS_DIR),
        product,
    })
}

// ---- running one ----

/// `path` relative to the vault, `/`-separated (the journal's and the
/// report's spelling -- never an absolute path).
fn vault_relative(vault: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(vault).unwrap_or(path);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Writes this publish's journal steps into its own job directory.
struct Journal<'a> {
    dir: PathBuf,
    vault: &'a Path,
}

impl Journal<'_> {
    fn write(&self, step: PublishStep, video: &Path, note: Option<&Path>) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let journal = PublishJournal {
            step,
            video: vault_relative(self.vault, video),
            note: note.map(|n| vault_relative(self.vault, n)),
        };
        let json = serde_json::to_string_pretty(&journal).map_err(io::Error::other)?;
        write_atomic_replacing(&self.dir.join(PUBLISH_JOURNAL), &json)
    }

    /// A step after the video landed: the video is safe whatever happens
    /// here, so a failed journal write is logged, never raised.
    fn note_step(&self, step: PublishStep, video: &Path, note: Option<&Path>) {
        if let Err(e) = self.write(step, video, note) {
            log::warn!("editor publish: could not update the publish journal: {e}");
        }
    }

    /// The publish returned (whatever its outcome): its journal served no
    /// purpose, so its directory goes -- owned, no-follow.
    fn remove(&self) {
        if std::fs::symlink_metadata(&self.dir).is_err() {
            return;
        }
        if let Err(e) = super::store_io::remove_dir_no_follow(&self.dir) {
            log::warn!("editor publish: could not remove the publish journal: {e}");
        }
    }
}

fn copy_error(e: io::Error) -> EditorError {
    #[cfg(windows)]
    let disk_full = e.kind() == io::ErrorKind::StorageFull || e.raw_os_error() == Some(112);
    #[cfg(not(windows))]
    let disk_full = e.kind() == io::ErrorKind::StorageFull;
    if e.kind() == io::ErrorKind::Interrupted {
        err(EditorErrorCode::Cancelled, "The publish was cancelled.")
    } else if disk_full {
        err(
            EditorErrorCode::DiskFull,
            "The vault's disk filled up while the video was being copied. Nothing was added \
             to the vault.",
        )
    } else if e.kind() == io::ErrorKind::PermissionDenied {
        err(
            EditorErrorCode::WriteDenied,
            "Vault Buddy is not allowed to write into that folder. Nothing was added to the \
             vault.",
        )
    } else {
        err(
            EditorErrorCode::Internal,
            format!("The video could not be copied into the vault: {e}"),
        )
    }
}

/// Up to and including the video landing. The caller rolls the created
/// directories back on any error.
fn land_video(
    env: &dyn PublishEnv,
    plan: &PublishPlan,
    journal: &Journal<'_>,
    cancel: &AtomicBool,
    reporter: &mut JobReporter<'_>,
) -> Result<(PathBuf, PathBuf), EditorError> {
    if let Some(short) = export_space_shortfall(plan.size, env.free_bytes(&plan.dir)) {
        return Err(err(
            EditorErrorCode::DiskFull,
            format!(
                "Not enough disk space in that vault: about {} MB more is needed.",
                short.div_ceil(1024 * 1024)
            ),
        ));
    }
    let (video, note) = reserve_final_screen(&plan.dir, &plan.base);
    journal
        .write(
            PublishStep::Reserved,
            &video,
            plan.note.is_some().then_some(note.as_path()),
        )
        .map_err(|e| {
            err(
                EditorErrorCode::Internal,
                format!("Could not start the publish: {e}"),
            )
        })?;
    reporter.progress(JobPhase::Publishing, 0.0);
    copy_into_vault(&plan.source, &plan.dir, &plan.base, |source, dest| {
        env.stream(source, dest, plan.size, cancel, &mut |fraction| {
            reporter.progress(JobPhase::Publishing, fraction)
        })
    })
    .map_err(copy_error)
}

/// The note, after the video: `(where it landed, warning)`.
fn write_note(
    env: &dyn PublishEnv,
    meta: &TutorialNoteMeta,
    video: &Path,
    target: &Path,
) -> (Option<PathBuf>, Option<String>) {
    // A21: the name the video LANDED under, never the reservation's.
    let content = render_tutorial_note(meta, &file_name(video));
    match env.write_note(target, &content) {
        Ok(path) => (Some(path), None),
        Err(e) => {
            log::warn!("editor publish: the video landed but its note could not be written: {e}");
            (
                None,
                Some(
                    "The video was published, but its companion note could not be written."
                        .to_string(),
                ),
            )
        }
    }
}

fn run_publish(
    env: &dyn PublishEnv,
    plan: &PublishPlan,
    journal: &Journal<'_>,
    cancel: &AtomicBool,
    reporter: &mut JobReporter<'_>,
) -> Result<PublishReceipt, EditorError> {
    let created = prepare_export_dir(&plan.vault_path, &plan.dir).map_err(unavailable)?;
    let (video, note_target) =
        land_video(env, plan, journal, cancel, reporter).inspect_err(|_| {
            rollback_export_dir(&created);
        })?;
    let wants_note = plan.note.as_ref();
    journal.note_step(
        PublishStep::Video,
        &video,
        wants_note.map(|_| note_target.as_path()),
    );
    let (note, warning) = match wants_note {
        Some(meta) => write_note(env, meta, &video, &note_target),
        None => (None, None),
    };
    if let Some(landed) = &note {
        journal.note_step(PublishStep::Note, &video, Some(landed));
    }
    journal.note_step(PublishStep::Complete, &video, note.as_deref());
    log::info!(
        "editor publish: published product {} into vault {}",
        plan.product.id,
        plan.vault_id
    );
    Ok(PublishReceipt {
        video_path: video.to_string_lossy().into_owned(),
        note_path: note.map(|n| n.to_string_lossy().into_owned()),
        vault_id: plan.vault_id.clone(),
        vault_name: plan.vault_name.clone(),
        warning,
    })
}

/// `editor_publish_product`'s body: refuse, register the job, publish, end
/// the job with exactly one terminal -- and remove the journal.
pub(crate) fn publish_in(
    state: &EditorState,
    root: &Path,
    env: &dyn PublishEnv,
    session_id: &str,
    product_id: &str,
    destination: &PublishDestination,
) -> Result<PublishReceipt, EditorError> {
    let plan = plan_publish(state, root, env, session_id, product_id, destination)?;
    let (job_id, cancel) = start_job_in(state, session_id, JobKind::Publish)?;
    let mut reporter = JobReporter::new(
        &state.jobs,
        &NoSubscriber,
        session_id,
        &job_id,
        JobKind::Publish,
    );
    reporter.progress(JobPhase::Preparing, 0.0);
    let journal = Journal {
        dir: plan.jobs_dir.join(&job_id),
        vault: &plan.vault_path,
    };
    // A panic must still END the job, or the gate and the next publish in
    // this session would wait for it forever (the render job's rule).
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_publish(env, &plan, &journal, &cancel, &mut reporter)
    }))
    .unwrap_or_else(|_| {
        log::error!("editor publish {job_id}: the publish panicked");
        Err(err(
            EditorErrorCode::Internal,
            "The publish stopped unexpectedly.",
        ))
    });
    journal.remove();
    let (phase, terminal) = match &outcome {
        Ok(_) => (
            JobPhase::Complete,
            JobTerminal {
                product_id: Some(plan.product.id.clone()),
                ..JobTerminal::default()
            },
        ),
        Err(e) => (
            if e.code == EditorErrorCode::Cancelled {
                JobPhase::Cancelled
            } else {
                log::warn!("editor publish {job_id} failed: {}", e.message);
                JobPhase::Failed
            },
            JobTerminal {
                error: Some(e.clone()),
                ..JobTerminal::default()
            },
        ),
    };
    reporter.finish(phase, terminal);
    outcome
}

/// ASYNC (ADR §3.3): the copy runs on the blocking pool; the reply is the
/// receipt (no Channel -- the job is registered for the gate and
/// `editor_get_jobs`, not for progress).
#[tauri::command]
pub async fn editor_publish_product(
    window: WebviewWindow,
    app: AppHandle,
    session_id: String,
    product_id: String,
    destination: PublishDestination,
) -> Result<PublishReceipt, EditorError> {
    require_editor_window(&window)?;
    let root = local_data(&app)?;
    blocking(move || {
        publish_in(
            &app.state::<EditorState>(),
            &root,
            &LiveEnv,
            &session_id,
            &product_id,
            &destination,
        )
    })
    .await
}

// ---- the shutdown gate ----

/// Is a PUBLISH job not yet ended? By kind -- the render term's own rule.
pub(crate) fn publish_blocks_shutdown(state: &EditorState) -> bool {
    lock_ignoring_poison(&state.jobs).any_running(JobKind::Publish)
}

/// Cancel every publish and wait, at most `limit`, for all of them to end;
/// `true` iff they did.
pub(crate) fn cancel_all_in(state: &EditorState, limit: Duration, poll: Duration) -> bool {
    lock_ignoring_poison(&state.jobs).cancel_kind(JobKind::Publish);
    crate::shutdown_gate::wait_until_cleared(|| !publish_blocks_shutdown(state), limit, poll)
}

/// `shutdown_gate`'s publish term (F19).
pub fn blocks_shutdown(app: &AppHandle) -> bool {
    !PUBLISHES_ABANDONED.load(Ordering::SeqCst)
        && publish_blocks_shutdown(&app.state::<EditorState>())
}

/// The quit workers' publish step, beside the render cancel: stop every
/// copy (its temp is removed, its directories rolled back) and wait,
/// bounded. On expiry it LOGS and proceeds.
///
/// Callers must NOT be on the main/event-loop thread: this sleeps.
pub fn cancel_all_bounded(app: &AppHandle, limit: Duration) {
    let state = app.state::<EditorState>();
    if !publish_blocks_shutdown(&state) {
        return;
    }
    log::info!("editor publish: cancelling every publish before shutdown");
    if !cancel_all_in(&state, limit, CANCEL_POLL) {
        PUBLISHES_ABANDONED.store(true, Ordering::SeqCst);
        log::warn!("editor publish: a publish did not stop within {limit:?}; exiting anyway");
    }
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
