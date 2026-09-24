//! The editor's background-job channel (Task 25; ADR §3.3 "Job progress";
//! IPC-CONTRACTS.md "Progress, cancellation and reconciliation"): the wire
//! DTOs, the per-process `JobRegistry` that `editor_get_jobs` answers from,
//! and `JobReporter`, the ONE thing that emits a job's progress.
//!
//! **Jobs never use events.** Progress travels on the per-job
//! `tauri::ipc::Channel` the caller handed in (ADR invariant 7), scoped to
//! the one subscriber that started the job — never `app.emit`, which would
//! broadcast one editor's import to every webview.
//!
//! Two properties the frontend's `editorJobs` store relies on, both made
//! structural here rather than left to each job's author:
//! - **`sequence` strictly increases per job.** `JobReporter` owns the
//!   counter and bumps it before every message; nothing else emits.
//! - **Exactly one terminal message, and it is the last.** `finish` takes
//!   the reporter BY VALUE, so a job cannot report again after its terminal
//!   without a second reporter, and `progress` refuses a terminal phase.
//!
//! The registry is the AUTHORITATIVE record — a message the channel drops
//! (a webview reload, a listener disposed mid-job) is recovered by
//! `editor_get_jobs`, which reads it. Every message updates the record
//! BEFORE it is delivered, so a reconcile can never be older than the event
//! stream it replaces. Render (Task 46, `render_jobs`) and the shutdown gate
//! reuse this registry: `any_running(JobKind::Render)` is the gate's question.
//!
//! **Bounded (GAP-174, Task 46).** A session keeps at most
//! `MAX_TERMINAL_RECORDS` terminal records -- the most recent ones, so a
//! reconcile still sees a job that just finished -- and a closing session's
//! terminal records go with it (`forget_terminal`). A running job is never
//! pruned: its terminal message still has to land somewhere.
//!
//! **Lock order:** `EditorState::jobs` is a LEAF lock — taken only to read
//! or update one record, never held across I/O, a send, or while taking any
//! other lock. It may be taken while `by_project`/`sessions` are held
//! (`drop_session` does), for the same reason `save_locks`' map may.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::ipc::Channel;
use vault_buddy_core::editor::{new_entity_id, EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::require_session;
use super::EditorState;

/// How many TERMINAL records one session keeps (GAP-174): enough for a
/// reconcile after a reload to see what just finished, few enough that a
/// long session rendering and decoding waveforms does not grow the
/// registry without bound.
pub(crate) const MAX_TERMINAL_RECORDS: usize = 8;

/// `JobProgressDto.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Import,
    /// An editor render (Task 46, `render_jobs`). Exclusive: one per
    /// session. Its wire spelling, `"render"`, is what the close guard
    /// (`useEditorCloseGuard`) matches on.
    Render,
    /// A waveform decode (Task 28, `media_derive`). Not exclusive: several
    /// may be registered at once (one per visible asset); `media_derive`'s
    /// own gate runs them one at a time.
    Peaks,
    /// A publication into a vault (Task 48, `publish`, the tenth
    /// sanctioned vault write). Exclusive: one per session. It has no
    /// Channel -- `editor_publish_product` answers with its receipt -- but
    /// it is registered so `editor_get_jobs`, the close guard and the
    /// shutdown gate's publish term all see it while it writes.
    Publish,
}

impl JobKind {
    /// May only ONE job of this kind run per session? An import is (two
    /// batches racing the capacity check could roll each other back); a
    /// peaks decode is not — the timeline asks for every visible asset's
    /// waveform at once, and refusing all but one would draw one waveform.
    fn exclusive(self) -> bool {
        matches!(self, Self::Import | Self::Render | Self::Publish)
    }

    /// The refusal a second exclusive job of this kind gets.
    fn busy_message(self) -> &'static str {
        match self {
            Self::Render => {
                "A render is already running in this editing session. Wait for it to finish \
                 or cancel it."
            }
            Self::Publish => {
                "A video is already being published from this editing session. Wait for it to finish."
            }
            _ => {
                "An import is already running in this editing session. Wait for it to finish \
                 or cancel it."
            }
        }
    }
}

