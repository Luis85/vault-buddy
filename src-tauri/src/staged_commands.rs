//! The staged-capture surface: the captures waiting in staging to be
//! resumed or discarded, and the Obsidian hand-off for one that has been
//! saved.
//!
//! Split out of `export_commands` for size (that module would otherwise sit
//! 189 nonblank lines over the Rust LOC cap, and the baselines in this repo
//! are shrink-only), and the seam is a real one: `export_commands` owns the
//! export LIFECYCLE — the one-at-a-time reservation, the cancel flag and all
//! five `screen:*` events — while everything here is about a staged capture
//! as an OBJECT. The one crossing is deliberate: `screen:discarded` is
//! emitted through `export_commands::emit_discarded`, because every event
//! this feature sends goes through that module's single warning emitter and
//! a structural test pins it at exactly one call site.
//!
//! **`discard_staged_capture` is destructive**, and the second destructive
//! command in the app after `delete_task`. It takes an untrusted name from
//! the frontend and turns it into three paths, so it is gated by the shared
//! `editor_commands::is_safe_base` (never a second copy of those rules), it
//! refuses while THAT capture is being exported, and it refuses a symlink
//! leaf rather than deleting through one.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::uri;
use vault_buddy_screen::staging;

use crate::export_commands::{emit_discarded, timeline_from_sidecar, ExportState};

/// One resume-or-discard row (spec §10) — a staged capture as the UI sees it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedCaptureSummaryDto {
    pub base: String,
    pub vault_id: String,
    pub source_title: String,
    /// What was RECORDED.
    pub duration_ms: u64,
    /// What an export would PRODUCE — shorter whenever the editor cut
    /// something out, and the number the user is really deciding about.
    pub output_duration_ms: u64,
    pub recorded_at: String,
    pub width: u32,
    pub height: u32,
    pub edited: bool,
    /// Rebuilt by `screen_recovery` after an interrupted session, so it knows
    /// neither its vault nor its duration and CANNOT be saved (the export
    /// refuses an empty `vault_id` outright). The sweep writes the marker
    /// into the sidecar's flattened catch-all; surfacing it is what stops
    /// this list offering a save that provably cannot succeed.
    pub recovered: bool,
}

/// Is this sidecar one `screen_recovery` rebuilt? A `true` BOOLEAN, never a
/// truthy anything: the sidecar is hand-editable, and `"recovered": "no"`
/// must not read as a claim in either direction.
pub(crate) fn summary_is_recovered(extra: &serde_json::Map<String, serde_json::Value>) -> bool {
    extra.get("recovered") == Some(&serde_json::Value::Bool(true))
}

/// Does this staged capture carry an edit? The SAME predicate the export
/// fast path keys on, surfaced so the resume-or-discard list can say so.
///
/// Deriving it from "the sidecar HAS a timeline field" would mark every
/// capture the editor has ever been OPENED on as edited: the editor writes a
/// timeline on every operation and never writes null (the rule `6944ac0`
/// established and four tests forbid unlearning).
pub(crate) fn summary_is_edited(
    timeline: Option<serde_json::Value>,
    source_duration_ms: u64,
) -> bool {
    // A source whose duration is UNKNOWN cannot be compared against "the
    // whole", so it is not edited — unknown is not an edit. A recovered
    // capture's sidecar records `duration_ms: 0` because nothing on disk
    // remembers it, and `Timeline::whole(0)` is the EMPTY timeline, which
    // `is_untouched` never matches: without this arm every single recovered
    // capture was listed as "edited · 0:00".
    //
    // Deliberately NOT fixed in `Timeline::is_untouched`. That predicate is
    // the export fast path's, held byte-for-byte against a TypeScript twin
    // by `tests/fixtures/timeline-cases.json` (docs/Gaps.md GAP-136), and
    // it is answering its own question correctly: an empty timeline really
    // is not the whole of anything. The divergence is unreachable there —
    // `export_worker::prepare` refuses a recovered capture before a timeline
    // is ever consulted — so this stays a property of the SUMMARY.
    if source_duration_ms == 0 {
        return false;
    }
    !timeline_from_sidecar(timeline, source_duration_ms).is_untouched(source_duration_ms)
}

