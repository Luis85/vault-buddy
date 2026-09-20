//! Shutdown's view of the export: the predicate every exit path consults —
//! since GAP-160 through the one composition in `shutdown_gate`, not by
//! spelling it per door — and the bounded cancel-and-wait the two quit
//! paths run before the process can end.
//!
//! **Why this is not in `export_commands`.** The sibling domains keep their
//! shutdown helpers inside their own modules —
//! `capture_commands::finalize_if_recording`,
//! `screen_commands::finalize_if_capturing` — and this belonged there too,
//! but `export_commands` came to 792 nonblank lines against the 800 Rust cap
//! with it. That is the same seam `staged_commands` was split along, and the
//! same remedy: the file that owns the export LIFECYCLE keeps the lifecycle,
//! and shutdown's small, self-contained view of it moves here. Its only
//! callers are `shutdown_gate` (the predicate) and the two quit workers
//! (`cancel_if_exporting`).
//!
//! GAP-155 is what it closes. Before it, both quit gates read the two CAPTURE
//! predicates alone, so quitting during a ten-minute export ran `finish_quit`
//! immediately: every window destroyed and `exit(0)` called while the
//! `screen-export` thread was inside `commit_into_vault` (video → note →
//! staged-removal), able to die between the video landing and its note with
//! `Prepared::created_dirs` never reaching `rollback_export_dir` — and the
//! ffmpeg CHILD, a separate process nothing on the way out stopped, still
//! writing `.<base>.export.mp4.part` into staging after the app was gone.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use crate::export_commands::{cancel_flag, ExportState};

/// How long a shutdown waits for a cancelled export to unwind before it
/// gives up and exits anyway.
///
/// Deliberately far shorter than either capture's finalize bound
/// (`screen_commands::STOP_TIMEOUT` is 30 s, `stop_capture`'s is 15 s),
/// because this is not a finalize. What has to happen here is: the export
/// loop notices the flag on its next `recv_timeout(CANCEL_POLL)` — 200 ms,
/// so 5 s is twenty-five of those — then kills the ffmpeg child, reaps it,
/// joins two reader threads, unlinks the truncated output and lets
/// `export_worker` roll back the directories it created. None of that is a
/// flush of buffered media, which is what the capture bounds are paying for.
///
/// The same bound covers the one stretch a cancel CANNOT shorten: an export
/// already past ffmpeg and inside `commit_into_vault` ignores the flag, and
/// that stretch is a `rename_noreplace` on one volume plus a small note
/// write — comfortably inside this window, which is why waiting it out is
/// worth doing at all rather than exiting immediately.
///
/// On expiry the caller LOGS and proceeds: a wedged export must never make
/// the app unquittable.
pub(crate) const EXPORT_CANCEL_TIMEOUT: Duration = Duration::from_secs(5);
/// How often the shutdown wait re-reads the reservation. `ExportState` is a
/// plain `Mutex<Option<..>>` with no condvar to be woken by (unlike
/// `ScreenCaptureState`), so this polls.
pub(crate) const EXPORT_CANCEL_POLL: Duration = Duration::from_millis(50);

/// Poll `cleared` until it answers true or `limit` elapses; `true` iff it
/// cleared in time.
///
/// Split out of `cancel_if_exporting` as a pure function over a predicate
/// and two durations precisely so BOTH arms — the clear and the expiry —
/// are asserted on the platform the suite runs on. The real caller's
/// predicate needs an `AppHandle`, so nothing about the bound would
/// otherwise execute in any test anywhere (the GAP-117 class).
pub(crate) fn wait_until_cleared(
    mut cleared: impl FnMut() -> bool,
    limit: Duration,
    poll: Duration,
) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if cleared() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        // Never overshoot the deadline by a whole poll interval.
        std::thread::sleep(poll.min(deadline - now));
    }
}

/// An export is in flight, so a quit must deal with it first.
///
/// The sibling of `capture_commands::recording_blocks_shutdown` and
/// `screen_commands::capture_blocks_shutdown`, and the third term of
/// `shutdown_gate::shutdown_blocker` — the one composition every exit path
/// reads, including the updater's, which used to read nothing (GAP-160).
/// Deliberately NOT a term of `tray::hide_buddy`'s gate, which shares the
/// same shape. See `cancel_if_exporting` for why.
pub fn export_blocks_shutdown(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<ExportState>().0).is_some()
}

