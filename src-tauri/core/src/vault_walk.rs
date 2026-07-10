//! The shared reparse-safe recursive vault walk. Every vault-scanning
//! domain (tasks, search) drives this ONE walk through a per-file callback,
//! so the escape/cycle discipline is single-sourced instead of hand-synced
//! copies that can drift.

use crate::transcript::dir_entries;
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
/// derived from it — is deterministic. Unreadable dirs degrade silently
/// (scan noise, the documented exception to the no-swallow rule).
pub(crate) fn walk_vault(canon_root: &Path, on_file: &mut dyn FnMut(&Path, &str) -> Flow) {
    let mut walked = HashSet::new();
    walk_dir(canon_root, canon_root, &mut walked, on_file);
}

fn walk_dir(
    dir: &Path,
    canon_root: &Path,
    walked: &mut HashSet<PathBuf>,
    on_file: &mut dyn FnMut(&Path, &str) -> Flow,
) -> Flow {
    if !walked.insert(dir.to_path_buf()) {
        return Flow::Continue; // already walked — reparse-point cycle guard
    }
    let mut entries = dir_entries(dir);
    entries.sort_by(|a, b| a.2.cmp(&b.2));
    for (path, ft, name) in entries {
        if ft.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            match std::fs::canonicalize(&path) {
                Ok(child) if child.starts_with(canon_root) => {
                    if let Flow::Stop = walk_dir(&child, canon_root, walked, on_file) {
                        return Flow::Stop;
                    }
                }
                _ => continue,
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
