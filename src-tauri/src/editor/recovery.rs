//! Project/session recovery (Task 37 Part A; F-44; ADR §4 "Recovery
//! journal", R6; PERSISTENCE-AND-SECURITY.md "Recovery and garbage
//! collection"): the `recovery.json` journal a dirty session leaves behind,
//! the startup re-pin sweep that reconciles the project store against
//! staging, and the sweep of abandoned package imports (Task 39).
//!
//! **The journal.** After every acknowledged edit (`editor_execute`, a
//! finished import's `AddAssets`, a caption import) the session is
//! SCHEDULED for a journal write; the named `editor-journal` thread writes
//! `recovery.json` = `{schema, sessionRevision, savedRevision, project}` at
//! most once per `JOURNAL_DEBOUNCE` per session, through
//! `write_atomic_replacing` (temp + fsync + replacing rename). A save whose
//! revision is still the session's current one deletes it
//! (`save_commands::save_project_with`); `editor_close_session(keep)`
//! flushes a pending write synchronously before the session goes.
//!
//! **Every journal write, flush and delete runs under the session's SAVE
//! lock** (`save_commands::session_save_lock`), the same lock a save and a
//! discard hold. That is what makes "save at the current revision deletes
//! the journal" true: without it a journal write that read the session just
//! before the save could land just after the delete and resurrect a journal
//! for edits that are already on disk. The pending entry is also only ever
//! TAKEN under that lock, by exactly one of the worker and a closing
//! `keep`, so a close can never race the worker into writing nothing.
//!
//! A journal is never written for a CLEAN session (revision == persisted):
//! there is nothing to recover, and a fresh session opened over a project
//! must never overwrite a journal an earlier process left behind before the
//! user has chosen Resume or Discard for it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::{
    is_valid_id, limits, validate_project, EditorError, EditorErrorCode, Project,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

use super::package_import::importing_project_id;
use super::project_store::{pin_staged, pinned_project, project_dir, store_dir, SourceLocator};
use super::publish::{PublishJournal, PublishStep, PUBLISH_JOURNAL};
use super::redact::redact_name;
use super::render_jobs::JOBS_DIR;
use super::save_commands::session_save_lock;
use super::store_io::{load_sources, read_bounded, remove_dir_no_follow, RECOVERY_FILE};
use super::EditorState;
use crate::editor_commands::is_safe_base;

/// `recovery.json`'s schema identifier.
pub const RECOVERY_SCHEMA: &str = "vault-buddy-recovery/1";

/// An import build directory this old is an abandoned one (Task 39).
const STALE_IMPORT_AFTER: Duration = Duration::from_secs(60 * 60);

/// At most one journal write per session per this window (ADR §4).
pub(crate) const JOURNAL_DEBOUNCE: Duration = Duration::from_millis(500);

/// `recovery.json` on disk. `savedRevision` is the revision the project
/// store had committed when the journal was written (`null` for a session
/// that never had one, which no production open produces today).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecoveryJournal {
    pub schema: String,
    pub session_revision: u64,
    pub saved_revision: Option<u64>,
    pub project: Project,
}

struct Pending {
    due: Instant,
    root: PathBuf,
}

/// The per-session debounce state the `editor-journal` thread drains. A
/// LEAF lock: never held across I/O or while taking another lock.
#[derive(Default)]
pub struct JournalQueue {
    pending: Mutex<HashMap<String, Pending>>,
    wake: Condvar,
}

impl JournalQueue {
    /// Schedule `session_id`'s journal. An entry already waiting keeps its
    /// EARLIER deadline — a steady stream of edits must still be journaled
    /// every `JOURNAL_DEBOUNCE`, never postponed forever.
    pub(crate) fn schedule(&self, session_id: &str, root: &Path, now: Instant) {
        lock_ignoring_poison(&self.pending)
            .entry(session_id.to_string())
            .or_insert_with(|| Pending {
                due: now + JOURNAL_DEBOUNCE,
                root: root.to_path_buf(),
            });
        self.wake.notify_all();
    }

    /// Remove and return `session_id`'s pending root, due or not.
    fn take(&self, session_id: &str) -> Option<PathBuf> {
        lock_ignoring_poison(&self.pending)
            .remove(session_id)
            .map(|p| p.root)
    }

