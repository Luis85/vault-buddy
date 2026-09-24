//! Render jobs, the products ledger and the shutdown gate's render term
//! (Task 46; F-41, F-42; ADR R5, R12; A16).
//!
//! **A render, start to finish.** `begin_render` refuses everything it can
//! BEFORE a job exists, in the ADR's order: the session and its revision (a
//! pending edit cannot be rendered -- `revisionConflict`), then ffmpeg
//! (`encoderUnavailable`, naming the fix), then the plan
//! (`core::editor::render_plan`, over the `sources.json` records whose file
//! is on disk), the probed build's capabilities, the product cap and free
//! space (`export_space_shortfall`; an unmeasurable volume proceeds). Only
//! then is a `render` job registered -- one per session -- and
//! `run_render_job` runs on the named `editor-render` thread
//! (`render_commands`): `preparing` → `rendering` (the child, writing
//! `jobs\<jobId>\out.mp4.part`) → `publishing` → `complete{productId}`.
//!
//! **Move first, record second.** Publishing MOVES the part to
//! `products\<productId>.mp4` (`rename_noreplace`, one volume) and only
//! then commits `products.json`. A ledger entry therefore always has its
//! file; a failed ledger write removes the moved file (it was never
//! recorded, so it was never a product). Recording first would leave an
//! entry pointing at nothing whenever the move failed.
//!
//! **Products are immutable.** Nothing opens a product file for writing
//! after the ledger records it (ADR invariant 5): not a restore (which
//! clones the ledger's snapshot into the session), not a save (which reads
//! the ledger into `record.products`), not a later render (a fresh id).
//! The ledger is committed the moment a product lands, independent of the
//! edit (R5's departure from the bundle), so an unsaved project keeps its
//! products. Its file name is exactly `<productId>.mp4`, and every reader
//! refuses a ledger that says otherwise (`has_canonical_file_name`).
//!
//! **Cancel and failure leave nothing.** A cancel kills the child (the
//! runner deletes its output) and the job removes its own directory; a
//! failure does the same. Neither touches the project. The job directory
//! is removed BEFORE the terminal message, so a discard waiting for the
//! render to end (`media_derive::stop_session_derivations`) never removes
//! the project under a still-open file.
//!
//! **Locks.** Publishing holds the session's SAVE lock (the one a save and
//! a discard hold): a save reads the ledger under it, and a discard that
//! removes the directory can never interleave with a move into it. A
//! session gone by then makes the job `cancelled` -- its product would have
//! no project to land in.
//!
//! **Shutdown (R12).** `blocks_shutdown` is the fourth term of
//! `shutdown_gate::shutdown_blocker`: true while a render is `rendering` or
//! `publishing`. Both quit workers call `cancel_all_bounded` first, beside
//! `export_shutdown::cancel_if_exporting`. A wedged render must not make
//! the app unquittable: after the bound expires, the gate stops counting
//! renders (`RENDERS_ABANDONED`), so Alt+F4's re-triggered close cannot loop.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::render_plan::{self, PlanSource, RenderPlan};
use vault_buddy_core::editor::{
    has_canonical_file_name, limits, new_entity_id, new_product, product_file_name, EditorError,
    EditorErrorCode, EditorProjection, InternalCommand, Map, Product, Project, RenderRange,
};
use vault_buddy_core::screen_capture_config::{
    export_size_estimate_bytes, export_space_shortfall, ScreenQuality,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::ffmpeg_args::EncodeSettings;
use vault_buddy_screen::render::video_graph::file_input_count;
use vault_buddy_screen::ScreenError;

use super::authz::require_session;
use super::media_jobs::{start_job_in, JobKind, JobPhase, JobReporter, JobTerminal, ProgressSink};
use super::prefs_commands::project_id_for;
use super::project_store::{project_dir, resolve_source, SourceMediaKind, SourceRecord};
use super::save_commands::{map_write_error, session_save_lock};
use super::store_io::{load_sources, read_bounded};
use super::EditorState;

pub(crate) const PRODUCTS_FILE: &str = "products.json";
pub(crate) const PRODUCTS_DIR: &str = "products";
pub(crate) const JOBS_DIR: &str = "jobs";
pub(crate) const PART_FILE: &str = "out.mp4.part";

/// The largest `products.json` accepted: every product may embed a whole
/// project snapshot, so the bound is the project-file bound per product.
const LEDGER_MAX_BYTES: u64 = limits::MAX_PROJECT_JSON_BYTES * (limits::MAX_PRODUCTS as u64 + 1);
/// How often a quit's cancel re-reads the registry.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// Set once a quit's bounded render cancel expired: from then on the gate
/// stops counting renders, so a wedged one cannot make Alt+F4's
/// re-triggered close loop forever (the process is exiting anyway).
static RENDERS_ABANDONED: AtomicBool = AtomicBool::new(false);

fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

fn internal(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::Internal, message)
}

