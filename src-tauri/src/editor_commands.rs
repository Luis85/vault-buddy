//! The capture editor's IPC surface (spec 5.1, 8, 11).
//!
//! **Why opening the editor takes two commands.** Spec 11 lists
//! `open_capture_editor` as SYNC, because it shows and focuses a window and
//! the window APIs are main-thread-only. But the editor also needs its
//! capture's sidecar, which is disk I/O a sync command must never do. So the
//! sync command shows the window and stashes the base name, and the editor
//! webview drains that stash and fetches its own data asynchronously once it
//! has mounted — the same split `document_commands::begin_document_import`
//! and `take_pending_import` already use, for the same reason: the target
//! window has its own Pinia store and cannot be handed state directly.
//!
//! **Why the stash alone is not enough.** `window_close.rs` answers the
//! editor's own close with `prevent_close()` + `hide()` — the window is
//! never destroyed, so its webview mounts exactly ONCE per process. Draining
//! the stash only from `onMounted` (the naive reading of the
//! `begin_document_import` precedent) works the first time and never again:
//! open capture A (mounts, drains "A"), close the editor, open capture B —
//! `open_capture_editor` stashes "B" and shows the window, but nothing in
//! the already-mounted webview re-reads the stash, so the editor comes back
//! up still showing A while "B" sits there forever. The precedent's OTHER
//! half is what actually closes this: `begin_document_import` calls
//! `commands::show_panel`, which emits `panel-shown`, and `PanelRoot` drains
//! `take_pending_import` on EVERY one of those events, not just the first.
//! `open_capture_editor` therefore emits `EDITOR_OPEN_EVENT` to the editor
//! window alone (the `region:begin` shape — one window, never `app.emit`),
//! immediately before `show()`, so a listener installed once at mount (like
//! `RegionRoot`'s for `region:begin`) re-fires the drain on every open,
//! including one that lands on an editor already showing a different
//! capture. Task 6, which writes `EditorRoot`, must subscribe to it.

use std::path::Path;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

use crate::editor::store_io::{self, ProjectSummaryDto};

const EDITOR_LABEL: &str = "editor";

/// Emitted to the editor window ONLY (`emit_to`, never `app.emit`) — see the
/// module doc for why draining the stash on mount alone is not enough.
const EDITOR_OPEN_EVENT: &str = "editor:open";

/// What `take_editor_request` hands the editor window: either a staged
/// capture's base (`open_capture_editor`) or a tutorial project's id
/// (`open_project_editor`, Task 37 Part B, F4). Serialized as
/// `{"kind": "staged"|"project", "value": "..."}` — an adjacently tagged
/// enum, `#[serde(tag = "kind", content = "value")]` — so `EditorRoot.vue`
/// can branch on `kind` without guessing which shape a bare string used to
/// mean.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum EditorRequestKind {
    Staged(String),
    Project(String),
}

/// What the editor window should open when it next mounts (or, via
/// `EDITOR_OPEN_EVENT`, the next time it is asked to check again).
/// Rust-owned because the panel and the editor are separate webviews with
/// separate stores; a one-shot slot, drained by `take_editor_request`.
#[derive(Default)]
pub struct EditorRequest(pub Mutex<Option<EditorRequestKind>>);

/// What the editor renders. `asset_path` is the staged `.mp4`'s own ABSOLUTE
/// path, which the webview hands to `convertFileSrc` to get a URL the asset
/// protocol can serve.
///
/// It carried the bare file name until P-5: `convertFileSrc` does no joining
/// — it percent-encodes its argument onto the asset origin — so a bare name
/// produced a URL that resolved to no file on disk, matched no scope entry,
/// and left the preview permanently blank. The opacity of the string was
/// never the security boundary; the `assetProtocol.scope` in
/// `tauri.conf.json` (`$APPLOCALDATA/screen-captures/*`) is, and it is
/// enforced by Tauri on every request regardless of what this field says.
/// Widening it is a `tauri.conf.json` edit, which no caller of this DTO can
/// make. The path itself is not attacker-chosen either: it is
/// `staging_dir(app_local_data_dir)` joined with an `is_safe_base`-validated
/// base, and `load_from_staging_dir` has already confirmed the file is there.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedCaptureDetail {
    pub base: String,
    pub asset_path: String,
    pub duration_ms: u64,
    pub source_title: String,
    pub width: u32,
    pub height: u32,
    pub recorded_at: String,
    pub timeline: Option<serde_json::Value>,
}

