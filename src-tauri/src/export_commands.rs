//! The export command's IPC surface: the one-at-a-time reservation, the
//! refusals that must happen before any work starts, and the only place the
//! staged sidecar's hand-editable `timeline` field is ever interpreted.
//!
//! **Export READS the cross-domain capture guard; it never claims it.** A
//! claim would need a second `release(CaptureKind::Screen)` site, and the
//! one release in the whole shell lives in
//! `screen_commands::clear_active_screen` with a structural test pinning it
//! at exactly one there and zero in the capture worker. Reading is also the
//! right behaviour on the merits: an export and a live capture both drive
//! the machine's H.264 encoder, and between an in-progress recording and a
//! save that can simply be repeated, the recording is the irreplaceable
//! one. `CaptureGuard::active()` is a take-decide-drop on its own mutex, so
//! the "never held while another lock is taken" rule is untouched.
//!
//! The heavy lifting — ffmpeg, the vault write, the note — lives in
//! `export_worker`, on a named `screen-export` thread.
//!
//! This module owns the export LIFECYCLE and, with it, ALL FIVE of the
//! feature's events: every one goes through the single `emit` below, which
//! warns rather than discarding a failed send, and a structural test pins
//! that at one call site. Everything about a staged capture as an OBJECT —
//! listing, discarding, and the Obsidian hand-off for a saved one — lives
//! in `staged_commands`, which is the same surface split across two files
//! because one would sit well over the Rust LOC cap. `screen:discarded` is
//! emitted from here on that module's behalf for exactly the reason above.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::timeline::{Segment, Timeline};

use crate::capture_guard::{CaptureGuard, CaptureKind};
use crate::export_worker::{ExportFailure, ExportSummary};

