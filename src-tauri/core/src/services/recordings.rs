use std::path::{Path, PathBuf};

use super::{app_config, find_vault, ServicePaths};
use crate::{capture_config, capture_paths, recordings};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    pub mp3: String,
    pub title: String,
    pub recorded_at: String,
    pub duration: Option<String>,
    // `type` is a Rust keyword — expose the camelCase `type` the frontend wants.
    #[serde(rename = "type")]
    pub recording_type: Option<String>,
    pub transcript_status: String,
}

/// Read-only list of a vault's past recordings for the Recordings view.
/// Scans the vault's recording roots (custom folder, or both mode defaults)
/// and reads each recording's companion note for type/duration. An unknown
/// vault or unreadable roots yield an empty list — never an error (mirrors
/// discovery's degrade-to-empty rule). Never writes into the vault.
pub fn list_recordings(paths: &ServicePaths, id: &str) -> Vec<RecordingDto> {
    let Ok(vault) = find_vault(paths, id) else {
        return Vec::new();
    };
    let cfg = capture_config::vault_config(&app_config(paths), id);
    // No swallowed error: a rejected (unsafe) folder is skipped WITH a warning,
    // matching run_recovery/scan_and_enqueue — a silent filter_map would hide it.
    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in cfg.recording_roots() {
        let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), folder) else {
            log::warn!("list_recordings: skipping unsafe recording folder {folder:?}");
            continue;
        };
        // Canonicalize before scanning: safe_recording_root is only lexical,
        // so a recording folder that is a symlink/junction out of the vault
        // would otherwise be scanned — enumerating capture MP3s and reading
        // companion-note frontmatter outside the vault. Same read guard as
        // list_tasks (and recovery asserts the same before its scan): a
        // merely missing root stays pushed — scanning it yields nothing — an
        // escape is warned and skipped, never failing the whole call.
        if root.exists() {
            if let Err(e) = capture_paths::assert_root_inside_vault(Path::new(&vault.path), &root) {
                log::warn!("list_recordings: recording folder resolves outside the vault: {e}");
                continue;
            }
        }
        roots.push(root);
    }
    recordings::list_recordings(&roots)
        .into_iter()
        .map(|e| RecordingDto {
            mp3: e.mp3_path.to_string_lossy().into_owned(),
            title: e.title,
            recorded_at: e.recorded_at,
            duration: e.duration,
            recording_type: e.recording_type,
            transcript_status: e.transcript_status.as_dto_str().to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::test_support::fixture;
    use super::*;

    #[test]
    fn list_recordings_degrades_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _) = fixture(dir.path(), "MyVault");
        assert!(list_recordings(&paths, "deadbeef01234567").is_empty());
        assert!(list_recordings(&paths, "unknown").is_empty());
    }

    // Codex review catch pinned as a test: safe_recording_root is only
    // lexical, so a recording folder that is a symlink out of the vault would
    // be scanned — enumerating capture MP3s and reading companion-note
    // frontmatter OUTSIDE the vault. The read must skip it (warn, degrade to
    // empty), same guard as list_tasks — and this path is MCP-exposed.
    #[cfg(unix)]
    #[test]
    fn list_recordings_skips_a_symlinked_root_outside_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        // A real capture layout OUTSIDE the vault, reachable only through the
        // symlinked default folder <vault>/Meetings.
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("2026").join("07")).unwrap();
        std::fs::write(
            outside
                .join("2026")
                .join("07")
                .join("2026-07-09 1405 Meeting.mp3"),
            "mp3",
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, vault.join("Meetings")).unwrap();
        assert!(list_recordings(&paths, "deadbeef01234567").is_empty());
    }
}