fn cancelled() -> EditorError {
    err(EditorErrorCode::Cancelled, "The render was cancelled.")
}

/// `encoderUnavailable` for a machine with no working ffmpeg -- naming the
/// fix, the phase-5 export's own wording.
pub(crate) fn no_ffmpeg() -> EditorError {
    err(
        EditorErrorCode::EncoderUnavailable,
        "Rendering needs ffmpeg, which is not installed. Install it, then set its location in \
         Buddy settings \u{2192} Integrations if it is not on your PATH.",
    )
}

// ---- the wire ----

/// `RenderRequest.range`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeDto {
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Contract reference `RenderRequest`. `quality` stays a string so an
/// unknown value is this module's `invalidRequest`, not a transport error.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderRequest {
    pub session_id: String,
    pub expected_revision: u64,
    pub name: String,
    pub range: Option<RangeDto>,
    pub quality: String,
}

/// `editor_start_render`'s immediate reply (ADR §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderStarted {
    pub job_id: String,
    pub revision: u64,
}

/// Contract reference `ProductDto`: the ledger's record minus its snapshot,
/// plus whether its file is on disk. `renderRange` is present-even-when-null.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductDto {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub filename: String,
    pub mime: String,
    pub revision: u64,
    pub duration_ms: u64,
    pub created_at: String,
    pub edit_fingerprint: String,
    pub render_range: Option<RangeDto>,
    pub available: bool,
}

// ---- the runner seam ----

/// What one render reads and writes.
pub(crate) struct RenderWork<'a> {
    pub plan: &'a RenderPlan,
    pub inputs: &'a [PathBuf],
    pub job_dir: &'a Path,
    pub dest: &'a Path,
    pub settings: &'a EncodeSettings,
}

/// The toolchain a render runs on: production is `render_commands::
/// FfmpegRenderer` (`screen::render::run::render` over the resolved ffmpeg);
/// the tests substitute a fake, so every rule here is exercised without a
/// child process.
pub(crate) trait RenderRunner: Send + Sync {
    /// The H.264 encoder this build has (`""`: none).
    fn h264_encoder(&self) -> String;
    /// Why this build cannot render `plan`, or `None`.
    fn refusal(&self, plan: &RenderPlan, settings: &EncodeSettings) -> Option<String>;
    /// Free bytes on `dir`'s volume; `None` is UNKNOWN and never refuses.
    fn free_bytes(&self, dir: &Path) -> Option<u64>;
    /// Render to `work.dest`, reporting whole percents; the output's
    /// verified duration. A cancel kills the child and deletes `dest`.
    fn render(
        &self,
        work: &RenderWork<'_>,
        cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(u64),
    ) -> Result<u64, ScreenError>;
}

/// Everything a registered render job owns: the frozen graph, the plan and
/// the files it reads.
pub(crate) struct RenderJob {
    pub session_id: String,
    pub job_id: String,
    pub cancel: Arc<AtomicBool>,
    pub root: PathBuf,
    pub project_id: String,
    pub product_id: String,
    pub name: String,
    pub project: Project,
    pub revision: u64,
    pub range: Option<RangeDto>,
    pub plan: RenderPlan,
    pub inputs: Vec<PathBuf>,
    pub settings: EncodeSettings,
}

// ---- the ledger ----

fn ledger_path(root: &Path, project_id: &str) -> Result<PathBuf, EditorError> {
    project_dir(root, project_id)
        .map(|d| d.join(PRODUCTS_FILE))
        .ok_or_else(|| err(EditorErrorCode::InvalidRequest, "Not a valid project id."))
}

