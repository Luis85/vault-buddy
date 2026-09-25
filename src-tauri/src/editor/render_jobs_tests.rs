//! `render_jobs.rs`' tests (Task 46): a real tempdir project and session,
//! and a FAKE runner in place of ffmpeg -- the render's own correctness is
//! `screen::render`'s (its round trips run the real ffmpeg); what these pin
//! is the JOB: what is refused before anything starts, what is left on
//! disk after a cancel, a failure or a success, and the ledger.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use serde_json::json;
use vault_buddy_core::editor::{
    edit_fingerprint, Asset, AssetKind, EditorCommand, EditorErrorCode, EditorSession,
    ExecuteRequest, Map,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::ScreenError;

use super::*;
use crate::editor::media_jobs::tests::CollectingSink;
use crate::editor::media_jobs::{cancel_job_in, jobs_in, JobPhase, JobProgressDto};
use crate::editor::project_store::{minimal_project, SourceLocator, SourceRecord};
use crate::editor::session_commands::{close_in, execute_in, CloseDisposition};
use crate::editor::store_io::create_project;

pub(super) const SESSION: &str = "ses-proj1";
pub(super) const PROJECT: &str = "proj1";

/// What the fake does in place of ffmpeg.
#[derive(Clone)]
pub(super) enum Behaviour {
    /// Writes these bytes to the part and reports success.
    Writes(Vec<u8>),
    /// Writes a part, signals `started`, then waits for the cancel flag and
    /// removes the part -- the real runner's cancel (kill + delete).
    UntilCancelled,
    /// Writes a part and reports an ffmpeg failure WITHOUT removing it, so
    /// the job itself is what has to clean up.
    Fails,
    /// Reports success but produced nothing at all.
    ClaimsWithoutOutput,
    /// Panics mid-render, as a bug in the render path would.
    Panics,
}

pub(super) struct FakeRunner {
    behaviour: Behaviour,
    started: Mutex<Option<mpsc::Sender<()>>>,
    finished: AtomicBool,
    pub(super) free: Option<u64>,
    pub(super) refuse: Option<String>,
}

impl FakeRunner {
    pub(super) fn new(behaviour: Behaviour) -> Self {
        Self {
            behaviour,
            started: Mutex::new(None),
            finished: AtomicBool::new(false),
            free: None,
            refuse: None,
        }
    }

    pub(super) fn signalling(behaviour: Behaviour) -> (Self, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        let runner = Self::new(behaviour);
        *runner.started.lock().unwrap() = Some(tx);
        (runner, rx)
    }
}

impl RenderRunner for FakeRunner {
    fn h264_encoder(&self) -> String {
        "libx264".into()
    }

    fn refusal(&self, _plan: &RenderPlan, _settings: &EncodeSettings) -> Option<String> {
        self.refuse.clone()
    }

    fn free_bytes(&self, _dir: &Path) -> Option<u64> {
        self.free
    }

    fn render(
        &self,
        work: &RenderWork<'_>,
        cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(u64),
    ) -> Result<u64, ScreenError> {
        let result = match &self.behaviour {
            Behaviour::Writes(bytes) => {
                std::fs::write(work.dest, bytes).unwrap();
                on_progress(50);
                on_progress(100);
                Ok(work.plan.duration_ms)
            }
            Behaviour::UntilCancelled => {
                std::fs::write(work.dest, b"partial").unwrap();
                if let Some(tx) = self.started.lock().unwrap().take() {
                    tx.send(()).unwrap();
                }
                while !cancel.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                // A kill and a reap take a moment; a discard must wait them out.
                std::thread::sleep(Duration::from_millis(150));
                std::fs::remove_file(work.dest).unwrap();
                Err(ScreenError::Cancelled)
            }
            Behaviour::Fails => {
                std::fs::write(work.dest, b"truncated").unwrap();
                // What ffmpeg's stderr really says: the absolute paths of
                // what it read and wrote (final review M8).
                let input = work.inputs.first().map(|p| p.display().to_string());
                Err(ScreenError::Sink(format!(
                    "ffmpeg exited with status 1: {}: Invalid data found; Error opening {}",
                    input.unwrap_or_default(),
                    work.dest.display()
                )))
            }
            Behaviour::ClaimsWithoutOutput => Ok(work.plan.duration_ms),
            Behaviour::Panics => panic!("a bug in the render path"),
        };
        self.finished.store(true, Ordering::SeqCst);
        result
    }
}

/// Cancels a render when dropped -- including when an assertion inside a
/// `thread::scope` panics, which would otherwise wait forever on a fake
/// that only ends when cancelled.
pub(super) struct CancelOnDrop(pub(super) std::sync::Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn asset(id: &str, duration_ms: u64) -> Asset {
    Asset {
        id: id.to_string(),
        kind: AssetKind::Video,
        name: format!("{id}.mp4"),
        duration_ms,
        width: None,
        height: None,
        size: None,
        builtin: None,
        media_type: None,
        linked_asset: None,
        original_name: None,
        extra: Map::new(),
    }
}

fn record(file: &str, duration_ms: u64) -> SourceRecord {
    SourceRecord {
        locator: SourceLocator::Media { file: file.into() },
        sha256: None,
        size: 3,
        duration_ms,
        width: Some(1280),
        height: Some(720),
        has_audio: true,
        has_video: true,
        media_kind: SourceMediaKind::Video,
        replaced_from: None,
    }
}

pub(super) fn execute(state: &EditorState, root: &Path, command: serde_json::Value) {
    let revision = snapshot_revision(state);
    let request = ExecuteRequest {
        session_id: SESSION.into(),
        expected_revision: revision,
        command_id: vault_buddy_core::editor::new_entity_id("cmd"),
        command: serde_json::from_value::<EditorCommand>(command).unwrap(),
    };
    execute_in(state, root, &request).unwrap();
}

pub(super) fn snapshot_revision(state: &EditorState) -> u64 {
    lock_ignoring_poison(&state.sessions)[SESSION]
        .snapshot()
        .revision
}

/// A live session over a project with two imported videos on disk, only
/// `cap` placed on the timeline (0-2000 ms). Returns the project dir.
pub(super) fn opened(root: &Path, state: &EditorState) -> PathBuf {
    let mut project = minimal_project(PROJECT);
    project.assets.push(asset("cap", 2_000));
    project.assets.push(asset("spare", 1_000));
    let mut sources = BTreeMap::new();
    sources.insert("cap".to_string(), record("cap.mp4", 2_000));
    sources.insert("spare".to_string(), record("spare.mp4", 1_000));
    create_project(root, &project, &sources).unwrap();
    let dir = project_dir(root, PROJECT).unwrap();
    std::fs::create_dir_all(dir.join("media")).unwrap();
    std::fs::write(dir.join("media").join("cap.mp4"), b"cap").unwrap();
    std::fs::write(dir.join("media").join("spare.mp4"), b"spa").unwrap();
    lock_ignoring_poison(&state.sessions).insert(
        SESSION.to_string(),
        EditorSession::resume(SESSION, project, 1),
    );
    lock_ignoring_poison(&state.by_project).insert(PROJECT.into(), SESSION.into());
    execute(
        state,
        root,
        json!({"kind": "addTrack", "trackKind": "video", "name": "V1", "index": 0}),
    );
    let track = lock_ignoring_poison(&state.sessions)[SESSION]
        .project()
        .tracks[0]
        .id
        .clone();
    execute(
        state,
        root,
        json!({"kind": "insertClip", "assetId": "cap", "trackId": track,
               "startMs": 0, "inMs": 0, "outMs": 2_000}),
    );
    dir
}

pub(super) fn request(expected_revision: u64) -> RenderRequest {
    serde_json::from_value(json!({
        "sessionId": SESSION,
        "expectedRevision": expected_revision,
        "name": "Walkthrough",
        "range": {"startMs": 250, "endMs": 1_500},
        "quality": "high",
    }))
    .unwrap()
}

pub(super) fn begin(
    state: &EditorState,
    root: &Path,
    runner: FakeRunner,
) -> Result<(RenderJob, FakeRunner), EditorError> {
    begin_render(state, root, &request(snapshot_revision(state)), || {
        Ok(runner)
    })
}

pub(super) fn terminal(sink: &CollectingSink) -> JobProgressDto {
    let messages = sink.messages();
    let last = messages.last().expect("a terminal message").clone();
    assert!(last.phase.is_terminal(), "{last:?}");
    assert_eq!(
        messages.iter().filter(|m| m.phase.is_terminal()).count(),
        1,
        "exactly one terminal"
    );
    last
}

pub(super) fn entries(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(it) => it
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// A product file's bytes and modification time: "never opened for
/// writing" means neither changes.
fn fingerprint(path: &Path) -> (Vec<u8>, std::time::SystemTime) {
    let modified = std::fs::metadata(path).unwrap().modified().unwrap();
    (std::fs::read(path).unwrap(), modified)
}

/// The refusal a `begin_render` was expected to make (`RenderJob` is not
/// `Debug`, so `unwrap_err` cannot name a job that started by mistake).
pub(super) fn refused<T>(result: Result<T, EditorError>) -> EditorError {
    match result {
        Err(e) => e,
        Ok(_) => panic!("expected a refusal, but the render started"),
    }
}

// ---- the wire ----

// Literal JSON, never a struct re-serialized against itself (GAP-135).
#[test]
fn render_wire_shapes_are_pinned() {
    let req = request(4);
    assert_eq!(req.session_id, SESSION);
    assert_eq!(req.expected_revision, 4);
    assert_eq!(req.name, "Walkthrough");
    assert_eq!(req.quality, "high");
    let range = req.range.unwrap();
    assert_eq!((range.start_ms, range.end_ms), (250, 1_500));
    let whole: RenderRequest = serde_json::from_value(json!({
        "sessionId": "s", "expectedRevision": 1, "name": "n", "range": null, "quality": "low"
    }))
    .unwrap();
    assert!(whole.range.is_none());

    assert_eq!(
        serde_json::to_value(RenderStarted {
            job_id: "job-a".into(),
            revision: 7
        })
        .unwrap(),
        json!({"jobId": "job-a", "revision": 7})
    );
    let dto = ProductDto {
        id: "prod-a".into(),
        project_id: "proj1".into(),
        name: "Walkthrough".into(),
        filename: "prod-a.mp4".into(),
        mime: "video/mp4".into(),
        revision: 3,
        duration_ms: 1_250,
        created_at: "2026-09-24T10:00:00+02:00".into(),
        edit_fingerprint: "sha256:ab".into(),
        render_range: Some(RangeDto {
            start_ms: 250,
            end_ms: 1_500,
        }),
        available: true,
    };
    assert_eq!(
        serde_json::to_value(&dto).unwrap(),
        json!({
            "id": "prod-a", "projectId": "proj1", "name": "Walkthrough",
            "filename": "prod-a.mp4", "mime": "video/mp4", "revision": 3,
            "durationMs": 1_250, "createdAt": "2026-09-24T10:00:00+02:00",
            "editFingerprint": "sha256:ab",
            "renderRange": {"startMs": 250, "endMs": 1_500}, "available": true
        })
    );
    let whole = ProductDto {
        render_range: None,
        available: false,
        ..dto
    };
    let v = serde_json::to_value(&whole).unwrap();
    assert_eq!(v["renderRange"], json!(null), "present-even-when-null");
    assert_eq!(v["available"], json!(false));
}

// ---- refusals before anything starts ----

// A pending edit cannot be rendered: the frozen revision is the one the
// caller saw, and a stale one is refused before a job, a directory or a
// child exists.
#[test]
fn render_of_a_stale_revision_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let stale = snapshot_revision(&state) - 1;
    let resolved = AtomicBool::new(false);
    let e = refused(begin_render(&state, root.path(), &request(stale), || {
        resolved.store(true, Ordering::SeqCst);
        Ok(FakeRunner::new(Behaviour::Writes(b"x".to_vec())))
    }));
    assert_eq!(e.code, EditorErrorCode::RevisionConflict);
    assert!(
        !resolved.load(Ordering::SeqCst),
        "the revision is checked before ffmpeg is even looked for"
    );
    assert!(jobs_in(&state, SESSION).unwrap().is_empty());
    assert!(!dir.join(JOBS_DIR).exists());
}

// ffmpeg's absence, a capability refusal and a full disk are each refused
// up front, with the code the frontend acts on -- and none registers a job.
#[test]
fn a_missing_tool_a_capability_refusal_and_a_full_disk_refuse_up_front() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state);
    let rev = snapshot_revision(&state);
    let e = refused(begin_render(&state, root.path(), &request(rev), || {
        Err::<FakeRunner, _>(no_ffmpeg())
    }));
    assert_eq!(e.code, EditorErrorCode::EncoderUnavailable);
    assert!(e.message.contains("Buddy settings"), "{}", e.message);

    let mut refusing = FakeRunner::new(Behaviour::Writes(b"x".to_vec()));
    refusing.refuse = Some("needs the \"xfade\" filter".into());
    let e = refused(begin(&state, root.path(), refusing));
    assert_eq!(e.code, EditorErrorCode::EncoderUnavailable);
    assert!(e.message.contains("xfade"));

    let mut full = FakeRunner::new(Behaviour::Writes(b"x".to_vec()));
    full.free = Some(1);
    let e = refused(begin(&state, root.path(), full));
    assert_eq!(e.code, EditorErrorCode::DiskFull);
    assert!(jobs_in(&state, SESSION).unwrap().is_empty());

    // An UNMEASURABLE volume (None) never refuses.
    let unknown = FakeRunner::new(Behaviour::Writes(b"x".to_vec()));
    assert!(begin(&state, root.path(), unknown).is_ok());
}

