//! The shared reparse-safe recursive vault walk. Every vault-scanning
//! domain (tasks, search) drives this ONE walk through a per-file callback,
//! so the escape/cycle discipline is single-sourced instead of hand-synced
//! copies that can drift. `dir_entries`/`dir_entries_checked` (the no-follow
//! dirent reader every level of the walk uses) live here too — this is their
//! natural owner, not `transcript.rs`, which only borrowed them for its own
//! `capture_mp3s` scan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Callback verdict: keep walking, or abort the entire walk (caps,
/// cancellation).
pub(crate) enum Flow {
    Continue,
    Stop,
}

/// Depth-first, name-ordered walk over every plain file under `canon_root`
/// (which the caller must have canonicalized). Discipline: dot-DIRECTORIES
/// are skipped (`.obsidian`, `.trash`, `.git`, …); a subdirectory is
/// descended only after canonicalizing it and confirming it still resolves
/// under `canon_root` — a symlink/junction escaping the vault is never
/// walked (the no-follow dirent type can't be trusted for a junction on
/// Windows); a walked-set breaks reparse cycles; symlinked files are
/// skipped (no-follow). Dot-FILES are passed through — per-domain filters
/// belong in the callback (search skips them, tasks deliberately considers
/// them). Entries are visited in name order so walk order — and anything
/// derived from it — is deterministic.
///
/// Returns the directories the walk could NOT fully enumerate — a failed
/// `read_dir` on the directory itself, or a subdirectory whose
/// `canonicalize` failed — each rendered as `"<path>: <error>"`. Empty in
/// the happy path (`Vec::new()` never allocates). The WALK itself stays
/// lenient either way: a bad directory is skipped, not fatal, so a
/// presentation caller (search, `list_tasks`'s VIEW mode) that ignores the
/// return value keeps today's exact degrade-silently behavior. A
/// guard-grade caller (`tasks::list_tasks_structural`) is the one that must
/// turn a non-empty result into a refusal — leaving that decision to the
/// caller is what let the file-level strict/lenient split (`ScanMode`) stay
/// entirely out of this domain-agnostic module.
pub(crate) fn walk_vault(
    canon_root: &Path,
    on_file: &mut dyn FnMut(&Path, &str) -> Flow,
) -> Vec<String> {
    let mut walked = HashSet::new();
    let mut unreadable = Vec::new();
    walk_dir(
        canon_root,
        canon_root,
        &mut walked,
        on_file,
        &mut unreadable,
    );
    unreadable
}

fn walk_dir(
    dir: &Path,
    canon_root: &Path,
    walked: &mut HashSet<PathBuf>,
    on_file: &mut dyn FnMut(&Path, &str) -> Flow,
    unreadable: &mut Vec<String>,
) -> Flow {
    if !walked.insert(dir.to_path_buf()) {
        return Flow::Continue; // already walked — reparse-point cycle guard
    }
    // Partial entries AND the failure, not one or the other: the CALLER decides
    // whether an incomplete walk matters (see the doc comment above), while the
    // entries that *were* readable keep flowing to presentation callers.
    let (mut entries, failure) = dir_entries_checked(dir);
    if let Some(e) = failure {
        unreadable.push(e);
    }
    entries.sort_by(|a, b| a.2.cmp(&b.2));
    for (path, ft, name) in entries {
        if ft.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            match std::fs::canonicalize(&path) {
                Ok(child) if child.starts_with(canon_root) => {
                    if let Flow::Stop = walk_dir(&child, canon_root, walked, on_file, unreadable) {
                        return Flow::Stop;
                    }
                }
                Ok(_) => {} // resolves outside canon_root — never walked, not a failure
                Err(e) => unreadable.push(format!("{}: {e}", path.display())),
            }
            continue;
        }
        if !ft.is_file() {
            continue; // symlinked files are not followed
        }
        if let Flow::Stop = on_file(&path, &name) {
            return Flow::Stop;
        }
    }
    Flow::Continue
}

/// `dir`'s entries as `(path, file_type, name)`, no-follow — `file_type()`
/// reads the dirent WITHOUT following symlinks, so a symlinked dir/junction can
/// never let a scan escape the vault.
///
/// Returns every readable entry PLUS the first failure, so a caller never has to
/// trade one for the other. `read_dir` succeeding only means the directory
/// opened: iteration and `file_type()` can each fail afterwards on a transient
/// network-vault I/O error. Silently dropping those would let a structural scan
/// treat a partial graph as complete, so a guard could approve a write past
/// hidden `parent-id` edges — but failing the WHOLE directory is equally wrong
/// in the other direction, costing presentation callers (the recordings browser,
/// list folders, search) every valid sibling because of one bad entry, where
/// they used to lose only that entry (Codex P2 ×2, PR #77).
///
/// So a per-entry failure is recorded and the scan CONTINUES over its siblings:
/// the view keeps everything reachable, and the guard still refuses because the
/// error is reported alongside.
pub(crate) fn dir_entries_checked(
    dir: &Path,
) -> (Vec<(PathBuf, std::fs::FileType, String)>, Option<String>) {
    let mut out = Vec::new();
    let read = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        // The directory itself would not open — there is nothing partial to keep.
        Err(e) => return (out, Some(format!("{}: {e}", dir.display()))),
    };
    let mut failure: Option<String> = None;
    for entry in read {
        match entry.and_then(|e| {
            let ft = e.file_type()?;
            Ok((e.path(), ft, e.file_name().to_string_lossy().into_owned()))
        }) {
            Ok(triple) => out.push(triple),
            // Keep the first reason; keep collecting the siblings.
            Err(e) => {
                failure.get_or_insert_with(|| format!("{}: {e}", dir.display()));
            }
        }
    }
    (out, failure)
}

/// Lenient wrapper: every entry that could be read, failures discarded — scan
/// noise, the documented exception to the no-swallow rule — for every caller
/// that doesn't need to know (the lists enumeration, `capture_mp3s`).
/// `walk_vault` above does NOT use this: it needs the failure to report what it
/// could not see.
pub(crate) fn dir_entries(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, String)> {
    dir_entries_checked(dir).0
}