/// `products.json`, validated: absent is an empty ledger; anything else
/// that is not a list of products of THIS project, each named exactly
/// `<productId>.mp4`, is `invalidProject` -- never trusted in part.
pub(crate) fn read_ledger(root: &Path, project_id: &str) -> Result<Vec<Product>, EditorError> {
    let path = ledger_path(root, project_id)?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(internal(format!("Cannot read the product ledger: {e}"))),
        Ok(_) => {}
    }
    let bytes = read_bounded(&path, LEDGER_MAX_BYTES)?;
    let products: Vec<Product> = serde_json::from_slice(&bytes).map_err(|e| {
        err(
            EditorErrorCode::InvalidProject,
            format!("The product ledger is damaged: {e}"),
        )
    })?;
    let foreign = products
        .iter()
        .find(|p| !has_canonical_file_name(p) || p.project_id != project_id);
    if products.len() > limits::MAX_PRODUCTS || foreign.is_some() {
        return Err(err(
            EditorErrorCode::InvalidProject,
            "The product ledger lists a product this project cannot own.",
        ));
    }
    Ok(products)
}

/// Rewrite `products.json` (temp + fsync + replacing rename).
pub(crate) fn write_ledger(
    root: &Path,
    project_id: &str,
    products: &[Product],
) -> Result<(), EditorError> {
    let path = ledger_path(root, project_id)?;
    let json = serde_json::to_string_pretty(products)
        .map_err(|e| internal(format!("Could not encode the product ledger: {e}")))?;
    write_atomic_replacing(&path, &json).map_err(map_write_error)
}

fn is_plain_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

/// `editor_get_products`' body: the ledger as `ProductDto`s.
pub(crate) fn products_in(
    state: &EditorState,
    root: &Path,
    session_id: &str,
) -> Result<Vec<ProductDto>, EditorError> {
    let project_id = project_id_for(state, session_id)?;
    let dir = project_dir(root, &project_id)
        .ok_or_else(|| internal("Not a valid project id."))?
        .join(PRODUCTS_DIR);
    Ok(read_ledger(root, &project_id)?
        .into_iter()
        .map(|p| ProductDto {
            available: is_plain_file(&dir.join(&p.filename)),
            render_range: p.render_range.as_ref().map(|r| RangeDto {
                start_ms: r.start_ms,
                end_ms: r.end_ms,
            }),
            id: p.id,
            project_id: p.project_id,
            name: p.name,
            filename: p.filename,
            mime: p.mime,
            revision: p.revision,
            duration_ms: p.duration_ms,
            created_at: p.created_at,
            edit_fingerprint: p.edit_fingerprint,
        })
        .collect())
}

/// `editor_restore_product`'s body: the product's snapshot becomes the
/// live graph through `InternalCommand::RestoreSnapshot` -- a new revision,
/// one undo step, the product (file and record) untouched.
pub(crate) fn restore_in(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    expected_revision: u64,
    product_id: &str,
    command_id: &str,
) -> Result<EditorProjection, EditorError> {
    let project_id = project_id_for(state, session_id)?;
    let product = read_ledger(root, &project_id)?
        .into_iter()
        .find(|p| p.id == product_id)
        .ok_or_else(|| {
            err(
                EditorErrorCode::InvalidRequest,
                "There is no such product in this project.",
            )
        })?;
    let snapshot = product.snapshot.ok_or_else(|| {
        err(
            EditorErrorCode::InvalidRequest,
            "This product has no saved edit to restore.",
        )
    })?;
    let command = InternalCommand::RestoreSnapshot(Box::new(
        vault_buddy_core::editor::commands::payloads::RestoreSnapshotPayload {
            product_id: product_id.to_string(),
            project: *snapshot,
        },
    ));
    let mut sessions = require_session(state, session_id)?;
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| internal("session vanished under its own lock"))?;
    session.execute_internal_as(expected_revision, command_id, &command)?;
    let projection = EditorProjection::of(session);
    drop(sessions);
    super::recovery::note_acknowledged(state, root, session_id);
    Ok(projection)
}

// ---- starting a render ----

fn validated_quality(request: &RenderRequest) -> Result<ScreenQuality, EditorError> {
    let name = request.name.trim();
    if name.is_empty() || name.chars().count() > limits::MAX_NAME_CHARS {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            format!(
                "A render needs a name of 1 to {} characters.",
                limits::MAX_NAME_CHARS
            ),
        ));
    }
    ScreenQuality::from_key(&request.quality).ok_or_else(|| {
        err(
            EditorErrorCode::InvalidRequest,
            format!("{:?} is not a render quality.", request.quality),
        )
    })
}