/// Is this base name safe to turn into a path inside the staging directory?
///
/// The base travels from the frontend, so it is untrusted input that becomes
/// a path. Every clause here is either a separator (no path component can
/// contain one) or a case Windows treats specially:
///
/// - a leading dot is how our own in-progress `.mp4.part` files are named,
///   and must never be opened as a finished capture;
/// - a trailing dot or space is silently STRIPPED by Windows from a file
///   name actually on disk (`staging_title::sanitize_title`'s own trim
///   exists for the same reason), so accepting one here would look up a path
///   that can never exist rather than the one that does;
/// - a `:` is a drive prefix (`"C:"`, `"C:Windows"`) or an NTFS
///   alternate-data-stream marker (`"cap:ads"`) — both resolve OUTSIDE the
///   staging directory entirely, because `PathBuf::join`/`push` REPLACES
///   `self` when its argument carries a prefix but no root (std's own doc
///   for `push`), so `staging_dir.join("C:Windows.json")` is
///   `C:Windows.json`, drive-C-relative. `sanitize_title` already maps `:`
///   to `-`, so no base this app itself produces can ever carry one;
/// - a reserved device STEM (`CON`, `COM1`, `NUL`, …) resolves to the
///   PHYSICAL device regardless of directory or extension —
///   `dir.join("COM1.json")` is the COM1 serial port, and `std::fs::read`
///   on an open port blocks forever, hanging `load_staged_capture`'s
///   `spawn_blocking` closure and leaking the thread instead of erroring
///   (GAP-108's hole at this end of the same pipe; `is_reserved_device_stem`
///   is shared with `staging` so both ends can draw from one list).
///
/// An interior `..` is deliberately NOT rejected. Within a single path
/// component `..` cannot traverse anywhere: `dir.join("a..b.json")` is a
/// plain file named that, in `dir`. Every shape that IS dangerous already
/// needs a separator (caught above) or an edge dot (caught above) —
/// `".."` alone is caught by the leading-dot check and `"a.."` by the
/// trailing-dot check. Rejecting interior dots bought nothing but
/// availability pain: `sanitize_title` maps the reserved CHARACTERS but
/// never touches `.`, so a window titled "Saving... please wait" produces a
/// base with an interior `...` — a real capture that a `contains("..")`
/// check would have refused forever, staged correctly and then never
/// openable, exportable or discardable again.
pub(crate) fn is_safe_base(base: &str) -> bool {
    unsafe_base_reason(base).is_none()
}

/// Why `base` is unsafe (see `is_safe_base`), as a category a log line can
/// carry instead of the base itself — which holds a recorded window's title
/// (Task 58, F-50). `None` is a safe base.
pub(crate) fn unsafe_base_reason(base: &str) -> Option<&'static str> {
    if base.is_empty() {
        Some("empty")
    } else if base.starts_with('.') || base.ends_with('.') || base.ends_with(' ') {
        Some("leading or trailing dot or space")
    } else if base.contains('/') || base.contains('\\') {
        Some("path separator")
    } else if base.contains(':') {
        Some("drive or stream colon")
    } else if staging::is_reserved_device_stem(base) {
        Some("reserved device name")
    } else if base.chars().any(char::is_control) {
        Some("control character")
    } else {
        None
    }
}