// The job hands the render EXACTLY the source files the plan reads, indexed
// from zero -- `render_args`' input-count assertion holds by construction,
// even though the project also holds an unused imported file.
#[test]
fn a_job_hands_the_render_exactly_the_inputs_its_plan_reads() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let (job, _) = begin(
        &state,
        root.path(),
        FakeRunner::new(Behaviour::Writes(Vec::new())),
    )
    .unwrap();
    assert_eq!(job.inputs, [dir.join("media").join("cap.mp4")]);
    assert_eq!(
        vault_buddy_screen::render::video_graph::file_input_count(&job.plan),
        job.inputs.len()
    );
    assert_eq!(job.plan.duration_ms, 1_250, "the range was frozen");
}

#[test]
fn only_one_render_per_session() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state);
    let (runner, started) = FakeRunner::signalling(Behaviour::UntilCancelled);
    let (job, runner) = begin(&state, root.path(), runner).unwrap();
    let job_id = job.job_id.clone();
    let sink = CollectingSink::default();
    std::thread::scope(|scope| {
        let _stop = CancelOnDrop(job.cancel.clone());
        scope.spawn(|| run_render_job(&state, job, &runner, &sink));
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        let e = refused(begin(
            &state,
            root.path(),
            FakeRunner::new(Behaviour::Writes(b"x".to_vec())),
        ));
        assert_eq!(e.code, EditorErrorCode::InvalidRequest);
        assert!(
            e.message.contains("A render is already running"),
            "{}",
            e.message
        );
        cancel_job_in(&state, SESSION, &job_id).unwrap();
    });
    assert_eq!(jobs_in(&state, SESSION).unwrap().len(), 1);
}

