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
}
