//! Setting aside an unreadable recovery journal (GAP-180 / R7, hardening
//! Task 10) -- split out of `recovery.rs` at its line cap (hardening Task
//! 18). `recovery.rs`' module doc says when each of the two set-aside
//! points runs; this module is how.
//!
//! A journal is moved aside -- via `rename_noreplace`, never deleted, so a
//! user can still recover the bytes by hand -- in two cases:
//!
//! - it fails to load with a CONTENT verdict (`is_content_verdict`): to
//!   `recovery.unreadable-<unix seconds>[-n].json` beside it (R7);
//! - it loads fine but is an UNRESUMED predecessor (R12, final review
//!   I-2): the session about to replace or delete it was minted by an open
//!   that did not resume it (`note_unresumed_predecessor`) and has not
//!   written the file itself yet: to `recovery.unresumed-<unix
//!   seconds>[-n].json`. A readable journal is only ever replaced or
//!   deleted by the session that resumed it or wrote it -- the one way to
//!   decline a readable journal is a Resume that failed for a transient
//!   reason, and the Resume dialog tells the user that file is kept.
//!
//! And a journal is only ever REPLACED once it is out of the way: a
//! set-aside that fails leaves it exactly where it is and refuses the
//! write that would have replaced it (hardening Task 18, carried from Task
//! 10's re-review), because overwriting it destroys the bytes this module
//! exists to keep.