    /// Drop `session_id`'s pending write without performing it.
    pub(crate) fn forget(&self, session_id: &str) {
        lock_ignoring_poison(&self.pending).remove(session_id);
    }

    #[cfg(test)]
    pub(crate) fn is_pending(&self, session_id: &str) -> bool {
        lock_ignoring_poison(&self.pending).contains_key(session_id)
    }

    fn due_sessions(&self, now: Instant) -> Vec<String> {
        lock_ignoring_poison(&self.pending)
            .iter()
            .filter(|(_, p)| p.due <= now)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Block until the earliest deadline passes, something is scheduled, or
    /// `idle` elapses — whichever is first.
    fn wait(&self, idle: Duration) {
        let pending = lock_ignoring_poison(&self.pending);
        let now = Instant::now();
        let timeout = pending
            .values()
            .map(|p| p.due.saturating_duration_since(now))
            .min()
            .unwrap_or(idle)
            .min(idle);
        if timeout.is_zero() {
            return;
        }
        let _ = self
            .wake
            .wait_timeout(pending, timeout)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

fn journal_path(root: &Path, project_id: &str) -> Option<PathBuf> {
    project_dir(root, project_id).map(|d| d.join(RECOVERY_FILE))
}

/// Note an acknowledged edit of `session_id`: its journal is written within
/// `JOURNAL_DEBOUNCE`.
pub(crate) fn note_acknowledged(state: &EditorState, root: &Path, session_id: &str) {
    state.journal.schedule(session_id, root, Instant::now());
}

/// Write the journal for `session_id` from its CURRENT state. The caller
/// holds the session's save lock. A gone or clean session writes nothing.
fn write_locked(state: &EditorState, root: &Path, session_id: &str) -> std::io::Result<()> {
    let journal = {
        let sessions = lock_ignoring_poison(&state.sessions);
        let Some(session) = sessions.get(session_id) else {
            return Ok(());
        };
        let snap = session.snapshot();
        if snap.persisted_revision == Some(snap.revision) {
            return Ok(());
        }
        RecoveryJournal {
            schema: RECOVERY_SCHEMA.to_string(),
            session_revision: snap.revision,
            saved_revision: snap.persisted_revision,
            project: session.project().clone(),
        }
    };
    let path = journal_path(root, &journal.project.id).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid project id")
    })?;
    let json = serde_json::to_string_pretty(&journal)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_atomic_replacing(&path, &json)
}

/// Perform `session_id`'s pending journal write now, if one is waiting —
/// `editor_close_session(keep)`'s synchronous flush. The caller holds the
/// session's save lock.
pub(crate) fn flush_locked(state: &EditorState, session_id: &str) {
    if let Some(root) = state.journal.take(session_id) {
        if let Err(e) = write_locked(state, &root, session_id) {
            log::warn!("editor recovery: could not write the journal for {session_id}: {e}");
        }
    }
}

/// Write every journal whose deadline has passed. Each is taken and written
/// under its session's save lock; a session that has ended in the meantime
/// is simply forgotten. Returns how many were written or skipped.
pub(crate) fn flush_due(state: &EditorState, now: Instant) -> usize {
    let due = state.journal.due_sessions(now);
    for session_id in &due {
        let Ok(lock) = session_save_lock(state, session_id) else {
            state.journal.forget(session_id);
            continue;
        };
        let _guard = lock_ignoring_poison(&lock);
        flush_locked(state, session_id);
    }
    due.len()
}

/// Delete `project_id`'s journal — owned-file-only and no-follow: a symlink
/// or a directory wearing the name is refused, never followed or recursed;
/// an absent journal is already gone (`Ok`).
pub(crate) fn remove_journal(root: &Path, project_id: &str) -> std::io::Result<()> {
    let path = journal_path(root, project_id).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid project id")
    })?;
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !meta.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{} is not a plain file; refusing to remove it",
                path.display()
            ),
        ));
    }
    std::fs::remove_file(&path)
}

