//! Recording file layout: dated folders, timestamped base names, and the
//! pairwise reservation rule — a base name is usable only when the .mp3,
//! the .md AND the hidden .mp3.part are all free, so a capture can never
//! overwrite a user note or an unrecovered orphan from an earlier crash.

use chrono::NaiveDate;
use std::path::{Path, PathBuf};

pub struct CaptureNames {
    pub base: String,
    pub final_mp3: PathBuf,
    pub note_md: PathBuf,
    pub part: PathBuf,
}

pub fn dated_folder(root: &Path, date: NaiveDate) -> PathBuf {
    root.join(date.format("%Y").to_string())
        .join(date.format("%m").to_string())
}

pub fn base_name(date: NaiveDate, hour: u32, minute: u32, label: &str) -> String {
    format!("{} {hour:02}{minute:02} {label}", date.format("%Y-%m-%d"))
}

pub fn part_file_name(base: &str) -> String {
    format!(".{base}.mp3.part")
}

pub fn base_from_part(part_file_name: &str) -> Option<String> {
    let stripped = part_file_name.strip_prefix('.')?;
    let base = stripped.strip_suffix(".mp3.part")?;
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

pub fn recovered_base(base: &str) -> String {
    format!("{base} (recovered)")
}

fn candidate(base: &str, attempt: u32) -> String {
    if attempt == 1 {
        base.to_string()
    } else {
        format!("{base} ({attempt})")
    }
}

pub fn reserve_names(dir: &Path, base: &str) -> CaptureNames {
    for attempt in 1.. {
        let b = candidate(base, attempt);
        let final_mp3 = dir.join(format!("{b}.mp3"));
        let note_md = dir.join(format!("{b}.md"));
        let part = dir.join(part_file_name(&b));
        if !final_mp3.exists() && !note_md.exists() && !part.exists() {
            return CaptureNames {
                base: b,
                final_mp3,
                note_md,
                part,
            };
        }
    }
    unreachable!("suffix search always terminates")
}

/// Join a configured recording folder onto the vault, refusing anything
/// that could land outside it: the config file is hand-editable, and the
/// PRD guarantees recordings are stored inside the vault.
pub fn safe_recording_root(vault_path: &Path, folder: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let rel = Path::new(folder);
    let escapes = rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        || folder.contains('\\') && folder.contains(':');
    if folder.is_empty() || escapes {
        return Err(format!(
            "Configured recording folder must stay inside the vault: {folder:?}"
        ));
    }
    Ok(vault_path.join(rel))
}

/// Runtime companion to `safe_recording_root`: canonicalize both paths
/// (the root must already exist) and require the root to resolve under
/// the vault — a pre-existing symlink or Windows junction at the
/// recording folder would otherwise carry writes outside the vault
/// despite the lexical check.
pub fn assert_root_inside_vault(vault_path: &Path, root: &Path) -> Result<(), String> {
    let vault =
        std::fs::canonicalize(vault_path).map_err(|e| format!("Cannot resolve vault path: {e}"))?;
    let resolved =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve recording folder: {e}"))?;
    if resolved.starts_with(&vault) {
        Ok(())
    } else {
        Err("Configured recording folder resolves outside the vault".to_string())
    }
}

/// Whether a `hard_link` error is decisive on its own and must propagate
/// rather than be papered over by the guarded rename fallback.
/// `AlreadyExists` is the live collision signal — `to` is taken, exactly
/// what non-replacing semantics need to report. `NotFound` means `from`
/// itself is missing, which the fallback (also rooted at `from`) cannot
/// fix either. Every other error — `Unsupported`, `PermissionDenied`,
/// raw EPERM/EXDEV/ERROR_INVALID_FUNCTION and whatever else exFAT/FAT32/
/// SMB happen to report for "this filesystem can't do hard links" — is
/// treated as a hard-link-capability problem and falls back instead of
/// propagating.
fn hard_link_error_is_decisive(e: &std::io::Error) -> bool {
    matches!(
        e.kind(),
        std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotFound
    )
}

/// Atomic non-replacing move: hard_link + remove_file fails with
/// AlreadyExists if `to` exists — unlike std::fs::rename, which
/// replaces on both Unix and Windows.
///
/// Two deliberate leniencies, both biased toward never losing audio:
///
/// - If `hard_link` succeeds but the follow-up `remove_file(from)` fails
///   (e.g. a Windows AV/indexer holding the source open), we still
///   return `Ok(())` and just log a warning. The save already succeeded
///   at `to`; the leftover `from` is at worst re-finalized later as a
///   `(recovered)` duplicate. Returning `Err` here while `to` exists
///   would send callers that retry-on-error (like
///   `rename_into_reserved`) into an endless suffix-minting loop, since
///   `to` existing looks like a fresh collision on every retry.
/// - Any `hard_link` error *except* `AlreadyExists`/`NotFound` (see
///   `hard_link_error_is_decisive`) falls back to the guarded
///   exists()+rename path — the same racy-but-pre-check-guarded
///   behavior this function had before hard links were introduced, so
///   it can never be worse than what shipped before. This covers
///   exFAT/FAT32/SMB filesystems that report all sorts of "can't hard
///   link" codes, not just the ones we happen to have enumerated.
///   NTFS/ext4 keep the atomic hard-link path.
pub fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(from, to) {
        Ok(()) => {
            if let Err(e) = std::fs::remove_file(from) {
                log::warn!(
                    "rename_noreplace: linked {from:?} to {to:?} but could not remove the \
                     source ({e}); leaving it behind for a later (recovered) finalize"
                );
            }
            Ok(())
        }
        Err(e) if hard_link_error_is_decisive(&e) => Err(e),
        Err(_) => {
            if to.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "destination exists",
                ));
            }
            std::fs::rename(from, to)
        }
    }
}

