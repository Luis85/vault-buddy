//! Startup recovery: finalize orphaned .part recordings left by a crash.
//! Never touches live recordings (staleness check; single-instance is the
//! first line of defense) and never deletes captured audio — the only
//! deletion is a .part with no MP3 frame in it, which holds no audio.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use vault_buddy_core::capture_note::{
    render_note, write_note_collision_safe, NoteMeta, NOTE_TMP_SUFFIX,
};
use vault_buddy_core::capture_paths::{
    base_from_part, is_capture_base, recovered_base, reserve_final,
};

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

/// How much of a .part the frame sniff reads. LAME emits the first frame
/// within the first ~1 s flush, so any real recording has a sync word in
/// the first few KB — and a frameless part has none anywhere, so a
/// bounded prefix decides either way without pulling a multi-hour
/// recording into memory. Deliberate behavior change vs. reading the
/// whole file: a .part whose ONLY sync word sits beyond this prefix
/// would now be deleted as frameless — that shape cannot come from our
/// encoder, so no test pins it.
const FRAME_SNIFF_LEN: u64 = 64 * 1024;

fn read_prefix(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Pure staleness decision (split from `is_stale` so clock cases are
/// testable without touching real file mtimes). A live .part can never be
/// caught by the skew branch: recovery passes are postponed entirely
/// while a recording is active (see `run_recovery` in capture_commands).
fn is_stale_at(modified: SystemTime, now: SystemTime, stale_after: Duration) -> bool {
    match now.duration_since(modified) {
        Ok(age) => age >= stale_after,
        // mtime ahead of the clock: a live recording's mtime tracks "now",
        // so small skew stays fresh; a gap beyond the window means a clock
        // jump left an orphan that would otherwise never be recovered.
        Err(e) => e.duration() >= stale_after,
    }
}

fn is_stale(path: &Path, stale_after: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    is_stale_at(modified, SystemTime::now(), stale_after)
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
        if name.ends_with(NOTE_TMP_SUFFIX) && name.starts_with('.') {
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
        // A read failure (permissions, AV lock, transient I/O) must NOT
        // look like "no audio" — deletion is only for provably frameless
        // parts. Unreadable files are left for a later pass.
        let Ok(bytes) = read_prefix(path, FRAME_SNIFF_LEN) else {
            log::warn!("recovery: cannot read {}, skipping", path.display());
            return;
        };
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
/// The move is the arbiter: `rename_noreplace` is an atomic non-replacing
/// move (std::fs::rename would REPLACE an existing destination on both
/// Unix and Windows), so a destination created between the reserve check
/// and the move fails with AlreadyExists, advances the suffix, and
/// retries — a sync-client race can never clobber an existing file.
pub fn rename_into_reserved(
    from: &Path,
    dir: &Path,
    base: &str,
) -> Result<(PathBuf, PathBuf), String> {
    loop {
        let (mp3, note) = reserve_final(dir, base);
        match vault_buddy_core::capture_paths::rename_noreplace(from, &mp3) {
            Ok(()) => return Ok((mp3, note)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // Some Windows API paths report a taken destination as
            // PermissionDenied instead of AlreadyExists.
            Err(_) if mp3.exists() => continue,
            Err(e) => return Err(format!("finalize rename failed: {e}")),
        }
    }
}

fn is_digit_dir(name: &str, len: usize) -> bool {
    name.len() == len && name.chars().all(|c| c.is_ascii_digit())
}

fn dir_entries(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, String)> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            // entry.file_type() reads the dirent and does NOT follow
            // symlinks: a symlinked directory (or Windows junction) must
            // never let the scan escape the vault or enter a cycle.
            if let Ok(ft) = entry.file_type() {
                let name = entry.file_name().to_string_lossy().into_owned();
                out.push((entry.path(), ft, name));
            }
        }
    }
    out
}

/// Vault Buddy writes only under `<root>/YYYY/MM` — recovery looks
/// nowhere else, so a capture-named file a user moved into an arbitrary
/// subfolder (or the root itself) is never touched.
fn walk(root: &Path, visit: &mut dyn FnMut(&Path)) {
    for (year_path, year_ft, year_name) in dir_entries(root) {
        if !year_ft.is_dir() || !is_digit_dir(&year_name, 4) {
            continue;
        }
        for (month_path, month_ft, month_name) in dir_entries(&year_path) {
            if !month_ft.is_dir() || !is_digit_dir(&month_name, 2) {
                continue;
            }
            for (file_path, file_ft, _) in dir_entries(&month_path) {
                if file_ft.is_file() {
                    visit(&file_path);
                }
            }
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

    /// Vault Buddy only ever writes under `<root>/YYYY/MM` — recovery
    /// walks nowhere else, so tests must place capture-named files there.
    fn month_dir(root: &std::path::Path) -> std::path::PathBuf {
        let d = root.join("2026").join("07");
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn staleness_decision_handles_clock_skew() {
        let now = SystemTime::now();
        let hour = Duration::from_secs(3600);
        let window = Duration::from_secs(60);
        // well past the window: an ordinary orphan
        assert!(is_stale_at(now - hour, now, window));
        // recent past: possibly still live
        assert!(!is_stale_at(now - Duration::from_secs(5), now, window));
        // slightly ahead of the clock (jitter, coarse fs timestamps): fresh
        assert!(!is_stale_at(now + Duration::from_secs(5), now, window));
        // far ahead: a clock jump stranded this orphan — it must become
        // recoverable, not stay "fresh" until the wall clock catches up
        assert!(is_stale_at(now + hour, now, window));
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
        let month = month_dir(dir.path());
        let part = month.join(".2026-07-04 1405 Meeting.mp3.part");
        std::fs::write(&part, mp3_bytes()).unwrap();
        // a zero stale_after makes everything stale without clock games
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
                let note = month.join("2026-07-04 1405 Meeting (recovered).md");
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
        let month = month_dir(dir.path());
        let part = month.join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        recover_root(dir.path(), "Work", Duration::ZERO, false);
        assert!(!month.join(format!("{BASE} (recovered).md")).exists());
        assert!(month.join(format!("{BASE} (recovered).mp3")).exists());
    }

    #[test]
    fn deletes_zero_frame_part() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        let part = month.join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, [0u8; 64]).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(matches!(actions[0], RecoveryAction::DeletedEmpty(_)));
        assert!(!part.exists());
        assert!(!month.join(format!("{BASE} (recovered).mp3")).exists());
    }

    #[test]
    fn large_part_with_early_sync_word_is_recovered() {
        // 128 KiB with the only sync word ~512 bytes in: well inside the
        // bounded prefix the sniff reads (see FRAME_SNIFF_LEN — the >64 KiB
        // negative is deliberately unpinned).
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        let mut bytes = vec![0u8; 128 * 1024];
        bytes[512] = 0xFF;
        bytes[513] = 0xFB;
        let part = month.join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, &bytes).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, false);
        assert!(
            matches!(&actions[0], RecoveryAction::Recovered { .. }),
            "expected Recovered, got {actions:?}"
        );
        assert!(!part.exists());
    }

    #[test]
    fn reports_fresh_part_without_touching_it() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        let part = month.join(format!(".{BASE}.mp3.part"));
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
        let month = month_dir(dir.path());
        let foreign = month.join(".download.mp3.part");
        std::fs::write(&foreign, [0u8; 64]).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(
            actions.is_empty(),
            "foreign part produced actions: {actions:?}"
        );
        assert!(foreign.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directories_are_not_followed() {
        // A symlink standing in for a dated year folder must not let
        // recovery walk outside the vault and rename/delete files there.
        let outside = tempfile::tempdir().unwrap();
        let outside_month = outside.path().join("07");
        std::fs::create_dir_all(&outside_month).unwrap();
        let part = outside_month.join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("2026")).unwrap();
        let actions = recover_root(root.path(), "Work", Duration::ZERO, true);
        assert!(actions.is_empty(), "walked through symlink: {actions:?}");
        assert!(part.exists(), "outside file untouched");
    }

    #[test]
    fn deletes_only_vault_buddy_note_temps() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        let ours = month.join(".b.md.vault-buddy.tmp");
        let foreign = month.join(".draft.md.tmp");
        std::fs::write(&ours, "half a note").unwrap();
        std::fs::write(&foreign, "another tool's temp").unwrap();
        recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(!ours.exists(), "our stale temp is cleaned");
        assert!(foreign.exists(), "foreign temp files are never touched");
    }

    #[test]
    fn root_level_parts_are_ignored() {
        // A capture-named .part left directly at the vault root (not under
        // <root>/YYYY/MM) is outside the layout recovery is allowed to
        // touch.
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(
            actions.is_empty(),
            "root-level part produced actions: {actions:?}"
        );
        assert!(part.exists());
    }

    #[test]
    fn non_dated_subfolders_are_ignored() {
        // A capture-named .part a user moved into an arbitrary subfolder
        // must survive — recovery only looks under <root>/YYYY/MM.
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Project");
        std::fs::create_dir_all(&sub).unwrap();
        let part = sub.join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(
            actions.is_empty(),
            "non-dated subfolder produced actions: {actions:?}"
        );
        assert!(part.exists());
    }
}