/// Read and validate `project_id`'s journal like a project: bounded, parsed
/// strictly, schema-checked, its own project id matching, and
/// `validate_project`. Anything else is `invalidProject` and the file is
/// left exactly as it was — this function writes nothing.
pub(crate) fn load_journal(root: &Path, project_id: &str) -> Result<RecoveryJournal, EditorError> {
    let invalid = |message: String| EditorError::new(EditorErrorCode::InvalidProject, message);
    let path = journal_path(root, project_id).ok_or_else(|| {
        EditorError::new(
            EditorErrorCode::InvalidRequest,
            format!("{project_id:?} is not a valid project id"),
        )
    })?;
    if !path.is_file() {
        return Err(EditorError::new(
            EditorErrorCode::InvalidRequest,
            "There are no unsaved changes to resume for this project.",
        ));
    }
    let bytes = read_bounded(&path, limits::MAX_PROJECT_JSON_BYTES)?;
    let journal: RecoveryJournal = serde_json::from_slice(&bytes)
        .map_err(|e| invalid(format!("The unsaved changes could not be read: {e}")))?;
    if journal.schema != RECOVERY_SCHEMA {
        return Err(invalid(format!(
            "The unsaved changes use an unknown format ({:?}).",
            journal.schema
        )));
    }
    if journal.project.id != project_id {
        return Err(invalid(
            "The unsaved changes belong to a different project.".to_string(),
        ));
    }
    validate_project(&journal.project)?;
    Ok(journal)
}

/// The `editor-journal` thread's body: wait for the next deadline, flush
/// what is due, until `stop` is set (production never sets it).
pub(crate) fn journal_worker_loop(state: &EditorState, stop: &AtomicBool) {
    while !stop.load(Ordering::SeqCst) {
        state.journal.wait(Duration::from_secs(1));
        flush_due(state, Instant::now());
    }
}

/// Start the `editor-journal` thread (wired into `lib.rs`'s `setup`).
pub fn spawn_journal_worker(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("editor-journal".into())
        .spawn(move || {
            static NEVER: AtomicBool = AtomicBool::new(false);
            let state = app.state::<EditorState>();
            journal_worker_loop(&state, &NEVER);
        });
    if let Err(e) = spawned {
        log::error!(
            "editor recovery: could not start the journal thread; edits are journaled \
             only when a session closes: {e}"
        );
    }
}

/// What the startup re-pin sweep did (F34).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RepinReport {
    /// `(projectId, base)` pairs whose pin was written back.
    pub repinned: Vec<(String, String)>,
    /// Projects left unpinned: their staged base is gone, or another
    /// project legitimately holds its pin. Reported, never deleted.
    pub orphaned: Vec<String>,
}

/// F34 (ADR R6): re-pin every project whose staged source's sidecar has
/// lost its pin — the crash between `create_project` and `pin_staged` that
/// Task 10's open-time adoption only repairs if the user reopens that exact
/// capture, or a sidecar rewritten by the coexisting legacy export path.
///
/// A project is re-pinned (through `pin_staged` → `staging::write_sidecar`,
/// the owned atomic sidecar rail) only when its staged capture still EXISTS
/// — sidecar readable, its own `base` matching, and the `.mp4` on disk —
/// and the sidecar's pin is absent or names a project that does not claim
/// this capture (gone, or whose `sources.json` names another). A capture
/// already pinned to ANOTHER project that does claim it is never stolen.
/// Everything else is REPORTED in `orphaned`: nothing is deleted and no pin
/// is ever invented for a capture that is not there.
///
/// Projects are visited in id order, so which of two unpinned claimants of
/// one capture wins is deterministic. A VIEW-style walk: an entry it cannot
/// read is logged and skipped, never allowed to abort the sweep.
pub fn run_startup_repin(root: &Path, staging_dir: &Path) -> RepinReport {
    let mut report = RepinReport::default();
    for id in valid_dir_names(&store_dir(root)) {
        let sources = match load_sources(root, &id) {
            Ok(sources) => sources,
            Err(e) => {
                log::warn!("editor-recovery-sweep: skipping {id}: {}", e.message);
                continue;
            }
        };
        let mut orphaned = false;
        for record in sources.values() {
            let SourceLocator::Staging { base } = &record.locator else {
                continue;
            };
            match repin_one(root, staging_dir, &id, base) {
                Repin::AlreadyPinned => {}
                Repin::Repinned => report.repinned.push((id.clone(), base.clone())),
                Repin::Orphaned => orphaned = true,
            }
        }
        if orphaned {
            report.orphaned.push(id);
        }
    }
    report
}