fn detail_from_sidecar(s: &staging::StagedSidecar, asset_path: &Path) -> StagedCaptureDetail {
    StagedCaptureDetail {
        base: s.base.clone(),
        // Lossy because JSON carries UTF-8 and a Windows path is UTF-16: a
        // path that does not round-trip would be unopenable, but it is also
        // one this app could never have written — `sanitize_title` produces
        // the base and `app_local_data_dir` produces the root.
        asset_path: asset_path.to_string_lossy().into_owned(),
        duration_ms: s.duration_ms,
        source_title: s.source_title.clone(),
        width: s.width,
        height: s.height,
        recorded_at: s.recorded_at.clone(),
        timeline: s.timeline.clone(),
    }
}

/// The disk read behind `load_staged_capture`, pulled out of its
/// `spawn_blocking` closure so it takes a plain `&Path` rather than deriving
/// one from `AppHandle` — which makes it unit-testable without a running
/// Tauri app, and pins the "`asset_path` names a file INSIDE this staging
/// directory" guarantee at the one call site that could actually break it
/// (T-4): `detail_from_sidecar` takes `asset_path` as a separate argument
/// precisely so nothing upstream of it can slip in a path from somewhere
/// else.
fn load_from_staging_dir(dir: &Path, requested_base: &str) -> Result<StagedCaptureDetail, String> {
    let sidecar_path = dir.join(staging::sidecar_file_name(requested_base));
    let sidecar = staging::read_sidecar(&sidecar_path)
        .ok_or_else(|| "That capture's details could not be read.".to_string())?;
    if sidecar.base != requested_base {
        // M-1: `read_sidecar` exists precisely because a sidecar can be
        // hand-edited (its own doc says so), so `sidecar.base` is untrusted.
        // A hand-edited `"base": "../../evil"` would otherwise ride through
        // validation on the REQUESTED name and come out the other side as
        // the identity the editor holds and later hands to save/discard.
        log::warn!(
            "load_staged_capture: sidecar at {} carries base {:?}, which does not match the \
             requested base {:?}; refusing",
            sidecar_path.display(),
            sidecar.base,
            requested_base
        );
        return Err("That capture's details do not match its file name.".to_string());
    }
    let mp4 = dir.join(staging::mp4_file_name(requested_base));
    if !mp4.is_file() {
        return Err("That capture's video file is missing.".to_string());
    }
    Ok(detail_from_sidecar(&sidecar, &mp4))
}