// ---- what a job leaves behind ----

// A16: the bytes land at products\<productId>.mp4, the ledger records the
// frozen revision, snapshot, fingerprint and range -- and a LATER edit plus
// a restore of that product leaves the product's file and record untouched
// while the live graph returns to the rendered one, one undo from the
// pre-restore graph.
#[test]
fn completed_render_records_an_immutable_product() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let rendered = lock_ignoring_poison(&state.sessions)[SESSION]
        .project()
        .clone();
    let rendered_revision = snapshot_revision(&state);
    let (job, runner) = begin(
        &state,
        root.path(),
        FakeRunner::new(Behaviour::Writes(b"rendered bytes".to_vec())),
    )
    .unwrap();
    let job_id = job.job_id.clone();
    let product_id = job.product_id.clone();
    let sink = CollectingSink::default();
    run_render_job(&state, job, &runner, &sink);

    let last = terminal(&sink);
    assert_eq!(last.phase, JobPhase::Complete);
    assert_eq!(last.kind, JobKind::Render);
    let phases: Vec<JobPhase> = sink.messages().iter().map(|m| m.phase).collect();
    for phase in [
        JobPhase::Preparing,
        JobPhase::Rendering,
        JobPhase::Publishing,
    ] {
        assert!(phases.contains(&phase), "{phase:?} in {phases:?}");
    }
    assert_eq!(
        last.terminal.unwrap().product_id.as_deref(),
        Some(product_id.as_str())
    );
    let file = dir.join(PRODUCTS_DIR).join(format!("{product_id}.mp4"));
    assert_eq!(std::fs::read(&file).unwrap(), b"rendered bytes");
    assert!(
        !dir.join(JOBS_DIR).join(&job_id).exists(),
        "job dir removed"
    );

    let ledger = read_ledger(root.path(), PROJECT).unwrap();
    assert_eq!(ledger.len(), 1);
    let product = &ledger[0];
    assert_eq!(product.id, product_id);
    assert_eq!(product.filename, format!("{product_id}.mp4"));
    assert_eq!(product.revision, rendered_revision);
    assert_eq!(product.name, "Walkthrough");
    assert_eq!(product.duration_ms, 1_250);
    assert_eq!(product.edit_fingerprint, edit_fingerprint(&rendered));
    assert_eq!(product.snapshot.as_deref(), Some(&rendered));
    let range = product.render_range.as_ref().unwrap();
    assert_eq!((range.start_ms, range.end_ms), (250, 1_500));
    let listed = products_in(&state, root.path(), SESSION).unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].available);

    let before = fingerprint(&file);
    let ledger_bytes = std::fs::read(dir.join(PRODUCTS_FILE)).unwrap();
    execute(
        &state,
        root.path(),
        json!({"kind": "rename", "title": "Edited later"}),
    );
    let edited = lock_ignoring_poison(&state.sessions)[SESSION]
        .project()
        .clone();
    let rev = snapshot_revision(&state);
    let stale = restore_in(&state, root.path(), SESSION, rev - 1, &product_id, "cmd-r0");
    assert_eq!(stale.unwrap_err().code, EditorErrorCode::RevisionConflict);
    let restored = restore_in(&state, root.path(), SESSION, rev, &product_id, "cmd-r1").unwrap();
    assert_eq!(restored.project, rendered, "the frozen graph is live again");
    assert_eq!(restored.snapshot.revision, rev + 1, "a new revision");
    assert_eq!(fingerprint(&file), before, "the product file is untouched");
    assert_eq!(
        std::fs::read(dir.join(PRODUCTS_FILE)).unwrap(),
        ledger_bytes,
        "the ledger is untouched"
    );
    execute(&state, root.path(), json!({"kind": "undo"}));
    assert_eq!(
        lock_ignoring_poison(&state.sessions)[SESSION].project(),
        &edited,
        "undo returns to the pre-restore graph"
    );
    let e = restore_in(&state, root.path(), SESSION, rev + 2, "prod-nope", "cmd-r2").unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
}