/// How long the exported file will be, so the list can show it beside the
/// recording's own length.
pub(crate) fn summary_output_duration_ms(
    timeline: Option<serde_json::Value>,
    source_duration_ms: u64,
) -> u64 {
    timeline_from_sidecar(timeline, source_duration_ms).output_duration_ms()
}

/// Why this discard must be refused, or `None`.
///
/// Deleting the `.mp4` out from under a running export would leave the
/// re-encode reading a handle to an unlinked file on Windows and produce a
/// truncated export with no error anywhere. Only the base being exported is
/// refused: a second staged capture is nobody's business but its own.
pub(crate) fn discard_conflict(exporting: Option<&str>, base: &str) -> Option<String> {
    match exporting {
        Some(active) if active == base => Some(format!(
            "{base} is being saved right now. Cancel the save first, or wait for it to finish."
        )),
        _ => None,
    }
}

/// The `file` parameter of the `obsidian://open` URI for a saved capture.
///
/// A saved capture is a PAIR — a `.mp4` and (usually) its `.md` note — and
/// the Open button can hand over either. `vault_relative_no_ext` alone (the
/// `open_task` / `open_recording` shape, which only ever opens markdown)
/// turns `Screen Captures/2026/09/Demo.mp4` into `…/Demo`, which Obsidian
/// resolves as `Demo.md`: Open beside a video would open the note, or
/// nothing at all in a vault that opted out of notes. So the extension is
/// dropped for exactly-`.md` and KEPT otherwise — `core::search`'s own rule
/// for the same URI, and the reason that rule exists there too.
pub(crate) fn capture_file_param(path: &Path, vault_root: &Path) -> Option<String> {
    if path.extension().and_then(|e| e.to_str()) == Some("md") {
        uri::vault_relative_no_ext(path, vault_root)
    } else {
        uri::vault_relative(path, vault_root)
    }
}

fn staging_dir_for(app: &AppHandle) -> Result<PathBuf, String> {
    let local = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Could not resolve the staging directory: {e}"))?;
    Ok(staging::staging_dir(&local))
}

/// Everything staging can hold for one capture: the video, its sidecar, and
/// an export temp abandoned by a killed or crashed save.
fn staged_file_names(base: &str) -> [String; 3] {
    [
        staging::mp4_file_name(base),
        staging::sidecar_file_name(base),
        staging::export_part_file_name(base),
    ]
}

/// Forget a staged capture, on disk.
///
/// **The export `.part` is the third file and the one a two-file discard
/// forgets.** It is often the LARGEST of the three, and `discard_conflict`
/// has already established that no LIVE export owns it — so any `.part`
/// bearing this base is abandoned by definition. Leaving it strands a file
/// keyed to a capture the user just told us to forget, until some later
/// session's `run_screen_recovery` sweep happens to reach it.
///
/// `NotFound` counts as success — "the path is clear", the
/// `delete_transcription_model` precedent — so a capture a sweep already
/// cleaned up never leaves the user an error they cannot act on.
///
/// Two passes, deliberately: every leaf is inspected BEFORE anything is
/// unlinked, so a symlink in the middle of the set cannot leave a
/// half-discarded capture (one file gone, the rest still listed). A symlink
/// leaf is REFUSED rather than unlinked — `delete_task`'s no-follow
/// discipline — which is what makes "this deletes nothing outside staging" a
/// property of the function rather than of whoever wrote the directory.
pub(crate) fn discard_staged_files(dir: &Path, base: &str) -> Result<(), String> {
    let mut targets = Vec::new();
    for name in staged_file_names(base) {
        let path = dir.join(&name);
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("That capture could not be discarded: {e}")),
            Ok(meta) if meta.file_type().is_symlink() => {
                log::warn!(
                    "screen discard: {} is a symlink; refusing to delete through it",
                    path.display()
                );
                return Err("That capture is not one Vault Buddy can discard.".to_string());
            }
            Ok(_) => targets.push(path),
        }
    }
    for path in targets {
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("That capture could not be discarded: {e}"));
            }
        }
    }
    Ok(())
}

