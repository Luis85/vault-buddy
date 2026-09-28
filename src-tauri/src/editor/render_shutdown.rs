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

/// The render term, given the latch it reads: true while a render has not
/// ended, unless `latch` says an earlier bounded cancel gave up on them.
/// The seam `blocks_shutdown` wraps (review finding I-4: the latch was
/// unreachable from a test behind an `AppHandle`).
pub(crate) fn blocks_shutdown_in(state: &EditorState, latch: &AtomicBool) -> bool {
    !latch.load(Ordering::SeqCst) && render_blocks_shutdown(state)
}

/// Cancel every render and wait, bounded; on expiry set `latch` (and log)
/// and proceed. With no render running it does nothing at all -- the
/// latch is only ever set by a cancel that really expired.
pub(crate) fn cancel_all_bounded_in(
    state: &EditorState,
    latch: &AtomicBool,
    limit: Duration,
    poll: Duration,
) {
    if !render_blocks_shutdown(state) {
        return;
    }
    log::info!("editor render: cancelling every render before shutdown");
    if !cancel_all_in(state, limit, poll) {
        latch.store(true, Ordering::SeqCst);
        log::warn!("editor render: a render did not stop within {limit:?}; exiting anyway");
    }
}

/// `shutdown_gate`'s fourth term (R12).
pub fn blocks_shutdown(app: &AppHandle) -> bool {
    blocks_shutdown_in(&app.state::<EditorState>(), &RENDERS_ABANDONED)
}

/// The quit workers' FIRST step, before the publish cancel and the
/// capture finalizes: kill every
/// render (its part is deleted) and wait, bounded. On expiry it LOGS and
/// proceeds -- a wedged render must never make the app unquittable.
///
/// Callers must NOT be on the main/event-loop thread: this sleeps.
pub fn cancel_all_bounded(app: &AppHandle, limit: Duration) {
    cancel_all_bounded_in(
        &app.state::<EditorState>(),
        &RENDERS_ABANDONED,
        limit,
        CANCEL_POLL,
    );
}

#[cfg(test)]
pub(crate) mod tests {
    //! Review finding I-4: the latch is what keeps a wedged render from
    //! looping Alt+F4's re-triggered close, and before the `_in` seam no
    //! test could reach it (it hid behind an `AppHandle`). Every test here
    //! owns a FRESH latch, so none of them can see another's expiry.

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use vault_buddy_core::sync_util::lock_ignoring_poison;

    use super::*;
    use crate::editor::media_jobs::{JobPhase, JobReporter, JobTerminal, NoSubscriber};
    use crate::structural_scan::{fn_body, shell_file};

    const SESSION: &str = "ses-shutdown";
    /// Small enough that a never-ending job is abandoned at once.
    const SHORT: Duration = Duration::from_millis(50);
    /// Large enough that a job which DOES end is never mistaken for one
    /// that did not, however slow the machine.
    const LONG: Duration = Duration::from_secs(10);
    const POLL: Duration = Duration::from_millis(5);

    /// A job of `kind` that nothing will ever end: registered, and so
    /// `queued`, with no runner behind it -- a wedged ffmpeg child or copy.
    pub(crate) fn never_ending(state: &EditorState, kind: JobKind) -> Arc<AtomicBool> {
        lock_ignoring_poison(&state.jobs).register(SESSION, kind).1
    }

    /// Run `body` beside a job of `kind` that ends -- `cancelled`, as a
    /// real runner answers a cancel -- as soon as it is asked to stop.
    pub(crate) fn with_job_ending_on_cancel(
        state: &EditorState,
        kind: JobKind,
        body: impl FnOnce(),
    ) {
        let (job_id, cancel) = lock_ignoring_poison(&state.jobs).register(SESSION, kind);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                // Bounded, so a failing assertion in `body` ends the test
                // instead of leaving this job waiting on a cancel nobody sends.
                let deadline = Instant::now() + LONG;
                while !cancel.load(Ordering::SeqCst) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
                JobReporter::new(&state.jobs, &NoSubscriber, SESSION, &job_id, kind)
                    .finish(JobPhase::Cancelled, JobTerminal::default());
            });
            body();
        });
    }

    #[test]
    fn a_render_that_never_ends_is_abandoned_and_stops_blocking() {
        let state = EditorState::default();
        let latch = AtomicBool::new(false);
        never_ending(&state, JobKind::Render);
        assert!(
            blocks_shutdown_in(&state, &latch),
            "a running render blocks"
        );
        cancel_all_bounded_in(&state, &latch, SHORT, POLL);
        assert!(
            latch.load(Ordering::SeqCst),
            "an expired cancel must set the abandon latch"
        );
        assert!(
            render_blocks_shutdown(&state),
            "the render is still running -- only the latch silences it"
        );
        assert!(
            !blocks_shutdown_in(&state, &latch),
            "an abandoned render must stop blocking, or Alt+F4's re-close loops"
        );
    }

    #[test]
    fn a_render_that_ends_inside_the_bound_leaves_the_latch_clear() {
        let state = EditorState::default();
        let latch = AtomicBool::new(false);
        with_job_ending_on_cancel(&state, JobKind::Render, || {
            cancel_all_bounded_in(&state, &latch, LONG, POLL);
        });
        assert!(
            !latch.load(Ordering::SeqCst),
            "a render that stopped was not abandoned"
        );
        assert!(!render_blocks_shutdown(&state));
        assert!(!blocks_shutdown_in(&state, &latch));
        // A LATER render must still be counted: the latch is for a wedged
        // one only.
        never_ending(&state, JobKind::Render);
        assert!(blocks_shutdown_in(&state, &latch));
    }

    #[test]
    fn a_set_latch_silences_a_running_render_and_a_clear_one_does_not() {
        let state = EditorState::default();
        never_ending(&state, JobKind::Render);
        assert!(blocks_shutdown_in(&state, &AtomicBool::new(false)));
        assert!(!blocks_shutdown_in(&state, &AtomicBool::new(true)));
    }

    #[test]
    fn with_no_render_running_the_cancel_never_touches_the_latch() {
        let state = EditorState::default();
        // A publish is not a render: the render cancel has nothing to do.
        never_ending(&state, JobKind::Publish);
        let latch = AtomicBool::new(false);
        cancel_all_bounded_in(&state, &latch, Duration::ZERO, POLL);
        assert!(!latch.load(Ordering::SeqCst));
        assert!(!blocks_shutdown_in(&state, &latch));
    }

    // The seam is only worth having if production goes through it with the
    // MODULE's latch: a wrapper handed a fresh `AtomicBool` (or the
    // publish term's) would latch nothing the gate ever reads.
    #[test]
    fn the_app_wrappers_pass_the_render_latch() {
        let src = shell_file("render_shutdown.rs");
        let gate = fn_body(&src, "pub fn blocks_shutdown(");
        assert!(gate.contains("blocks_shutdown_in("), "{gate}");
        assert!(gate.contains("&RENDERS_ABANDONED"), "{gate}");
        let cancel = fn_body(&src, "pub fn cancel_all_bounded(");
        assert!(cancel.contains("cancel_all_bounded_in("), "{cancel}");
        assert!(cancel.contains("&RENDERS_ABANDONED"), "{cancel}");
        assert!(cancel.contains("CANCEL_POLL"), "{cancel}");
        assert!(!src.contains("PUBLISHES_ABANDONED"), "the publish latch");
    }
}
