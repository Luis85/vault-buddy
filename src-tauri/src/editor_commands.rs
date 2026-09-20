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
            extra: Default::default(),
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

    // The detail the editor renders is derived, not echoed: every field the
    // wire contract names is mapped from the sidecar, and `asset_path` is
    // the file's own path — what `convertFileSrc` needs to build a URL the
    // asset protocol can serve.
    #[test]
    fn the_detail_maps_every_sidecar_field_and_carries_the_staged_path() {
        let d = detail_from_sidecar(&sidecar("cap one"), Path::new("/staging/cap one.mp4"));
        assert_eq!(d.base, "cap one");
        assert_eq!(d.asset_path, "/staging/cap one.mp4");
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
        let d = detail_from_sidecar(&s, Path::new("/staging/cap.mp4"));
        let t = d
            .timeline
            .expect("a saved timeline must survive the round trip");
        assert_eq!(t["segments"][0]["sourceEndMs"], 1000);
    }

    // P-5: `assetPath` must be a path `convertFileSrc` can turn into a URL
    // the asset protocol resolves -- i.e. THE STAGED FILE'S OWN path. It
    // carried the bare file name, which `convertFileSrc` percent-encodes
    // onto the asset origin without joining anything, so the URL named no
    // file on disk, matched no scope entry, and the preview stayed blank.
    // Pinned at the CALL SITE, which is the only place it can break:
    // `detail_from_sidecar` takes the path as a separate argument.
    //
    // The two halves are asserted separately on purpose. `parent == dir`
    // alone would also hold for a `dir`-relative name on some platforms, and
    // `is_absolute` alone would hold for any absolute path anywhere on the
    // disk -- it is the pair that says "this exact file, inside staging".
    #[test]
    fn load_from_staging_dir_returns_the_staged_mp4s_own_absolute_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = "cap one";
        staging::write_sidecar(dir.path(), base, &sidecar(base)).expect("write sidecar");
        std::fs::write(dir.path().join(staging::mp4_file_name(base)), b"x").expect("write mp4");

        let detail = load_from_staging_dir(dir.path(), base).expect("load");
        let asset = Path::new(&detail.asset_path);
        assert!(
            asset.is_absolute(),
            "assetPath must be absolute: convertFileSrc joins nothing, so a bare name \
             resolves to no file and matches no scope entry"
        );
        assert_eq!(
            asset.parent(),
            Some(dir.path()),
            "assetPath must name a file inside the staging directory"
        );
        assert_eq!(
            asset.file_name(),
            Some(std::ffi::OsStr::new("cap one.mp4")),
            "assetPath must name THIS capture's mp4"
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

    // C-1: the save path READ with the validated request base but WROTE
    // with `sidecar.base`, a field read off disk out of a file
    // `read_sidecar`'s own doc says may be hand-edited. `staging`'s own
    // tests pin that a base escaping the directory is refused; what this
    // pins is the shell's contribution -- WHICH base this call site hands
    // to the writer. The fixture is deliberately an ordinary, perfectly
    // safe name: "other" trips no containment or `is_safe_base` clause, so
    // nothing but the requested-vs-stored mismatch can fail it, and the
    // failure it demonstrates survives containment entirely -- saving an
    // edit to "cap" would create and own a DIFFERENT capture's sidecar.
    #[test]
    fn a_save_never_writes_to_a_name_other_than_the_one_it_was_asked_for() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = sidecar("cap");
        s.base = "other".to_string();
        std::fs::write(
            dir.path().join("cap.json"),
            serde_json::to_vec_pretty(&s).expect("serialize sidecar"),
        )
        .expect("write sidecar");

        let result = save_to_staging_dir(
            dir.path(),
            "cap",
            Some(serde_json::json!({"segments": [{"sourceStartMs": 0, "sourceEndMs": 1}]})),
        );

        assert!(result.is_err(), "a mismatched sidecar must not be written");
        assert!(
            !dir.path().join("other.json").exists(),
            "the save must address the capture it was asked for, never the \
             name the file on disk claims"
        );
    }

    // The ordinary path: a save lands on the requested sidecar, in place.
    #[test]
    fn a_save_rewrites_the_requested_sidecar_in_place() {
        let dir = tempfile::tempdir().expect("tempdir");
        staging::write_sidecar(dir.path(), "cap", &sidecar("cap")).expect("write sidecar");

        save_to_staging_dir(dir.path(), "cap", Some(serde_json::json!({"segments": []})))
            .expect("save");

        let back = staging::read_sidecar(&dir.path().join("cap.json")).expect("reads back");
        assert_eq!(back.timeline, Some(serde_json::json!({"segments": []})));
        assert_eq!(back.duration_ms, 42_000, "the rest of the sidecar survives");
    }

    // T-2: `is_safe_base` is well tested as a function above, but its
    // APPLICATION was not -- removing the guard from a command body left
    // the whole shell test suite green, because `AppHandle` makes a unit
    // test of the commands themselves impossible. The
    // `capture_exclusion`/`clear_active_screen` precedent: scan this file's
    // own production source and fail if a command stops calling it.
    //
    // The scan is GENERIC over the commands rather than hardcoded to the
    // ones that existed when it was written, because the hardcoded version
    // failed exactly once it mattered: it named three commands and checked
    // the last of them against a slice running to end-of-file, so when a
    // fourth base-taking command was appended with its own guard, THAT
    // guard satisfied the assertion about `load_staged_capture` -- the test
    // stayed green with the load path unguarded (mutation-proven). Each
    // body is now bounded by its own column-0 closing brace, so a guard in
    // one command can never stand in for another's, and a command added
    // later is scanned without anybody remembering to widen this.
    #[test]
    fn every_command_taking_a_base_guards_it_with_is_safe_base() {
        let src = include_str!("editor_commands.rs");
        let production = src.split("#[cfg(test)]").next().unwrap_or(src);

        let mut all: Vec<&str> = Vec::new();
        let mut checked: Vec<&str> = Vec::new();
        // Matched by PREFIX, not against the exact literal
        // `#[tauri::command]`: an attribute written with arguments --
        // `#[tauri::command(rename_all = "snake_case")]`, a shape this
        // codebase already uses -- is invisible to a literal match, so a
        // command added in it would be scanned by nothing at all
        // (mutation-proven: an unguarded `discard_staged_capture` in that
        // shape left this test green).
        for (offset, _) in production.match_indices("#[tauri::command") {
            let rest = &production[offset..];
            // `cargo fmt` closes every top-level item with a brace in
            // column 0 and indents everything inside one, so this is an
            // exact body boundary -- unlike a slice to the next command,
            // which would swallow any helper declared between them.
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
            if !signature.contains("base: String") {
                continue;
            }
            let guard = body.find("if !is_safe_base(&base) {").unwrap_or_else(|| {
                panic!(
                    "{name} accepts a base from the frontend but does not refuse \
                     an unsafe one before it becomes a path"
                )
            });
            // Presence is not refusal: a guard whose body only logs passes
            // a `contains` check while validating nothing (mutation-proven).
            // The block cannot be bounded by its first `}` -- every one of
            // these logs the refused base with a `{base:?}` interpolation --
            // so it is bounded by the four-space dedent `cargo fmt`
            // guarantees for a block at function-body level.
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

        // Without this the scan would pass vacuously on a file whose
        // commands had all drifted out of the shape it matches -- the
        // failure mode a source scan is most prone to. It lists EVERY
        // command, not only the base-taking ones, because the evasion that
        // matters is a NEW command that takes the untrusted name under some
        // other parameter name (`capture: String`) and is therefore skipped
        // by the loop above and invisible to a base-takers-only list. Tasks
        // 5 and 6 add discard and export -- a discard written in that shape
        // is an arbitrary DELETE. Adding any command here must force a human
        // to decide whether it takes a base and guards it.
        assert_eq!(
            all,
            [
                "open_capture_editor",
                "take_editor_request",
                "load_staged_capture",
                "save_capture_timeline"
            ],
            "this file's command set changed; confirm whether the new command \
             turns frontend text into a path, then update this list"
        );
        assert_eq!(
            checked,
            [
                "open_capture_editor",
                "load_staged_capture",
                "save_capture_timeline"
            ],
            "the set of commands taking a base changed; confirm the new one \
             guards it, then update this list"
        );
    }

    // The sidecar is read-modify-written, never rebuilt: everything except
    // the timeline is the capture's own recorded truth (duration, inputs,
    // the vault it belongs to), and a save that reconstructed those fields
    // from what the editor happens to know would quietly lose whatever the
    // editor does not carry.
    #[test]
    fn saving_a_timeline_preserves_every_other_sidecar_field() {
        let mut s = sidecar("cap");
        s.inputs = vec!["Mic".into(), "Speakers".into()];
        s.paused_ms = 7_000;
        s.extra
            .insert("exportedTo".into(), serde_json::json!("Work/cap.md"));
        let timeline = serde_json::json!({"segments": [{"sourceStartMs": 5, "sourceEndMs": 9}]});
        let updated = with_timeline(s.clone(), Some(timeline.clone()));

        // I-2: this asserted six of eleven fields, and a mutation that
        // rebuilt the struct while preserving those six -- blanking
        // `source_title`, `source_kind`, `width` and `height` -- left it
        // green. `width`/`height` are what phase 5's export encodes
        // against. Comparing the WHOLE struct is what stops the assertion
        // going stale the way a hand-listed six did: `..s` carries a field
        // added later automatically, so the next field to join the sidecar
        // is pinned here without anybody remembering to widen this.
        assert_eq!(
            updated,
            StagedSidecar {
                timeline: Some(timeline),
                ..s
            }
        );
    }

    // Clearing is how "revert to the whole capture" is expressed, and it
    // must remove the field rather than store an empty timeline — an empty
    // segment list is a capture Save refuses (spec 8.1), which is not the
    // same thing as an untouched one.
    #[test]
    fn clearing_a_timeline_removes_it_rather_than_storing_an_empty_one() {
        let mut s = sidecar("cap");
        s.timeline = Some(serde_json::json!({"segments": []}));
        let updated = with_timeline(s, None);
        assert!(updated.timeline.is_none());
    }
}
