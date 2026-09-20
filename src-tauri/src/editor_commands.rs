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

use tauri::{AppHandle, Emitter, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

const EDITOR_LABEL: &str = "editor";

/// Emitted to the editor window ONLY (`emit_to`, never `app.emit`) — see the
/// module doc for why draining the stash on mount alone is not enough.
const EDITOR_OPEN_EVENT: &str = "editor:open";

/// The base name the editor window should open when it next mounts (or, via
/// `EDITOR_OPEN_EVENT`, the next time it is asked to check again).
/// Rust-owned because the panel and the editor are separate webviews with
/// separate stores; a one-shot slot, drained by `take_editor_request`.
#[derive(Default)]
pub struct EditorRequest(pub Mutex<Option<String>>);

/// What the editor renders. `asset_path` is the file name RELATIVE to the
/// staging directory — the webview joins it onto the asset origin itself.
/// Deliberately not an absolute disk path: the asset protocol's scope is the
/// only thing that lets the webview read the file, and handing it a raw path
/// would invite a future caller to widen that scope.
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
///   name actually on disk (`staging::sanitize_title`'s own trim exists for
///   the same reason), so accepting one here would look up a path that can
///   never exist rather than the one that does;
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
fn is_safe_base(base: &str) -> bool {
    !base.is_empty()
        && !base.starts_with('.')
        && !base.ends_with('.')
        && !base.ends_with(' ')
        && !base.contains('/')
        && !base.contains('\\')
        && !base.contains(':')
        && !staging::is_reserved_device_stem(base)
        && base.chars().all(|c| !c.is_control())
}

fn detail_from_sidecar(s: &staging::StagedSidecar, asset_path: &str) -> StagedCaptureDetail {
    StagedCaptureDetail {
        base: s.base.clone(),
        asset_path: asset_path.to_string(),
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
/// Tauri app, and pins the "`asset_path` is a bare NAME" guarantee at the
/// one call site that could actually break it (T-4): `detail_from_sidecar`
/// takes `asset_path` as a separate argument precisely so nothing upstream
/// of it can slip in an absolute disk path instead.
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
    let mp4 = staging::mp4_file_name(requested_base);
    if !dir.join(&mp4).is_file() {
        return Err("That capture's video file is missing.".to_string());
    }
    Ok(detail_from_sidecar(&sidecar, &mp4))
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
    // M-2: the stash is claimed only from here on — a failure below must
    // leave NOTHING behind for a later mount/reopen to read, or a later
    // drain would silently open a capture the user was just told failed to
    // open. Both failure arms below roll it back for exactly that reason.
    *lock_ignoring_poison(&app.state::<EditorRequest>().0) = Some(base.clone());
    // Emitted BEFORE `show()`, the `region:begin` shape: the editor's own
    // listener is installed once at mount (the webview loads regardless of
    // `visible: false`, same as panel/bubble/overlay), so by the time this
    // command can ever run it already exists — this event is what makes an
    // ALREADY-mounted editor re-drain the stash instead of doing nothing
    // (see the module doc's "why the stash alone is not enough").
    if let Err(e) = app.emit_to(EDITOR_LABEL, EDITOR_OPEN_EVENT, ()) {
        *lock_ignoring_poison(&app.state::<EditorRequest>().0) = None;
        log::warn!("open_capture_editor: could not signal the editor window: {e}");
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

/// SYNC, one-shot: the editor webview drains this on mount, and again on
/// every `EDITOR_OPEN_EVENT`. Returns `None` when nothing is staged, which
/// is not an error — a user can alt-tab back to an editor that is already
/// showing a capture, and `EDITOR_OPEN_EVENT` is only emitted when
/// `open_capture_editor` actually staged something.
#[tauri::command]
pub fn take_editor_request(app: AppHandle) -> Option<String> {
    lock_ignoring_poison(&app.state::<EditorRequest>().0).take()
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

#[cfg(test)]
mod tests {
    use super::*;
    use vault_buddy_screen::staging::StagedSidecar;

    fn sidecar(base: &str) -> StagedSidecar {
        StagedSidecar {
            base: base.to_string(),
            vault_id: "v1".into(),
            source_title: "Screen 1".into(),
            source_kind: "screen".into(),
            inputs: vec!["Mic".into()],
            duration_ms: 42_000,
            paused_ms: 0,
            width: 1920,
            height: 1080,
            recorded_at: "2026-09-20T10:00:00Z".into(),
            timeline: None,
        }
    }

    // A base name travels from the frontend, so it is untrusted input that
    // becomes a PATH. `..` or a separator in it would read a file outside
    // the staging directory — the `sanitize_title` lesson from phase 2,
    // applied at the other end of the same pipe.
    #[test]
    fn a_base_that_could_escape_staging_is_refused() {
        assert!(is_safe_base("2026-09-20 1432 Figma walkthrough"));
        assert!(!is_safe_base("../../../etc/passwd"));
        assert!(!is_safe_base("a/b"));
        assert!(!is_safe_base("a\\b"));
        assert!(!is_safe_base(".."));
        assert!(!is_safe_base(""));
        // A leading dot is how our own in-progress `.part` files are named;
        // the editor must never be pointed at one.
        assert!(!is_safe_base(".hidden"));
        // An interior ".." is NOT rejected on its own (see `is_safe_base`'s
        // doc comment) — these three fixtures are the proof that dropping a
        // `contains("..")` check loses nothing: each is still refused, by a
        // DIFFERENT clause. ".." alone is caught by the leading-dot check,
        // "../../../etc/passwd" above by the separator check, and "a.." by
        // the trailing-dot check.
        assert!(!is_safe_base("a.."));
    }

    // I-1: `sanitize_title` maps the reserved CHARACTERS but never touches
    // `.` (GAP-108's sibling gap), so a window titled with an ellipsis --
    // "Loading...", "Saving...", "Installing... — Setup" — produces a base
    // with interior dots. Before this fix `is_safe_base` refused every one
    // of these forever: the capture staged correctly and could then never
    // be opened, exported or discarded again. This is the test that stops
    // someone re-adding a `contains("..")` check.
    #[test]
    fn a_base_with_interior_dots_from_an_ellipsis_title_is_accepted() {
        assert!(is_safe_base(
            "2026-09-20 1432 Saving... please wait - Figma"
        ));
    }

    // Windows silently strips a trailing dot or space from a file name, so a
    // base carrying either would look up a file that was never actually
    // written under that exact name (the `sanitize_title` trim exists for
    // the same reason, at capture time rather than open time).
    #[test]
    fn a_base_with_a_trailing_dot_or_space_is_refused() {
        assert!(!is_safe_base("cap."));
        assert!(!is_safe_base("cap "));
    }

    // I-2: on Windows, `PathBuf::join`/`push` REPLACES `self` when the
    // argument carries a prefix but no root (std's own doc), so
    // `staging_dir.join("C:Windows.json")` is `C:Windows.json` --
    // drive-C-relative, entirely outside staging. `cap:ads` is the NTFS
    // alternate-data-stream form of the same hole: it addresses the
    // `ads.json` STREAM of the file `cap`, not a file literally named
    // `cap:ads.json`. `sanitize_title` already maps `:` to `-`, so no base
    // this app itself produces can ever carry one.
    #[test]
    fn a_windows_drive_relative_or_alternate_stream_base_is_refused() {
        assert!(!is_safe_base("C:"));
        assert!(!is_safe_base("C:Windows"));
        assert!(!is_safe_base("cap:ads"));
    }

    // I-3 / GAP-108's other end: Windows resolves a path component whose
    // STEM matches a reserved device name to the PHYSICAL device, regardless
    // of directory or extension -- `dir.join("COM1.json")` is the COM1
    // serial port, and `std::fs::read` on an open port blocks forever,
    // hanging `load_staged_capture`'s `spawn_blocking` closure (and leaking
    // the thread) instead of erroring. The match is case-insensitive and
    // applies even behind a real-looking extension.
    #[test]
    fn a_reserved_device_name_is_refused() {
        assert!(!is_safe_base("CON"));
        assert!(!is_safe_base("con"));
        assert!(!is_safe_base("NUL"));
        assert!(!is_safe_base("COM1"));
        assert!(!is_safe_base("LPT1"));
        assert!(!is_safe_base("com1.foo"));
    }

    // T-1: no existing fixture carried a control character, so the
    // `is_control` clause named in the doc comment was pinned by nothing.
    #[test]
    fn a_base_with_a_control_character_is_refused() {
        assert!(!is_safe_base("cap\tx"));
        assert!(!is_safe_base("\u{0}"));
    }

    // The detail the editor renders is derived, not echoed: the webview gets
    // an ASSET url it can actually load, never a raw disk path, because the
    // asset protocol is the only way it can read the file at all.
    #[test]
    fn the_detail_carries_an_asset_url_not_a_disk_path() {
        let d = detail_from_sidecar(&sidecar("cap one"), "cap one.mp4");
        assert_eq!(d.base, "cap one");
        assert_eq!(d.asset_path, "cap one.mp4");
        assert_eq!(d.duration_ms, 42_000);
        assert_eq!(d.width, 1920);
        assert_eq!(d.height, 1080);
        assert_eq!(d.source_title, "Screen 1");
        // T-3: `recordedAt` is part of the wire contract the brief listed
        // and was mapped but never asserted.
        assert_eq!(d.recorded_at, "2026-09-20T10:00:00Z");
        assert!(
            d.timeline.is_none(),
            "an untouched capture has no timeline yet"
        );
    }

    // A sidecar written by a previous editing session must come back as a
    // timeline, or every crash would silently discard the edit it promised
    // to preserve.
    #[test]
    fn a_saved_timeline_round_trips_into_the_detail() {
        let mut s = sidecar("cap");
        s.timeline = Some(serde_json::json!({
            "segments": [{"sourceStartMs": 0, "sourceEndMs": 1000}]
        }));
        let d = detail_from_sidecar(&s, "cap.mp4");
        let t = d
            .timeline
            .expect("a saved timeline must survive the round trip");
        assert_eq!(t["segments"][0]["sourceEndMs"], 1000);
    }

    // T-4: the "assetPath is a NAME, not a disk path" guarantee is pinned
    // above only INSIDE `detail_from_sidecar`; this pins it at the CALL SITE
    // where it could actually break -- a mutation swapping the call site's
    // argument for `dir.join(&mp4).to_string_lossy()` compiled and left
    // every other test in this module green.
    #[test]
    fn load_from_staging_dir_returns_a_relative_asset_name_not_a_disk_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = "cap one";
        staging::write_sidecar(dir.path(), &sidecar(base)).expect("write sidecar");
        std::fs::write(dir.path().join(staging::mp4_file_name(base)), b"x").expect("write mp4");

        let detail = load_from_staging_dir(dir.path(), base).expect("load");
        assert_eq!(detail.asset_path, "cap one.mp4");
        assert!(
            !Path::new(&detail.asset_path).is_absolute(),
            "assetPath must be a bare name the webview joins onto the asset origin itself"
        );
    }

    // M-1: `read_sidecar` exists precisely because a sidecar can be
    // hand-edited (its own doc says so), so `sidecar.base` is untrusted. A
    // hand-edited base that disagrees with the file it lives in must not
    // become the identity `load_staged_capture` hands back -- that identity
    // is what the editor later hands to save and discard.
    #[test]
    fn a_sidecar_whose_base_disagrees_with_its_file_name_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = sidecar("cap");
        s.base = "../../evil".to_string();
        let json = serde_json::to_vec_pretty(&s).expect("serialize sidecar");
        std::fs::write(dir.path().join("cap.json"), json).expect("write sidecar");
        std::fs::write(dir.path().join("cap.mp4"), b"x").expect("write mp4");

        assert!(load_from_staging_dir(dir.path(), "cap").is_err());
    }

    // T-2: `is_safe_base` is well tested as a function above, but its
    // APPLICATION was not -- removing the guard from both command bodies
    // left the whole shell test suite green, because `AppHandle` makes a
    // unit test of the commands themselves impossible. The
    // `capture_exclusion`/`clear_active_screen` precedent: scan this file's
    // own production source and fail if either command body stops calling
    // `is_safe_base`.
    #[test]
    fn both_commands_guard_the_base_with_is_safe_base() {
        let src = include_str!("editor_commands.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);

        let open_fn = production
            .find("pub fn open_capture_editor(")
            .expect("open_capture_editor must exist in production source");
        let take_fn = production
            .find("pub fn take_editor_request(")
            .expect("take_editor_request must exist in production source");
        let load_fn = production
            .find("pub async fn load_staged_capture(")
            .expect("load_staged_capture must exist in production source");
        assert!(
            open_fn < take_fn && take_fn < load_fn,
            "this scan assumes the commands are declared in this order; update the offsets \
             if they are reordered"
        );

        let guard = "is_safe_base(&base)";
        assert!(
            production[open_fn..take_fn].contains(guard),
            "open_capture_editor must refuse an unsafe base before stashing it"
        );
        assert!(
            production[load_fn..].contains(guard),
            "load_staged_capture must refuse an unsafe base before it becomes a path"
        );
    }
}