/// The in-flight export. One at a time, process-wide.
pub struct ActiveExport {
    pub base: String,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct ExportState(pub Mutex<Option<ActiveExport>>);

/// THE chokepoint: drop the reservation. One place, so no path can finish an
/// export without freeing the slot — `clear_active_screen`'s discipline
/// applied to a much smaller piece of state.
pub(crate) fn clear_active_export(app: &AppHandle) {
    let state = app.state::<ExportState>();
    *lock_ignoring_poison(&state.0) = None;
}

/// The cancel flag of the export currently reserved, if any.
pub(crate) fn cancel_flag(app: &AppHandle) -> Option<Arc<AtomicBool>> {
    lock_ignoring_poison(&app.state::<ExportState>().0)
        .as_ref()
        .map(|active| Arc::clone(&active.cancel))
}

/// Why an export cannot start while a capture is running, or `None`.
///
/// A pure function over the guard's answer rather than an `if` inside the
/// command, so BOTH arms are asserted by a test on the platform the suite
/// actually runs on. A structural scan can only see that `active()` is
/// called; it cannot see that the refusal was not inverted.
pub(crate) fn busy_refusal(active: Option<CaptureKind>) -> Option<String> {
    active.map(CaptureKind::busy_message)
}

/// Turn the sidecar's hand-editable `timeline` field into a real timeline.
///
/// This is the ONLY place that value is interpreted (docs/Gaps.md GAP-134:
/// `load_staged_capture` passes it through as an opaque `serde_json::Value`
/// it never inspects). Anything malformed — absent, null, wrong-typed, a
/// segment with a non-numeric, negative or fractional bound — degrades to
/// the WHOLE capture, the same defensive-read posture as the rest of the
/// vault domain.
///
/// That default is safe only because `Timeline::is_untouched` is the
/// authority on the fast path: a whole-capture timeline answers it exactly
/// as an absent field would, so a degraded read REMUXES rather than
/// re-encoding. An EXPLICITLY empty segment list is NOT degraded — the user
/// deleted everything, and `export_refusal` must see that rather than have
/// their recording silently restored underneath them.
pub(crate) fn timeline_from_sidecar(
    value: Option<serde_json::Value>,
    source_duration_ms: u64,
) -> Timeline {
    let whole = || Timeline::whole(source_duration_ms);
    let Some(value) = value else { return whole() };
    let Some(segments) = value.get("segments").and_then(|s| s.as_array()) else {
        return whole();
    };
    let mut parsed = Vec::with_capacity(segments.len());
    for seg in segments {
        let (Some(start), Some(end)) = (
            seg.get("sourceStartMs").and_then(serde_json::Value::as_u64),
            seg.get("sourceEndMs").and_then(serde_json::Value::as_u64),
        ) else {
            return whole();
        };
        parsed.push(Segment {
            source_start_ms: start,
            source_end_ms: end,
        });
    }
    Timeline { segments: parsed }
}

/// The number `screen:exportProgress` carries.
///
/// Extracted from the emitter so a Rust test can pin the SHAPE. `export`
/// throttles on whole percents, so the number that passes the gate and the
/// number the user sees are the same number — but the event carries the
/// FRACTION spec §8.3 names, and 0..100 where 0..1 is expected renders a
/// progress bar that is full from the first tick and stays there. That is a
/// one-token error the compiler has no opinion about.
///
/// It cannot call `select::progress_fraction`: that one takes two
/// millisecond counts, not a percent, and re-deriving a fraction from a
/// percent is all this is.
pub(crate) fn progress_payload_fraction(percent: u64) -> f64 {
    percent as f64 / 100.0
}

/// Emit `event`, and SAY SO when it fails.
///
/// The single `app.emit` call site in this module (a structural test pins
/// that, and scans this very prefix for the discarded-result idiom — so
/// spelling that idiom out even in prose would fail it). AGENTS.md's
/// diagnostics invariant forbids a swallowed error, and a discarded emit
/// result is the most invisible kind there is: the editor simply never
/// learns the export finished, its bar sits at whatever the last progress
/// tick said, and nothing in the log explains it.
fn emit(app: &AppHandle, event: &str, payload: serde_json::Value) {
    if let Err(e) = app.emit(event, payload) {
        log::warn!("screen export: could not emit {event}: {e}");
    }
}

pub(crate) fn emit_export_progress(app: &AppHandle, base: &str, percent: u64) {
    emit(
        app,
        "screen:exportProgress",
        serde_json::json!({ "base": base, "fraction": progress_payload_fraction(percent) }),
    );
}

fn emit_exported(app: &AppHandle, summary: &ExportSummary) {
    emit(
        app,
        "screen:exported",
        serde_json::json!({
            "base": summary.base,
            "videoPath": summary.video_path.to_string_lossy(),
            "notePath": summary.note_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "vaultId": summary.vault_id,
            "vaultName": summary.vault_name,
            "warning": summary.warning,
        }),
    );
}

fn emit_export_cancelled(app: &AppHandle, base: &str) {
    emit(
        app,
        "screen:exportCancelled",
        serde_json::json!({ "base": base }),
    );
}

fn emit_export_failed(app: &AppHandle, base: &str, message: &str) {
    emit(
        app,
        "screen:exportFailed",
        serde_json::json!({ "base": base, "message": message }),
    );
}

/// Not in the spec, and added deliberately. Without it a discarded capture
/// leaves `lastStaged` pointing at a base that is no longer on disk, so the
/// panel keeps offering **Edit** on it and `open_capture_editor` →
/// `load_staged_capture` fails with a banner the user cannot act on.
pub(crate) fn emit_discarded(app: &AppHandle, base: &str) {
    emit(app, "screen:discarded", serde_json::json!({ "base": base }));
}

/// Export the staged capture `base` and save it into its vault.
///
/// Async: it reads the sidecar, probes the disk, runs a full re-encode on a
/// worker thread and waits for it. None of that may sit on the main thread.
///
/// A CANCEL is not a failure (spec §14): the staged capture and its timeline
/// are untouched, the editor stays open, and this returns `Ok`.
#[tauri::command]
pub async fn export_and_save_capture(app: AppHandle, base: String) -> Result<(), String> {
    if !crate::editor_commands::is_safe_base(&base) {
        log::warn!("export_and_save_capture: refused a base outside staging: {base:?}");
        return Err("That capture name is not one of ours.".to_string());
    }
    // Read, never claim — see this module's doc.
    if let Some(message) = busy_refusal(app.state::<CaptureGuard>().active()) {
        return Err(message);
    }
    {
        let state = app.state::<ExportState>();
        let mut guard = lock_ignoring_poison(&state.0);
        if let Some(active) = guard.as_ref() {
            return Err(format!("An export of {} is already running.", active.base));
        }
        *guard = Some(ActiveExport {
            base: base.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
        });
    }
    // A NAMED thread, not `spawn_blocking`. Every spawned thread in this app
    // is named because a crash record must identify the dying thread, and
    // this is the longest-running piece of work the app does — exactly the
    // thread a crash is most likely to be in. `spawn_blocking` would put it
    // on a tokio pool thread carrying tokio's name.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<ExportSummary, ExportFailure>>();
    let worker_app = app.clone();
    let worker_base = base.clone();
    let spawned = std::thread::Builder::new()
        .name("screen-export".into())
        .spawn(move || {
            let outcome = crate::export_worker::export_blocking(&worker_app, &worker_base);
            // A dropped receiver is not a reason to panic across the FFI
            // boundary; the caller has already gone away.
            let _ = done_tx.send(outcome);
        });
    if let Err(e) = spawned {
        clear_active_export(&app);
        return Err(format!("The export could not be started: {e}"));
    }
    // `recv` blocks, so it goes on the runtime's blocking side rather than
    // the main thread. Unbounded on purpose: an export of a long recording
    // can legitimately take many minutes, and a deadline here would abandon
    // a worker that is still writing into the user's vault.
    let received = tauri::async_runtime::spawn_blocking(move || done_rx.recv())
        .await
        .map_err(|e| format!("The export could not be awaited: {e}"))?;
    clear_active_export(&app);
    match received {
        Ok(Ok(summary)) => {
            emit_exported(&app, &summary);
            Ok(())
        }
        Ok(Err(ExportFailure::Cancelled)) => {
            emit_export_cancelled(&app, &base);
            Ok(())
        }
        Ok(Err(ExportFailure::Failed(message))) => {
            emit_export_failed(&app, &base, &message);
            Err(message)
        }
        // The worker thread died without sending — a panic inside it. The
        // staged capture is untouched, so the honest answer is "try again".
        Err(_) => {
            let message = "The export stopped unexpectedly.".to_string();
            emit_export_failed(&app, &base, &message);
            Err(message)
        }
    }
}

/// Stop the running export.
///
/// SYNC: it takes one mutex, sets one flag and drops it — no I/O, so the
/// documented rule keeps it off the blocking pool. `Ok` even when nothing is
/// running: a Cancel click racing the export's own completion is not an
/// error the user should be shown.
#[tauri::command]
pub fn cancel_export(app: AppHandle) -> Result<(), String> {
    match lock_ignoring_poison(&app.state::<ExportState>().0).as_ref() {
        Some(active) => {
            log::info!("screen export: cancel requested for {}", active.base);
            active.cancel.store(true, Ordering::Relaxed);
        }
        None => log::info!("screen export: cancel with no export running; ignoring"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production_src() -> &'static str {
        include_str!("export_commands.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix")
    }

    fn worker_src() -> &'static str {
        include_str!("export_worker.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix")
    }

    // Export must not claim the cross-domain capture guard: the one
    // `release(CaptureKind::Screen)` in the whole shell lives in
    // `screen_commands::clear_active_screen`, and a structural test there
    // pins it at exactly one. A claim here would need a second release site.
    #[test]
    fn export_reads_the_capture_guard_but_never_claims_or_releases_it() {
        for src in [production_src(), worker_src()] {
            assert!(
                !src.contains("try_claim("),
                "export must not claim CaptureGuard"
            );
            assert!(
                !src.contains(".release("),
                "export must not release CaptureGuard"
            );
        }
        assert!(
            production_src().contains("CaptureGuard>().active()"),
            "export must refuse while a capture of either kind is live"
        );
    }

    // ...and the refusal must not be inverted. A structural scan can see
    // that `active()` is called; only this can see what the answer is used
    // for.
    #[test]
    fn an_idle_machine_is_not_refused_and_a_busy_one_is_told_which_kind() {
        assert_eq!(busy_refusal(None), None);
        let audio = busy_refusal(Some(CaptureKind::Audio)).expect("a live recording refuses");
        assert!(audio.contains("recording"), "{audio}");
        let screen = busy_refusal(Some(CaptureKind::Screen)).expect("a live capture refuses");
        assert!(screen.contains("screen"), "{screen}");
    }

    #[test]
    fn a_malformed_sidecar_timeline_degrades_to_the_whole_capture() {
        // An absent timeline is an unedited capture.
        assert_eq!(timeline_from_sidecar(None, 5_000), Timeline::whole(5_000));
        // So is a null, an empty object, a wrong-typed field, and a segment
        // list with a bound that is not a plain non-negative integer — every
        // one is a hand-edit or a version skew, and none of them may become
        // a partial export.
        for bad in [
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!({ "segments": "nope" }),
            serde_json::json!({ "segments": [{"sourceStartMs": "x", "sourceEndMs": 3}] }),
            serde_json::json!({ "segments": [{"sourceStartMs": -1, "sourceEndMs": 3}] }),
            serde_json::json!({ "segments": [{"sourceStartMs": 1.5, "sourceEndMs": 3}] }),
            serde_json::json!({ "segments": [{"sourceStartMs": 0}] }),
            serde_json::json!([]),
        ] {
            assert_eq!(
                timeline_from_sidecar(Some(bad.clone()), 5_000),
                Timeline::whole(5_000),
                "not defaulted: {bad}"
            );
        }
    }

    #[test]
    fn a_well_formed_sidecar_timeline_is_honoured_exactly() {
        let value = serde_json::json!({
            "segments": [
                {"sourceStartMs": 4_000, "sourceEndMs": 9_000},
                {"sourceStartMs": 0, "sourceEndMs": 1_000}
            ]
        });
        let t = timeline_from_sidecar(Some(value), 9_000);
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[0].source_start_ms, 4_000);
        assert_eq!(t.segments[1].source_end_ms, 1_000);
        // And it must NOT read as untouched — this is the edited path.
        assert!(!t.is_untouched(9_000));
    }

