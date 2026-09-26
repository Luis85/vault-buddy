//! Setting aside an unreadable recovery journal (GAP-180 / R7, hardening
//! Task 10) -- split out of `recovery.rs` at its line cap (hardening Task
//! 18). `recovery.rs`' module doc says when each of the two set-aside
//! points runs; this module is how.
//!
//! A journal is only ever moved aside when it fails to load with a CONTENT
//! verdict (`is_content_verdict`), via `rename_noreplace` to
//! `recovery.unreadable-<unix seconds>[-n].json` beside it -- never deleted,
//! so a user can still recover the bytes by hand. And it is only ever
//! REPLACED once it is out of the way: a set-aside that fails leaves it
//! exactly where it is and refuses the write that would have replaced it
//! (hardening Task 18, carried from Task 10's re-review), because
//! overwriting it destroys the bytes this module exists to keep.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::{EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::recovery::{journal_path, load_journal, remove_journal};
use super::EditorState;

/// GAP-180 / R7 (hardening Task 10, fix round 1): does `e` say something
/// about the JOURNAL'S CONTENT -- a bad parse, an unknown schema, the wrong
/// project id, `validate_project`'s own checks, or the file being over the
/// size bound -- rather than a TRANSIENT I/O failure (a sharing violation, a
/// delete-pending permission error, a read racing this very writer's own
/// replace) that says nothing about the file itself? `load_journal` reports
/// every content verdict as `InvalidProject` (`read_bounded`'s own "too big"
/// case included) and every I/O failure as `Internal`
/// (`read_bounded`'s own mapping); the file simply being absent is
/// `InvalidRequest`, also not a verdict. Only the first kind may ever
/// justify moving a journal aside -- review Important 1's fix.
pub(super) fn is_content_verdict(e: &EditorError) -> bool {
    e.code == EditorErrorCode::InvalidProject
}

/// Move `path` (an already-confirmed content-unreadable journal, an owned
/// plain file) aside, via `rename_noreplace`, to
/// `recovery.unreadable-<unix seconds>.json` beside it. A same-second
/// collision (two quarantines in one second, or a clock that ran backwards)
/// retries with a numeric suffix (review minor 7); any other failure, or 20
/// collisions, is an ERROR -- the journal is then still in place, and the
/// caller must not replace or delete it.
fn quarantine_journal_file(path: &Path) -> io::Result<()> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    for attempt in 0..20u32 {
        let name = if attempt == 0 {
            format!("recovery.unreadable-{secs}.json")
        } else {
            format!("recovery.unreadable-{secs}-{attempt}.json")
        };
        match rename_noreplace(path, &path.with_file_name(name)) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many same-second collisions"))
}

/// Why a journal write may not replace what sits at `recovery.json` yet.
#[derive(Debug)]
pub(super) struct PredecessorBlocked(String);

/// R7(a): before a journal write REPLACES whatever already sits at `path`,
/// get it out of the way when it has to be. Only a journal that fails to
/// load with a CONTENT verdict is ever moved -- an earlier process's
/// journal this session never resumed, about to be silently destroyed by
/// this session's own first acknowledged edit. `Ok` means the write may go
/// ahead: nothing is there (the ordinary case), a journal that loads fine
/// is there (this session's own, or one it resumed -- only one session can
/// hold a project open at a time), something that is not a plain file is
/// there (a symlink or a directory, left alone -- `remove_journal`'s
/// owned-file discipline), or the unreadable one was just moved aside.
/// `Err` means it may NOT: the unreadable journal could not be moved, or
/// the file could not even be read (a transient failure says nothing about
/// its content, so it might be exactly the bytes R7 keeps).
pub(super) fn quarantine_before_overwrite(
    root: &Path,
    project_id: &str,
    path: &Path,
) -> Result<(), PredecessorBlocked> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(PredecessorBlocked(format!("it could not be checked: {e}"))),
    }
    match load_journal(root, project_id) {
        Ok(_) => Ok(()),
        Err(e) if is_content_verdict(&e) => quarantine_journal_file(path)
            .map_err(|e| PredecessorBlocked(format!("it could not be set aside: {e}"))),
        Err(e) => Err(PredecessorBlocked(format!(
            "it could not be read: {}",
            e.message
        ))),
    }
}

