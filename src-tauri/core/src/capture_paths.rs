//! Recording file layout: dated folders, timestamped base names, and the
//! pairwise reservation rule — a base name is usable only when the .mp3,
//! the .md, the hidden .mp3.part, AND the .transcript.md are all free, so
//! a capture can never overwrite a user note or an unrecovered orphan from
//! an earlier crash.

use chrono::NaiveDate;
use std::path::{Path, PathBuf};

pub struct CaptureNames {
    pub base: String,
    pub final_mp3: PathBuf,
    pub note_md: PathBuf,
    pub part: PathBuf,
    pub transcript_md: PathBuf,
}

pub fn dated_folder(root: &Path, date: NaiveDate) -> PathBuf {
    root.join(date.format("%Y").to_string())
        .join(date.format("%m").to_string())
}

pub fn base_name(date: NaiveDate, hour: u32, minute: u32, label: &str) -> String {
    format!("{} {hour:02}{minute:02} {label}", date.format("%Y-%m-%d"))
}

/// Ownership check for .mp3.part files: only bases matching Vault Buddy's
/// capture pattern `YYYY-MM-DD HHmm <label>` are ours to delete or rename.
/// Another tool's `.download.mp3.part` in a vault must never be touched.
pub fn is_capture_base(base: &str) -> bool {
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

/// Longest accepted rename title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;

/// Chars of the `YYYY-MM-DD HHmm ` prefix every capture base starts with.
const CAPTURE_PREFIX_CHARS: usize = 16;

/// Strip everything that can never reach a file name: path separators and
/// the rest of the Windows-reserved set, plus control characters. Then
/// trim whitespace and the trailing dots/spaces Windows rejects.
fn sanitize_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|c| {
            !matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') && !c.is_control()
        })
        .collect();
    cleaned.trim().trim_end_matches(['.', ' ']).to_string()
}

pub struct RenamePlan {
    pub dir: PathBuf,
    pub new_base: String,
    pub mp3_from: PathBuf,
    pub note_from: PathBuf,
}

/// Ownership filter shared by the rename and transcription commands: a
/// `.mp3` (any case) whose stem carries the capture-pattern prefix. Any
/// command that mints or moves files NEXT TO a given mp3 must pass this —
/// an arbitrary user mp3 must never grow a Vault Buddy sidecar or be
/// shuffled by our rename machinery.
pub fn is_capture_mp3(path: &Path) -> bool {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_mp3 = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3"));
    is_mp3 && is_capture_base(&stem)
}

/// A canonical containment match: the owning vault plus the canonical forms
/// of BOTH sides, so callers derive vault-relative paths from the same
/// canonical prefix (`\\?\`-form on Windows) instead of mixing raw and
/// resolved paths — mixing them makes `strip_prefix` fail (the `open_task`
/// precedent in task_commands.rs).
pub struct OwningVault<'v> {
    pub vault: &'v crate::discovery::Vault,
    pub vault_canonical: PathBuf,
    pub path_canonical: PathBuf,
}

/// The registered vault whose folder contains `path`, matched on CANONICAL
/// paths. `Path::starts_with` compares raw components without resolving
/// `..` or links, so a lexical prefix check accepts `<vault>\..\anywhere`
/// and symlink escapes (GAP-01). An unresolvable `path` is a rejection —
/// never a fallback to lexical matching; a registry entry whose own folder
/// can't resolve is skipped.
pub fn vault_owning_path<'v>(
    vaults: &'v [crate::discovery::Vault],
    path: &Path,
) -> Option<OwningVault<'v>> {
    let path_canonical = std::fs::canonicalize(path).ok()?;
    for vault in vaults {
        let Ok(vault_canonical) = std::fs::canonicalize(&vault.path) else {
            continue;
        };
        if path_canonical.starts_with(&vault_canonical) {
            return Some(OwningVault {
                vault,
                vault_canonical,
                path_canonical,
            });
        }
    }
    None
}