fn summary_from_sidecar(s: &staging::StagedSidecar) -> StagedCaptureSummaryDto {
    StagedCaptureSummaryDto {
        base: s.base.clone(),
        vault_id: s.vault_id.clone(),
        source_title: s.source_title.clone(),
        duration_ms: s.duration_ms,
        output_duration_ms: summary_output_duration_ms(s.timeline.clone(), s.duration_ms),
        recorded_at: s.recorded_at.clone(),
        width: s.width,
        height: s.height,
        edited: summary_is_edited(s.timeline.clone(), s.duration_ms),
        recovered: summary_is_recovered(&s.extra),
    }
}

/// The resume-or-discard list, built from what is actually on disk.
///
/// A VIEW, so it degrades: an unreadable directory is an empty list and any
/// row it cannot trust is skipped, never an error (AGENTS.md's
/// view-may-degrade / guard-must-refuse split). Three rows are skipped and
/// each for its own reason — a name that could never have come from this app,
/// a sidecar whose own `base` disagrees with the file it was read from (that
/// field is hand-editable, and the row would NAME one capture while
/// ADDRESSING another), and a sidecar whose video is gone (the row would open
/// an editor on nothing).
///
/// Newest first, then by base, so a redraw never reshuffles the list.
pub(crate) fn staged_summaries(dir: &Path) -> Vec<StagedCaptureSummaryDto> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(base) = name.strip_suffix(".json") else {
            continue;
        };
        if !crate::editor_commands::is_safe_base(base) {
            continue;
        }
        let Some(sidecar) = staging::read_sidecar(&entry.path()) else {
            continue;
        };
        if sidecar.base != base {
            continue;
        }
        // No-follow, like the recovery sweep: a symlink named as one of ours
        // is not a staged capture and must not be offered as one.
        let video = dir.join(staging::mp4_file_name(base));
        if !std::fs::symlink_metadata(&video).is_ok_and(|m| m.is_file()) {
            continue;
        }
        out.push(summary_from_sidecar(&sidecar));
    }
    out.sort_by(|a, b| {
        b.recorded_at
            .cmp(&a.recorded_at)
            .then_with(|| a.base.cmp(&b.base))
    });
    out
}

/// Forget a staged capture — its video, its sidecar and any abandoned export
/// temp. Irreversible; the editor confirm-gates it (spec §10).
///
/// ASYNC: three unlinks on a volume that may be slow or networked.
#[tauri::command]
pub async fn discard_staged_capture(app: AppHandle, base: String) -> Result<(), String> {
    if !crate::editor_commands::is_safe_base(&base) {
        log::warn!("discard_staged_capture: refused a base outside staging: {base:?}");
        return Err("That capture name is not one of ours.".to_string());
    }
    let exporting = lock_ignoring_poison(&app.state::<ExportState>().0)
        .as_ref()
        .map(|active| active.base.clone());
    if let Some(message) = discard_conflict(exporting.as_deref(), &base) {
        return Err(message);
    }
    let dir = staging_dir_for(&app)?;
    let target = base.clone();
    tauri::async_runtime::spawn_blocking(move || discard_staged_files(&dir, &target))
        .await
        .map_err(|e| format!("That capture could not be discarded: {e}"))??;
    log::info!("screen discard: forgot the staged capture {base}");
    emit_discarded(&app, &base);
    Ok(())
}

/// Every staged capture waiting to be resumed or discarded.
///
/// ASYNC: a directory read plus one sidecar parse per capture.
#[tauri::command]
pub async fn list_staged_captures(app: AppHandle) -> Vec<StagedCaptureSummaryDto> {
    let dir = match staging_dir_for(&app) {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("list_staged_captures: {e}");
            return Vec::new();
        }
    };
    match tauri::async_runtime::spawn_blocking(move || staged_summaries(&dir)).await {
        Ok(rows) => rows,
        Err(e) => {
            log::warn!("list_staged_captures: the staging scan failed: {e}");
            Vec::new()
        }
    }
}

