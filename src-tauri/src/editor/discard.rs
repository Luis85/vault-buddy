//! A project discard's first half (final whole-branch review I1 + C2):
//! mark the session CLOSING, then stop — or wait for — everything that
//! could still be writing into the project directory, and REFUSE the
//! discard when something cannot be stopped in time.
//!
//! Why this exists: `editor_close_session{discardProject}` removes the
//! project directory under the session's save lock, but a webcam take's
//! finish (a remux of up to 15 minutes writing `takes\.<id>.remux.webm`)
//! and a media import (a copy of up to 4 GiB into `media\`) are not
//! derivations `media_derive::stop_session_derivations` knows about, and
//! ffmpeg's output handle carries no `FILE_SHARE_DELETE`. A discard racing
//! either used to fail part way through the removal, and — `read_dir`
//! order putting `project.json` before `takes\` — leave a project that
//! could be neither opened nor discarded again.
//!
//! **The closing mark.** `mark_closing` records the session in
//! `EditorState::closing` for the whole discard; every START path checks it
//! (`refuse_if_closing`) — a job registration (`media_jobs::start_job_in`,
//! checked under the jobs lock, which `mark_closing` also takes, so a
//! registration either lands before the mark and is seen by the quiesce,
//! or after it and is refused), a take's begin, append and finish (under
//! the take's entry lock), a reconnect and a caption import (after their
//! claim). Each checks AFTER registering itself, and the quiesce looks
//! AFTER marking, so of any start racing a discard, one of the two sees
//! the other. The mark is dropped when the close returns, success or not —
//! after a successful discard the session is gone, so later calls get
//! `sessionGone` anyway.
//!
//! **Lock order**: `jobs`, then `closing` — both leaves; `closing` is
//! never held while any other lock is taken.

use std::time::Duration;

use vault_buddy_core::editor::{EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::media_jobs::JobKind;
use super::EditorState;

/// How long a discard waits for an import or a take write to stop before
/// refusing. Short in tests, so a refusal test does not idle for seconds.
const QUIESCE_WAIT: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(5)
};

fn refusal(message: &str) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidRequest, message)
}

/// `true` while `session_id` is being discarded.
pub(crate) fn is_closing(state: &EditorState, session_id: &str) -> bool {
    lock_ignoring_poison(&state.closing).contains(session_id)
}

/// Refuse new work on a session a discard is removing.
pub(crate) fn refuse_if_closing(state: &EditorState, session_id: &str) -> Result<(), EditorError> {
    if is_closing(state, session_id) {
        return Err(refusal(
            "This project is being discarded, so nothing new can start in it.",
        ));
    }
    Ok(())
}

/// The session's closing mark, removed when dropped.
pub(crate) struct ClosingMark<'a> {
    state: &'a EditorState,
    session_id: String,
}

impl Drop for ClosingMark<'_> {
    fn drop(&mut self) {
        lock_ignoring_poison(&self.state.closing).remove(&self.session_id);
    }
}

/// Mark `session_id` closing — under the jobs lock, so no job registers
/// between the mark and the quiesce unseen (module doc). A second discard
/// of the same session while one runs is refused.
pub(crate) fn mark_closing<'a>(
    state: &'a EditorState,
    session_id: &str,
) -> Result<ClosingMark<'a>, EditorError> {
    let _jobs = lock_ignoring_poison(&state.jobs);
    if !lock_ignoring_poison(&state.closing).insert(session_id.to_string()) {
        return Err(refusal("This project is already being discarded."));
    }
    Ok(ClosingMark {
        state,
        session_id: session_id.to_string(),
    })
}

/// Wait (at most `QUIESCE_WAIT`) for `done` — `false` if it never was.
fn wait_until(done: impl Fn() -> bool) -> bool {
    let started = std::time::Instant::now();
    while !done() {
        if started.elapsed() >= QUIESCE_WAIT {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Everything a discard must stop before it removes the project — run
/// AFTER `mark_closing` and BEFORE the save lock is taken (a finish, a
/// render's publish and a derived-media commit all need that lock to end).
///
/// Derived media, renders and publishes are cancelled and waited for
/// (`stop_session_derivations`); an import is cancelled (it stops before
/// its next file) and waited for; a reconnect and a take's write or finish
/// cannot be interrupted, so they are waited for. Whatever is still running
/// when the wait ends REFUSES the discard, in words that say what to wait
/// for, and nothing has been touched.
pub(crate) fn quiesce(state: &EditorState, session_id: &str) -> Result<(), EditorError> {
    super::media_derive::stop_session_derivations(state, session_id);
    lock_ignoring_poison(&state.jobs).cancel_session_kind(session_id, JobKind::Import);
    let import_stopped =
        wait_until(|| !lock_ignoring_poison(&state.jobs).is_running(session_id, JobKind::Import));
    if !import_stopped {
        return Err(refusal(
            "Media is still being copied into this project. Wait for the import to finish, then \
             discard the project.",
        ));
    }
    if !wait_until(|| !lock_ignoring_poison(&state.relinks).contains(session_id)) {
        return Err(refusal(
            "Missing media is still being reconnected. Wait for it to finish, then discard the \
             project.",
        ));
    }
    if !state.takes.wait_idle(session_id, QUIESCE_WAIT) {
        return Err(refusal(
            "A webcam take is still being saved. Wait for it to finish, then discard the project.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "discard_tests.rs"]
mod tests;