    // An empty segment list is NOT the same as an absent field: the user
    // deleted everything, and `export_refusal` must see it rather than have
    // the whole capture silently restored underneath them.
    #[test]
    fn an_explicitly_empty_segment_list_is_kept_empty_not_restored() {
        let t = timeline_from_sidecar(Some(serde_json::json!({ "segments": [] })), 5_000);
        assert!(
            t.is_empty(),
            "an explicit empty edit was replaced by the whole capture"
        );
    }

    // The base becomes a path (the staging sidecar, the staged mp4, the
    // export temp), so it is gated by the SAME predicate every other
    // base-taking command uses — never a second copy of those rules.
    #[test]
    fn the_command_guards_its_base_with_the_shared_predicate() {
        for bad in [
            "../obsidian",
            "a/b",
            ".hidden",
            "trailing.",
            "C:Windows",
            "COM1",
            "x ",
        ] {
            assert!(
                !crate::editor_commands::is_safe_base(bad),
                "{bad:?} must be refused before it becomes a path"
            );
        }
        let body = production_src()
            .split_once("pub async fn export_and_save_capture")
            .expect("the command")
            .1;
        let body = &body[..body.find("\n}\n").expect("the command's end")];
        assert!(
            body.contains("is_safe_base(&base)"),
            "export_and_save_capture must refuse an unsafe base in its own body"
        );
    }

