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

use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

const EDITOR_LABEL: &str = "editor";

/// The base name the editor window should open when it next mounts.
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
/// a path. A separator or `..` would read a file outside staging entirely;
/// a leading dot is how our own in-progress `.part` files are named and must
/// never be opened as a finished capture. This is `sanitize_title`'s lesson
/// (phase 2, spec 10) applied at the other end of the same pipe.
///
/// Also refuses a base ending in a trailing dot or space: Windows silently
/// strips either from a file name on disk, so `dir.join(base + ".mp4")`
/// would resolve to a DIFFERENT path than the one actually reserved on
/// disk (`staging::sanitize_title`'s own trim exists for exactly this
/// reason) — accepting such a base here would look up a file that can
/// never exist rather than the one that does.
fn is_safe_base(base: &str) -> bool {
    !base.is_empty()
        && !base.starts_with('.')
        && !base.ends_with('.')
        && !base.ends_with(' ')
        && !base.contains('/')
        && !base.contains('\\')
        && !base.contains("..")
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

/// SYNC: shows and focuses the editor window and stashes which capture it
/// should open. Window APIs are main-thread-only and this command touches no
/// disk, so sync is correct — see the module doc for why the data arrives
/// separately.
#[tauri::command]
pub fn open_capture_editor(app: AppHandle, base: String) -> Result<(), String> {
    if !is_safe_base(&base) {
        return Err("That capture name is not one of ours.".to_string());
    }
    *lock_ignoring_poison(&app.state::<EditorRequest>().0) = Some(base);
    let window = app
        .get_webview_window(EDITOR_LABEL)
        .ok_or_else(|| "The editor window is missing.".to_string())?;
    window
        .show()
        .map_err(|e| format!("Could not open the editor: {e}"))?;
    // An editor already open on another capture is raised, not duplicated:
    // there is one editor window and the stash it just read is the new one.
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

/// SYNC, one-shot: the editor webview drains this on mount. Returns `None`
/// when the window was opened with nothing staged, which is not an error —
/// a user can alt-tab back to an editor that is already showing a capture.
#[tauri::command]
pub fn take_editor_request(app: AppHandle) -> Option<String> {
    lock_ignoring_poison(&app.state::<EditorRequest>().0).take()
}

/// ASYNC: reads the sidecar off disk, so it must not sit on the main thread.
#[tauri::command]
pub async fn load_staged_capture(
    app: AppHandle,
    base: String,
) -> Result<StagedCaptureDetail, String> {
    if !is_safe_base(&base) {
        return Err("That capture name is not one of ours.".to_string());
    }
    let dir = staging::staging_dir(
        &app.path()
            .app_local_data_dir()
            .map_err(|e| format!("Could not resolve the staging directory: {e}"))?,
    );
    tauri::async_runtime::spawn_blocking(move || {
        let sidecar_path = dir.join(staging::sidecar_file_name(&base));
        let sidecar = staging::read_sidecar(&sidecar_path)
            .ok_or_else(|| "That capture's details could not be read.".to_string())?;
        let mp4 = staging::mp4_file_name(&base);
        if !dir.join(&mp4).is_file() {
            return Err("That capture's video file is missing.".to_string());
        }
        Ok(detail_from_sidecar(&sidecar, &mp4))
    })
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
        // A `..` with NO leading dot and NO separator around it: this isolates
        // the `contains("..")` check from the leading-dot and separator
        // checks above, which every other `..`-bearing fixture here also
        // trips. Without it, deleting the `contains("..")` check entirely
        // leaves every assertion above still passing.
        assert!(!is_safe_base("a..b"));
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
}
