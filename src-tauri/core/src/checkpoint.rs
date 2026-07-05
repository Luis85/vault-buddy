//! Decides when the shell's 1 Hz upkeep tick may touch the window and when
//! it may persist the buddy's position.
//!
//! Why this is a state machine and not a "save when moved" check: saving
//! window state involves the window-state plugin locking its shared cache
//! and reading window geometry. Doing that WHILE the window is being
//! dragged proved fatal on Windows — the OS modal move loop floods the
//! main thread with move events, the plugin's Moved listener takes the
//! same cache lock on the main thread, and a save running concurrently on
//! another thread holds that lock while waiting for the main thread to
//! answer a geometry query: a permanent two-thread deadlock. The app
//! freezes mid-drag and dies without any crash record. So the tick must
//! (a) stay away from the window entirely while it is moving and (b) only
//! persist a position once it has stopped changing.

/// Ticks the checkpointer must observe before the first save. A save that
/// lands before the window-state plugin's restore would poison its cache
/// with the pre-restore default position.
const BASELINE_TICKS: u32 = 3;

/// True when the window has not moved recently enough to be considered at
/// rest. `ms_since_last_move` is `None` when no move was ever observed —
/// a freshly launched, untouched window is at rest by definition.
pub fn is_quiescent(ms_since_last_move: Option<u64>, threshold_ms: u64) -> bool {
    match ms_since_last_move {
        None => true,
        Some(ms) => ms >= threshold_ms,
    }
}

/// Settled-position checkpoint: feed it the window position once per
/// at-rest tick; it answers "persist now?" only when a previously observed
/// change has stopped, never on the tick a change is first seen.
pub struct PositionCheckpointer {
    observed: u32,
    last: Option<(i32, i32)>,
    /// True while a position is waiting to reach disk — set at construction
    /// (the baseline must be persisted once), re-set on every observed move,
    /// cleared only by a confirmed `mark_saved`. A failed save leaves it set
    /// so the next settled tick retries.
    needs_save: bool,
}

impl Default for PositionCheckpointer {
    fn default() -> Self {
        Self {
            observed: 0,
            last: None,
            needs_save: true,
        }
    }
}

impl PositionCheckpointer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the position seen by this tick. Returns true when the caller
    /// should persist it now.
    pub fn observe(&mut self, pos: (i32, i32)) -> bool {
        self.observed = self.observed.saturating_add(1);
        let settled = self.last == Some(pos);
        if self.last.is_some() && !settled {
            self.needs_save = true;
        }
        self.last = Some(pos);
        settled && self.observed >= BASELINE_TICKS && self.needs_save
    }

    /// Confirm the save actually landed. Until then `needs_save` stays set
    /// and `observe` keeps asking for a save — a failed write must be
    /// retried, not forgotten.
    pub fn mark_saved(&mut self) {
        self.needs_save = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_window_is_quiescent() {
        assert!(is_quiescent(None, 2000));
    }

    #[test]
    fn recent_move_is_not_quiescent() {
        assert!(!is_quiescent(Some(0), 2000));
        assert!(!is_quiescent(Some(1999), 2000));
    }

    #[test]
    fn old_move_is_quiescent_again() {
        assert!(is_quiescent(Some(2000), 2000));
        assert!(is_quiescent(Some(60_000), 2000));
    }

    #[test]
    fn baseline_save_waits_three_ticks() {
        // A save before the window-state plugin's restore has landed would
        // poison its cache with the pre-restore position — the first save
        // must wait out the baseline window.
        let mut cp = PositionCheckpointer::new();
        assert!(!cp.observe((10, 20)), "tick 1 must not save");
        assert!(!cp.observe((10, 20)), "tick 2 must not save");
        assert!(cp.observe((10, 20)), "tick 3 persists the baseline");
    }

    #[test]
    fn stable_position_is_saved_only_once() {
        let mut cp = PositionCheckpointer::new();
        for _ in 0..3 {
            cp.observe((10, 20));
        }
        cp.mark_saved();
        assert!(!cp.observe((10, 20)));
        assert!(!cp.observe((10, 20)));
    }

    #[test]
    fn failed_baseline_save_is_retried() {
        let mut cp = PositionCheckpointer::new();
        for _ in 0..3 {
            cp.observe((10, 20));
        }
        // no mark_saved — the write failed; the next tick must ask again
        assert!(cp.observe((10, 20)));
    }

    #[test]
    fn a_change_is_never_saved_on_the_tick_it_is_first_seen() {
        // Regression: the old loop saved on the tick the position CHANGED —
        // i.e. exactly while the window was being dragged — which armed the
        // save/Moved-listener deadlock that froze the app mid-drag.
        let mut cp = PositionCheckpointer::new();
        for _ in 0..3 {
            cp.observe((10, 20));
        }
        cp.mark_saved();
        assert!(
            !cp.observe((300, 400)),
            "a just-moved position must not be persisted yet"
        );
    }

    #[test]
    fn a_change_is_saved_once_it_settles() {
        let mut cp = PositionCheckpointer::new();
        for _ in 0..3 {
            cp.observe((10, 20));
        }
        cp.mark_saved();
        cp.observe((300, 400));
        assert!(cp.observe((300, 400)), "settled position is persisted");
        cp.mark_saved();
        assert!(!cp.observe((300, 400)), "and only once");
    }

    #[test]
    fn failed_settled_save_is_retried_next_tick() {
        let mut cp = PositionCheckpointer::new();
        for _ in 0..3 {
            cp.observe((10, 20));
        }
        cp.mark_saved();
        cp.observe((300, 400));
        assert!(cp.observe((300, 400)));
        // no mark_saved — the write failed
        assert!(cp.observe((300, 400)), "dirty state persists until saved");
    }

    #[test]
    fn continuous_movement_never_saves() {
        // While the position keeps changing the window is in motion — a
        // save now is exactly the mid-drag write this type exists to
        // prevent.
        let mut cp = PositionCheckpointer::new();
        for i in 0..20 {
            assert!(!cp.observe((i, i)), "moving position must never save");
        }
    }
}