/// The session's project at `expected_revision` (`revisionConflict`
/// otherwise), cloned so an edit landing mid-render never reaches it.
fn freeze(
    state: &EditorState,
    session_id: &str,
    expected_revision: u64,
) -> Result<(Project, u64), EditorError> {
    let sessions = require_session(state, session_id)?;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| internal("session vanished under its own lock"))?;
    let revision = session.snapshot().revision;
    if revision != expected_revision {
        return Err(err(
            EditorErrorCode::RevisionConflict,
            format!("expected revision {expected_revision} but the session is at {revision}"),
        ));
    }
    Ok((session.project().clone(), revision))
}

fn plan_source(input_index: usize, record: &SourceRecord) -> PlanSource {
    PlanSource {
        input_index,
        has_video: record.has_video,
        has_audio: record.has_audio,
        width: record.width.unwrap_or(0),
        height: record.height.unwrap_or(0),
        is_image: record.media_kind == SourceMediaKind::Image,
    }
}

/// Plan `project` over the records whose file is on disk, then plan AGAIN
/// over only the ones the first plan read, indexed from zero -- so the job
/// hands the render exactly `file_input_count(plan)` paths, the invariant
/// `render_args` asserts (Task 42's carry), by construction.
fn plan_with_inputs(
    root: &Path,
    project_id: &str,
    project: &Project,
    range: Option<(u64, u64)>,
) -> Result<(RenderPlan, Vec<PathBuf>), EditorError> {
    let on_disk: BTreeMap<String, (SourceRecord, PathBuf)> = load_sources(root, project_id)?
        .into_iter()
        .filter_map(|(id, record)| {
            let path = resolve_source(root, project_id, &record)?;
            is_plain_file(&path).then_some((id, (record, path)))
        })
        .collect();
    let all = on_disk
        .iter()
        .enumerate()
        .map(|(i, (id, (record, _)))| (id.clone(), plan_source(i, record)))
        .collect();
    let first = render_plan::plan(project, &all, range)?;
    let mut used = BTreeMap::new();
    let mut inputs = Vec::new();
    for input in &first.inputs {
        let (record, path) = &on_disk[&input.asset_id];
        used.insert(input.asset_id.clone(), plan_source(inputs.len(), record));
        inputs.push(path.clone());
    }
    let plan = render_plan::plan(project, &used, range)?;
    if file_input_count(&plan) != inputs.len() {
        return Err(internal(format!(
            "the render plan reads {} files but {} were resolved",
            file_input_count(&plan),
            inputs.len()
        )));
    }
    Ok((plan, inputs))
}

fn check_capacity(root: &Path, project_id: &str) -> Result<(), EditorError> {
    if read_ledger(root, project_id)?.len() >= limits::MAX_PRODUCTS {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            format!(
                "This project already has {} rendered products. Remove an older product first.",
                limits::MAX_PRODUCTS
            ),
        ));
    }
    Ok(())
}

fn check_space(
    runner: &dyn RenderRunner,
    dir: &Path,
    plan: &RenderPlan,
    settings: &EncodeSettings,
) -> Result<(), EditorError> {
    let needed = export_size_estimate_bytes(
        plan.duration_ms,
        settings.width,
        settings.height,
        settings.fps,
        settings.quality,
    );
    match export_space_shortfall(needed, runner.free_bytes(dir)) {
        None => Ok(()),
        Some(short) => Err(err(
            EditorErrorCode::DiskFull,
            format!(
                "Not enough disk space to render: about {} MB more is needed.",
                short.div_ceil(1024 * 1024)
            ),
        )),
    }
}

