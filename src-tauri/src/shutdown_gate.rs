//! The one composition of "something is running that a process exit would
//! destroy".
//!
//! Five domains can be mid-write when the app is asked to go away, and an
//! exit path has to consult all five:
//!
//! - `capture_commands::recording_blocks_shutdown` — an audio recording,
//!   whose `.mp3.part` is stranded by an exit;
//! - `screen_commands::capture_blocks_shutdown` — a screen capture, same;
//! - `export_shutdown::export_blocks_shutdown` — a screen-capture EXPORT,
//!   the only one of the three mid-write INTO A VAULT, and the only one
//!   with a separate ffmpeg child process that nothing on the way out
//!   would otherwise stop;
//! - `editor::render_jobs::blocks_shutdown` (Task 46, ADR R12) — an editor
//!   RENDER not yet ended (queued and preparing too, since fix round 1): an
//!   ffmpeg child writing a product into the project store, or about to
//!   start one, or a finished one being moved into
//!   `products\` and recorded. Like the export it has a child and needs no
//!   indicator, so hide-to-tray does not gate on it either.
//! - `editor::publish::blocks_shutdown` (Task 48, F19) — a PUBLISH, the
//!   tenth sanctioned vault write, copying a product into a vault. Its own
//!   term by KIND: folding it into the render term's set would report it as
//!   a render and, since the render cancel never ends it, re-open GAP-190's
//!   Alt+F4 loop. The quit workers cancel it (bounded; a cancelled copy
//!   removes its temp) and the updater refuses while it runs.
//!
//! GAP-160 is why the disjunction lives in one place instead of being
//! spelled at each door. `tray::quit` and `window_close::handle_main_close`
//! each wrote it out, and the third door — `commands::prepare_update_install`,
//! the updater's — consulted NO predicate at all: Install & restart mid-
//! recording stranded a `.part`, and mid-export killed the ninth sanctioned
//! vault write and orphaned the ffmpeg child. That is exactly the failure
//! GAP-155 had just closed for the other two doors, reachable through a
//! third. A fourth door must not be able to miss a term.
//!
//! **`tray::hide_buddy` is deliberately NOT a caller**, and that is the
//! asymmetry a structural test in `export_shutdown` pins in both directions.
//! Hide is refused mid-capture because the buddy is the RECORDING
//! indicator — a recording must never run with nothing on screen saying so.
//! An export needs no indicator (it renders its own progress in the editor
//! window, which hide-to-tray does not touch), and refusing hide for the
//! minutes an export runs would pin the app on screen during exactly the
//! operation a user wants to walk away from. Two rules, and this module is
//! only one of them.
//!
//! What a caller does with the answer is its own business, and the three
//! doors differ: the two quit paths park a worker that cancels the export,
//! the renders and the publishes (bounded), finalizes the captures, then
//! exits. The updater REFUSES — it is
//! a synchronous command that must stay on the main thread (see
//! `commands::prepare_update_install`), so it cannot sleep-wait for
//! anything; and unlike a tray quit the user is right there, having just
//! clicked Install & restart, so naming what is running and letting them
//! stop it is both possible and honest.

use tauri::AppHandle;

/// What a shutdown found running. Ordered as `shutdown_blocker` tests
/// them; at most one is reported, because the refusal only needs to name
/// something the user can act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownBlocker {
    Recording,
    ScreenCapture,
    Export,
    /// An editor render (Task 46, R12): an ffmpeg child writing a product,
    /// or a finished one being moved into place and recorded.
    Render,
    /// A publication into a vault (Task 48, F19): the tenth sanctioned
    /// vault write, copying a product in. Counted by its own kind, never
    /// inside the render term.
    Publish,
}