/// `JobProgressDto.phase`: `queued → preparing → rendering → publishing →
/// complete`, with `cancelled`/`failed` as the other two terminals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobPhase {
    Queued,
    Preparing,
    /// A render's ffmpeg child is running (Task 46).
    Rendering,
    /// A render's output is being moved into `products\` and recorded in
    /// the ledger (Task 46); Task 48's publication uses it too.
    Publishing,
    Complete,
    Cancelled,
    Failed,
}

impl JobPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Complete | Self::Cancelled | Self::Failed)
    }
}

/// One file of an import batch that did not make it in — a DISPLAY name
/// only (the file's own name), never a path (ADR §3.3).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PerFileError {
    pub name: String,
    pub error: String,
}

/// `JobTerminal`: every field optional on the wire, absent (not `null`)
/// when a job kind has nothing to say about it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobTerminal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_file: Option<Vec<PerFileError>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<EditorError>,
}

/// One message on a job's channel. `terminal` is present-even-when-null
/// (the frontend decoder treats an absent key as a protocol error).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobProgressDto {
    pub session_id: String,
    pub job_id: String,
    pub kind: JobKind,
    pub sequence: u64,
    pub phase: JobPhase,
    pub fraction: f64,
    pub terminal: Option<JobTerminal>,
}

/// One row of `editor_get_jobs`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRecordDto {
    pub job_id: String,
    pub kind: JobKind,
    pub phase: JobPhase,
    pub fraction: f64,
    pub terminal: Option<JobTerminal>,
}

/// `editor_import_media`'s immediate reply.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStarted {
    pub job_id: String,
}

struct JobRecord {
    session_id: String,
    kind: JobKind,
    phase: JobPhase,
    fraction: f64,
    terminal: Option<JobTerminal>,
    /// Registration order, so `records_for` lists jobs oldest first.
    started: u64,
    cancel: Arc<AtomicBool>,
}

/// Every job this process has started, keyed by job id.
#[derive(Default)]
pub struct JobRegistry {
    jobs: HashMap<String, JobRecord>,
    next: u64,
}

impl JobRegistry {
    /// Register a new `queued` job for `session_id`; returns its id and the
    /// flag `cancel` sets.
    pub(crate) fn register(
        &mut self,
        session_id: &str,
        kind: JobKind,
    ) -> (String, Arc<AtomicBool>) {
        let job_id = new_entity_id("job");
        let cancel = Arc::new(AtomicBool::new(false));
        self.next += 1;
        self.jobs.insert(
            job_id.clone(),
            JobRecord {
                session_id: session_id.to_string(),
                kind,
                phase: JobPhase::Queued,
                fraction: 0.0,
                terminal: None,
                started: self.next,
                cancel: Arc::clone(&cancel),
            },
        );
        (job_id, cancel)
    }

    /// Ask a job to stop. A job of ANOTHER session is refused exactly like
    /// an unknown one (a session may not learn or touch another's jobs); a
    /// job that already finished is acknowledged as a no-op — a Cancel
    /// racing completion is not the user's error (IPC-CONTRACTS.md: "job
    /// may already be publishing/completed").
    pub(crate) fn cancel(&self, session_id: &str, job_id: &str) -> Result<(), EditorError> {
        match self.jobs.get(job_id) {
            Some(record) if record.session_id == session_id => {
                record.cancel.store(true, Ordering::SeqCst);
                Ok(())
            }
            _ => Err(EditorError::new(
                EditorErrorCode::InvalidRequest,
                "There is no such job in this editing session.",
            )),
        }
    }