/// Every refusal, in the ADR's order, then the job's registration (one
/// render per session). `resolve` finds the toolchain; it runs only once
/// the session and revision are known good.
pub(crate) fn begin_render<R: RenderRunner>(
    state: &EditorState,
    root: &Path,
    request: &RenderRequest,
    resolve: impl FnOnce() -> Result<R, EditorError>,
) -> Result<(RenderJob, R), EditorError> {
    let quality = validated_quality(request)?;
    let (project, revision) = freeze(state, &request.session_id, request.expected_revision)?;
    let runner = resolve()?;
    let project_id = project.id.clone();
    let range = request.range.map(|r| (r.start_ms, r.end_ms));
    let (plan, inputs) = plan_with_inputs(root, &project_id, &project, range)?;
    if plan.duration_ms == 0 {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "There is nothing to render yet. Place a clip on the timeline first.",
        ));
    }
    let settings = EncodeSettings {
        width: plan.canvas.width,
        height: plan.canvas.height,
        fps: plan.canvas.fps,
        quality,
        h264_encoder: runner.h264_encoder(),
        has_audio: true,
    };
    if let Some(message) = runner.refusal(&plan, &settings) {
        return Err(err(EditorErrorCode::EncoderUnavailable, message));
    }
    check_capacity(root, &project_id)?;
    let dir = project_dir(root, &project_id).ok_or_else(|| internal("Not a valid project id."))?;
    check_space(&runner, &dir, &plan, &settings)?;
    let (job_id, cancel) = start_job_in(state, &request.session_id, JobKind::Render)?;
    let job = RenderJob {
        session_id: request.session_id.clone(),
        job_id,
        cancel,
        root: root.to_path_buf(),
        project_id,
        product_id: new_entity_id("prod"),
        name: request.name.trim().to_string(),
        project,
        revision,
        range: request.range,
        plan,
        inputs,
        settings,
    };
    Ok((job, runner))
}

// ---- running one ----

fn job_dir(job: &RenderJob) -> Result<PathBuf, EditorError> {
    project_dir(&job.root, &job.project_id)
        .map(|d| d.join(JOBS_DIR).join(&job.job_id))
        .ok_or_else(|| internal("Not a valid project id."))
}

fn render_error(e: ScreenError) -> EditorError {
    match e {
        ScreenError::Cancelled => cancelled(),
        ScreenError::ToolMissing => no_ffmpeg(),
        ScreenError::Refused(message) => err(EditorErrorCode::EncoderUnavailable, message),
        other => internal(format!("The render failed: {other}")),
    }
}

/// Move the verified part into `products\` and record it -- in THAT order
/// (module doc) -- under the session's save lock.
fn publish(
    job: &RenderJob,
    part: &Path,
    duration_ms: u64,
    state: &EditorState,
) -> Result<String, EditorError> {
    let lock = session_save_lock(state, &job.session_id).map_err(|_| cancelled())?;
    let _guard = lock_ignoring_poison(&lock);
    if job.cancel.load(Ordering::SeqCst) {
        return Err(cancelled());
    }
    let mut ledger = read_ledger(&job.root, &job.project_id)?;
    if ledger.len() >= limits::MAX_PRODUCTS {
        return Err(err(
            EditorErrorCode::InvalidRequest,
            "This project already has the most rendered products it can keep. Remove an older \
             product first.",
        ));
    }
    let products = project_dir(&job.root, &job.project_id)
        .ok_or_else(|| internal("Not a valid project id."))?
        .join(PRODUCTS_DIR);
    std::fs::create_dir_all(&products).map_err(map_write_error)?;
    let filename = product_file_name(&job.product_id);
    let dest = products.join(&filename);
    rename_noreplace(part, &dest)
        .map_err(|e| internal(format!("The render could not be kept: {e}")))?;
    let range = job.range.map(|r| RenderRange {
        start_ms: r.start_ms,
        end_ms: r.end_ms,
        extra: Map::new(),
    });
    let now = chrono::Local::now().to_rfc3339();
    ledger.push(new_product(
        &job.project,
        job.revision,
        &job.product_id,
        &job.name,
        &filename,
        duration_ms,
        range,
        &now,
    ));
    if let Err(e) = write_ledger(&job.root, &job.project_id, &ledger) {
        // Never recorded, so never a product: take the file back out.
        if let Err(remove) = std::fs::remove_file(&dest) {
            log::warn!("editor render: could not remove an unrecorded product: {remove}");
        }
        return Err(e);
    }
    Ok(job.product_id.clone())
}