enum Repin {
    AlreadyPinned,
    Repinned,
    Orphaned,
}

/// MAY `project_id` claim staged capture `base`? A project directory that
/// is gone cannot; one whose `sources.json` reads and names another base
/// does not. One that exists but whose `sources.json` cannot be read right
/// now (fix round 1) is treated as claiming it: an unreadable file is never
/// proof its pin is free to take, so the sweep reports rather than steals.
fn claims(root: &Path, project_id: &str, base: &str) -> bool {
    if !project_dir(root, project_id).is_some_and(|d| d.is_dir()) {
        return false;
    }
    match load_sources(root, project_id) {
        Ok(sources) => sources
            .values()
            .any(|r| matches!(&r.locator, SourceLocator::Staging { base: b } if b == base)),
        Err(e) => {
            log::warn!(
                "editor-recovery-sweep: {project_id} holds a pin but its sources cannot be read, \
                 leaving the pin alone: {}",
                e.message
            );
            true
        }
    }
}

fn repin_one(root: &Path, staging_dir: &Path, project_id: &str, base: &str) -> Repin {
    if !is_safe_base(base) {
        log::warn!("editor-recovery-sweep: {project_id} names an unsafe staged base");
        return Repin::Orphaned;
    }
    let sidecar = staging::read_sidecar(&staging_dir.join(staging::sidecar_file_name(base)));
    let video = staging_dir.join(staging::mp4_file_name(base)).is_file();
    let Some(sidecar) = sidecar.filter(|s| s.base == base && video) else {
        return Repin::Orphaned;
    };
    match pinned_project(&sidecar) {
        Some(pin) if pin == project_id => return Repin::AlreadyPinned,
        Some(pin) if claims(root, &pin, base) => return Repin::Orphaned,
        _ => {}
    }
    match pin_staged(staging_dir, base, project_id) {
        Ok(()) => Repin::Repinned,
        Err(e) => {
            log::warn!("editor-recovery-sweep: could not re-pin {project_id}: {e}");
            Repin::Orphaned
        }
    }
}

/// The sorted names of `dir`'s subdirectories that are valid ids -- a
/// project id in the store, a job id under `jobs\`. A VIEW: an unreadable
/// directory is an empty list.
fn valid_dir_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|id| is_valid_id(id))
        .collect();
    ids.sort();
    ids
}

/// The largest `publish.json` read: three short fields.
const PUBLISH_JOURNAL_MAX_BYTES: u64 = 64 * 1024;

/// What one interrupted publish's journal says, in words (F36).
fn publish_report(journal: &PublishJournal) -> String {
    let video = &journal.video;
    match (journal.step, &journal.note) {
        (PublishStep::Reserved, _) => format!(
            "A publish was interrupted before its video was saved as {video}. A hidden partial copy \
             may be left in that folder; publish it again."
        ),
        (PublishStep::Video, Some(_)) => format!(
            "A publish was interrupted: the video was saved as {video} but its note was not."
        ),
        (_, Some(note)) => format!(
            "A publish was interrupted after it saved the video as {video} and its note as \
             {note}."
        ),
        (_, None) => format!("A publish was interrupted after it saved the video as {video}."),
    }
}

