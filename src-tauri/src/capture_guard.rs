//! The one process-wide "what is capturing right now" guard.
//!
//! A screen capture and an audio recording cannot run concurrently (spec
//! 7.3): both contend for the same audio endpoints, and WASAPI loopback
//! capture of one endpoint from two sessions is a reliability hazard. Two
//! independent per-domain mutexes could not enforce that — each start would
//! see its own domain idle and proceed — so both domains claim THIS guard
//! before touching their own state.
//!
//! LOCK ORDERING: this mutex is never held while any other lock is
//! acquired. `try_claim` takes it, decides, and drops it before returning,
//! so "claim the guard, then take the domain's own state lock" needs no
//! ordering rule to remember and cannot deadlock against the reverse.
//!
//! The guard does not replace `CaptureState`'s own double-start check. It
//! sits in front of it: defence in depth, and the only place that knows
//! about both domains at once.

use std::sync::Mutex;
use vault_buddy_core::sync_util::lock_ignoring_poison;

/// Which capture domain currently owns the devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    Audio,
    // Constructed in production since Task 8 wired the screen-capture
    // commands: the start path claims it, `clear_active_screen` releases
    // it, and the tray dispatches its capture items on it — so the
    // `#[allow(dead_code)]` this variant used to carry is gone (verified by
    // removing it and compiling, not assumed).
    Screen,
}

impl CaptureKind {
    // STILL not called in production — `busy_message` is what the UI
    // renders, and the tray dispatches on the variant itself, so nothing
    // needs this prose form yet; a later audit-log surface is its intended
    // caller. Exercised today only by this module's own tests, and the
    // allow is still load-bearing: removing it fails the build with
    // "method `label` is never used" (checked, not assumed). It is needed
    // at all because `capture_guard` is a PRIVATE `mod` in lib.rs — a `pub`
    // item inside a private module is not reachable from outside the crate,
    // so rustc treats it as private for dead-code purposes.
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            CaptureKind::Audio => "audio recording",
            CaptureKind::Screen => "screen capture",
        }
    }

    /// The refusal a start gets when the OTHER kind (or its own) holds the
    /// guard. Names the running kind so the user knows what to stop.
    pub fn busy_message(self) -> String {
        match self {
            CaptureKind::Audio => "A recording is already in progress.".to_string(),
            CaptureKind::Screen => "A screen capture is already in progress.".to_string(),
        }
    }
}

#[derive(Default)]
pub struct CaptureGuard(Mutex<Option<CaptureKind>>);

impl CaptureGuard {
    /// Claim the devices for `kind`. `Err(held)` names the kind that already
    /// holds them — including `kind` itself, because the guard is
    /// deliberately NOT re-entrant.
    pub fn try_claim(&self, kind: CaptureKind) -> Result<(), CaptureKind> {
        let mut guard = lock_ignoring_poison(&self.0);
        match *guard {
            Some(held) => Err(held),
            None => {
                *guard = Some(kind);
                Ok(())
            }
        }
    }

    /// Release the claim, but ONLY if `kind` is what holds it. Audio's
    /// `clear_active` runs on paths that never claimed; an unkeyed release
    /// there would free a live screen capture's claim.
    pub fn release(&self, kind: CaptureKind) {
        let mut guard = lock_ignoring_poison(&self.0);
        if *guard == Some(kind) {
            *guard = None;
        }
    }