impl ShutdownBlocker {
    /// The refusal a user-initiated UPDATE INSTALL gets.
    ///
    /// Deliberately not `CaptureGuard::busy_message`, which reads "A
    /// recording is already **in progress**" — that is the refusal a
    /// second START gets, where "already" is the whole point. Here the user
    /// asked to replace the running application, so the message has to name
    /// what would be destroyed AND the action that clears it; "already"
    /// would be answering a question nobody asked.
    pub fn install_refusal(self) -> String {
        match self {
            ShutdownBlocker::Recording => {
                "A recording is in progress. Stop the recording, then install the update."
            }
            ShutdownBlocker::ScreenCapture => {
                "A screen capture is in progress. Stop the capture, then install the update."
            }
            ShutdownBlocker::Export => {
                "A screen capture is being saved into a vault. Wait for the save to finish, \
                 or cancel it in the editor, then install the update."
            }
            ShutdownBlocker::Render => {
                "A video is being rendered in the editor. Wait for the render to finish, or \
                 cancel it in the editor, then install the update."
            }
            ShutdownBlocker::Publish => {
                "A video is being published into a vault. Wait for the publish to finish, \
                 then install the update."
            }
        }
        .to_string()
    }
}

/// What is running that a process exit would destroy, if anything.
///
/// The single composition every exit path reads. Short-circuits, so it
/// takes at most one domain's lock more than it has to.
pub fn shutdown_blocker(app: &AppHandle) -> Option<ShutdownBlocker> {
    if crate::capture_commands::recording_blocks_shutdown(app) {
        Some(ShutdownBlocker::Recording)
    } else if crate::screen_commands::capture_blocks_shutdown(app) {
        Some(ShutdownBlocker::ScreenCapture)
    } else if crate::export_shutdown::export_blocks_shutdown(app) {
        Some(ShutdownBlocker::Export)
    } else if crate::editor::render_jobs::blocks_shutdown(app) {
        Some(ShutdownBlocker::Render)
    } else if crate::editor::publish::blocks_shutdown(app) {
        Some(ShutdownBlocker::Publish)
    } else {
        None
    }
}