/// Task 48 (F36; ADR R13): every publish a crash interrupted, in words, by
/// project then job. A journal not at `complete` is REPORTED and left
/// exactly where it is -- never deleted, never retried (docs/Gaps.md: no
/// resume from the journal), so it is reported again on the next start. A
/// `complete` one (a crash after the last step, before the publish removed
/// its own job directory) is removed quietly: nothing was lost. A journal
/// that cannot be read is reported as such, and kept.
pub(crate) fn interrupted_publishes(root: &Path) -> Vec<String> {
    let mut reports = Vec::new();
    for project in valid_dir_names(&store_dir(root)) {
        let Some(jobs) = project_dir(root, &project).map(|d| d.join(JOBS_DIR)) else {
            continue;
        };
        for job in valid_dir_names(&jobs) {
            let dir = jobs.join(&job);
            let path = dir.join(PUBLISH_JOURNAL);
            if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
                continue;
            }
            let journal = read_bounded(&path, PUBLISH_JOURNAL_MAX_BYTES)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<PublishJournal>(&bytes).ok());
            match journal {
                Some(j) if j.step == PublishStep::Complete => {
                    if let Err(e) = remove_dir_no_follow(&dir) {
                        log::warn!("editor-recovery-sweep: could not remove {job}: {e}");
                    }
                }
                Some(j) => reports.push(publish_report(&j)),
                None => reports.push(format!(
                    "A publish was interrupted, and its record ({job}) could not be read."
                )),
            }
        }
    }
    reports
}

/// Task 39: remove every import build directory (`.<projectId>.importing`,
/// `package_import`'s own name) whose last change is at least an hour
/// before `now`. Nothing else in the store is touched.
///
/// Ownership is the NAME (`package_import::importing_project_id`: a leading
/// dot, a valid project id, the `.importing` suffix) AND the kind: only a
/// real directory, never a file or a link wearing the name. Removal is the
/// store's own owned, no-follow walk (`store_io::remove_dir_no_follow`),
/// never `remove_dir_all`. The hour is what keeps an import running in
/// this very process safe — the sweep runs at startup, and an import is
/// seconds to minutes long. A VIEW-style walk: a failure is logged and the
/// sweep moves on.
pub(crate) fn sweep_stale_imports(root: &Path, now: std::time::SystemTime) -> Vec<String> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(store_dir(root)) else {
        return removed;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if importing_project_id(&name).is_none() {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let stale = meta
            .modified()
            .is_ok_and(|at| at + STALE_IMPORT_AFTER <= now);
        if !meta.file_type().is_dir() || !stale {
            continue;
        }
        match remove_dir_no_follow(&path) {
            Ok(()) => removed.push(name),
            Err(e) => log::warn!(
                "editor-recovery-sweep: could not remove {}: {e}",
                redact_name(&name)
            ),
        }
    }
    removed
}

/// Run `sweep_stale_imports` (Task 39), then `run_startup_repin`, on the named `editor-recovery-sweep` thread
/// (wired into `lib.rs`'s `setup`, right after `run_screen_recovery`). That
/// is SPAWN order only: the screen sweep runs on its own thread with its own
/// retry loop and is not awaited. It does not need to be — it acts only on
/// `.part` files, whose base was never published and so is named by no
/// project, and a capture it promotes is `recovered`, which
/// `editor_open_staged` refuses (F7), so no project can reference it. It holds the editor's `open` lock for the whole pass: an editor
/// opening a capture in the same instant pins under that lock too, so the
/// two can never write one sidecar's pin at once.
pub fn spawn_startup_repin(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("editor-recovery-sweep".into())
        .spawn(move || {
            let Ok(root) = app.path().app_local_data_dir() else {
                log::warn!("editor-recovery-sweep: could not resolve the local data directory");
                return;
            };
            let state = app.state::<EditorState>();
            let _open = lock_ignoring_poison(&state.open);
            for report in interrupted_publishes(&root) {
                log::warn!("editor-recovery-sweep: {report}");
            }
            let swept = sweep_stale_imports(&root, std::time::SystemTime::now());
            if !swept.is_empty() {
                log::info!(
                    "editor-recovery-sweep: removed {} abandoned project import(s)",
                    swept.len()
                );
            }
            let report = run_startup_repin(&root, &staging::staging_dir(&root));
            if !report.repinned.is_empty() {
                log::info!(
                    "editor-recovery-sweep: re-pinned {} project(s)",
                    report.repinned.len()
                );
            }
            if !report.orphaned.is_empty() {
                log::warn!(
                    "editor-recovery-sweep: {} project(s) left unpinned (their staged capture \
                     is gone or belongs to another project): {}",
                    report.orphaned.len(),
                    report.orphaned.join(", ")
                );
            }
        });
    if let Err(e) = spawned {
        log::warn!("editor-recovery-sweep: could not spawn thread: {e}");
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