/// Stop-time recheck: only the final destinations matter — the session's
/// own .part must not push an ordinary save onto a suffixed name.
pub fn reserve_final(dir: &Path, base: &str) -> (PathBuf, PathBuf) {
    for attempt in 1.. {
        let b = candidate(base, attempt);
        let final_mp3 = dir.join(format!("{b}.mp3"));
        let note_md = dir.join(format!("{b}.md"));
        if !final_mp3.exists() && !note_md.exists() {
            return (final_mp3, note_md);
        }
    }
    unreachable!("suffix search always terminates")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 4).unwrap()
    }

    #[test]
    fn dated_folder_is_year_slash_month() {
        let dir = dated_folder(Path::new("/v/Meetings"), date());
        assert_eq!(dir, Path::new("/v/Meetings/2026/07"));
    }

    #[test]
    fn base_name_format() {
        assert_eq!(
            base_name(date(), 14, 5, "Meeting"),
            "2026-07-04 1405 Meeting"
        );
    }

    #[test]
    fn part_name_roundtrip() {
        let part = part_file_name("2026-07-04 1405 Meeting");
        assert_eq!(part, ".2026-07-04 1405 Meeting.mp3.part");
        assert_eq!(
            base_from_part(&part).as_deref(),
            Some("2026-07-04 1405 Meeting")
        );
        assert_eq!(base_from_part("random.txt"), None);
    }

    #[test]
    fn reserve_uses_plain_base_when_all_free() {
        let dir = tempfile::tempdir().unwrap();
        let names = reserve_names(dir.path(), "b");
        assert_eq!(names.base, "b");
        assert_eq!(names.final_mp3, dir.path().join("b.mp3"));
        assert_eq!(names.note_md, dir.path().join("b.md"));
        assert_eq!(names.part, dir.path().join(".b.mp3.part"));
    }

    #[test]
    fn reserve_advances_when_note_or_part_exists() {
        let dir = tempfile::tempdir().unwrap();
        // a pre-existing user note blocks the plain base
        std::fs::write(dir.path().join("b.md"), "user note").unwrap();
        // an unrecovered orphan blocks " (2)"
        std::fs::write(dir.path().join(".b (2).mp3.part"), "x").unwrap();
        let names = reserve_names(dir.path(), "b");
        assert_eq!(names.base, "b (3)");
    }

    #[test]
    fn reserve_final_ignores_own_part_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".b.mp3.part"), "recording").unwrap();
        let (mp3, md) = reserve_final(dir.path(), "b");
        assert_eq!(mp3, dir.path().join("b.mp3"));
        assert_eq!(md, dir.path().join("b.md"));
    }

    #[test]
    fn reserve_final_advances_past_existing_mp3() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.mp3"), "sync client wrote this").unwrap();
        let (mp3, _) = reserve_final(dir.path(), "b");
        assert_eq!(mp3, dir.path().join("b (2).mp3"));
    }

    #[test]
    fn recovered_base_appends_marker() {
        assert_eq!(recovered_base("b"), "b (recovered)");
    }

    #[test]
    fn safe_root_accepts_plain_and_nested_folders() {
        let vault = Path::new("/v");
        assert_eq!(
            safe_recording_root(vault, "Meetings").unwrap(),
            Path::new("/v/Meetings")
        );
        assert_eq!(
            safe_recording_root(vault, "Capture/Meetings").unwrap(),
            Path::new("/v/Capture/Meetings")
        );
    }

    #[test]
    fn safe_root_rejects_escaping_folders() {
        let vault = Path::new("/v");
        assert!(safe_recording_root(vault, "../outside").is_err());
        assert!(safe_recording_root(vault, "a/../../outside").is_err());
        assert!(safe_recording_root(vault, "/etc").is_err());
        assert!(safe_recording_root(vault, "C:\\other").is_err());
    }

    #[test]
    fn root_inside_vault_passes_canonical_check() {
        let vault = tempfile::tempdir().unwrap();
        let root = vault.path().join("Meetings");
        std::fs::create_dir_all(&root).unwrap();
        assert!(assert_root_inside_vault(vault.path(), &root).is_ok());
    }

    // Also stands in for the "odd hard_link errors fall back, not
    // propagate" contract at the destination-collision boundary: this is
    // the one case where propagating is still correct (AlreadyExists is
    // decisive, see `hard_link_error_is_decisive`), and it's exercised
    // through the real `hard_link` + guarded-fallback path since `to`
    // already exists before `rename_noreplace` is even called.
    #[test]
    fn rename_noreplace_refuses_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("to.txt");
        std::fs::write(&from, "source").unwrap();
        std::fs::write(&to, "already here").unwrap();
        let err = rename_noreplace(&from, &to).expect_err("must not replace");
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&from).unwrap(), "source");
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "already here");
    }

    // Direct coverage for the F2 policy: only AlreadyExists/NotFound are
    // decisive; everything else (Unsupported, PermissionDenied, the raw
    // EPERM/EXDEV/ERROR_INVALID_FUNCTION codes exFAT/FAT32/SMB report,
    // and anything else) must fall back rather than propagate. We can't
    // portably force `hard_link` itself to fail with an arbitrary errno
    // in a test, so this asserts the classification directly.
    #[test]
    fn hard_link_error_is_decisive_only_for_already_exists_and_not_found() {
        use std::io::{Error, ErrorKind};
        assert!(hard_link_error_is_decisive(&Error::new(
            ErrorKind::AlreadyExists,
            "taken"
        )));
        assert!(hard_link_error_is_decisive(&Error::new(
            ErrorKind::NotFound,
            "gone"
        )));
        assert!(!hard_link_error_is_decisive(&Error::new(
            ErrorKind::Unsupported,
            "no hard links"
        )));
        assert!(!hard_link_error_is_decisive(&Error::new(
            ErrorKind::PermissionDenied,
            "eperm"
        )));
        assert!(!hard_link_error_is_decisive(&Error::from_raw_os_error(1)));
        assert!(!hard_link_error_is_decisive(&Error::other(
            "invalid function"
        )));
    }

    #[test]
    fn rename_noreplace_moves_when_free() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("from.txt");
        let to = dir.path().join("to.txt");
        std::fs::write(&from, "payload").unwrap();
        rename_noreplace(&from, &to).unwrap();
        assert!(!from.exists(), "source removed");
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "payload");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_root_outside_vault_is_rejected() {
        let vault = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = vault.path().join("Meetings");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        assert!(assert_root_inside_vault(vault.path(), &link).is_err());
    }
}