/// Per session: is `recovery.json`'s predecessor settled? Carried from Task
/// 10's re-review: the check parses the whole previous journal, and ran
/// under the save lock on EVERY debounced write. Once it has cleared -- or
/// the session has written the file itself -- nothing but this session can
/// put a journal there, so later writes skip it. A leaf lock.
#[derive(Default)]
pub struct PredecessorMemo {
    settled: Mutex<HashMap<String, Settled>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Settled {
    Clear,
    Blocked,
}

impl PredecessorMemo {
    /// Forget `session_id` (its session is ending).
    pub(super) fn forget(&self, session_id: &str) {
        lock_ignoring_poison(&self.settled).remove(session_id);
    }

    /// Record `session_id`'s outcome; returns the one it replaced.
    fn settle(&self, session_id: &str, outcome: Settled) -> Option<Settled> {
        lock_ignoring_poison(&self.settled).insert(session_id.to_string(), outcome)
    }

    fn is_clear(&self, session_id: &str) -> bool {
        lock_ignoring_poison(&self.settled).get(session_id) == Some(&Settled::Clear)
    }
}

/// May `session_id`'s journal write replace `path` now? Checks at most once
/// per session once cleared; a blocked check is retried on the next write
/// and logged only the first time it blocks (no path, no name).
pub(super) fn predecessor_cleared(
    state: &EditorState,
    session_id: &str,
    root: &Path,
    project_id: &str,
    path: &Path,
) -> bool {
    let memo = &state.journal.predecessors;
    if memo.is_clear(session_id) {
        return true;
    }
    #[cfg(test)]
    state
        .test_hooks
        .predecessor_checks
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    match quarantine_before_overwrite(root, project_id, path) {
        Ok(()) => {
            memo.settle(session_id, Settled::Clear);
            true
        }
        Err(PredecessorBlocked(why)) => {
            if memo.settle(session_id, Settled::Blocked) != Some(Settled::Blocked) {
                log::warn!(
                    "editor recovery: an earlier recovery journal is in the way and {why}; \
                     the unsaved changes are not journaled until it can be kept aside"
                );
            }
            false
        }
    }
}

/// `discardRecovery`'s own action (R7b): DELETE a readable journal, as
/// always -- but a journal unreadable by CONTENT VERDICT is kept, moved
/// aside, because Discard destroying a journal nobody has ever been able
/// to read is not the decision the user made (they discarded the RECOVERY
/// OFFER, not evidence of what it was). A set-aside that fails is an error
/// and leaves the journal where it is. A journal that could not be READ
/// (a transient I/O failure) is an error too (hardening Task 18, carried
/// from Task 10's re-review): deleting bytes nobody has looked at is the
/// loss R7 exists to prevent, and the user can retry. An absent journal is
/// already gone (`remove_journal`'s no-op); anything else wearing the name
/// is `remove_journal`'s to refuse.
pub(crate) fn discard_or_quarantine_journal(root: &Path, project_id: &str) -> io::Result<()> {
    match load_journal(root, project_id) {
        Ok(_) => remove_journal(root, project_id),
        Err(e) if is_content_verdict(&e) => {
            let path = journal_path(root, project_id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid project id"))?;
            quarantine_journal_file(&path)?;
            log::info!(
                "editor recovery: discardRecovery kept an unreadable journal's bytes aside for \
                 {project_id} instead of deleting them"
            );
            Ok(())
        }
        Err(e) if e.code == EditorErrorCode::Internal => Err(io::Error::other(e.message)),
        Err(_) => remove_journal(root, project_id),
    }
}

#[cfg(test)]
#[path = "journal_quarantine_tests.rs"]
mod tests;
