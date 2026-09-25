//! The staged-capture surface: the captures waiting in staging to be
//! edited or discarded, and the Obsidian hand-off for one that has been
//! published into a vault.
//!
//! A staged capture as an OBJECT. Until Task 59 its sibling was
//! `export_commands`, the phase-5 export's lifecycle; that export is
//! retired ("save unchanged" is now the tutorial editor's Render + Publish),
//! and the one event this surface sends, `screen:discarded`, moved here
//! with its single warning emitter (`emit_discarded`), which a structural
//! test pins at exactly one call site.
//!
//! **`discard_staged_capture` is destructive**, and the second destructive
//! command in the app after `delete_task`. It takes an untrusted name from
//! the frontend and turns it into paths, so it is gated by the shared
//! `editor_commands::is_safe_base` (never a second copy of those rules), it
//! refuses a capture a tutorial project has PINNED (R6), and it refuses a
//! symlink leaf rather than deleting through one.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use vault_buddy_core::timeline::Timeline;
use vault_buddy_core::uri;
use vault_buddy_screen::{staging, staging_files};

use crate::editor::project_store::pinned_project;
use crate::editor::EditorState;

/// `screen:discarded`, and SAY SO when the send fails.
///
/// Not in the spec, and added deliberately. Without it a discarded capture
/// leaves the panel store's `lastStaged` pointing at a base that is no
/// longer on disk, so the capture bar keeps offering **Edit** on it and the
/// editor opens on a sidecar that is gone. The single `app.emit` call site
/// in this module (a structural test pins that): AGENTS.md's diagnostics
/// invariant forbids a swallowed error, and a discarded emit result is the
/// most invisible kind there is. `staging_commands`' bulk clear emits
/// through here too.
pub(crate) fn emit_discarded(app: &AppHandle, base: &str) {
    if let Err(e) = app.emit("screen:discarded", serde_json::json!({ "base": base })) {
        log::warn!("screen discard: could not emit screen:discarded: {e}");
    }
}

/// The sidecar's hand-editable `timeline` field as a real timeline — a
/// capture that predates the editor has no field at all, which is the
/// WHOLE capture. `core::timeline::Timeline::from_sidecar_value` is the ONE
/// place the field's value is interpreted (the tutorial editor's migration
/// reads it through the same function); this is only the absent-field arm.
fn timeline_from_sidecar(value: Option<serde_json::Value>, source_duration_ms: u64) -> Timeline {
    match value {
        Some(v) => Timeline::from_sidecar_value(&v, source_duration_ms),
        None => Timeline::whole(source_duration_ms),
    }
}

/// One resume-or-discard row (spec §10) — a staged capture as the UI sees it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedCaptureSummaryDto {
    pub base: String,
    pub vault_id: String,
    pub source_title: String,
    /// What was RECORDED.
    pub duration_ms: u64,
    /// What the phase-4 editor's saved cut would PRODUCE — shorter whenever
    /// it cut something out. A capture edited before Task 59 keeps its
    /// sidecar timeline, and the tutorial editor migrates exactly that cut.
    pub output_duration_ms: u64,
    pub recorded_at: String,
    pub width: u32,
    pub height: u32,
    pub edited: bool,
    /// Rebuilt by `screen_recovery` after an interrupted session, so it knows
    /// neither its vault nor its duration, and the tutorial editor refuses
    /// to open it (`editor_open_staged`, F7). The sweep writes the marker
    /// into the sidecar's flattened catch-all; surfacing it is what stops
    /// this list offering an Edit that provably cannot succeed.
    pub recovered: bool,
    /// The tutorial project this capture is PINNED to (R6), if any —
    /// `crate::editor::project_store::pinned_project`. `Some` means a
    /// tutorial project has adopted this staged capture by reference: it
    /// must not be discarded (`discard_conflict`) until that project is
    /// discarded first, and `StagedCaptureList` labels the row instead of
    /// offering Discard.
    pub project_id: Option<String>,
}

/// Is this sidecar one `screen_recovery` rebuilt? A `true` BOOLEAN, never a
/// truthy anything: the sidecar is hand-editable, and `"recovered": "no"`
/// must not read as a claim in either direction.
pub(crate) fn summary_is_recovered(extra: &serde_json::Map<String, serde_json::Value>) -> bool {
    extra.get("recovered") == Some(&serde_json::Value::Bool(true))
}

/// Does this staged capture carry a phase-4 edit? `Timeline::is_untouched`,
/// the SAME predicate the tutorial editor's migration keys on, surfaced so
/// the resume-or-discard list can say so.
///
/// Deriving it from "the sidecar HAS a timeline field" would mark every
/// capture the phase-4 editor was ever OPENED on as edited: it wrote a
/// timeline on every operation and never wrote null (the rule `6944ac0`
/// established).
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
    // pinned by `tests/fixtures/timeline-cases.json` (docs/Gaps.md GAP-136),
    // and it is answering its own question correctly: an empty timeline
    // really is not the whole of anything. The divergence is unreachable
    // there — `editor_open_staged` refuses a recovered capture before its
    // timeline is ever migrated (F7) — so this stays a property of the
    // SUMMARY.
    if source_duration_ms == 0 {
        return false;
    }
    !timeline_from_sidecar(timeline, source_duration_ms).is_untouched(source_duration_ms)
}