fn render_and_publish(
    state: &EditorState,
    job: &RenderJob,
    runner: &dyn RenderRunner,
    reporter: &mut JobReporter<'_>,
) -> Result<String, EditorError> {
    if job.cancel.load(Ordering::SeqCst) {
        return Err(cancelled());
    }
    let dir = job_dir(job)?;
    std::fs::create_dir_all(&dir).map_err(map_write_error)?;
    let dest = dir.join(PART_FILE);
    reporter.progress(JobPhase::Rendering, 0.0);
    let work = RenderWork {
        plan: &job.plan,
        inputs: &job.inputs,
        job_dir: &dir,
        dest: &dest,
        settings: &job.settings,
    };
    let duration_ms = runner
        .render(&work, &job.cancel, &mut |percent| {
            reporter.progress(JobPhase::Rendering, percent as f64 / 100.0);
        })
        .map_err(render_error)?;
    reporter.progress(JobPhase::Publishing, 1.0);
    publish(job, &dest, duration_ms, state)
}

/// Remove the job's own directory (its part, its ASS documents), owned and
/// no-follow. Logged, never raised: the terminal still has to be sent.
fn remove_job_dir(job: &RenderJob) {
    let Ok(dir) = job_dir(job) else {
        return;
    };
    if std::fs::symlink_metadata(&dir).is_err() {
        return;
    }
    if let Err(e) = super::store_io::remove_dir_no_follow(&dir) {
        log::warn!("editor render: could not remove the job directory: {e}");
    }
}

/// The render job's body, on the `editor-render` thread: exactly one
/// terminal, sent only after the job directory is gone.
pub(crate) fn run_render_job(
    state: &EditorState,
    job: RenderJob,
    runner: &dyn RenderRunner,
    sink: &dyn ProgressSink,
) {
    let mut reporter = JobReporter::new(
        &state.jobs,
        sink,
        &job.session_id,
        &job.job_id,
        JobKind::Render,
    );
    reporter.progress(JobPhase::Preparing, 0.0);
    let outcome = render_and_publish(state, &job, runner, &mut reporter);
    remove_job_dir(&job);
    match outcome {
        Ok(product_id) => reporter.finish(
            JobPhase::Complete,
            JobTerminal {
                product_id: Some(product_id),
                ..JobTerminal::default()
            },
        ),
        Err(e) => {
            let phase = if e.code == EditorErrorCode::Cancelled {
                JobPhase::Cancelled
            } else {
                log::warn!("editor render {} failed: {}", job.job_id, e.message);
                JobPhase::Failed
            };
            reporter.finish(
                phase,
                JobTerminal {
                    error: Some(e),
                    ..JobTerminal::default()
                },
            );
        }
    }
}

// ---- the shutdown gate (R12) ----

/// Is a render in a phase a process exit would destroy?
pub(crate) fn render_blocks_shutdown(state: &EditorState) -> bool {
    lock_ignoring_poison(&state.jobs).blocks_shutdown()
}

/// Cancel every render and wait, at most `limit`, for all of them to end;
/// `true` iff they did.
pub(crate) fn cancel_all_in(state: &EditorState, limit: Duration, poll: Duration) -> bool {
    lock_ignoring_poison(&state.jobs).cancel_kind(JobKind::Render);
    crate::export_shutdown::wait_until_cleared(
        || !lock_ignoring_poison(&state.jobs).any_running(JobKind::Render),
        limit,
        poll,
    )
}

/// `shutdown_gate`'s fourth term (R12).
pub fn blocks_shutdown(app: &AppHandle) -> bool {
    !RENDERS_ABANDONED.load(Ordering::SeqCst) && render_blocks_shutdown(&app.state::<EditorState>())
}

/// The quit workers' render step, beside `export_shutdown::
/// cancel_if_exporting` and before the capture finalizes: kill every
/// render (its part is deleted) and wait, bounded. On expiry it LOGS and
/// proceeds -- a wedged render must never make the app unquittable.
///
/// Callers must NOT be on the main/event-loop thread: this sleeps.
pub fn cancel_all_bounded(app: &AppHandle, limit: Duration) {
    let state = app.state::<EditorState>();
    if !lock_ignoring_poison(&state.jobs).any_running(JobKind::Render) {
        return;
    }
    log::info!("editor render: cancelling every render before shutdown");
    if !cancel_all_in(&state, limit, CANCEL_POLL) {
        RENDERS_ABANDONED.store(true, Ordering::SeqCst);
        log::warn!("editor render: a render did not stop within {limit:?}; exiting anyway");
    }
}

#[cfg(test)]
#[path = "render_jobs_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "render_ledger_tests.rs"]
mod ledger_tests;