/// The stash-then-emit-then-show sequence `open_capture_editor` and
/// `open_project_editor` share verbatim (Task 37 Part B, fix round 1):
/// write `request` into the stash, emit `EDITOR_OPEN_EVENT` to the editor
/// window alone (rolling the stash back on a failed emit — M-2's "leave
/// NOTHING behind for a later drain to read"), `show()` it (rolling the
/// stash back on a failed show too), then `unminimize()`/`set_focus()`.
/// `caller` names the command in the one log line each failure path can
/// still produce, so the two commands keep distinguishable diagnostics
/// despite sharing every line of logic. Each command's own PRE-check
/// (`open_capture_editor`'s `is_safe_base`; `open_project_editor` has none,
/// see its own doc) stays at its own call site, before this runs — this
/// function is only the part that was byte-for-byte identical between them.
fn stash_and_open(
    app: &AppHandle,
    window: &WebviewWindow,
    caller: &str,
    request: EditorRequestKind,
) -> Result<(), String> {
    *lock_ignoring_poison(&app.state::<EditorRequest>().0) = Some(request);
    // Emitted BEFORE `show()`, the `region:begin` shape: the editor's own
    // listener is installed once at mount (the webview loads regardless of
    // `visible: false`, same as panel/bubble/overlay), so by the time this
    // command can ever run it already exists — this event is what makes an
    // ALREADY-mounted editor re-drain the stash instead of doing nothing
    // (see the module doc's "why the stash alone is not enough").
    if let Err(e) = app.emit_to(EDITOR_LABEL, EDITOR_OPEN_EVENT, ()) {
        *lock_ignoring_poison(&app.state::<EditorRequest>().0) = None;
        log::warn!("{caller}: could not signal the editor window: {e}");
        return Err(format!("Could not signal the editor: {e}"));
    }
    if let Err(e) = window.show() {
        *lock_ignoring_poison(&app.state::<EditorRequest>().0) = None;
        return Err(format!("Could not open the editor: {e}"));
    }
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

/// SYNC: shows and focuses the editor window and stashes which capture it
/// should open. Window APIs are main-thread-only and this command touches no
/// disk, so sync is correct — see the module doc for why the data arrives
/// separately.
#[tauri::command]
pub fn open_capture_editor(app: AppHandle, base: String) -> Result<(), String> {
    if !is_safe_base(&base) {
        log::warn!("open_capture_editor: refused a base outside staging: {base:?}");
        return Err("That capture name is not one of ours.".to_string());
    }
    let window = app
        .get_webview_window(EDITOR_LABEL)
        .ok_or_else(|| "The editor window is missing.".to_string())?;
    stash_and_open(
        &app,
        &window,
        "open_capture_editor",
        EditorRequestKind::Staged(base),
    )
}

/// SYNC: `open_capture_editor`'s own shape (Task 37 Part B, F4), applied to a
/// tutorial project id instead of a staged capture's base — the shared
/// `stash_and_open` sequence, below. Deliberately PANEL-callable rather than
/// one of the editor window's own session commands: it lives here, beside
/// `open_capture_editor` and OUTSIDE `src/editor/`, so neither Task 10's
/// `authz_guard` structural scan (which walks only that directory) nor Task
/// 11's editor-only capability manifest ever sees or constrains it — the
/// panel calls it under its existing `default.json` grant, never
/// `editor.json`'s. Opening a session over the project id this stashes is
/// `editor_open_project`'s job once the editor window drains the stash;
/// this command touches no session and no disk at all, exactly like its
/// sibling.
#[tauri::command]
pub fn open_project_editor(app: AppHandle, project_file_id: String) -> Result<(), String> {
    let window = app
        .get_webview_window(EDITOR_LABEL)
        .ok_or_else(|| "The editor window is missing.".to_string())?;
    stash_and_open(
        &app,
        &window,
        "open_project_editor",
        EditorRequestKind::Project(project_file_id),
    )
}

/// SYNC, one-shot: the editor webview drains this on mount, and again on
/// every `EDITOR_OPEN_EVENT`. Returns `None` when nothing is staged, which
/// is not an error — a user can alt-tab back to an editor that is already
/// showing a capture, and `EDITOR_OPEN_EVENT` is only emitted when
/// `open_capture_editor`/`open_project_editor` actually staged something.
#[tauri::command]
pub fn take_editor_request(app: AppHandle) -> Option<EditorRequestKind> {
    lock_ignoring_poison(&app.state::<EditorRequest>().0).take()
}

/// ASYNC: a panel-callable, READ-ONLY listing (Task 37 Part B, F4) —
/// delegates to the SAME `store_io::list_projects` helper
/// `editor::save_commands::editor_list_projects` uses, without opening or
/// touching any session. Unlike that command it takes no
/// `window: WebviewWindow` and calls no `require_editor_window`: it is not
/// one of the editor window's own session commands, so it lives here,
/// outside `src/editor/`, and the panel reaches it under `default.json`'s
/// grant. A `read_dir` plus one `project.json` parse per project, so this
/// must not sit on the main thread.
#[tauri::command]
pub async fn list_tutorial_projects(app: AppHandle) -> Result<Vec<ProjectSummaryDto>, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("Could not resolve the app data directory: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || store_io::list_projects(&root))
        .await
        .map_err(|e| format!("Listing tutorial projects failed: {e}"))
}

/// ASYNC: reads the sidecar off disk, so it must not sit on the main thread.
/// A thin `spawn_blocking` wrapper around `load_from_staging_dir` — see that
/// function's doc for why the disk work lives there instead of here.
#[tauri::command]
pub async fn load_staged_capture(
    app: AppHandle,
    base: String,
) -> Result<StagedCaptureDetail, String> {
    if !is_safe_base(&base) {
        log::warn!("load_staged_capture: refused a base outside staging: {base:?}");
        return Err("That capture name is not one of ours.".to_string());
    }
    let dir = staging::staging_dir(
        &app.path()
            .app_local_data_dir()
            .map_err(|e| format!("Could not resolve the staging directory: {e}"))?,
    );
    tauri::async_runtime::spawn_blocking(move || load_from_staging_dir(&dir, &base))
        .await
        .map_err(|e| format!("Loading the capture failed: {e}"))?
}

/// Return the sidecar with only its timeline replaced.
///
/// Read-modify-write, never rebuild. Everything else in the sidecar is the
/// capture's own recorded truth — its duration, its inputs, the vault it was
/// recorded for — and the editor does not know all of it. A save that
/// reconstructed the struct from what the editor carries would silently drop
/// whatever it does not.
fn with_timeline(
    mut sidecar: staging::StagedSidecar,
    timeline: Option<serde_json::Value>,
) -> staging::StagedSidecar {
    sidecar.timeline = timeline;
    sidecar
}

/// The disk write behind `save_capture_timeline`, pulled out of its
/// `spawn_blocking` closure for the same reason `load_from_staging_dir` is:
/// taking a plain `&Path` makes it unit-testable without a running Tauri
/// app, and it pins the one thing that could actually break here — WHICH
/// base names the file being written.
///
/// `requested_base` is the validated one from the command, and it is what
/// both the read and the write are addressed by. `existing.base` is a field
/// read off disk out of a file `read_sidecar`'s own doc says may be
/// hand-edited, so it is untrusted input and must never become a path;
/// `staging::write_sidecar` refuses the disagreement (C-1), which is the
/// same refusal `load_from_staging_dir` already makes on the read side.
fn save_to_staging_dir(
    dir: &Path,
    requested_base: &str,
    timeline: Option<serde_json::Value>,
) -> Result<(), String> {
    let sidecar_path = dir.join(staging::sidecar_file_name(requested_base));
    let existing = staging::read_sidecar(&sidecar_path)
        .ok_or_else(|| "That capture's details could not be read.".to_string())?;
    staging::write_sidecar(dir, requested_base, &with_timeline(existing, timeline))
        .map_err(|e| format!("Could not save the edit: {e}"))?;
    Ok(())
}

/// ASYNC: the sidecar rewrite is a temp + fsync + replacing rename
/// (`staging::write_sidecar`, over `capture_note::write_atomic_replacing`),
/// and that must not sit on the main thread — on every editor operation,
/// no less.
#[tauri::command]
pub async fn save_capture_timeline(
    app: AppHandle,
    base: String,
    timeline: Option<serde_json::Value>,
) -> Result<(), String> {
    if !is_safe_base(&base) {
        // M-1: both siblings log their refusal, and this is the one of the
        // three that WRITES -- the refusal you most want in the log when
        // reading a bug report.
        log::warn!("save_capture_timeline: refused a base outside staging: {base:?}");
        return Err("That capture name is not one of ours.".to_string());
    }
    let dir = staging::staging_dir(
        &app.path()
            .app_local_data_dir()
            .map_err(|e| format!("Could not resolve the staging directory: {e}"))?,
    );
    tauri::async_runtime::spawn_blocking(move || save_to_staging_dir(&dir, &base, timeline))
        .await
        .map_err(|e| format!("Saving the edit failed: {e}"))?
}

#[cfg(test)]
#[path = "editor_commands_tests.rs"]
mod tests;
