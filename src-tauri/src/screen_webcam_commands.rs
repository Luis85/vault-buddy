//! The synchronized webcam's shell surface (F-22): listing webcams for the
//! Record Screen picker, turning `start_screen_capture`'s optional
//! `webcamId` into the session's webcam request, and turning the finished
//! webcam track into the staging sidecar's `webcam` block.
//!
//! Its own module (pre-flight F31): `screen_commands.rs` and
//! `screen_capture_worker.rs` both sit near the 800-line Rust cap, and none
//! of this is the capture LIFECYCLE they own. `list_capture_webcams` is an
//! ordinary NON-editor command: `generate_handler!`, `build.rs`'s
//! `ALL_COMMANDS` and ONE grant in `capabilities/default.json`.

use std::path::Path;

use tauri::AppHandle;
use vault_buddy_core::screen_capture_config::ScreenQuality;
use vault_buddy_screen::session::webcam::{WebcamOutcome, WebcamParams};
use vault_buddy_screen::source::{self, WebcamDeviceId};
use vault_buddy_screen::staging;

/// ASYNC: Media Foundation device enumeration activates the capture-device
/// category and can take hundreds of ms (`list_capture_sources`' reason).
/// It degrades to an empty list — the picker's "No webcam" is an ordinary
/// state, and a failed task is not something the user can act on.
#[tauri::command]
pub async fn list_capture_webcams(_app: AppHandle) -> Vec<source::CaptureWebcamInfo> {
    tauri::async_runtime::spawn_blocking(source::list_webcams)
        .await
        .unwrap_or_else(|e| {
            log::warn!("list_capture_webcams: task failed: {e}");
            Vec::new()
        })
}

/// `start_screen_capture`'s `webcamId`, checked BEFORE anything is claimed.
/// Absent means no webcam (today's capture); present must be a canonical
/// `webcam:<hash>`, or the start is refused — a lenient parse would open
/// SOME device the user did not pick, or silently record without one.
pub(crate) fn parse_webcam_request(
    webcam_id: Option<&str>,
) -> Result<Option<WebcamDeviceId>, String> {
    match webcam_id {
        None => Ok(None),
        Some(id) => WebcamDeviceId::parse(id)
            .map(Some)
            .ok_or_else(|| "Unknown webcam.".to_string()),
    }
}

/// The session's webcam request for capture `base`, recording into this
/// capture's OWN webcam files in `dir`.
pub(crate) fn webcam_params(
    device: Option<WebcamDeviceId>,
    dir: &Path,
    base: &str,
    quality: ScreenQuality,
) -> Option<WebcamParams> {
    device.map(|device| WebcamParams {
        device,
        part: dir.join(staging::webcam_part_file_name(base)),
        staged: dir.join(staging::webcam_file_name(base)),
        quality,
    })
}

/// The sidecar's `webcam` block for a finished track: a staging file NAME
/// (never a path), and the MEASURED offset and length (GAP-199).
pub(crate) fn webcam_block(base: &str, outcome: &WebcamOutcome) -> staging::WebcamSidecar {
    staging::WebcamSidecar {
        file: staging::webcam_file_name(base),
        width: outcome.width,
        height: outcome.height,
        device_label: outcome.device_label.clone(),
        offset_ms: outcome.offset_ms,
        duration_ms: Some(outcome.duration_ms),
        extra: serde_json::Map::new(),
    }
}

#[cfg(test)]
#[path = "screen_webcam_commands_tests.rs"]
mod tests;