/// True while anything above is running. The form the two quit paths want:
/// they deal with all five regardless of which answered, so they never
/// need to know which one did.
pub fn shutdown_is_blocked(app: &AppHandle) -> bool {
    shutdown_blocker(app).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural_scan::{fn_body, offset_of, shell_file};

    const ALL: [ShutdownBlocker; 5] = [
        ShutdownBlocker::Recording,
        ShutdownBlocker::ScreenCapture,
        ShutdownBlocker::Export,
        ShutdownBlocker::Render,
        ShutdownBlocker::Publish,
    ];

    // ---- GAP-160: the refusal has to be actionable ----

    // A refusal the user cannot act on is a dead end: they clicked Install
    // & restart and something said no. Each message must name WHAT is
    // running and WHAT clears it, and the three must be distinguishable —
    // "stop the recording" is the wrong instruction for an export, which
    // has no Stop and is cancelled in the editor instead.
    #[test]
    fn every_install_refusal_names_what_is_running_and_what_to_do() {
        for blocker in ALL {
            let msg = blocker.install_refusal();
            assert!(
                msg.contains("install the update"),
                "{blocker:?}: the refusal must say what was refused: {msg:?}"
            );
            // A publish (Task 48) has no Cancel of its own -- a short
            // copy -- so waiting for it IS the action that clears it.
            assert!(
                msg.contains("Stop the") || msg.contains("cancel it") || msg.contains("Wait for"),
                "{blocker:?}: the refusal must name the action that clears it: {msg:?}"
            );
            // Fix round 1: a lost line continuation once left a run of
            // spaces in the middle of the render refusal.
            assert!(
                !msg.contains("  "),
                "{blocker:?}: a run of spaces in the refusal: {msg:?}"
            );
        }
        let refusals: std::collections::BTreeSet<String> =
            ALL.iter().map(|b| b.install_refusal()).collect();
        assert_eq!(refusals.len(), ALL.len(), "five distinguishable refusals");
        // Task 48: the tenth vault write names the vault too.
        assert!(ShutdownBlocker::Publish.install_refusal().contains("vault"));
        let export = ShutdownBlocker::Export.install_refusal();
        assert!(
            ShutdownBlocker::Render.install_refusal().contains("render"),
            "the render refusal names the render (Task 46)"
        );
        assert!(
            export.contains("vault"),
            "the export is the one mid-write into a vault, and the refusal \
             is the only place the user learns that: {export:?}"
        );
    }

    // This is NOT the refusal a second START gets. `CaptureGuard`'s says a
    // capture is "already in progress", which answers "why can I not start
    // another one" — the wrong question when the user asked to replace the
    // running app. The second assertion is what keeps the first honest: it
    // proves the phrase really is the other surface's voice rather than a
    // string nobody uses.
    #[test]
    fn the_install_refusal_is_not_the_start_refusal() {
        for blocker in ALL {
            let msg = blocker.install_refusal();
            assert!(
                !msg.contains("already in progress"),
                "{blocker:?}: that is the refusal a START gets: {msg:?}"
            );
        }
        assert!(crate::capture_guard::CaptureKind::Audio
            .busy_message()
            .contains("already in progress"));
    }

    // ---- GAP-160: one gate, and every door goes through it ----

    // The composition itself. A gate that forgot a domain refuses nothing
    // for it, and no caller can tell — which is precisely how the updater
    // came to consult none of the three.
    #[test]
    fn the_gate_composes_all_five_domain_predicates() {
        let src = shell_file("shutdown_gate.rs");
        let body = fn_body(&src, "pub fn shutdown_blocker(");
        for needle in [
            "recording_blocks_shutdown",
            "capture_blocks_shutdown",
            "export_blocks_shutdown",
            // Task 46 (R12): an editor render.
            "render_jobs::blocks_shutdown(",
            // Task 48 (F19): a publication into a vault -- its OWN term,
            // by kind, never folded into the render term's set.
            "publish::blocks_shutdown(",
        ] {
            assert!(
                body.contains(needle),
                "the one shutdown gate must consult {needle}"
            );
        }
    }

    // Every door reads the one gate rather than re-spelling the
    // disjunction. `hide_buddy` is the deliberate exception and is pinned
    // the other way round in `export_shutdown`.
    #[test]
    fn all_three_exit_paths_consult_the_one_gate() {
        let tray = shell_file("tray.rs");
        let close = shell_file("window_close.rs");
        let commands = shell_file("commands.rs");
        assert!(fn_body(&tray, "pub fn quit(").contains("shutdown_is_blocked("));
        assert!(fn_body(&close, "fn handle_main_close(").contains("shutdown_is_blocked("));
        assert!(
            fn_body(&commands, "pub fn prepare_update_install(").contains("shutdown_blocker("),
            "the updater door is the one that had no gate at all (GAP-160)"
        );
    }

    // ORDER, which no "does it call the gate" assertion sees. The refusal
    // has to come before every side effect the prepare step has:
    //
    // - before `mark_clean_shutdown`, or a refused install latches crash
    //   detection off for the rest of the session and the next real native
    //   fault reports no crash;
    // - before `close_panel`, or the panel the user has to read the refusal
    //   IN is hidden by the very call that refuses;
    // - before `save_window_state`, which is simply work a refusal should
    //   not do.
    #[test]
    fn the_updater_refuses_before_it_touches_anything() {
        let commands = shell_file("commands.rs");
        let body = fn_body(&commands, "pub fn prepare_update_install(");
        let gate = offset_of(body, "shutdown_blocker(");
        for after in ["mark_clean_shutdown(", "close_panel(", "save_window_state("] {
            assert!(
                gate < offset_of(body, after),
                "prepare_update_install must refuse before {after} runs"
            );
        }
    }

    // The refusal must actually LEAVE. A gate that logs and falls through
    // installs the update anyway, and every assertion above still passes.
    #[test]
    fn the_updater_returns_the_refusal_rather_than_falling_through() {
        let commands = shell_file("commands.rs");
        let body = fn_body(&commands, "pub fn prepare_update_install(");
        let gate = offset_of(body, "shutdown_blocker(");
        let returned = offset_of(body, "return Err(");
        assert!(
            gate < returned && returned < offset_of(body, "mark_clean_shutdown("),
            "the gate must return Err before the prepare step's side effects"
        );
        assert!(
            body.contains("install_refusal()"),
            "the refusal the frontend surfaces must be the one this module writes"
        );
    }
}