#[test]
fn cancel_deletes_the_part_and_keeps_the_project() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let project_json = std::fs::read(dir.join("project.json")).unwrap();
    let revision = snapshot_revision(&state);
    let (runner, started) = FakeRunner::signalling(Behaviour::UntilCancelled);
    let (job, runner) = begin(&state, root.path(), runner).unwrap();
    let job_id = job.job_id.clone();
    let sink = CollectingSink::default();
    std::thread::scope(|scope| {
        let _stop = CancelOnDrop(job.cancel.clone());
        scope.spawn(|| run_render_job(&state, job, &runner, &sink));
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(dir.join(JOBS_DIR).join(&job_id).join(PART_FILE).is_file());
        cancel_job_in(&state, SESSION, &job_id).unwrap();
    });
    let last = terminal(&sink);
    assert_eq!(last.phase, JobPhase::Cancelled);
    assert_eq!(
        last.terminal.unwrap().error.map(|e| e.code),
        Some(EditorErrorCode::Cancelled)
    );
    assert!(!dir.join(JOBS_DIR).join(&job_id).exists(), "part deleted");
    assert!(entries(&dir.join(PRODUCTS_DIR)).is_empty());
    assert!(read_ledger(root.path(), PROJECT).unwrap().is_empty());
    assert_eq!(
        std::fs::read(dir.join("project.json")).unwrap(),
        project_json
    );
    assert_eq!(
        snapshot_revision(&state),
        revision,
        "the project is untouched"
    );
}

