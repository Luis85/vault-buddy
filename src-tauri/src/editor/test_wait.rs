//! Handshakes for the shell's lock-ordering tests (hardening Task 21, T-7;
//! test-only).
//!
//! A test that proves "B waits for A's lock" used to sleep 200 ms and then
//! read `!b.is_finished()`. A sleep proves nothing on a loaded host: B may
//! not have been scheduled at all yet, so the assertion read a thread that
//! had not STARTED as one that was blocked. The lock every such test holds
//! is a session's `Arc<Mutex<()>>` from `session_save_lock`, so the
//! waiter's own clone is observable: once the strong count says it holds
//! its handle, its next step is the `lock()` the test is holding -- it is
//! queued, not merely spawned (the `recovery_tests.rs` "save queued behind
//! a discard" shape, final review M12).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::save_commands::session_save_lock;
use super::EditorState;

/// Spin until `holders` handles on `session_id`'s save lock exist besides
/// this function's own -- the map's, the test's or the thread's that holds
/// it, and the waiter's. `false` after 10 s: a waiter that never takes its
/// handle never asked for the lock.
///
/// Returns rather than panics: a caller whose scope still holds a thread
/// parked on a channel (the gated-writer tests) must release it before any
/// assertion, or `thread::scope` deadlocks joining it.
pub(crate) fn wait_for_holders(state: &EditorState, session_id: &str, holders: usize) -> bool {
    let lock = session_save_lock(state, session_id).expect("the session is live");
    let started = Instant::now();
    while Arc::strong_count(&lock) < holders + 1 {
        if started.elapsed() > Duration::from_secs(10) {
            return false;
        }
        std::thread::yield_now();
    }
    true
}
