//! Startup recovery: finalize orphaned .part recordings left by a crash.
//! Never touches live recordings (staleness check; single-instance is the
//! first line of defense) and never deletes captured audio — the only
//! deletion is a .part with no MP3 frame in it, which holds no audio.

use std::path::{Path, PathBuf};
use std::time::Duration;
use vault_buddy_core::capture_note::{render_note, write_note_collision_safe, NoteMeta};
use vault_buddy_core::capture_paths::{base_from_part, recovered_base, reserve_final};

#[derive(Debug)]
pub enum RecoveryAction {
    Recovered { mp3: PathBuf },
    DeletedEmpty(PathBuf),
    Fresh(PathBuf),
}

pub fn has_mp3_frame(bytes: &[u8]) -> bool {
    bytes
        .windows(2)
        .any(|w| w[0] == 0xFF && (w[1] & 0xE0) == 0xE0)
}

/// Ownership check for .mp3.part files: only bases matching Vault Buddy's
/// capture pattern `YYYY-MM-DD HHmm <label>` are ours to delete or rename.
/// Another tool's `.download.mp3.part` in a vault must never be touched.
fn is_capture_base(base: &str) -> bool {
    let b: Vec<char> = base.chars().collect();
    if b.len() < 17 {
        return false;
    }
    let digit = |i: usize| b[i].is_ascii_digit();
    (0..4).all(digit)
        && b[4] == '-'
        && (5..7).all(digit)
        && b[7] == '-'
        && (8..10).all(digit)
        && b[10] == ' '
        && (11..15).all(digit)
        && b[15] == ' '
}

fn is_stale(path: &Path, stale_after: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|age| age >= stale_after)
        .unwrap_or(false)
}

pub fn recover_root(
    root: &Path,
    vault_name: &str,
    stale_after: Duration,
    write_note: bool,
) -> Vec<RecoveryAction> {
    let mut actions = Vec::new();
    walk(root, &mut |path| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // Ownership marker filter: only OUR temps are ever deleted. A
        // foreign `.something.md.tmp` from another tool must survive —
        // this is the app's first write path into user vaults.
        if name.ends_with(vault_buddy_core::capture_note::NOTE_TMP_SUFFIX) && name.starts_with('.')
        {
            if is_stale(path, stale_after) {
                log::info!("recovery: removing stale note temp {}", path.display());
                let _ = std::fs::remove_file(path);
            }
            return;
        }
        let Some(base) = base_from_part(&name) else {
            return;
        };
        if !is_capture_base(&base) {
            return; // not ours — never delete or rename foreign files
        }
        if !is_stale(path, stale_after) {
            actions.push(RecoveryAction::Fresh(path.to_path_buf()));
            return;
        }
        let bytes = std::fs::read(path).unwrap_or_default();
        if !has_mp3_frame(&bytes) {
            log::info!("recovery: deleting frameless part {}", path.display());
            let _ = std::fs::remove_file(path);
            actions.push(RecoveryAction::DeletedEmpty(path.to_path_buf()));
            return;
        }
        let dir = path.parent().unwrap_or(root);
        let (mp3, note) = match rename_into_reserved(path, dir, &recovered_base(&base)) {
            Ok(paths) => paths,
            Err(e) => {
                log::warn!("recovery: rename failed for {}: {e}", path.display());
                return;
            }
        };
        log::info!("recovery: finalized {}", mp3.display());
        if write_note {
            let meta = NoteMeta {
                recorded_at: String::new(),
                duration_secs: 0,
                vault_name: vault_name.to_string(),
                recording_type: "Recording".to_string(),
                input_devices: Vec::new(),
                event: Some("recovered after crash".to_string()),
            };
            let mp3_name = mp3.file_name().unwrap_or_default().to_string_lossy();
            let _ = write_note_collision_safe(&note, &render_note(&meta, &mp3_name));
        }
        actions.push(RecoveryAction::Recovered { mp3 });
    });
    actions
}