// Two ways to fail, and neither may leave a product, a part or a ledger
// entry: ffmpeg failing (its truncated part left behind for the JOB to
// remove), and a "success" whose output never reached disk -- the move is
// what fails, and a ledger written before the move would then record a
// product with no file (the mutation check).
#[test]
fn failed_render_leaves_no_product_and_no_part() {
    for behaviour in [Behaviour::Fails, Behaviour::ClaimsWithoutOutput] {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let dir = opened(root.path(), &state);
        let (job, runner) = begin(&state, root.path(), FakeRunner::new(behaviour)).unwrap();
        let job_id = job.job_id.clone();
        let sink = CollectingSink::default();
        run_render_job(&state, job, &runner, &sink);
        let last = terminal(&sink);
        assert_eq!(last.phase, JobPhase::Failed);
        let error = last.terminal.unwrap().error.expect("the failure is named");
        // Final review M8: ffmpeg's stderr names absolute paths; the
        // webview gets a fixed sentence and the log a redacted copy.
        let root_text = root.path().display().to_string();
        assert!(!error.message.contains(&root_text), "{}", error.message);
        assert!(!dir.join(JOBS_DIR).join(&job_id).exists(), "no part left");
        assert!(entries(&dir.join(PRODUCTS_DIR)).is_empty(), "no product");
        assert!(
            read_ledger(root.path(), PROJECT).unwrap().is_empty(),
            "no ledger entry without a file"
        );
    }
}

