//! The 1 s metronome's tick and the main-thread window upkeep it posts.
//!
//! Split out of `lib.rs` when that file reached 802 nonblank against the 800
//! cap. The seam is not arbitrary: everything here is the PERIODIC half of
//! the shell — what runs on a timer and what it is allowed to touch — while
//! `lib.rs` keeps the one-time wiring (the builder, the plugin order, setup,
//! the event handlers). `run()` is unchanged.
//!
//! The invariant these two carry is the one that cost the most to find, so it
//! is repeated where the code is rather than left behind in `lib.rs`: window
//! state saves and window getters run on the MAIN THREAD ONLY. An off-main
//! save takes the window-state plugin's cache lock and then reads window
//! geometry, while the plugin's own Moved listener takes the same lock on the
//! main thread — the collision deadlocked the app during a drag with no crash
//! record. So the metronome is a pure timer: it decides, and posts the work.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::Manager as _;

use vault_buddy_core::sync_util::lock_ignoring_poison;

/// How long the window must sit still before the upkeep tick may touch it.
/// A live OS move loop floods the main thread with Moved events; window
/// work colliding with that flood is what used to deadlock the app
/// (see `window_upkeep_tick`).
const QUIESCE_MS: u64 = 2_000;

/// Consecutive unserviced upkeep ticks before the watchdog reports the main
/// thread as wedged. Upkeep closures are normally dispatched within the
/// second — even during a drag, whose modal loop still pumps posted work.
const MAIN_THREAD_STALL_TICKS: u32 = 10;

/// Instant of the last window Moved event; `None` until the window first
/// moves. Stamped by the window-event hook on the main thread, read by the
/// upkeep tick so every window touch stays away from a window in motion.
/// A plain `Mutex<Option<Instant>>` (the codebase's shared-state idiom — see
/// `MARKER_GATE`) rather than a hand-encoded atomic sentinel.
static LAST_MOVE: Mutex<Option<Instant>> = Mutex::new(None);

pub(crate) fn stamp_window_moved() {
    *lock_ignoring_poison(&LAST_MOVE) = Some(Instant::now());
}

fn ms_since_last_move() -> Option<u64> {
    // Copy the Option out of the guard (Instant is Copy) before mapping —
    // Option::map takes self by value and can't move out of a MutexGuard.
    (*lock_ignoring_poison(&LAST_MOVE)).map(|at| at.elapsed().as_millis() as u64)
}

/// Instant until which the panel's focus-out check must NOT hide the panel.
/// One iteration of the metronome loop: heartbeat the run marker, then post
/// the window work to the main thread with backpressure so at most one
/// upkeep closure is ever outstanding. Split out of the loop so the whole
/// body runs inside `catch_unwind` (skips are early returns, not `continue`).
pub(crate) fn metronome_tick(
    handle: &tauri::AppHandle,
    checkpointer: &Arc<Mutex<vault_buddy_core::checkpoint::PositionCheckpointer>>,
    upkeep_pending: &Arc<AtomicBool>,
    ticks: &mut u32,
    stalled: &mut u32,
) {
    *ticks = ticks.saturating_add(1);
    // Re-stamp the run marker every ~15s, whatever the window or main thread
    // are doing: a hidden buddy or a busy UI is still a running session and
    // must keep heartbeating. This is a backstop once re-armed — see
    // `heartbeat_running_marker`'s doc for why a premature "clean" stamp
    // needs an explicit re-arm, not just this.
    if ticks.is_multiple_of(15) {
        crate::diagnostics::heartbeat_running_marker();
    }
    if upkeep_pending.load(Ordering::Acquire) {
        // The previous tick's closure was never serviced. Don't stack more
        // work behind it; report a wedge once it is clearly not a transient
        // stall — this exact silence used to be an invisible mid-drag
        // deadlock, so it must reach the log.
        *stalled = stalled.saturating_add(1);
        if *stalled == MAIN_THREAD_STALL_TICKS {
            log::error!(
                "main thread has not serviced window upkeep for \
                 ~{MAIN_THREAD_STALL_TICKS}s — the UI may be wedged; \
                 last window move {:?} ms ago",
                ms_since_last_move()
            );
        }
        return;
    }
    if *stalled >= MAIN_THREAD_STALL_TICKS {
        log::info!("main thread responsive again after ~{stalled}s of window-upkeep backlog");
    }
    *stalled = 0;
    upkeep_pending.store(true, Ordering::Release);
    let handle2 = handle.clone();
    let cp = checkpointer.clone();
    let pending = upkeep_pending.clone();
    let posted = handle.run_on_main_thread(move || {
        // A panic here would unwind into the native event loop (a process
        // abort on Windows) — isolate it, and always clear the pending flag
        // so one bad tick can't wedge the backpressure gate forever.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            window_upkeep_tick(&handle2, &cp);
        }));
        if outcome.is_err() {
            log::error!("window upkeep tick panicked; continuing");
        }
        pending.store(false, Ordering::Release);
    });
    if posted.is_err() {
        // Event loop gone (shutdown in progress) — not a stall. Clear the
        // gate so a late tick before teardown doesn't false-report a wedge.
        log::warn!("window upkeep post skipped: event loop unavailable");
        upkeep_pending.store(false, Ordering::Release);
    }
}