/// Pure planning for the post-save rename: validates ownership and the
/// title, derives the new base. Execution (reservation + rename_noreplace +
/// embed retarget) lives in the capture crate so the safety rails are shared
/// with the save path.
pub fn rename_plan(mp3: &Path, new_title: &str) -> Result<RenamePlan, String> {
    let stem = mp3
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_capture_mp3(mp3) {
        // Ownership filter: rename only files carrying our capture
        // pattern — never an arbitrary user mp3 handed in by mistake.
        return Err("Not a Vault Buddy capture file".to_string());
    }
    let mut title = sanitize_title(new_title);
    // The prompt prefills the full current base; a title that itself
    // starts with a capture prefix must not end up double-prefixed.
    if is_capture_base(&title) {
        title = title.chars().skip(CAPTURE_PREFIX_CHARS).collect();
    }
    if title.is_empty() {
        return Err("Title is empty after removing unusable characters".to_string());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!(
            "Title is too long (max {MAX_TITLE_CHARS} characters)"
        ));
    }
    let prefix: String = stem.chars().take(CAPTURE_PREFIX_CHARS).collect();
    let dir = mp3.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    let note_from = dir.join(format!("{stem}.md"));
    Ok(RenamePlan {
        dir,
        new_base: format!("{prefix}{title}"),
        mp3_from: mp3.to_path_buf(),
        note_from,
    })
}

