//! The editor window's panel-callable doors (spec 5.1, 11): opening it on a
//! staged capture or a tutorial project, and listing the projects.
//!
//! Through phase 4 this module also carried the capture editor's own
//! sidecar read and timeline write; Task 59 retired both with the phase-4
//! editor. A staged capture now opens through the tutorial editor's
//! session (`editor::session_commands::editor_open_staged`), which reads
//! the sidecar natively and never writes a timeline back into it.
//!
//! **Why opening the editor takes two commands.** Spec 11 lists
//! `open_capture_editor` as SYNC, because it shows and focuses a window and
//! the window APIs are main-thread-only. But opening a session over the
//! capture is disk I/O a sync command must never do. So the sync command
//! shows the window and stashes the base name, and the editor webview
//! drains that stash and opens its session asynchronously once it has
//! mounted — the same split `document_commands::begin_document_import`
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
//! capture. `EditorRoot` subscribes to it.

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
///   on an open port blocks forever, hanging whichever `spawn_blocking`
///   closure reads the sidecar and leaking the thread instead of erroring
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

#[cfg(test)]
#[path = "editor_commands_tests.rs"]
mod tests;