// ---- the shutdown gate and the discard ----

// R12: a render blocks shutdown while its child writes (`rendering`) or its
// product is moved and recorded (`publishing`) -- and a quit's bounded
// cancel ends it.
#[test]
fn shutdown_is_blocked_while_rendering() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state);
    assert!(!render_blocks_shutdown(&state));
    let (runner, started) = FakeRunner::signalling(Behaviour::UntilCancelled);
    let (job, runner) = begin(&state, root.path(), runner).unwrap();
    // Fix round 1 (review Minor 6): a queued or preparing render is seconds
    // from a child, so the quit path must cancel and wait for it too -- the
    // gate and the cancel read the SAME set (every non-terminal render).
    assert!(
        render_blocks_shutdown(&state),
        "a queued render is on its way to a child"
    );
    let sink = CollectingSink::default();
    std::thread::scope(|scope| {
        let _stop = CancelOnDrop(job.cancel.clone());
        scope.spawn(|| run_render_job(&state, job, &runner, &sink));
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(render_blocks_shutdown(&state));
        assert!(
            cancel_all_in(&state, Duration::from_secs(10), Duration::from_millis(5)),
            "the bounded cancel saw the render end"
        );
        assert!(!render_blocks_shutdown(&state));
    });
    assert_eq!(terminal(&sink).phase, JobPhase::Cancelled);
    // A wedged render does not make the quit wait forever.
    let mut jobs = lock_ignoring_poison(&state.jobs);
    let (wedged, _) = jobs.register(SESSION, JobKind::Render);
    drop(jobs);
    JobReporter::new(&state.jobs, &sink, SESSION, &wedged, JobKind::Render)
        .progress(JobPhase::Rendering, 0.5);
    assert!(!cancel_all_in(
        &state,
        Duration::from_millis(40),
        Duration::from_millis(5)
    ));
}