/// How long the saved cut is, so the list can show it beside the
/// recording's own length.
pub(crate) fn summary_output_duration_ms(
    timeline: Option<serde_json::Value>,
    source_duration_ms: u64,
) -> u64 {
    timeline_from_sidecar(timeline, source_duration_ms).output_duration_ms()
}

/// Why this discard must be refused, or `None`.
///
/// `pinned` is `crate::editor::project_store::pinned_project`'s answer for
/// this capture (R6): a tutorial project has adopted this exact staged
/// capture by reference, so deleting it out from under the project would
/// orphan the project's own source record. That is the ONLY refusal left:
/// until Task 59 a capture being EXPORTED was refused too (F29), and the
/// export is retired. The editor's render and publish jobs never read a
/// staged capture that is not pinned, so they need no arm here.
pub(crate) fn discard_conflict(pinned: Option<&str>) -> Option<String> {
    if pinned.is_some() {
        return Some(
            "This capture is used by a tutorial project. Discard the project first.".to_string(),
        );
    }
    None
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

pub(crate) fn staging_dir_for(app: &AppHandle) -> Result<PathBuf, String> {
    let local = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Could not resolve the staging directory: {e}"))?;
    Ok(staging::staging_dir(&local))
}

/// Forget a staged capture, on disk: every file
/// `staging_files::capture_file_names` says it owns, the synchronized
/// webcam file (F-22) and the stems its sidecar lists (Task 53, F24)
/// included — the list is read FIRST, since the sidecar is one of the files
/// removed.
///
/// **The export `.part` is the third file and the one a two-file discard
/// forgets.** Nothing writes one since Task 59 retired the phase-5 export,
/// so any `.part` bearing this base is abandoned by definition — left by an
/// older build — and often the LARGEST file of the set. Leaving it strands
/// a file keyed to a capture the user just told us to forget, until some
/// later session's `run_screen_recovery` sweep happens to reach it.
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
    let stems = staging::listed_stems(dir, base);
    for name in staging_files::capture_file_names(base, &stems) {
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
        project_id: pinned_project(s),
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

/// Forget a staged capture — its video, its sidecar, its webcam file (F-22)
/// and any abandoned export temp. Irreversible; the staged list
/// confirm-gates it (spec §10).
///
/// ASYNC: up to four unlinks on a volume that may be slow or networked.
#[tauri::command]
pub async fn discard_staged_capture(app: AppHandle, base: String) -> Result<(), String> {
    if !crate::editor_commands::is_safe_base(&base) {
        log::warn!("discard_staged_capture: refused a base outside staging: {base:?}");
        return Err("That capture name is not one of ours.".to_string());
    }
    let dir = staging_dir_for(&app)?;
    let target = base.clone();
    let editor = app.clone();
    // The pin check reads the sidecar, so it rides the same spawn_blocking
    // as the discard itself — a sync command must not touch disk on the
    // async runtime thread, and this is one more small file read joining
    // the unlinks that already needed the blocking pool.
    tauri::async_runtime::spawn_blocking(move || {
        discard_unpinned(&editor.state::<EditorState>().open, &dir, &target)
    })
    .await
    .map_err(|e| format!("That capture could not be discarded: {e}"))??;
    log::info!("screen discard: forgot the staged capture {base}");
    emit_discarded(&app, &base);
    Ok(())
}

/// Discard `base` unless a tutorial project pins it — the body of
/// `discard_staged_capture`, apart from Tauri glue.
///
/// **Under `EditorState::open`** (final review I2), the lock
/// `editor_open_staged` holds across read-sidecar -> create the project ->
/// pin: without it a discard landing between the create and the pin read
/// "unpinned" and deleted the only source of the project that open then
/// registered. The pin is read INSIDE the lock, so it is the one the open
/// left. `open` is the outermost editor lock; nothing else is held here.
pub(crate) fn discard_unpinned(open: &Mutex<()>, dir: &Path, base: &str) -> Result<(), String> {
    let _open = vault_buddy_core::sync_util::lock_ignoring_poison(open);
    if let Some(message) = discard_conflict(pinned_project_of(dir, base).as_deref()) {
        return Err(message);
    }
    discard_staged_files(dir, base)
}

/// The tutorial-project id pinning this staged capture, or `None` — both
/// when the sidecar carries no pin and when it cannot be read at all, the
/// same defensive-read posture as everywhere else a sidecar is consulted.
pub(crate) fn pinned_project_of(dir: &Path, base: &str) -> Option<String> {
    let sidecar = staging::read_sidecar(&dir.join(staging::sidecar_file_name(base)))?;
    pinned_project(&sidecar)
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

/// Open a PUBLISHED capture — its video or its note — in Obsidian (the
/// Publish dialog's Open).
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
#[path = "staged_commands_tests.rs"]
mod tests;