    /// Which domain holds the guard right now. `tray::menu_target` reads it
    /// to route the tray's one set of capture controls at whichever domain
    /// actually put the tray into its recording state.
    pub fn active(&self) -> Option<CaptureKind> {
        *lock_ignoring_poison(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_guard_grants_either_kind() {
        let g = CaptureGuard::default();
        assert_eq!(g.active(), None);
        assert_eq!(g.try_claim(CaptureKind::Audio), Ok(()));
        assert_eq!(g.active(), Some(CaptureKind::Audio));
    }

    #[test]
    fn a_held_guard_refuses_the_other_kind_and_names_the_holder() {
        // The whole point of the guard (spec 7.3): the refusal must say
        // WHICH kind is running, because the two produce different UI copy
        // ("A recording is already in progress." vs the screen wording).
        let g = CaptureGuard::default();
        g.try_claim(CaptureKind::Screen).unwrap();
        assert_eq!(g.try_claim(CaptureKind::Audio), Err(CaptureKind::Screen));
        assert_eq!(g.active(), Some(CaptureKind::Screen));
    }

    #[test]
    fn a_held_guard_refuses_a_second_claim_of_its_own_kind() {
        // Regression: the guard must not be re-entrant. An audio double-start
        // was previously rejected only by CaptureState; if the guard granted
        // it, a future refactor that leaned on the guard alone would let two
        // recordings start.
        let g = CaptureGuard::default();
        g.try_claim(CaptureKind::Audio).unwrap();
        assert_eq!(g.try_claim(CaptureKind::Audio), Err(CaptureKind::Audio));
    }

    #[test]
    fn release_frees_the_guard_for_the_other_kind() {
        let g = CaptureGuard::default();
        g.try_claim(CaptureKind::Audio).unwrap();
        g.release(CaptureKind::Audio);
        assert_eq!(g.active(), None);
        assert_eq!(g.try_claim(CaptureKind::Screen), Ok(()));
    }

    #[test]
    fn release_of_a_kind_that_is_not_held_never_frees_the_other_kind() {
        // Regression, and the reason `release` takes a kind at all: audio's
        // `clear_active` runs on paths where audio never claimed (a start
        // that failed before the claim, the recovery sweep). An unkeyed
        // release there would free a LIVE screen capture's claim and let a
        // second capture start on top of it.
        let g = CaptureGuard::default();
        g.try_claim(CaptureKind::Screen).unwrap();
        g.release(CaptureKind::Audio);
        assert_eq!(g.active(), Some(CaptureKind::Screen));
    }

    #[test]
    fn release_on_an_idle_guard_is_a_no_op() {
        // `clear_active` is called from several audio paths, some of which
        // never claimed. It must be safe to call unconditionally.
        let g = CaptureGuard::default();
        g.release(CaptureKind::Audio);
        assert_eq!(g.active(), None);
    }

    #[test]
    fn the_busy_message_names_the_running_kind() {
        // Spec 14: the typed `alreadyCapturing` error is rendered by the UI;
        // it must tell the user which capture to stop, not just that one is
        // running.
        assert!(CaptureKind::Audio.busy_message().contains("recording"));
        assert!(CaptureKind::Screen
            .busy_message()
            .contains("screen capture"));
    }

    // Structural regression: the guard is released from exactly one place
    // in the audio domain (`clear_active`), plus one defensive
    // already-reserved arm in `start_capture_blocking`. If a future edit
    // adds a third release site, or moves one out of its function, the
    // claim can be freed on a path that never made it -- letting a screen
    // capture and a recording run on the same audio endpoints, which is the
    // single failure this guard exists to prevent. There is no error and no
    // log line either way.
    //
    // Scans the WHOLE shell crate, not `capture_commands.rs` alone. The
    // single-file form this replaces was blind to a release added anywhere
    // else -- the same hole the screen half was PROVEN to have (a rogue
    // release in `staged_commands.rs` left all 185 tests green).
    // `structural_scan` strips test halves, comments and string literals,
    // so this comment and the assertions below cannot satisfy themselves.
    #[test]
    fn audio_releases_the_guard_only_from_the_clear_active_chokepoint() {
        // The METHOD is the needle and the ARGUMENT is what classifies the
        // site, because a module that does not import `CaptureKind` writes
        // `release(crate::capture_guard::CaptureKind::Audio)` -- a spelling
        // a whole-call literal cannot see.
        let releases = crate::structural_scan::call_args(".release(");
        let sites: Vec<&String> = releases
            .iter()
            .filter(|(_, arg)| arg.contains("Audio"))
            .map(|(file, _)| file)
            .collect();
        assert_eq!(
            sites.len(),
            2,
            "expected exactly two Audio releases in the whole shell crate \
             (clear_active, and the defensive already-reserved arm); found {sites:?}. \
             If you added a release site, funnel it through clear_active instead."
        );
        for site in &sites {
            assert!(
                site.ends_with("capture_commands.rs"),
                "an Audio release outside the audio domain: {site}"
            );
        }

        // And in the right FUNCTIONS: a release that drifted out of
        // `clear_active` into, say, `emit_saved` would keep the count at two
        // while freeing the claim on a path the stop-waiters never reach.
        let production =
            crate::structural_scan::production_code(include_str!("capture_commands.rs"));
        let chokepoint = crate::structural_scan::offset_of(&production, "fn clear_active(");
        let after_chokepoint = crate::structural_scan::offset_of(&production, "fn emit_saved(");
        let defensive =
            crate::structural_scan::offset_of(&production, "fn start_capture_blocking(");
        let releases: Vec<usize> = production
            .match_indices(".release(CaptureKind::Audio)")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(releases.len(), 2);
        assert!(
            chokepoint < releases[0] && releases[0] < after_chokepoint,
            "the first Audio release must sit inside clear_active"
        );
        assert!(
            defensive < releases[1],
            "the second Audio release must sit inside start_capture_blocking's \
             already-reserved arm"
        );
    }
}