    /// Stop every job of a session that is ending — its results would have
    /// nowhere to land.
    pub(crate) fn cancel_session(&self, session_id: &str) {
        for record in self.jobs.values().filter(|r| r.session_id == session_id) {
            record.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// `session_id`'s jobs, oldest first — `editor_get_jobs`' answer.
    pub(crate) fn records_for(&self, session_id: &str) -> Vec<JobRecordDto> {
        let mut rows: Vec<(&String, &JobRecord)> = self
            .jobs
            .iter()
            .filter(|(_, r)| r.session_id == session_id)
            .collect();
        rows.sort_by_key(|(_, r)| r.started);
        rows.into_iter()
            .map(|(id, r)| JobRecordDto {
                job_id: id.clone(),
                kind: r.kind,
                phase: r.phase,
                fraction: r.fraction,
                terminal: r.terminal.clone(),
            })
            .collect()
    }

    /// Stop `session_id`'s jobs of one `kind` — a discard stopping its
    /// derived-media decodes before it removes the project (Task 28).
    pub(crate) fn cancel_session_kind(&self, session_id: &str, kind: JobKind) {
        for record in self
            .jobs
            .values()
            .filter(|r| r.session_id == session_id && r.kind == kind)
        {
            record.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Is a job of `kind` still running in `session_id`?
    pub(crate) fn is_running(&self, session_id: &str, kind: JobKind) -> bool {
        self.jobs
            .values()
            .any(|r| r.session_id == session_id && r.kind == kind && !r.phase.is_terminal())
    }

    /// Stop every job of `kind`, in every session -- a quit's render cancel.
    pub(crate) fn cancel_kind(&self, kind: JobKind) {
        for record in self.jobs.values().filter(|r| r.kind == kind) {
            record.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Is a job of `kind` still running in ANY session?
    pub(crate) fn any_running(&self, kind: JobKind) -> bool {
        self.jobs
            .values()
            .any(|r| r.kind == kind && !r.phase.is_terminal())
    }

    /// Drop a closing session's TERMINAL records (GAP-174): nobody can ask
    /// for them any more (`editor_get_jobs` needs a live session). A job
    /// still running keeps its record until its terminal lands.
    pub(crate) fn forget_terminal(&mut self, session_id: &str) {
        self.jobs
            .retain(|_, r| r.session_id != session_id || !r.phase.is_terminal());
    }

    /// Keep `session_id`'s most recent `MAX_TERMINAL_RECORDS` terminal
    /// records, dropping older ones (GAP-174).
    fn prune_terminal(&mut self, session_id: &str) {
        let mut terminal: Vec<(u64, String)> = self
            .jobs
            .iter()
            .filter(|(_, r)| r.session_id == session_id && r.phase.is_terminal())
            .map(|(id, r)| (r.started, id.clone()))
            .collect();
        if terminal.len() <= MAX_TERMINAL_RECORDS {
            return;
        }
        terminal.sort_unstable();
        let excess = terminal.len() - MAX_TERMINAL_RECORDS;
        for (_, id) in terminal.into_iter().take(excess) {
            self.jobs.remove(&id);
        }
    }

    /// Drop a finished job's record. Only for a job whose result travels
    /// in its command's own REPLY (a peaks decode): no Channel message
    /// could have been dropped, so there is nothing for `editor_get_jobs`
    /// to recover and keeping it would only grow the registry (GAP-174).
    pub(crate) fn forget(&mut self, job_id: &str) {
        self.jobs.remove(job_id);
    }

    fn update(&mut self, message: &JobProgressDto) {
        if let Some(record) = self.jobs.get_mut(&message.job_id) {
            record.phase = message.phase;
            record.fraction = message.fraction;
            record.terminal = message.terminal.clone();
        }
        if message.phase.is_terminal() {
            self.prune_terminal(&message.session_id);
        }
    }
}

/// Register a job for a LIVE session — `sessionGone` otherwise, before
/// anything is registered — refusing a second job of the same kind while
/// one is still running in that session (fix round 1: two imports racing
/// the per-file capacity check could make one batch's `AddAssets` fail and
/// roll the whole batch back; the UI's own guard is not the authority). The
/// check and the registration happen under ONE lock, so two concurrent
/// starts cannot both pass it.
pub(crate) fn start_job_in(
    state: &EditorState,
    session_id: &str,
    kind: JobKind,
) -> Result<(String, Arc<AtomicBool>), EditorError> {
    drop(require_session(state, session_id)?);
    let mut jobs = lock_ignoring_poison(&state.jobs);
    if kind.exclusive() && jobs.is_running(session_id, kind) {
        return Err(EditorError::new(
            EditorErrorCode::InvalidRequest,
            kind.busy_message(),
        ));
    }
    Ok(jobs.register(session_id, kind))
}

/// `editor_cancel_job`'s body: the session must be live, and the job its own.
pub(crate) fn cancel_job_in(
    state: &EditorState,
    session_id: &str,
    job_id: &str,
) -> Result<(), EditorError> {
    drop(require_session(state, session_id)?);
    lock_ignoring_poison(&state.jobs).cancel(session_id, job_id)
}

/// `editor_get_jobs`' body: the live session's jobs, oldest first.
pub(crate) fn jobs_in(
    state: &EditorState,
    session_id: &str,
) -> Result<Vec<JobRecordDto>, EditorError> {
    drop(require_session(state, session_id)?);
    Ok(lock_ignoring_poison(&state.jobs).records_for(session_id))
}

/// Where a job's messages go. Production is the caller's `Channel`; tests
/// collect into a `Vec`.
pub(crate) trait ProgressSink {
    fn deliver(&self, message: JobProgressDto);
}

/// A job with no Channel subscriber (a peaks decode answers in its
/// command's reply): its reporter still keeps the REGISTRY current, so
/// `editor_get_jobs` shows it running and `editor_cancel_job` can stop it.
pub(crate) struct NoSubscriber;

impl ProgressSink for NoSubscriber {
    fn deliver(&self, _message: JobProgressDto) {}
}

impl ProgressSink for Channel<JobProgressDto> {
    fn deliver(&self, message: JobProgressDto) {
        // A failed send means the webview went away (reload, closed
        // window). The registry already holds this message's state, so a
        // later `editor_get_jobs` recovers it; the job itself continues.
        if let Err(e) = self.send(message) {
            log::warn!("editor job progress could not be delivered: {e}");
        }
    }
}

/// The ONE emitter of a job's messages — see the module doc for the two
/// properties it makes structural.
pub(crate) struct JobReporter<'a> {
    jobs: &'a Mutex<JobRegistry>,
    sink: &'a dyn ProgressSink,
    session_id: String,
    job_id: String,
    kind: JobKind,
    sequence: u64,
}

impl<'a> JobReporter<'a> {
    pub(crate) fn new(
        jobs: &'a Mutex<JobRegistry>,
        sink: &'a dyn ProgressSink,
        session_id: &str,
        job_id: &str,
        kind: JobKind,
    ) -> Self {
        Self {
            jobs,
            sink,
            session_id: session_id.to_string(),
            job_id: job_id.to_string(),
            kind,
            sequence: 0,
        }
    }

    /// A non-terminal step. A terminal phase here is a programming error:
    /// it is logged and reported as `preparing`, never as a second
    /// terminal the frontend would have to ignore.
    pub(crate) fn progress(&mut self, phase: JobPhase, fraction: f64) {
        let phase = if phase.is_terminal() {
            log::error!(
                "editor job {}: progress() given terminal {phase:?}",
                self.job_id
            );
            JobPhase::Preparing
        } else {
            phase
        };
        self.emit(phase, fraction, None);
    }

    /// The job's one terminal message. Consumes the reporter.
    pub(crate) fn finish(mut self, phase: JobPhase, terminal: JobTerminal) {
        let phase = if phase.is_terminal() {
            phase
        } else {
            log::error!(
                "editor job {}: finish() given non-terminal {phase:?}",
                self.job_id
            );
            JobPhase::Failed
        };
        self.emit(phase, 1.0, Some(terminal));
    }

    fn emit(&mut self, phase: JobPhase, fraction: f64, terminal: Option<JobTerminal>) {
        self.sequence += 1;
        let message = JobProgressDto {
            session_id: self.session_id.clone(),
            job_id: self.job_id.clone(),
            kind: self.kind,
            sequence: self.sequence,
            phase,
            fraction: fraction.clamp(0.0, 1.0),
            terminal,
        };
        // Registry first (see the module doc), then the channel.
        lock_ignoring_poison(self.jobs).update(&message);
        self.sink.deliver(message);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;

    /// Collects every delivered message, in order.
    #[derive(Default)]
    pub(crate) struct CollectingSink(pub Mutex<Vec<JobProgressDto>>);

    impl ProgressSink for CollectingSink {
        fn deliver(&self, message: JobProgressDto) {
            self.0.lock().unwrap().push(message);
        }
    }

    impl CollectingSink {
        pub(crate) fn messages(&self) -> Vec<JobProgressDto> {
            self.0.lock().unwrap().clone()
        }
    }

    // The wire shape, pinned as LITERAL JSON: camelCase envelope, lowercase
    // kind/phase, `terminal: null` present on a progress message, and a
    // terminal's absent fields omitted rather than null.
    #[test]
    fn job_progress_wire_shape_is_pinned() {
        let progress = JobProgressDto {
            session_id: "ses-a".into(),
            job_id: "job-b".into(),
            kind: JobKind::Import,
            sequence: 2,
            phase: JobPhase::Preparing,
            fraction: 0.5,
            terminal: None,
        };
        assert_eq!(
            serde_json::to_value(&progress).unwrap(),
            json!({
                "sessionId": "ses-a", "jobId": "job-b", "kind": "import",
                "sequence": 2, "phase": "preparing", "fraction": 0.5, "terminal": null
            })
        );
        let done = JobProgressDto {
            phase: JobPhase::Complete,
            fraction: 1.0,
            sequence: 3,
            terminal: Some(JobTerminal {
                asset_ids: Some(vec!["asset-1".into()]),
                per_file: Some(vec![PerFileError {
                    name: "broken.mov".into(),
                    error: "damaged".into(),
                }]),
                ..JobTerminal::default()
            }),
            ..progress
        };
        assert_eq!(
            serde_json::to_value(&done).unwrap(),
            json!({
                "sessionId": "ses-a", "jobId": "job-b", "kind": "import",
                "sequence": 3, "phase": "complete", "fraction": 1.0,
                "terminal": {
                    "assetIds": ["asset-1"],
                    "perFile": [{ "name": "broken.mov", "error": "damaged" }]
                }
            })
        );
        let row = JobRecordDto {
            job_id: "job-b".into(),
            kind: JobKind::Import,
            phase: JobPhase::Cancelled,
            fraction: 0.25,
            terminal: Some(JobTerminal::default()),
        };
        assert_eq!(
            serde_json::to_value(&row).unwrap(),
            json!({
                "jobId": "job-b", "kind": "import", "phase": "cancelled",
                "fraction": 0.25, "terminal": {}
            })
        );
        assert_eq!(
            serde_json::to_value(JobKind::Peaks).unwrap(),
            json!("peaks")
        );
        // Task 46: the close guard matches render jobs by THIS spelling
        // (`useEditorCloseGuard`'s `liveRenderJobIds`, whose test decodes
        // the same literal), and the two phases a render adds.
        assert_eq!(
            serde_json::to_value(JobKind::Render).unwrap(),
            json!("render")
        );
        assert_eq!(
            serde_json::to_value(JobKind::Publish).unwrap(),
            json!("publish")
        );
        assert_eq!(
            serde_json::to_value([JobPhase::Rendering, JobPhase::Publishing]).unwrap(),
            json!(["rendering", "publishing"])
        );
        assert_eq!(
            serde_json::to_value(JobStarted {
                job_id: "job-b".into()
            })
            .unwrap(),
            json!({ "jobId": "job-b" })
        );
    }

    #[test]
    fn cancel_is_scoped_to_the_owning_session_and_idempotent_after_completion() {
        let mut registry = JobRegistry::default();
        let (job, flag) = registry.register("ses-a", JobKind::Import);
        let e = registry.cancel("ses-b", &job).unwrap_err();
        assert_eq!(e.code, EditorErrorCode::InvalidRequest);
        assert!(
            !flag.load(Ordering::SeqCst),
            "another session cannot cancel it"
        );
        assert!(registry.cancel("ses-a", "job-nope").is_err());
        registry.cancel("ses-a", &job).unwrap();
        assert!(flag.load(Ordering::SeqCst));
        registry.cancel("ses-a", &job).unwrap();
    }

    #[test]
    fn records_are_per_session_oldest_first_and_track_the_last_message() {
        let jobs = Mutex::new(JobRegistry::default());
        let (first, _) = lock_ignoring_poison(&jobs).register("ses-a", JobKind::Import);
        let (_, other) = lock_ignoring_poison(&jobs).register("ses-b", JobKind::Import);
        let (second, _) = lock_ignoring_poison(&jobs).register("ses-a", JobKind::Import);
        let sink = CollectingSink::default();
        let mut reporter = JobReporter::new(&jobs, &sink, "ses-a", &first, JobKind::Import);
        reporter.progress(JobPhase::Preparing, 0.5);
        assert!(lock_ignoring_poison(&jobs).is_running("ses-a", JobKind::Import));
        reporter.finish(JobPhase::Complete, JobTerminal::default());

        let rows = lock_ignoring_poison(&jobs).records_for("ses-a");
        let ids: Vec<&str> = rows.iter().map(|r| r.job_id.as_str()).collect();
        assert_eq!(ids, [first.as_str(), second.as_str()]);
        assert_eq!(rows[0].phase, JobPhase::Complete);
        assert_eq!(rows[0].terminal, Some(JobTerminal::default()));
        assert_eq!(rows[1].phase, JobPhase::Queued);
        lock_ignoring_poison(&jobs).cancel_session("ses-b");
        assert!(other.load(Ordering::SeqCst));
    }

    // The command bodies: all three refuse a session this process does not
    // hold, and cancel/get never reach another session's job.
    #[test]
    fn job_commands_are_scoped_to_a_live_session() {
        use vault_buddy_core::editor::EditorSession;

        use crate::editor::project_store::minimal_project;

        let state = EditorState::default();
        let e = start_job_in(&state, "ses-a", JobKind::Import).unwrap_err();
        assert_eq!(e.code, EditorErrorCode::SessionGone);
        assert!(lock_ignoring_poison(&state.jobs)
            .records_for("ses-a")
            .is_empty());

        for id in ["ses-a", "ses-b"] {
            lock_ignoring_poison(&state.sessions).insert(
                id.to_string(),
                EditorSession::resume(id, minimal_project("proj1"), 1),
            );
        }
        let (job, flag) = start_job_in(&state, "ses-a", JobKind::Import).unwrap();
        assert_eq!(jobs_in(&state, "ses-a").unwrap()[0].job_id, job);
        assert!(jobs_in(&state, "ses-b").unwrap().is_empty());
        assert!(cancel_job_in(&state, "ses-b", &job).is_err());
        assert!(!flag.load(Ordering::SeqCst));
        cancel_job_in(&state, "ses-a", &job).unwrap();
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(
            cancel_job_in(&state, "ses-gone", &job).unwrap_err().code,
            EditorErrorCode::SessionGone
        );
        assert_eq!(
            jobs_in(&state, "ses-gone").unwrap_err().code,
            EditorErrorCode::SessionGone
        );
    }

    // Fix round 1: Rust, not only the UI, refuses a second import in a
    // session while one is running — two batches racing the capacity check
    // near MAX_ASSETS could otherwise make one batch's AddAssets fail and
    // roll that whole batch back. Another session, or a finished job, does
    // not block.
    #[test]
    fn a_second_import_in_the_same_session_is_refused_while_one_runs() {
        use vault_buddy_core::editor::EditorSession;

        use crate::editor::project_store::minimal_project;

        let state = EditorState::default();
        for id in ["ses-a", "ses-b"] {
            lock_ignoring_poison(&state.sessions).insert(
                id.to_string(),
                EditorSession::resume(id, minimal_project("proj1"), 1),
            );
        }
        let (first, _) = start_job_in(&state, "ses-a", JobKind::Import).unwrap();
        let e = start_job_in(&state, "ses-a", JobKind::Import).unwrap_err();
        assert_eq!(e.code, EditorErrorCode::InvalidRequest);
        assert!(e.message.contains("already running"), "{}", e.message);
        assert_eq!(
            jobs_in(&state, "ses-a").unwrap().len(),
            1,
            "nothing registered"
        );
        start_job_in(&state, "ses-b", JobKind::Import).unwrap();

        let sink = CollectingSink::default();
        JobReporter::new(&state.jobs, &sink, "ses-a", &first, JobKind::Import)
            .finish(JobPhase::Complete, JobTerminal::default());
        start_job_in(&state, "ses-a", JobKind::Import).unwrap();

        // Task 28: peaks decodes are NOT exclusive — every visible asset
        // asks at once — and a forgotten record is gone from the listing.
        let (p1, _) = start_job_in(&state, "ses-a", JobKind::Peaks).unwrap();
        let (p2, _) = start_job_in(&state, "ses-a", JobKind::Peaks).unwrap();
        lock_ignoring_poison(&state.jobs).forget(&p1);
        let ids: Vec<String> = jobs_in(&state, "ses-a")
            .unwrap()
            .into_iter()
            .map(|r| r.job_id)
            .collect();
        assert!(ids.contains(&p2) && !ids.contains(&p1), "{ids:?}");
    }

    // A terminal phase handed to `progress` must not become a second
    // terminal on the wire; a non-terminal handed to `finish` still ends
    // the job (as `failed`), never leaves it running forever.
    #[test]
    fn the_reporter_never_emits_a_terminal_from_progress() {
        let jobs = Mutex::new(JobRegistry::default());
        let sink = CollectingSink::default();
        let mut reporter = JobReporter::new(&jobs, &sink, "ses-a", "job-x", JobKind::Import);
        reporter.progress(JobPhase::Complete, 2.0);
        reporter.finish(JobPhase::Preparing, JobTerminal::default());
        let messages = sink.messages();
        assert_eq!(messages[0].phase, JobPhase::Preparing);
        assert!(messages[0].terminal.is_none());
        assert_eq!(messages[0].fraction, 1.0, "clamped");
        assert_eq!(messages[1].phase, JobPhase::Failed);
        assert_eq!(
            messages.iter().map(|m| m.sequence).collect::<Vec<_>>(),
            [1, 2]
        );
    }
    // GAP-174 (Task 46): render jobs would grow the registry faster than
    // imports ever did. A session keeps only its most recent
    // MAX_TERMINAL_RECORDS terminal records -- the newest, so a reconcile
    // still sees what just finished -- never prunes a RUNNING job, never
    // touches another session's, and a closing session's terminal records
    // go with it.
    #[test]
    fn terminal_records_beyond_the_bound_are_pruned_oldest_first() {
        let jobs = Mutex::new(JobRegistry::default());
        let sink = CollectingSink::default();
        let (running, _) = lock_ignoring_poison(&jobs).register("ses-a", JobKind::Import);
        let (other, _) = lock_ignoring_poison(&jobs).register("ses-b", JobKind::Peaks);
        JobReporter::new(&jobs, &sink, "ses-b", &other, JobKind::Peaks)
            .finish(JobPhase::Complete, JobTerminal::default());
        let mut finished = Vec::new();
        for _ in 0..MAX_TERMINAL_RECORDS + 3 {
            let (id, _) = lock_ignoring_poison(&jobs).register("ses-a", JobKind::Peaks);
            JobReporter::new(&jobs, &sink, "ses-a", &id, JobKind::Peaks)
                .finish(JobPhase::Complete, JobTerminal::default());
            finished.push(id);
        }
        let ids: Vec<String> = lock_ignoring_poison(&jobs)
            .records_for("ses-a")
            .into_iter()
            .map(|r| r.job_id)
            .collect();
        let mut expected = vec![running.clone()];
        expected.extend(finished[3..].iter().cloned());
        assert_eq!(ids, expected, "the three OLDEST terminal records went");
        assert_eq!(lock_ignoring_poison(&jobs).records_for("ses-b").len(), 1);

        lock_ignoring_poison(&jobs).forget_terminal("ses-a");
        let left: Vec<String> = lock_ignoring_poison(&jobs)
            .records_for("ses-a")
            .into_iter()
            .map(|r| r.job_id)
            .collect();
        assert_eq!(left, [running], "a running job keeps its record");
    }
}