// Fix round 1 (review Minor 3): the gate and the quit's cancel agree BY
// KIND. Another kind in `publishing` (Task 48's publish job will use it) is
// not a render: counting it would report it as "a video is being rendered"
// and, since the render cancel never ends it, re-open GAP-190's loop.
#[test]
fn only_render_jobs_count_for_the_render_gate() {
    let state = EditorState::default();
    let sink = CollectingSink::default();
    let (other, _) = lock_ignoring_poison(&state.jobs).register(SESSION, JobKind::Import);
    JobReporter::new(&state.jobs, &sink, SESSION, &other, JobKind::Import)
        .progress(JobPhase::Publishing, 0.5);
    assert!(!render_blocks_shutdown(&state));
    assert!(cancel_all_in(
        &state,
        Duration::from_millis(40),
        Duration::from_millis(5)
    ));
}

// Fix round 1 (review Minor 7): a panic on the render thread still ends
// the job -- `failed`, with its directory gone -- rather than leaving it
// `rendering` forever, refusing every later render and the updater.
#[test]
fn a_panicking_render_ends_the_job_as_failed() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let (job, runner) = begin(&state, root.path(), FakeRunner::new(Behaviour::Panics)).unwrap();
    let job_id = job.job_id.clone();
    let sink = CollectingSink::default();
    run_render_job(&state, job, &runner, &sink);
    let last = terminal(&sink);
    assert_eq!(last.phase, JobPhase::Failed);
    assert_eq!(
        last.terminal.unwrap().error.map(|e| e.code),
        Some(EditorErrorCode::Internal)
    );
    assert!(!render_blocks_shutdown(&state));
    assert!(!dir.join(JOBS_DIR).join(&job_id).exists());
}

// A discard removes the project directory the render is writing into, so
// the render must be KILLED and its part deleted before the removal -- not
// merely told to stop by the session's close afterwards.
#[test]
fn discarding_a_project_while_rendering_stops_the_render_first() {
    let root = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let (runner, started) = FakeRunner::signalling(Behaviour::UntilCancelled);
    let (job, runner) = begin(&state, root.path(), runner).unwrap();
    let sink = CollectingSink::default();
    std::thread::scope(|scope| {
        let _stop = CancelOnDrop(job.cancel.clone());
        scope.spawn(|| run_render_job(&state, job, &runner, &sink));
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        close_in(
            &state,
            root.path(),
            staging.path(),
            SESSION,
            CloseDisposition::DiscardProject,
        )
        .unwrap();
        assert!(
            runner.finished.load(Ordering::SeqCst),
            "the render was still running when the discard returned"
        );
    });
    assert!(!dir.exists(), "the project is gone");
    assert_eq!(terminal(&sink).phase, JobPhase::Cancelled);
}

// R12, structural: both quit workers cancel the render (bounded) BEFORE the
// two unbounded capture finalizes -- a render left running behind them
// would keep an ffmpeg child writing for as long as they take. The hide
// chokepoint does not gate on renders.
#[test]
fn quit_cancels_a_render_before_finalizing_captures() {
    use crate::structural_scan::{fn_body, offset_of, shell_file};

    let tray = shell_file("tray.rs");
    let close = shell_file("window_close.rs");
    for (name, body) in [
        ("tray::quit", fn_body(&tray, "pub fn quit(")),
        (
            "handle_main_close",
            fn_body(&close, "fn handle_main_close("),
        ),
    ] {
        let cancel = offset_of(body, "render_jobs::cancel_all_bounded(");
        for later in ["finalize_if_recording(", "finalize_if_capturing("] {
            assert!(
                cancel < offset_of(body, later),
                "{name}: the render must be cancelled before {later}"
            );
        }
    }
    let hide = fn_body(&tray, "pub fn hide_buddy(");
    assert!(!hide.contains("render_jobs"), "hide is not a quit (R12)");
}