/// One round of window upkeep: re-assert always-on-top and checkpoint the
/// parked position.
///
/// MUST run on the main thread. Saving window state takes the window-state
/// plugin's cache lock and then reads window geometry, and the plugin's own
/// Moved listener takes the same lock on the main thread. Run off-main, a
/// save colliding with a drag's Moved flood deadlocked both threads — the
/// background thread held the cache lock while waiting for the main thread
/// to answer a geometry query, the main thread sat in the Moved listener
/// waiting for the cache lock — and the app froze mid-drag with no crash
/// record (nothing panicked, nothing faulted; the frozen process was killed
/// externally). On the main thread the same lock pair is serialized by
/// construction, so the deadlock is gone regardless. The Moved-age gate plus
/// the button-down gate keep even main-thread window work away from a live
/// drag, and only a settled position is ever persisted, so a save never
/// coincides with a move in practice.
fn window_upkeep_tick(
    handle: &tauri::AppHandle,
    checkpointer: &Mutex<vault_buddy_core::checkpoint::PositionCheckpointer>,
) {
    use tauri_plugin_window_state::{AppHandleExt, StateFlags};

    if !vault_buddy_core::checkpoint::is_quiescent(ms_since_last_move(), QUIESCE_MS) {
        return;
    }
    // The Moved-age gate above misses a drag the user pauses for >2s with the
    // button still held (the move loop is live but emits no Moved events). We
    // run on the main thread here, so a direct button-state read is valid and
    // catches exactly that case — never touch a window the user is dragging.
    #[cfg(windows)]
    if crate::commands::primary_button_down() {
        return;
    }
    let Some(window) = handle.get_webview_window("main") else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    // Windows re-shuffles the topmost band when other topmost windows appear
    // (taskbar previews, flyouts), which can drop the buddy behind the
    // taskbar. No event reaches us, so re-assert always-on-top every tick — a
    // cheap z-order-only SetWindowPos that never moves, resizes, or steals
    // focus. Log a failure instead of swallowing it: a persistent failure is
    // how the buddy silently sinks behind the taskbar. (Mid-drag ticks are
    // skipped above, but the window is moving itself then — its z-order
    // cannot be usurped while it owns the move loop.)
    if let Err(e) = window.set_always_on_top(true) {
        log::warn!("always-on-top re-assert failed: {e}");
    }
    if let Ok(pos) = window.outer_position() {
        // The checkpointer defers the first save past the window-state
        // plugin's restore and asks for one only once a changed position has
        // settled; failed writes stay dirty and are retried next tick.
        if lock_ignoring_poison(checkpointer).observe((pos.x, pos.y)) {
            match handle.save_window_state(StateFlags::POSITION) {
                Ok(()) => lock_ignoring_poison(checkpointer).mark_saved(),
                Err(e) => log::warn!("position checkpoint failed: {e}"),
            }
        }
    }
}