/// The one collision-suffix scheme: attempt 1 is the plain base, attempt
/// N is `base (N)`. Reservation, the stop-time recheck, and the note
/// writer must all mint names from here so they can never diverge.
pub(crate) fn candidate(base: &str, attempt: u32) -> String {
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
        let transcript_md = dir.join(format!("{b}.transcript.md"));
        let part = dir.join(part_file_name(&b));
        if !final_mp3.exists() && !note_md.exists() && !transcript_md.exists() && !part.exists() {
            return CaptureNames {
                base: b,
                final_mp3,
                note_md,
                part,
                transcript_md,
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

/// Like `assert_root_inside_vault`, but for a folder that need NOT exist yet:
/// canonicalize the nearest EXISTING ancestor of `root` and require it under
/// the (canonical) vault. This catches a symlink/junction planted at any
/// existing ancestor even when the leaf is missing — `assert_root_inside_vault`
/// can't, because it can only canonicalize a path that exists. Pair with
/// `safe_recording_root` (which lexically rejects `..`/absolute), so the
/// not-yet-created tail can only descend, never escape the validated ancestor.
/// Use it to validate a configured folder up front (before saving or creating).
pub fn assert_path_inside_vault(vault_path: &Path, root: &Path) -> Result<(), String> {
    let vault =
        std::fs::canonicalize(vault_path).map_err(|e| format!("Cannot resolve vault path: {e}"))?;
    // Walk up to the first ancestor that exists on disk and resolve it. A
    // never-created leaf (`Tasks`, `Inbox/Tasks`) just resolves to the vault;
    // a symlinked ancestor resolves outside and is rejected.
    let mut ancestor = root;
    loop {
        if let Ok(resolved) = std::fs::canonicalize(ancestor) {
            return if resolved.starts_with(&vault) {
                Ok(())
            } else {
                Err("Configured folder resolves outside the vault".to_string())
            };
        }
        match ancestor.parent() {
            Some(parent) => ancestor = parent,
            // Ran out of ancestors without finding one inside the vault.
            None => return Err("Configured folder resolves outside the vault".to_string()),
        }
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
        let transcript_md = dir.join(format!("{b}.transcript.md"));
        if !final_mp3.exists() && !note_md.exists() && !transcript_md.exists() {
            return (final_mp3, note_md);
        }
    }
    unreachable!("suffix search always terminates")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Vault;
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
    fn reserve_includes_transcript_name() {
        let dir = tempfile::tempdir().unwrap();
        let names = reserve_names(dir.path(), "b");
        assert_eq!(names.transcript_md, dir.path().join("b.transcript.md"));
    }

    #[test]
    fn reserve_advances_when_transcript_exists() {
        let dir = tempfile::tempdir().unwrap();
        // a stray transcript sidecar for the plain base blocks it
        std::fs::write(dir.path().join("b.transcript.md"), "x").unwrap();
        let names = reserve_names(dir.path(), "b");
        assert_eq!(names.base, "b (2)");
    }

    #[test]
    fn reserve_final_advances_past_existing_transcript() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.transcript.md"), "x").unwrap();
        let (mp3, _) = reserve_final(dir.path(), "b");
        assert_eq!(mp3, dir.path().join("b (2).mp3"));
    }

    #[test]
    fn recovered_base_appends_marker() {
        assert_eq!(recovered_base("b"), "b (recovered)");
    }

    // Round-trip: every name this module can mint — plain, suffixed,
    // recovered — must be recognized as ours, and foreign names must not.
    #[test]
    fn is_capture_base_matches_every_generated_name_shape() {
        let base = base_name(date(), 14, 5, "Meeting");
        assert!(is_capture_base(&base));
        assert!(is_capture_base(&format!("{base} (2)")), "suffixed");
        assert!(is_capture_base(&recovered_base(&base)), "recovered");
        assert!(!is_capture_base("download"));
        assert!(!is_capture_base("2026-07-04 Meeting"), "missing HHmm");
        assert!(!is_capture_base(""));
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

    #[test]
    fn path_inside_vault_accepts_a_not_yet_created_leaf() {
        // A folder that doesn't exist yet resolves to the (existing) vault via
        // its nearest ancestor — accepted so a first-time save/create works.
        let vault = tempfile::tempdir().unwrap();
        assert!(assert_path_inside_vault(vault.path(), &vault.path().join("Tasks")).is_ok());
        assert!(assert_path_inside_vault(vault.path(), &vault.path().join("Inbox/Tasks")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn path_inside_vault_rejects_symlinked_ancestor_with_missing_leaf() {
        // `Link` is a symlink OUT of the vault and the leaf `Sub` doesn't exist:
        // assert_root_inside_vault couldn't see this (leaf missing), but the
        // nearest-existing-ancestor canonicalization catches the escape.
        let vault = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), vault.path().join("Link")).unwrap();
        let escaping = vault.path().join("Link").join("Sub");
        assert!(!escaping.exists());
        assert!(assert_path_inside_vault(vault.path(), &escaping).is_err());
    }

    #[test]
    fn path_inside_vault_rejects_when_vault_dir_is_gone() {
        // Vault folder removed from disk (stale registry): nothing under it can
        // be validated, so the nearest existing ancestor is outside the vault.
        let parent = tempfile::tempdir().unwrap();
        let vault = parent.path().join("gone-vault");
        // vault dir never created
        assert!(assert_path_inside_vault(&vault, &vault.join("Tasks")).is_err());
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

    #[test]
    fn rename_plan_keeps_the_capture_prefix_and_sibling_note() {
        let mp3 = Path::new("/v/Meetings/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, "Standup with Alice").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Standup with Alice");
        assert!(
            is_capture_base(&plan.new_base),
            "retitled base is still ours"
        );
        assert_eq!(plan.dir, Path::new("/v/Meetings/2026/07"));
        assert_eq!(plan.mp3_from, mp3);
        assert_eq!(
            plan.note_from,
            Path::new("/v/Meetings/2026/07/2026-07-04 1405 Meeting.md")
        );
    }

    #[test]
    fn rename_plan_strips_separators_and_control_characters() {
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, " a/b\\c:d*e?f\"g<h>i|j\u{7}k ").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 abcdefghijk");
    }

    #[test]
    fn rename_plan_keeps_unicode_and_interior_dots() {
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, "Café v1.2 ☕").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Café v1.2 ☕");
    }

    #[test]
    fn rename_plan_trims_trailing_dots_and_spaces() {
        // Windows rejects file names ending in dots or spaces.
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, "Notes.. . ").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Notes");
    }

    #[test]
    fn rename_plan_rejects_empty_after_sanitizing_and_overlong() {
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        assert!(rename_plan(mp3, "").is_err());
        assert!(rename_plan(mp3, "  /\\:  ").is_err());
        let long = "x".repeat(MAX_TITLE_CHARS + 1);
        assert!(rename_plan(mp3, &long).is_err());
        let just_fits = "x".repeat(MAX_TITLE_CHARS);
        assert!(rename_plan(mp3, &just_fits).is_ok());
    }

    #[test]
    fn rename_plan_strips_a_leading_capture_prefix_from_the_title() {
        // The prompt prefills the FULL current base; confirming unedited
        // (or editing the tail of it) must not double the prefix.
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, "2026-07-04 1405 Meeting").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Meeting");
        let plan = rename_plan(mp3, "2026-07-04 1405 Meeting with Alice").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Meeting with Alice");
    }

    #[test]
    fn rename_plan_refuses_foreign_files() {
        // Ownership filter: only our capture pattern may be renamed.
        assert!(rename_plan(Path::new("/v/2026/07/holiday.mp3"), "t").is_err());
        assert!(rename_plan(Path::new("/v/2026/07/2026-07-04 1405 Meeting.wav"), "t").is_err());
        assert!(rename_plan(Path::new("/v/2026/07/2026-07-04 Meeting.mp3"), "t").is_err());
    }

    #[test]
    fn rename_plan_on_a_suffixed_base_replaces_label_and_suffix() {
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting (2).mp3");
        let plan = rename_plan(mp3, "Standup").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Standup");
    }

    fn vault_at(dir: &Path) -> Vault {
        Vault {
            id: "v1".into(),
            name: "V".into(),
            path: dir.to_string_lossy().into_owned(),
            open: false,
        }
    }

    #[test]
    fn is_capture_mp3_requires_capture_stem_and_mp3_extension() {
        assert!(is_capture_mp3(Path::new("/v/2026-07-04 1405 Meeting.mp3")));
        // extension is case-insensitive, same as rename_plan's check
        assert!(is_capture_mp3(Path::new("/v/2026-07-04 1405 Meeting.MP3")));
        assert!(!is_capture_mp3(Path::new("/v/holiday-song.mp3")));
        assert!(!is_capture_mp3(Path::new("/v/2026-07-04 1405 Meeting.md")));
    }

    #[test]
    fn vault_owning_path_matches_a_file_inside_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        std::fs::create_dir(&vault_dir).unwrap();
        let mp3 = vault_dir.join("2026-07-04 1405 Meeting.mp3");
        std::fs::write(&mp3, "x").unwrap();
        let vaults = vec![vault_at(&vault_dir)];
        let owned = vault_owning_path(&vaults, &mp3).expect("inside the vault");
        assert_eq!(owned.vault.id, "v1");
        assert_eq!(owned.path_canonical, std::fs::canonicalize(&mp3).unwrap());
        assert_eq!(
            owned.vault_canonical,
            std::fs::canonicalize(&vault_dir).unwrap()
        );
    }

    #[test]
    fn vault_owning_path_rejects_a_dotdot_escape() {
        // GAP-01: Path::starts_with compares raw components, so
        // `<vault>/../outside.mp3` passed the old lexical prefix check while
        // pointing at a real file OUTSIDE the vault.
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        std::fs::create_dir(&vault_dir).unwrap();
        let outside = dir.path().join("2026-07-04 1405 Outside.mp3");
        std::fs::write(&outside, "x").unwrap();
        let sneaky = vault_dir.join("..").join("2026-07-04 1405 Outside.mp3");
        let vaults = vec![vault_at(&vault_dir)];
        assert!(sneaky.exists(), "the escape path must point at a real file");
        assert!(vault_owning_path(&vaults, &sneaky).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn vault_owning_path_rejects_a_symlink_escaping_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        std::fs::create_dir(&vault_dir).unwrap();
        let outside = dir.path().join("2026-07-04 1405 Outside.mp3");
        std::fs::write(&outside, "x").unwrap();
        let link = vault_dir.join("2026-07-04 1405 Linked.mp3");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let vaults = vec![vault_at(&vault_dir)];
        assert!(vault_owning_path(&vaults, &link).is_none());
    }

    #[test]
    fn vault_owning_path_rejects_missing_files_and_skips_dead_vaults() {
        let dir = tempfile::tempdir().unwrap();
        let vault_dir = dir.path().join("vault");
        std::fs::create_dir(&vault_dir).unwrap();
        let mp3 = vault_dir.join("2026-07-04 1405 Meeting.mp3");
        std::fs::write(&mp3, "x").unwrap();
        // an unresolvable path is a rejection, not a fallback to lexical matching
        assert!(vault_owning_path(&[vault_at(&vault_dir)], Path::new("/no/such.mp3")).is_none());
        // a registry entry whose folder is gone is skipped; a later vault still matches
        let dead = Vault {
            id: "dead".into(),
            name: "Dead".into(),
            path: dir.path().join("gone").to_string_lossy().into_owned(),
            open: false,
        };
        let vaults = vec![dead, vault_at(&vault_dir)];
        assert_eq!(vault_owning_path(&vaults, &mp3).unwrap().vault.id, "v1");
    }
}
