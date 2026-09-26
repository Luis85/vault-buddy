//! The shutdown gate's render term (Task 46; ADR R12) -- split out of
//! `render_jobs.rs` at its line cap, with no change of behaviour.
//!
//! `blocks_shutdown` is the fourth term of
//! `shutdown_gate::shutdown_blocker`: true while a render job has not ended
//! (fix round 1 widened the ADR's `rendering`/`publishing` to every
//! non-terminal phase, so the gate and the quit's cancel read one set).
//! Both quit workers call `cancel_all_bounded` first, before the publish
//! cancel and the capture finalizes. A wedged render must not make
//! the app unquittable: after the bound expires, the gate stops counting
//! renders (`RENDERS_ABANDONED`), so Alt+F4's re-triggered close cannot loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::media_jobs::JobKind;
use super::EditorState;

/// How often a quit's cancel re-reads the registry.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// Set once a quit's bounded render cancel expired: from then on the gate
/// stops counting renders, so a wedged one cannot make Alt+F4's
/// re-triggered close loop forever (the process is exiting anyway).
static RENDERS_ABANDONED: AtomicBool = AtomicBool::new(false);

/// Is a RENDER job not yet ended? The same set `cancel_all_in` cancels and
/// waits for (fix round 1): by KIND, so another job in `publishing` (Task
/// 48's publish) is neither reported as a render nor left outside the
/// cancel to loop the gate; and every non-terminal phase, because a queued
/// or preparing render is seconds away from starting an ffmpeg child that
/// would otherwise outlive the process.
pub(crate) fn render_blocks_shutdown(state: &EditorState) -> bool {
    lock_ignoring_poison(&state.jobs).any_running(JobKind::Render)
}

/// Cancel every render and wait, at most `limit`, for all of them to end;
/// `true` iff they did.
pub(crate) fn cancel_all_in(state: &EditorState, limit: Duration, poll: Duration) -> bool {
    lock_ignoring_poison(&state.jobs).cancel_kind(JobKind::Render);
    crate::shutdown_gate::wait_until_cleared(
        || !lock_ignoring_poison(&state.jobs).any_running(JobKind::Render),
        limit,
        poll,
    )
}

/// `shutdown_gate`'s fourth term (R12).
pub fn blocks_shutdown(app: &AppHandle) -> bool {
    !RENDERS_ABANDONED.load(Ordering::SeqCst) && render_blocks_shutdown(&app.state::<EditorState>())
}

/// The quit workers' FIRST step, before the publish cancel and the
/// capture finalizes: kill every
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