use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::{EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::recovery::{journal_path, journal_present, load_journal, remove_journal};
use super::EditorState;

/// The set-aside name prefix of a journal that could not be read (R7).
pub(super) const UNREADABLE: &str = "recovery.unreadable";
/// ... and of a readable journal no session resumed (R12).
pub(super) const UNRESUMED: &str = "recovery.unresumed";

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

/// Move `path` (a journal already judged to go aside, an owned plain file)
/// aside, via `rename_noreplace`, to `<prefix>-<unix seconds>.json` beside
/// it (`UNREADABLE` or `UNRESUMED`). A same-second collision (two set-asides
/// in one second, or a clock that ran backwards) retries with a numeric
/// suffix (review minor 7); any other failure, or 20 collisions, is an
/// ERROR -- the journal is then still in place, and the caller must not
/// replace or delete it.
fn set_aside_journal_file(path: &Path, prefix: &str) -> io::Result<()> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    for attempt in 0..20u32 {
        let name = if attempt == 0 {
            format!("{prefix}-{secs}.json")
        } else {
            format!("{prefix}-{secs}-{attempt}.json")
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

/// R7(a) and R12: before a journal write REPLACES whatever already sits at
/// `path`, get it out of the way when it has to be. Moved aside: a journal
/// that fails to load with a CONTENT verdict, and -- when `unresumed` (this
/// session was minted over a journal it did not resume and has not written
/// its own yet) -- one that loads fine too: either is an earlier process's
/// journal, about to be silently destroyed by this session's own first
/// acknowledged edit. `Ok` means the write may go ahead: nothing is there
/// (the ordinary case), a journal that loads fine is there and `unresumed`
/// is false (this session's own, or one it resumed -- only one session can
/// hold a project open at a time), something that is not a plain file is
/// there (a symlink or a directory, left alone -- `remove_journal`'s
/// owned-file discipline), or the predecessor was just moved aside. `Err`
/// means it may NOT: the predecessor could not be moved, or the file could
/// not even be read (a transient failure says nothing about its content,
/// so it might be exactly the bytes R7 keeps).
pub(super) fn quarantine_before_overwrite(
    root: &Path,
    project_id: &str,
    path: &Path,
    unresumed: bool,
) -> Result<(), PredecessorBlocked> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(PredecessorBlocked(format!("it could not be checked: {e}"))),
    }
    let prefix = match load_journal(root, project_id) {
        Ok(_) if unresumed => UNRESUMED,
        Ok(_) => return Ok(()),
        Err(e) if is_content_verdict(&e) => UNREADABLE,
        Err(e) => {
            return Err(PredecessorBlocked(format!(
                "it could not be read: {}",
                e.message
            )))
        }
    };
    set_aside_journal_file(path, prefix)
        .map_err(|e| PredecessorBlocked(format!("it could not be set aside: {e}")))
}

/// Per session: is `recovery.json`'s predecessor settled, and is it one the
/// session did not resume (R12)? Carried from Task 10's re-review: the
/// check parses the whole previous journal, and ran under the save lock on
/// EVERY debounced write. Once it has cleared -- or the session has written
/// the file itself -- nothing but this session can put a journal there, so
/// later writes skip it. A leaf lock.
#[derive(Default)]
pub struct PredecessorMemo {
    sessions: Mutex<HashMap<String, Memo>>,
}

#[derive(Default)]
struct Memo {
    settled: Option<Settled>,
    /// Minted over a journal it did not resume (`note_unresumed_predecessor`).
    unresumed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Settled {
    Clear,
    Blocked,
}

impl PredecessorMemo {
    /// Forget `session_id` (its session is ending).
    pub(super) fn forget(&self, session_id: &str) {
        lock_ignoring_poison(&self.sessions).remove(session_id);
    }

    /// Record `session_id`'s outcome; returns the one it replaced.
    fn settle(&self, session_id: &str, outcome: Settled) -> Option<Settled> {
        let mut sessions = lock_ignoring_poison(&self.sessions);
        let memo = sessions.entry(session_id.to_string()).or_default();
        memo.settled.replace(outcome)
    }

    fn is_clear(&self, session_id: &str) -> bool {
        lock_ignoring_poison(&self.sessions)
            .get(session_id)
            .is_some_and(|m| m.settled == Some(Settled::Clear))
    }

    fn mark_unresumed(&self, session_id: &str) {
        lock_ignoring_poison(&self.sessions)
            .entry(session_id.to_string())
            .or_default()
            .unresumed = true;
    }

    /// Is what sits at `recovery.json` still a predecessor `session_id` did
    /// not resume -- marked unresumed, and not yet cleared?
    fn holds_unresumed(&self, session_id: &str) -> bool {
        lock_ignoring_poison(&self.sessions)
            .get(session_id)
            .is_some_and(|m| m.unresumed && m.settled != Some(Settled::Clear))
    }
}

/// R12: `session_id` was just MINTED by an open that did not resume the
/// project's journal (`editor_open_project` without `useRecovery`, or a
/// staged capture's Edit). Whatever wears `recovery.json`'s name now is an
/// earlier session's, so this session's first write, a save and a Discard
/// of its own changes set it aside rather than replace or delete it, even
/// when it reads cleanly. Called under `EditorState::open`, before the open
/// returns -- no edit of the session can exist yet.
pub(crate) fn note_unresumed_predecessor(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    project_id: &str,
) {
    if journal_present(root, project_id) {
        state.journal.predecessors.mark_unresumed(session_id);
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
    let unresumed = memo.holds_unresumed(session_id);
    match quarantine_before_overwrite(root, project_id, path, unresumed) {
        Ok(()) => {
            memo.settle(session_id, Settled::Clear);
            true
        }
        Err(PredecessorBlocked(why)) => {
            if memo.settle(session_id, Settled::Blocked) != Some(Settled::Blocked) {
                log::warn!(
                    "editor recovery: an earlier recovery journal is in the way and {why}; \
                     it is kept where it is, and the unsaved changes are not journaled (nor \
                     that journal removed by a save) until it can be set aside"
                );
            }
            false
        }
    }
}

/// A save of the session's CURRENT revision removes `recovery.json` --
/// nothing is left to recover -- but R7 and R12 hold here too (Task 18 fix
/// round 1; final review I-2): the file may be an earlier process's journal
/// this session never wrote or resumed, and deleting it is the same loss as
/// overwriting it. So the save runs the writer's own check first: once it
/// clears (nothing there, this session's own journal, or the predecessor
/// moved aside), whatever is still at the name is removed; while it is
/// blocked the file is kept and the save still succeeds (logged once by
/// `predecessor_cleared`). The caller holds the session's save lock.
pub(crate) fn remove_after_save(
    state: &EditorState,
    session_id: &str,
    root: &Path,
    project_id: &str,
) -> io::Result<()> {
    let path = journal_path(root, project_id)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid project id"))?;
    if !predecessor_cleared(state, session_id, root, project_id, &path) {
        return Ok(());
    }
    remove_journal(root, project_id)
}

/// What `discardRecovery` does with what `load_journal` found.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DiscardAction {
    /// Delete it (`remove_journal`: an absent one is already gone, and
    /// anything but a plain file wearing the name is refused).
    Remove,
    /// Move it aside under this prefix.
    SetAside(&'static str),
    /// Leave it exactly where it is and fail the Discard.
    Refuse,
}

/// `discardRecovery`'s rule (R7b, R12). A readable journal is deleted --
/// the user discarded it -- unless `keep_readable` (a predecessor this
/// session never resumed, and the session is discarding its OWN unwritten
/// changes, not that journal: `discard_keeps_readable`), when it is kept
/// aside. A journal unreadable by CONTENT VERDICT is kept aside too:
/// Discard destroying a journal nobody has ever been able to read is not the
/// decision the user made (they discarded the RECOVERY OFFER, not evidence
/// of what it was). The name being absent (`invalidRequest`) is already
/// gone. Anything else -- a journal that could not be READ (a transient
/// I/O failure: deleting bytes nobody has looked at is the loss R7 exists
/// to prevent, and the user can retry, hardening Task 18), or an error this
/// rule has never seen (final review M-4) -- fails the Discard and deletes
/// nothing.
pub(super) fn discard_action(
    loaded: Result<(), &EditorError>,
    keep_readable: bool,
) -> DiscardAction {
    match loaded {
        Ok(()) if keep_readable => DiscardAction::SetAside(UNRESUMED),
        Ok(()) => DiscardAction::Remove,
        Err(e) if is_content_verdict(e) => DiscardAction::SetAside(UNREADABLE),
        Err(e) if e.code == EditorErrorCode::InvalidRequest => DiscardAction::Remove,
        Err(_) => DiscardAction::Refuse,
    }
}

/// Is `session_id`'s Discard about its own changes rather than the journal
/// at the name (R12)? True while that journal is still a predecessor the
/// session did not resume AND the session is dirty: a CLEAN session's
/// Discard is the recovery offer's own (A27) -- the user chose to discard
/// exactly that journal.
pub(crate) fn discard_keeps_readable(state: &EditorState, session_id: &str) -> bool {
    if !state.journal.predecessors.holds_unresumed(session_id) {
        return false;
    }
    lock_ignoring_poison(&state.sessions)
        .get(session_id)
        .is_some_and(|s| {
            let snap = s.snapshot();
            snap.persisted_revision != Some(snap.revision)
        })
}

/// `discardRecovery`'s own action -- `discard_action`'s rule applied. A
/// set-aside that fails is an error and leaves the journal where it is.
pub(crate) fn discard_or_quarantine_journal(
    root: &Path,
    project_id: &str,
    keep_readable: bool,
) -> io::Result<()> {
    let loaded = load_journal(root, project_id);
    match discard_action(loaded.as_ref().map(|_| ()), keep_readable) {
        DiscardAction::Remove => remove_journal(root, project_id),
        DiscardAction::SetAside(prefix) => {
            let path = journal_path(root, project_id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid project id"))?;
            set_aside_journal_file(&path, prefix)?;
            log::info!(
                "editor recovery: discardRecovery kept a journal's bytes aside ({prefix}) for \
                 {project_id} instead of deleting them"
            );
            Ok(())
        }
        DiscardAction::Refuse => Err(io::Error::other(
            loaded.err().map(|e| e.message).unwrap_or_default(),
        )),
    }
}

#[cfg(test)]
#[path = "journal_quarantine_tests.rs"]
mod tests;