/// Open a SAVED capture — its video or its note — in Obsidian.
///
/// SYNC and read-only, `open_task`'s shape exactly: canonicalise both sides
/// (so `strip_prefix` agrees on Windows' `\\?\` form), require containment
/// inside the vault, then hand off through `uri::launch`, which logs the
/// launch like every other vault open. This never writes.
#[tauri::command]
pub fn open_screen_capture(id: String, path: String) -> Result<(), String> {
    let vault = crate::commands::find_vault(&id)?;
    let canon_vault = std::fs::canonicalize(&vault.path)
        .map_err(|e| format!("Cannot resolve vault folder: {e}"))?;
    let canon_path = std::fs::canonicalize(Path::new(&path))
        .map_err(|e| format!("Cannot resolve the saved capture: {e}"))?;
    if !canon_path.starts_with(&canon_vault) {
        log::warn!("open_screen_capture: {path} is outside the vault it names");
        return Err("That capture is outside its vault.".to_string());
    }
    let rel = capture_file_param(&canon_path, &canon_vault).ok_or_else(|| {
        log::warn!("open_screen_capture: {path} resolved outside its vault");
        "That capture is outside its vault.".to_string()
    })?;
    uri::launch(&uri::open_file_uri(&id, &rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn production_src() -> &'static str {
        include_str!("staged_commands.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix")
    }

    fn export_src() -> &'static str {
        include_str!("export_commands.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix")
    }

    #[test]
    fn a_staged_capture_is_summarised_as_edited_only_when_its_timeline_differs_from_the_whole() {
        // The SAME predicate the export fast path keys on, surfaced to the
        // UI. Deriving "edited" from the FIELD'S PRESENCE would mark every
        // capture the editor has ever been opened on as edited: the editor
        // writes a timeline on every operation and never writes null.
        assert!(!summary_is_edited(None, 5_000));
        assert!(!summary_is_edited(
            Some(serde_json::json!({ "segments": [{"sourceStartMs": 0, "sourceEndMs": 5_000}] })),
            5_000
        ));
        assert!(summary_is_edited(
            Some(
                serde_json::json!({ "segments": [{"sourceStartMs": 1_000, "sourceEndMs": 5_000}] })
            ),
            5_000
        ));
    }

    // REGRESSION (fix wave): a capture recovered by `screen_recovery` after
    // an interrupted session carries `duration_ms: 0` — nothing on disk
    // records the duration once the sidecar is gone. `Timeline::whole(0)` is
    // the EMPTY timeline and an empty timeline is never `is_untouched`, so
    // the naive predicate answered TRUE and every recovered capture was
    // listed as "edited · 0:00". Unknown is not edited.
    #[test]
    fn a_capture_whose_source_duration_is_unknown_is_not_reported_as_edited() {
        assert!(!summary_is_edited(None, 0));
        // ...and not by accident of the timeline being absent either.
        assert!(!summary_is_edited(
            Some(serde_json::json!({ "segments": [] })),
            0
        ));
    }

    // A recovered capture must SAY it is recovered: the `recovered: true`
    // marker `screen_recovery::minimal_sidecar` writes lives in the
    // sidecar's flattened catch-all, and the DTO used to drop it — so the
    // one surface the recovery sweep exists to feed hid the only fact the
    // user needs (that this capture no longer knows its vault and cannot be
    // saved).
    #[test]
    fn a_recovered_capture_is_marked_recovered_in_the_summary() {
        let mut s = sidecar_fixture();
        assert!(!summary_from_sidecar(&s).recovered, "an ordinary capture");
        s.extra
            .insert("recovered".into(), serde_json::Value::Bool(true));
        assert!(summary_from_sidecar(&s).recovered);
        // A non-boolean value (the sidecar is hand-editable) is not a claim.
        s.extra
            .insert("recovered".into(), serde_json::Value::String("yes".into()));
        assert!(!summary_from_sidecar(&s).recovered);
    }

    fn sidecar_fixture() -> staging::StagedSidecar {
        staging::StagedSidecar {
            base: "2026-09-20 1432 Demo".into(),
            vault_id: "v1".into(),
            source_title: "Demo".into(),
            source_kind: "screen".into(),
            inputs: Vec::new(),
            duration_ms: 5_000,
            paused_ms: 0,
            width: 1920,
            height: 1080,
            recorded_at: "2026-09-20T14:32:00Z".into(),
            timeline: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn the_summary_reports_the_output_duration_the_export_will_produce() {
        let trimmed =
            serde_json::json!({ "segments": [{"sourceStartMs": 1_000, "sourceEndMs": 4_000}] });
        assert_eq!(summary_output_duration_ms(Some(trimmed), 9_000), 3_000);
        assert_eq!(summary_output_duration_ms(None, 9_000), 9_000);
    }

    // NOTE — this test is NOT the discard guard's proof, and must not be
    // read as one. It calls `is_safe_base` directly, so it would pass
    // unchanged if `discard_staged_capture` dropped its guard entirely.
    // `editor_commands` already owns tests for this function; the reason to
    // restate the table beside the app's SECOND destructive command is that
    // these seven strings are the ones a destructive path must refuse. The
    // load-bearing test is the structural scan below — that one goes red
    // when the guard is removed. Do not let this test's green stand in for
    // that one's.
    #[test]
    fn the_strings_a_destructive_base_taker_must_refuse() {
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
    }

    #[test]
    fn discard_refuses_while_that_capture_is_being_exported() {
        assert!(discard_conflict(Some("2026-09-20 1432 Demo"), "2026-09-20 1432 Demo").is_some());
        assert!(discard_conflict(Some("2026-09-20 1432 Other"), "2026-09-20 1432 Demo").is_none());
        assert!(discard_conflict(None, "2026-09-20 1432 Demo").is_none());
    }

    // A video is an ATTACHMENT and a note is a note, and Obsidian's `file`
    // parameter treats them differently: it resolves an extensionless name
    // as `<name>.md`. Dropping the extension unconditionally — the
    // `open_task`/`open_recording` shape, which only ever opens markdown —
    // would make the Open button beside a saved capture open the note, or
    // nothing at all in a vault that opted out of notes.
    #[test]
    fn the_open_uri_keeps_a_videos_extension_and_drops_a_notes() {
        let vault = Path::new("/vault");
        assert_eq!(
            capture_file_param(Path::new("/vault/Screen Captures/2026/09/Demo.mp4"), vault)
                .as_deref(),
            Some("Screen Captures/2026/09/Demo.mp4")
        );
        assert_eq!(
            capture_file_param(Path::new("/vault/Screen Captures/2026/09/Demo.md"), vault)
                .as_deref(),
            Some("Screen Captures/2026/09/Demo")
        );
        assert_eq!(
            capture_file_param(Path::new("/elsewhere/Demo.mp4"), vault),
            None
        );
    }

    fn staged_sidecar(base: &str, recorded_at: &str) -> staging::StagedSidecar {
        staging::StagedSidecar {
            base: base.to_string(),
            vault_id: "v1".into(),
            source_title: "Figma".into(),
            source_kind: "screen".into(),
            inputs: vec!["Mic".into()],
            duration_ms: 9_000,
            paused_ms: 0,
            width: 1280,
            height: 720,
            recorded_at: recorded_at.to_string(),
            timeline: None,
            extra: serde_json::Map::new(),
        }
    }

    /// A staged capture on disk: its sidecar and its video.
    fn stage(dir: &Path, base: &str, recorded_at: &str) {
        staging::write_sidecar(dir, base, &staged_sidecar(base, recorded_at)).unwrap();
        std::fs::write(dir.join(staging::mp4_file_name(base)), b"footage").unwrap();
    }

    // The export temp is the THIRD file, and it is often the largest. A
    // discard that removes the video and the sidecar but leaves
    // `.<base>.export.mp4.part` strands a file keyed to a capture the user
    // just told us to forget, until some later session's recovery sweep
    // happens to reach it. A two-file assertion cannot see that.
    #[test]
    fn discarding_a_capture_removes_its_video_its_sidecar_and_any_export_temp() {
        let dir = tempfile::tempdir().unwrap();
        let base = "2026-09-20 1432 Demo";
        stage(dir.path(), base, "2026-09-20T14:32:00+02:00");
        let temp = dir.path().join(staging::export_part_file_name(base));
        std::fs::write(&temp, b"abandoned").unwrap();
        // A file that is NOT ours, so a discard that simply emptied the
        // directory would be caught rather than passing as thorough.
        let foreign = dir.path().join("notes.txt");
        std::fs::write(&foreign, b"keep me").unwrap();

        discard_staged_files(dir.path(), base).expect("the discard lands");

        assert!(!dir.path().join(staging::mp4_file_name(base)).exists());
        assert!(!dir.path().join(staging::sidecar_file_name(base)).exists());
        assert!(
            !temp.exists(),
            "the abandoned export temp survived the discard"
        );
        assert!(
            foreign.is_file(),
            "the discard deleted a file that was not ours"
        );
    }

    // "The path is clear" is success — the `delete_transcription_model`
    // precedent. A capture whose files a sweep already removed must not
    // leave the user with an error they cannot act on.
    #[test]
    fn discarding_a_capture_that_is_already_gone_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        discard_staged_files(dir.path(), "2026-09-20 1432 Gone").expect("nothing to do is success");
    }

    // No-follow, `delete_task`'s discipline: a symlink where one of our
    // files should be is refused outright, and NOTHING is unlinked — not
    // even the files inspected before it — so a discard can never be half
    // done.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_staged_file_is_refused_and_nothing_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("precious.json");
        std::fs::write(&target, b"not ours").unwrap();
        let base = "2026-09-20 1432 Demo";
        std::fs::write(dir.path().join(staging::mp4_file_name(base)), b"footage").unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(staging::sidecar_file_name(base)))
            .unwrap();

        discard_staged_files(dir.path(), base).expect_err("a symlink leaf must be refused");

        assert!(target.is_file(), "the discard deleted through a symlink");
        assert!(
            dir.path().join(staging::mp4_file_name(base)).is_file(),
            "a refused discard removed the video anyway"
        );
    }

    #[test]
    fn the_staged_list_is_newest_first_and_skips_what_it_cannot_trust() {
        let dir = tempfile::tempdir().unwrap();
        stage(dir.path(), "A", "2026-09-18T09:00:00+02:00");
        stage(dir.path(), "B", "2026-09-20T14:32:00+02:00");
        // A sidecar with no video: the resume row would open an editor on
        // nothing.
        staging::write_sidecar(
            dir.path(),
            "C",
            &staged_sidecar("C", "2026-09-21T09:00:00+02:00"),
        )
        .unwrap();
        // A sidecar whose own base disagrees with its file name: the row
        // would NAME one capture and ADDRESS another, and that field is
        // hand-editable.
        std::fs::write(
            dir.path().join("D.json"),
            serde_json::to_vec(&staged_sidecar("elsewhere", "2026-09-22T09:00:00+02:00")).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join(staging::mp4_file_name("D")), b"x").unwrap();
        // A name that could never have come from this app, and must never
        // become a path.
        std::fs::write(dir.path().join(".hidden.json"), b"{}").unwrap();

        let rows = staged_summaries(dir.path());
        let bases: Vec<&str> = rows.iter().map(|r| r.base.as_str()).collect();
        assert_eq!(bases, ["B", "A"], "newest first, and nothing untrusted");
        assert_eq!(rows[0].duration_ms, 9_000);
        assert_eq!(rows[0].output_duration_ms, 9_000);
        assert!(!rows[0].edited);
        assert_eq!(rows[0].source_title, "Figma");
        assert_eq!(rows[0].vault_id, "v1");
    }

    #[test]
    fn an_edited_staged_capture_reports_its_edit_and_its_shorter_length() {
        let dir = tempfile::tempdir().unwrap();
        let base = "2026-09-20 1432 Demo";
        let mut s = staged_sidecar(base, "2026-09-20T14:32:00+02:00");
        s.timeline = Some(
            serde_json::json!({ "segments": [{"sourceStartMs": 1_000, "sourceEndMs": 4_000}] }),
        );
        staging::write_sidecar(dir.path(), base, &s).unwrap();
        std::fs::write(dir.path().join(staging::mp4_file_name(base)), b"footage").unwrap();

        let rows = staged_summaries(dir.path());
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].edited,
            "an edited capture was summarised as untouched"
        );
        assert_eq!(rows[0].output_duration_ms, 3_000);
        assert_eq!(rows[0].duration_ms, 9_000, "the source length must survive");
    }

    #[test]
    fn every_new_command_is_registered_and_the_base_takers_are_guarded() {
        let lib = include_str!("lib.rs");

        let mut all: Vec<&str> = Vec::new();
        let mut checked: Vec<&str> = Vec::new();
        // BOTH modules, because this surface spans two files: the export
        // lifecycle and the staged-capture surface it is split from (they
        // are two files only because one would be 189 lines over the LOC
        // cap). A scan of one of them goes quietly blind the moment a
        // command moves across.
        for (module, src) in [
            ("export_commands", export_src()),
            ("staged_commands", production_src()),
        ] {
            // Matched by PREFIX, not against the literal `#[tauri::command]`: an
            // attribute written with arguments — `#[tauri::command(rename_all =
            // "snake_case")]`, a shape this codebase already uses — is invisible
            // to a literal match, so a command added in it would be scanned by
            // nothing at all. The same evasion was mutation-proven against
            // `editor_commands`' sibling scan.
            for (offset, _) in src.match_indices("#[tauri::command") {
                let rest = &src[offset..];
                // `cargo fmt` closes every top-level item with a brace in column
                // 0 and indents everything inside one, so this is an exact body
                // boundary. An open-ended slice is the latent form of the bug
                // that let one command's guard satisfy another's assertion in
                // phase 4.
                let body = match rest.find("\n}\n") {
                    Some(i) => &rest[..i + 3],
                    None => rest,
                };
                let signature = &body[..body.find('{').unwrap_or(body.len())];
                let name = signature
                    .split("fn ")
                    .nth(1)
                    .and_then(|s| s.split('(').next())
                    .expect("a #[tauri::command] must declare a function");
                all.push(name);
                // Registered, or the frontend cannot call it at all — the half
                // of "this command exists" that a scan of this file alone
                // cannot see.
                assert!(
                    lib.contains(&format!("{module}::{name},")),
                    "{name} is not in lib.rs's generate_handler! list"
                );
                if !signature.contains("base: String") {
                    continue;
                }
                let guard = body.find("is_safe_base(&base)").unwrap_or_else(|| {
                    panic!(
                        "{name} accepts a base from the frontend but does not refuse \
                         an unsafe one before it becomes a path"
                    )
                });
                // Presence is not refusal: a guard whose body only logs passes a
                // `contains` check while validating nothing. Bounded by the
                // four-space dedent `cargo fmt` guarantees for a block at
                // function-body level, because every one of these logs the
                // refused base with a `{base:?}` interpolation and so contains
                // its own `}` first.
                let end = body[guard..]
                    .find("\n    }")
                    .map(|i| guard + i)
                    .unwrap_or(body.len());
                assert!(
                    body[guard..end].contains("return Err("),
                    "{name} checks is_safe_base but does not REFUSE an unsafe base"
                );
                checked.push(name);
            }
        }
        // EVERY command, not only the base-taking ones: the evasion that
        // matters is a new command taking the untrusted name under some
        // other parameter name (`capture: String`), which the loop above
        // skips and a base-takers-only list cannot see. A discard written in
        // that shape is an arbitrary DELETE.
        assert_eq!(
            all,
            [
                "export_and_save_capture",
                "cancel_export",
                "discard_staged_capture",
                "list_staged_captures",
                "open_screen_capture",
            ],
            "this surface's command set changed; confirm whether the new command \
             turns frontend text into a path, then update this list"
        );
        assert_eq!(
            checked,
            ["export_and_save_capture", "discard_staged_capture"],
            "the set of commands taking a base changed; confirm the new one \
             guards it, then update this list"
        );
    }
}