/// Shutdown's export path: CANCEL the running export and wait, bounded, for
/// the reservation to clear. The sibling of `finalize_if_recording` /
/// `finalize_if_capturing`, and the one that does not finalize.
///
/// **Cancel, not finish.** An export is repeatable and its staged capture is
/// kept (the cancel path deletes only the truncated output), so there is
/// nothing irreplaceable to preserve by waiting minutes for a re-encode to
/// run to completion. A capture is the opposite, which is why its siblings
/// wait unboundedly.
///
/// This reuses `cancel_export`'s machinery rather than growing a second
/// cancel path: setting the flag is what makes the export loop kill the
/// ffmpeg CHILD — a separate process that nothing else on the way out would
/// stop, and which otherwise keeps writing `.<base>.export.mp4.part` into
/// staging after the app is gone.
///
/// **Why `tray::hide_buddy` is deliberately not a caller.** It gates on the
/// same two capture predicates and looks like the third site of one rule, but
/// it is the HIDE chokepoint: the buddy is the RECORDING indicator, and hide
/// is refused mid-capture so a recording can never be running with nothing on
/// screen saying so. An export shows no indicator and hides nothing, and
/// refusing hide-to-tray for the minutes an export runs would pin the app on
/// screen during exactly the operation a user wants to walk away from. Three
/// call sites, two rules — a structural test pins the asymmetry.
///
/// Callers must NOT be on the main/event-loop thread: this sleeps.
pub fn cancel_if_exporting(app: &AppHandle) {
    let Some(cancel) = cancel_flag(app) else {
        return;
    };
    log::info!("screen export: cancelling an in-flight export before shutdown");
    cancel.store(true, Ordering::Relaxed);
    if !wait_until_cleared(
        || !export_blocks_shutdown(app),
        EXPORT_CANCEL_TIMEOUT,
        EXPORT_CANCEL_POLL,
    ) {
        // Proceed anyway. A wedged export must not make the app
        // unquittable, and this line is the only evidence the truncated
        // export temp and any directories it created were left behind —
        // `screen_recovery`'s startup sweep is what clears them next launch.
        log::warn!(
            "screen export: cancel did not unwind within {EXPORT_CANCEL_TIMEOUT:?}; exiting anyway"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- GAP-155: the export is the third thing a quit must not abandon ----

    // `shell_file` and `fn_body` were born here and now live in
    // `structural_scan` — `shutdown_gate` needed the same two, and a scan
    // helper that exists twice is the drift that module exists to stop.
    use crate::structural_scan::{fn_body, shell_file};

    // GAP-155. Both quit paths gated on the two CAPTURE domains alone, so a
    // user who started a ten-minute export and quit hit `finish_quit`
    // immediately: windows destroyed and `exit(0)` while the `screen-export`
    // thread was mid-`commit_into_vault` (video → note → staged-removal), so
    // the process could die between the video landing and its note, with
    // `Prepared::created_dirs` never reaching `rollback_export_dir` — and the
    // ffmpeg CHILD, a separate process, kept writing into staging afterwards.
    //
    // RE-POINTED by GAP-160, not weakened. The quit doors no longer spell
    // the three predicates themselves — they read one composition,
    // `shutdown_gate::shutdown_is_blocked`, because the updater's door
    // spelled none of them and nobody noticed. So the export term is
    // asserted where it now lives (inside the gate) and the doors are
    // asserted to consult the gate, which is the same claim in two hops.
    //
    // The hide assertion is the point of the test rather than a footnote and
    // survives verbatim, now closed against BOTH spellings: `hide_buddy`
    // shares the quit gate's shape and must grow NEITHER the export
    // predicate nor the composition that contains it. It is the HIDE
    // chokepoint, not a quit — the buddy is the RECORDING indicator, an
    // export needs no indicator, and blocking hide-to-tray for the minutes
    // an export runs would pin the app on screen during exactly the
    // operation a user wants to walk away from. This pins the asymmetry so a
    // later "unify the three call sites" cleanup reddens.
    #[test]
    fn both_quit_gates_consult_the_export_predicate_and_the_hide_chokepoint_does_not() {
        let tray = shell_file("tray.rs");
        let close = shell_file("window_close.rs");
        let needle = "export_blocks_shutdown";
        let gate = "shutdown_is_blocked(";
        assert!(
            fn_body(&shell_file("shutdown_gate.rs"), "pub fn shutdown_blocker(").contains(needle),
            "the shared shutdown gate must consult the export predicate"
        );
        assert!(
            fn_body(&tray, "pub fn quit(").contains(gate),
            "tray::quit must not exit while an export is mid-commit into a vault"
        );
        assert!(
            fn_body(&close, "fn handle_main_close(").contains(gate),
            "Alt+F4 must not exit while an export is mid-commit into a vault"
        );
        let hide = fn_body(&tray, "pub fn hide_buddy(");
        assert!(
            !hide.contains(needle) && !hide.contains(gate),
            "hide_buddy is the HIDE chokepoint, not a quit: an export needs no \
             on-screen indicator, and gating hide-to-tray on one would trap the \
             app on screen for the whole export"
        );
    }

    // ORDER, which no call-count assertion sees. The cancel has to unwind the
    // export BEFORE the path that destroys every window and ends the process:
    // cancelling after `finish_quit` is cancelling nothing.
    #[test]
    fn each_shutdown_worker_cancels_the_export_before_the_process_can_exit() {
        let tray = shell_file("tray.rs");
        let quit = fn_body(&tray, "pub fn quit(");
        assert!(
            crate::structural_scan::offset_of(quit, "cancel_if_exporting(")
                < crate::structural_scan::offset_of(quit, "finish_quit(&app)"),
            "tray::quit's worker must cancel the export before finish_quit \
             destroys the windows and exits"
        );
        let close = shell_file("window_close.rs");
        let main_close = fn_body(&close, "fn handle_main_close(");
        assert!(
            crate::structural_scan::offset_of(main_close, "cancel_if_exporting(")
                < crate::structural_scan::offset_of(main_close, "window.close()"),
            "the close-finalize worker must cancel the export before it \
             re-triggers the close that destroys the buddy"
        );
    }

    // The shutdown cancel must reuse the cancel flag the export loop already
    // polls — setting it is what kills the ffmpeg child, deletes the truncated
    // output and lets the worker roll its directories back. A wait that never
    // sets it just stalls the quit for the whole bound and then exits anyway.
    #[test]
    fn the_shutdown_cancel_sets_the_flag_the_export_loop_polls_and_then_waits() {
        let src = shell_file("export_shutdown.rs");
        let body = fn_body(&src, "pub fn cancel_if_exporting(");
        assert!(
            body.contains("store(true"),
            "the shutdown cancel must SET the flag, not merely wait on it"
        );
        assert!(
            body.contains("wait_until_cleared("),
            "the shutdown cancel must wait for the reservation to clear"
        );
    }

    #[test]
    fn the_shutdown_wait_returns_as_soon_as_the_reservation_clears() {
        let polls = std::cell::Cell::new(0u32);
        let started = std::time::Instant::now();
        let cleared = wait_until_cleared(
            || {
                polls.set(polls.get() + 1);
                polls.get() >= 3
            },
            std::time::Duration::from_secs(30),
            std::time::Duration::from_millis(1),
        );
        assert!(cleared, "a reservation that cleared must report cleared");
        assert_eq!(polls.get(), 3, "it must stop polling the moment it clears");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "it must return on the clear, not sit out the whole bound"
        );
    }

    // A wedged export must not make the app unquittable: on expiry the wait
    // reports the failure to its caller, which logs and proceeds.
    #[test]
    fn a_wedged_export_expires_the_wait_instead_of_blocking_the_quit_forever() {
        let limit = std::time::Duration::from_millis(40);
        let started = std::time::Instant::now();
        let cleared = wait_until_cleared(|| false, limit, std::time::Duration::from_millis(5));
        let elapsed = started.elapsed();
        assert!(
            !cleared,
            "a never-clearing reservation must report a timeout"
        );
        assert!(
            elapsed >= limit,
            "it gave up after {elapsed:?}, before its own bound"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "it waited {elapsed:?} — far past the bound it was given"
        );
    }

    // The bound is the cancel's, not a finalize's: killing a child, reaping
    // it, joining two reader threads and unlinking one file is nothing like
    // flushing buffered media, so it must stay well under the screen stop's
    // 30 s and the audio stop's 15 s — a quit is the one moment a user is
    // least willing to wait.
    #[test]
    fn the_shutdown_cancel_bound_is_shorter_than_either_capture_finalize() {
        assert!(EXPORT_CANCEL_TIMEOUT < std::time::Duration::from_secs(15));
        assert!(EXPORT_CANCEL_POLL < EXPORT_CANCEL_TIMEOUT);
    }
}
