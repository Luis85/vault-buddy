//! Closing an editor session (`editor_close_session`'s logic, split out of
//! `session_commands.rs` at its line cap): `close_in` and the part of it that
//! runs under the session's save lock, `close_locked`, plus the two helpers
//! that take a session out of `EditorState`.
//!
//! `editor_close_session` itself stays in `session_commands.rs` beside the
//! other session commands and calls `close_in`; nothing here is a command.

use std::path::Path;

use vault_buddy_core::editor::{EditorError, EditorSession};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::errors::internal;
use super::prefs_commands::project_id_for;
use super::recovery;
use super::session_commands::CloseDisposition;
use super::store_io::remove_project;
use super::EditorState;

/// Take the session out of the two maps — and do nothing else under them
/// (final review M1: the maps are never held across disk I/O, and the
/// cleanups `drop_session` runs next unlink files). The session is MOVED
/// out: from here on every command finds it gone, and the caller owns its
/// last state.
fn unregister_session(state: &EditorState, session_id: &str) -> Option<EditorSession> {
    let mut by_project = lock_ignoring_poison(&state.by_project);
    let mut sessions = lock_ignoring_poison(&state.sessions);
    let session = sessions.remove(session_id)?;
    let project_id = &session.project().id;
    if by_project.get(project_id).map(String::as_str) == Some(session_id) {
        by_project.remove(project_id);
    }
    Some(session)
}

fn drop_session(state: &EditorState, session_id: &str) {
    unregister_session(state, session_id);
    // Prune the per-session save lock (`EditorState::save_locks`'s own
    // doc) along with the session it belongs to, so the map only grows
    // with sessions currently open rather than every session ever opened
    // in this process's life.
    lock_ignoring_poison(&state.save_locks).remove(session_id);
    // Whatever journal write was still pending has either been flushed
    // (`keep`) or must never happen (`discard*`).
    state.journal.forget(session_id);
    // A running import (Task 25) of a closing session stops before its next
    // file; its results would have no session to land in. Its peaks decodes
    // are jobs too; its thumbnail renders are not (Task 28).
    // Its finished records go too (GAP-174): nothing can ask for them once
    // the session is gone; a render still running keeps its record until
    // its terminal lands (Task 46). Final review M3: this runs for EVERY
    // disposition, so closing a session ends its renders and publishes — a
    // render can only land in a live session (`render_jobs::publish`). The
    // window's X never closes a session (the close guard only hides the
    // window), which is why nothing on a window close cancels a render.
    let mut jobs = lock_ignoring_poison(&state.jobs);
    jobs.cancel_session(session_id);
    jobs.forget_terminal(session_id);
    drop(jobs);
    super::media_derive::cancel_session_thumbnails(state, session_id);
    // Its unfinished webcam takes can never be finished now (Task 49): their
    // `.part` files go; a finished take's `.webm` stays with its asset.
    state.takes.forget_session(session_id);
}