    #[test]
    fn the_progress_payload_carries_a_fraction_not_a_percent() {
        assert_eq!(progress_payload_fraction(0), 0.0);
        assert_eq!(progress_payload_fraction(50), 0.5);
        assert_eq!(progress_payload_fraction(100), 1.0);
        // The bug this exists for: a bar that is full from the first tick
        // and stays there, because 0..100 went where 0..1 was expected.
        assert!(progress_payload_fraction(1) < 0.5);
    }

    // Every one of the five export events goes through the one emitter that
    // logs a failed send. AGENTS.md's diagnostics invariant forbids a
    // swallowed error, and a `let _ = app.emit(..)` is the most invisible
    // kind there is: the editor simply never learns the export finished, its
    // bar sits at the last progress tick, and nothing in the log says why.
    #[test]
    fn every_export_event_is_emitted_through_the_one_warning_emitter() {
        let src = production_src();
        assert!(
            !src.contains("let _ = app.emit"),
            "an export event is emitted with its failure swallowed"
        );
        assert_eq!(
            src.matches("app.emit(").count(),
            1,
            "every event must go through the single emitter that warns on failure"
        );
        for event in [
            "screen:exportProgress",
            "screen:exported",
            "screen:exportCancelled",
            "screen:exportFailed",
            "screen:discarded",
        ] {
            // The QUOTED form, so the module's own prose about an event
            // is not mistaken for a second emit of it.
            assert_eq!(
                src.matches(&format!("\"{event}\"")).count(),
                1,
                "{event} is emitted from more or fewer than one place"
            );
        }
    }

    // The ONE seam between `ExportSummary` and the editor is a JSON object
    // literal, so nothing in either language notices when a key stops being
    // emitted: Rust still compiles (the field is constructed, just not
    // serialised) and the Vitest suite still passes (it emits its own
    // payload). `vaultName` is the key at risk, because it exists purely to
    // be READ — drop it and the editor silently falls back to "into your
    // vault", which looks like working software. `src/types.ts`'s
    // `ExportResult` is the other half; this is the GAP-135 class, pinned
    // structurally because there is no derive to enforce it.
    #[test]
    fn the_exported_event_carries_every_field_the_editor_reads() {
        let src = production_src();
        let body = src
            .split_once("fn emit_exported(")
            .expect("the exported emitter")
            .1;
        let body = &body[..body.find("\n}\n").expect("the emitter's end")];
        for key in [
            "\"base\"",
            "\"videoPath\"",
            "\"notePath\"",
            "\"vaultId\"",
            "\"vaultName\"",
            "\"warning\"",
        ] {
            assert!(
                body.contains(key),
                "screen:exported must carry {key} — src/types.ts ExportResult declares it"
            );
        }
    }
}