/// Finalize `from` under the first free suffixed name for `base` in `dir`.
/// The rename is the arbiter: a destination created between the reserve
/// check and the rename advances the suffix and retries (Windows renames
/// are non-replacing; on Unix the exists() pre-check plus the tight retry
/// loop is the accepted dev-platform approximation).
pub fn rename_into_reserved(
    from: &Path,
    dir: &Path,
    base: &str,
) -> Result<(PathBuf, PathBuf), String> {
    loop {
        let (mp3, note) = reserve_final(dir, base);
        match std::fs::rename(from, &mp3) {
            Ok(()) => return Ok((mp3, note)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // Windows reports rename-onto-existing as PermissionDenied or
            // AlreadyExists depending on the API path.
            Err(_) if mp3.exists() => continue,
            Err(e) => return Err(format!("finalize rename failed: {e}")),
        }
    }
}

fn walk(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Minimal valid-looking MP3 frame header bytes.
    fn mp3_bytes() -> Vec<u8> {
        let mut v = vec![0u8; 32];
        v.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        v.extend_from_slice(&[0u8; 400]);
        v
    }

    fn make_stale(path: &std::path::Path) {
        // recover_root treats files older than stale_after as orphans; a
        // zero stale_after makes everything stale without clock games.
        let _ = path;
    }

    #[test]
    fn sync_word_detection() {
        assert!(has_mp3_frame(&mp3_bytes()));
        assert!(!has_mp3_frame(&[0u8; 512]));
        assert!(!has_mp3_frame(b""));
    }

    #[test]
    fn recovers_stale_part_with_audio() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(".2026-07-04 1405 Meeting.mp3.part");
        std::fs::write(&part, mp3_bytes()).unwrap();
        make_stale(&part);
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            RecoveryAction::Recovered { mp3 } => {
                assert_eq!(
                    mp3.file_name().unwrap().to_string_lossy(),
                    "2026-07-04 1405 Meeting (recovered).mp3"
                );
                assert!(mp3.exists());
                assert!(!part.exists());
                let note = dir.path().join("2026-07-04 1405 Meeting (recovered).md");
                assert!(note.exists(), "recovery note written");
                let text = std::fs::read_to_string(note).unwrap();
                assert!(text.contains(r#"event: "recovered after crash""#));
            }
            other => panic!("expected Recovered, got {other:?}"),
        }
    }

    // All bases below use the capture timestamp pattern — recovery's
    // ownership filter ignores anything else.
    const BASE: &str = "2026-07-04 1405 Voice Note";

    #[test]
    fn respects_note_toggle() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        recover_root(dir.path(), "Work", Duration::ZERO, false);
        assert!(!dir.path().join(format!("{BASE} (recovered).md")).exists());
        assert!(dir.path().join(format!("{BASE} (recovered).mp3")).exists());
    }

    #[test]
    fn deletes_zero_frame_part() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, [0u8; 64]).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(matches!(actions[0], RecoveryAction::DeletedEmpty(_)));
        assert!(!part.exists());
        assert!(!dir.path().join(format!("{BASE} (recovered).mp3")).exists());
    }

    #[test]
    fn reports_fresh_part_without_touching_it() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::from_secs(3600), true);
        assert!(matches!(actions[0], RecoveryAction::Fresh(_)));
        assert!(part.exists());
    }

    #[test]
    fn walks_dated_subfolders_and_avoids_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let month = dir.path().join("2026").join("06");
        std::fs::create_dir_all(&month).unwrap();
        // an earlier recovered capture already claimed the recovered name
        std::fs::write(month.join(format!("{BASE} (recovered).mp3")), "earlier").unwrap();
        std::fs::write(month.join(format!(".{BASE}.mp3.part")), mp3_bytes()).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, false);
        match &actions[0] {
            RecoveryAction::Recovered { mp3 } => {
                assert_eq!(
                    mp3.file_name().unwrap().to_string_lossy(),
                    format!("{BASE} (recovered) (2).mp3")
                );
                assert_eq!(
                    std::fs::read_to_string(month.join(format!("{BASE} (recovered).mp3"))).unwrap(),
                    "earlier",
                    "earlier recovery untouched"
                );
            }
            other => panic!("expected Recovered, got {other:?}"),
        }
    }

    #[test]
    fn foreign_mp3_parts_are_never_touched() {
        // Another tool's hidden partial download must survive recovery
        // untouched — even a zero-frame one must NOT be deleted.
        let dir = tempfile::tempdir().unwrap();
        let foreign = dir.path().join(".download.mp3.part");
        std::fs::write(&foreign, [0u8; 64]).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(
            actions.is_empty(),
            "foreign part produced actions: {actions:?}"
        );
        assert!(foreign.exists());
    }

    #[test]
    fn deletes_only_vault_buddy_note_temps() {
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join(".b.md.vault-buddy.tmp");
        let foreign = dir.path().join(".draft.md.tmp");
        std::fs::write(&ours, "half a note").unwrap();
        std::fs::write(&foreign, "another tool's temp").unwrap();
        recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(!ours.exists(), "our stale temp is cleaned");
        assert!(foreign.exists(), "foreign temp files are never touched");
    }
}