/// Close a session. `discardProject` UNPINS the staged capture first and
/// only then removes the project directory: a failure between the two
/// leaves an unpinned orphan the next open adopts back, whereas the
/// reverse order would leave a pin naming a deleted project, and a pinned
/// capture refuses Discard. The recording itself is never touched (R6).
/// On any failure the session stays open so the user can retry.
///
/// **`discardProject` holds `EditorState::open`** (review finding I-1) from
/// before its closing mark until the project is gone — the lock every open,
/// every other pin writer and every other project remover holds — and finds
/// the pins to clear by scanning the staging sidecars, never through
/// `sources.json` (GAP-214 item 7), so a pin an open in progress lands is
/// seen, and a project whose `sources.json` is damaged can still go.
///
/// **Every disposition holds the per-session SAVE lock** (Task 12 fix round
/// 2 for `discardProject`; Task 37 for `keep` and `discardRecovery`) — the
/// lock `editor_save_project` holds for its whole read-through-write
/// sequence and every recovery-journal write runs under. Without it a
/// discard could remove the directory under an in-flight save, a `keep`
/// could flush a journal beside a save that is deleting it, and a
/// `discardRecovery` could delete a journal a racing write then recreates.
/// A concurrent close waits behind an in-flight save, never the reverse.
///
/// `drop_session` runs INSIDE that lock (`close_locked`), so a save queued
/// behind this close finds the session gone (`sessionGone`) rather than
/// finding it still registered and failing against a removed directory
/// with a generic write error (Task 12 review, carried to Task 37).
pub(crate) fn close_in(
    state: &EditorState,
    root: &Path,
    staging_dir: &Path,
    session_id: &str,
    disposition: CloseDisposition,
) -> Result<(), EditorError> {
    let project_id = project_id_for(state, session_id)?;
    // Review finding I-1: a discard takes `EditorState::open` — the lock
    // every other pin writer and project remover holds — BEFORE its closing
    // mark, and keeps it until the project is gone (`_open` is declared
    // first, so it is released last). An open in progress therefore lands
    // its pin before this discard scans for pins, and no open can reuse
    // this session while it is being removed. Keep and discardRecovery
    // neither unpin nor remove anything, so they do not wait on opens.
    let _open = match disposition {
        CloseDisposition::DiscardProject => {
            #[cfg(test)]
            state
                .test_hooks
                .discard_waiting_for_open
                .store(true, std::sync::atomic::Ordering::SeqCst);
            let open = lock_ignoring_poison(&state.open);
            // Task 4 fix round 1: every refusal that does not need the
            // quiesce — ownership, and a staging folder the pin scan could
            // not list — lands HERE, before the closing mark and the
            // quiesce's cancels, so it cancels nothing (GAP-214 item 5).
            super::project_discard::precheck_discard(root, staging_dir, &project_id)?;
            Some(open)
        }
        _ => None,
    };
    // Final review I1/C2: a discard first marks the session closing (every
    // start path refuses it from here on), then — BEFORE the save lock is
    // taken (their final writes need it) — waits, cancelling nothing, for a
    // take's write or finish, a reconnect and an import, and only once all
    // three have ended cancels and waits for derived media, renders and
    // publishes (GAP-214 item 5). It refuses rather than remove the
    // directory under any of them (`discard::quiesce`).
    let _closing = match disposition {
        CloseDisposition::DiscardProject => {
            let mark = super::discard::mark_closing(state, session_id)?;
            super::discard::quiesce(state, session_id)?;
            Some(mark)
        }
        _ => None,
    };
    let session_lock = super::save_commands::session_save_lock(state, session_id)?;
    let _save_guard = lock_ignoring_poison(&session_lock);
    close_locked(
        state,
        root,
        staging_dir,
        session_id,
        &project_id,
        disposition,
    )
}

/// The part of `close_in` that runs under the session's save lock — ending
/// with `drop_session`, so nothing queued on that lock ever sees a session
/// whose project this close has already changed underneath it.
pub(crate) fn close_locked(
    state: &EditorState,
    root: &Path,
    staging_dir: &Path,
    session_id: &str,
    project_id: &str,
    disposition: CloseDisposition,
) -> Result<(), EditorError> {
    match disposition {
        // Task 37: the journal lands before the session goes. Hardening Task
        // 18 (C-2): the session is moved out of `sessions` FIRST and its
        // journal written from that moved value, so an edit acknowledged
        // during the close is either in it or refused `sessionGone` —
        // never acknowledged and then lost with the session.
        CloseDisposition::Keep => {
            if let Some(session) = unregister_session(state, session_id) {
                recovery::write_closing_locked(state, root, session_id, &session);
            }
        }
        // Task 37: `recovery.json` goes (owned-file check); the saved
        // project, its sources and the pin are untouched. R7b (hardening
        // Task 10 fix round 1): a journal that was never readable to begin
        // with is kept aside instead of deleted -- `discard_or_quarantine_
        // journal`'s own call, never a bare `remove_journal` here.
        //
        // Hardening Task 18 fix round 1: a refusal keeps the session open,
        // so its pending journal write is forgotten only once the discard
        // has succeeded, and the user sees role wording -- the detail (a
        // redacted handle at most) goes to the log.
        CloseDisposition::DiscardRecovery => {
            super::journal_quarantine::discard_or_quarantine_journal(root, project_id).map_err(
                |e| {
                    log::warn!("editor recovery: discardRecovery failed for {project_id}: {e}");
                    internal("The unsaved changes could not be discarded right now. Try again.")
                },
            )?;
            state.journal.forget(session_id);
        }
        CloseDisposition::DiscardProject => {
            // GAP-214 item 7: the pins are found where they live — the
            // staging sidecars — never through `sources.json`, which may be
            // exactly what is damaged (and a hand-edited pin may name this
            // project from a capture `sources.json` does not list).
            // GAP-214 item 8: ownership was proven before any pin is
            // released — in `close_in`, before the quiesce cancelled
            // anything (`project_discard::precheck_discard`).
            super::project_discard::unpin_everywhere(staging_dir, project_id)?;
            remove_project(root, project_id)?;
        }
    }
    // Task 47: a Review render is disposable -- the session's last one goes
    // with the session (a discard already removed the whole directory).
    // Under the save lock, so a review landing concurrently either lands
    // before this sweep or finds its job cancelled by `drop_session`.
    if disposition != CloseDisposition::DiscardProject {
        super::render_review::sweep_reviews(root, project_id, None);
    }
    drop_session(state, session_id);
    Ok(())
}

#[cfg(test)]
#[path = "session_close_tests.rs"]
mod tests;
