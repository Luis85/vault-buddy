//! Lists: folders under the tasks root. Read-lenient enumeration (any
//! folder is a list, hand-created and nested alike — the `type: Task`
//! philosophy applied to folders), naming (create/rename), and — in
//! `relocate` — the two write verbs that move task FILES between/out of
//! lists (`move_task_to_list`, `delete_task_list`), all on the same
//! containment and never-clobber discipline as every other sanctioned vault
//! write. Spec: docs/superpowers/specs/2026-07-11-task-lists-sorting-ordering-design.md.
//!
//! `relocate` is split out purely for the crate's nonblank-LOC cap — a pure
//! move, not a rewrite; see its own doc comment.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

mod relocate;
pub use relocate::{delete_task_list, move_task_to_list, DeleteListError, DeleteListOutcome};

/// Every directory under `root` (including empty ones — a just-created list
/// must appear before its first task), as relative `/`-joined paths, name
/// ordered per directory (parents before children). Same walk discipline as
/// `vault_walk`: descend only after canonicalizing and confirming containment
/// (a symlinked/junctioned folder never escapes), a walked-set bounds reparse
/// cycles, dot-directories are skipped (they'd be invisible to the task walk
/// anyway). Missing/unresolvable root → empty, never an error.
pub fn task_lists(root: &Path) -> Vec<String> {
    let Ok(canon_root) = std::fs::canonicalize(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut walked = HashSet::new();
    collect_dirs(&canon_root, &canon_root, &mut walked, &mut out);
    out
}

fn collect_dirs(
    dir: &Path,
    canon_root: &Path,
    walked: &mut HashSet<PathBuf>,
    out: &mut Vec<String>,
) {
    if !walked.insert(dir.to_path_buf()) {
        return; // reparse-point cycle guard, same as vault_walk
    }
    // Emit this directory as a list — AFTER the cycle-guard insert above, and
    // never the root itself (whose relative path is ""). Emitting here rather
    // than from the parent's loop is what keeps a Windows junction/reparse dir
    // that canonicalizes back to the root (or an already-walked ancestor) from
    // leaking a blank "" or a duplicate parent into the list picker: on the
    // second visit the guard returns above, before this push (Codex, PR #53
    // re-review). A symlinked dir never reaches here on Unix — dir_entries'
    // file_type() is no-follow, so is_dir() already filtered it out below; the
    // leak is Windows-junction-only, where is_dir() is true for a reparse dir.
    if dir != canon_root {
        if let Ok(rel) = dir.strip_prefix(canon_root) {
            out.push(rel_to_list(rel));
        }
    }
    let mut entries = crate::vault_walk::dir_entries(dir);
    entries.sort_by(|a, b| a.2.cmp(&b.2));
    for (path, ft, name) in entries {
        if !ft.is_dir() || name.starts_with('.') {
            continue;
        }
        match std::fs::canonicalize(&path) {
            Ok(child) if child.starts_with(canon_root) => {
                collect_dirs(&child, canon_root, walked, out);
            }
            _ => continue,
        }
    }
}

/// A root-relative dir as the list identity: `/`-joined on every platform.
fn rel_to_list(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Write-side name gate (the tags posture: read anything, create only what
/// validates): trimmed non-empty, single path segment (no `/` or `\`), no
/// leading dot (dot-dirs are skipped by every walk — a `.foo` list would be
/// invisible the moment it was created).
pub fn is_valid_list_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && !n.contains('/') && !n.contains('\\') && !n.starts_with('.')
}

/// Normalize a caller-supplied list identity into `/`-joined `Normal`
/// components (the safe_recording_root component rule): `..`, absolute
/// paths, and Windows drive-prefixed forms are rejected; `CurDir` segments
/// drop out; `""` — the tasks root — is valid and normalizes to itself.
/// Shared by the move below and services::add_task so the two write gates
/// can never disagree on what a list path is.
pub fn normalize_list_rel(list: &str) -> Result<String, String> {
    if list.contains('\\') && list.contains(':') {
        return Err("List path must stay inside the tasks folder".to_string());
    }
    let mut parts: Vec<String> = Vec::new();
    for c in Path::new(list).components() {
        match c {
            Component::Normal(s) => {
                let seg = s.to_string_lossy();
                // A dot-prefixed segment names a hidden folder that every task
                // walk (task_lists + vault_walk) skips, so a task landed there
                // would vanish from the view. Reject it the same way
                // create_task_list does (is_valid_list_name) so this shared
                // normalizer — used by add_task's config default,
                // move_task_to_list, and set_task_lists_config — stays
                // consistent with the create gate on ALL segments, not just a
                // single-segment create (Codex, PR #53).
                if seg.starts_with('.') {
                    return Err("List folders cannot start with a dot.".to_string());
                }
                parts.push(seg.into_owned());
            }
            Component::CurDir => {}
            _ => return Err("List path must stay inside the tasks folder".to_string()),
        }
    }
    Ok(parts.join("/"))
}

/// Create the list folder `root/<name>` (creating `root` itself if needed).
/// Idempotent — an existing folder is success, not a clobber (a folder is not
/// data). Containment is asserted BEFORE creation (a symlink/junction at any
/// existing ancestor is caught while the leaf doesn't exist yet) and AFTER
/// (closing the swap-in race), the document-import discipline. Returns the
/// trimmed list name actually used.
pub fn create_task_list(root: &Path, name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if !is_valid_list_name(trimmed) {
        return Err(
            "List names need at least one character and cannot contain / or \\ or start with a dot."
                .to_string(),
        );
    }
    let target = root.join(trimmed);
    crate::capture_paths::assert_path_inside_vault(root, &target)?;
    std::fs::create_dir_all(&target).map_err(|e| format!("Could not create the list: {e}"))?;
    crate::capture_paths::assert_root_inside_vault(root, &target)?;
    Ok(trimmed.to_string())
}

/// Rename a list folder's leaf to `to` (a single valid segment) at the same
/// parent, moving every contained task with it. Refuses a collision (never
/// clobber). Returns the new `/`-joined relative name.
pub fn rename_task_list(root: &Path, from: &str, to: &str) -> Result<String, String> {
    if !is_valid_list_name(to) {
        return Err(
            "List names need at least one character and cannot contain / or \\ or start with a dot."
                .to_string(),
        );
    }
    let from_rel = normalize_list_rel(from)?;
    if from_rel.is_empty() {
        return Err("The tasks root is not a list and cannot be renamed.".to_string());
    }
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let from_dir = canon_root.join(&from_rel);
    if !from_dir.is_dir() {
        return Err("That list no longer exists — reopen the list to refresh.".to_string());
    }
    // The leaf exists (just confirmed above), so this canonicalizes from_dir
    // itself and rejects a symlink/junction escaping the vault — the same
    // source containment move_task_to_list requires. Without it, is_dir()
    // alone (which follows symlinks) let a symlinked list leaf reach
    // std::fs::rename below; POSIX rename() never dereferences the source's
    // final component, so that call would rename the symlink ENTRY in place
    // (still pointing outside) before the destination-side
    // assert_root_inside_vault happened to also reject it — an
    // undocumented accident, and only after the mutation already happened.
    crate::capture_paths::assert_path_inside_vault(&canon_root, &from_dir)?;
    // New rel = the from's parent joined with the `to` leaf.
    let parent = Path::new(&from_rel).parent();
    let new_rel = match parent
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
    {
        Some(p) => format!("{p}/{}", to.trim()),
        None => to.trim().to_string(),
    };
    let to_dir = canon_root.join(&new_rel);
    crate::capture_paths::assert_path_inside_vault(&canon_root, &to_dir)?;
    if to_dir.exists() {
        return Err(format!("A list named \"{}\" already exists.", to.trim()));
    }
    std::fs::rename(&from_dir, &to_dir).map_err(|e| format!("Could not rename the list: {e}"))?;
    crate::capture_paths::assert_root_inside_vault(&canon_root, &to_dir)?;
    Ok(new_rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(name), body).unwrap();
    }

    const TASK: &str = "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\n---\n";

    #[test]
    fn task_lists_missing_root_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(task_lists(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn task_lists_enumerates_flat_and_nested_including_empty() {
        // Empty folders count — a just-created list must appear before its
        // first task. Name order per directory, parents before children.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Waiting")).unwrap();
        std::fs::create_dir_all(root.join("work/q3")).unwrap();
        std::fs::create_dir_all(root.join("Inbox")).unwrap();
        write(root, "top.md", TASK);
        let lists = task_lists(root);
        assert_eq!(lists, vec!["Inbox", "Waiting", "work", "work/q3"]);
    }

    #[test]
    fn task_lists_skips_dot_directories() {
        // .trash/.obsidian are invisible to the task walk — surfacing them as
        // lists would offer targets no view can show.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".trash")).unwrap();
        std::fs::create_dir_all(root.join("Real")).unwrap();
        assert_eq!(task_lists(root), vec!["Real"]);
    }

    #[cfg(unix)]
    #[test]
    fn task_lists_does_not_follow_symlinked_dir_out_of_root() {
        // The dir enumeration must not escape the tasks folder through a
        // symlink — same guard as the task walk.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("sub")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
        assert!(task_lists(&root).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn task_lists_terminates_on_a_directory_cycle() {
        // A link back to an ancestor inside the root must terminate and list
        // each real folder once, never a blank ("" = the root) or a duplicate.
        // On Unix the symlinked `loop` is filtered by dir_entries' no-follow
        // file_type() (is_dir() is false), so it never reaches the walk; the
        // emit-after-cycle-guard restructure is what protects the equivalent
        // Windows junction, where a reparse dir reports is_dir()==true and
        // canonicalizes back to the root (Codex, PR #53 re-review).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("sub").join("loop")).unwrap();
        let lists = task_lists(&root);
        assert_eq!(lists, vec!["sub"]);
        assert!(
            !lists.iter().any(|l| l.is_empty()),
            "a reparse loop back to the root must never emit a blank list"
        );
    }

    #[test]
    fn list_name_validation() {
        assert!(is_valid_list_name("Inbox"));
        assert!(is_valid_list_name("Next actions"));
        for bad in ["", "   ", "a/b", "a\\b", ".hidden", ".", ".."] {
            assert!(!is_valid_list_name(bad), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn normalize_list_rel_rejects_dot_prefixed_segments() {
        // A dot-prefixed folder is skipped by every task walk, so a task
        // placed there would vanish from the view — the shared normalizer
        // (add_task's config default, move, set_task_lists_config) must reject
        // it on EVERY segment the same way create_task_list does, not only a
        // single-segment create (Codex, PR #53).
        for bad in [".hidden", "Work/.hidden", ".git/objects", "a/.b/c"] {
            assert!(normalize_list_rel(bad).is_err(), "{bad:?} must be rejected");
        }
        // Non-dot names still normalize, including nested and the root.
        assert_eq!(normalize_list_rel("Inbox").unwrap(), "Inbox");
        assert_eq!(normalize_list_rel("Work/Q3").unwrap(), "Work/Q3");
        assert_eq!(normalize_list_rel("").unwrap(), "");
        assert_eq!(normalize_list_rel("./Inbox").unwrap(), "Inbox"); // CurDir drops out
    }

    #[test]
    fn create_task_list_creates_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(create_task_list(&root, " Inbox ").unwrap(), "Inbox");
        assert!(root.join("Inbox").is_dir());
        // Existing folder is success — a folder is not data to clobber.
        assert_eq!(create_task_list(&root, "Inbox").unwrap(), "Inbox");
    }

    #[test]
    fn create_task_list_rejects_invalid_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        for bad in ["", "a/b", "a\\b", ".hidden"] {
            assert!(create_task_list(&root, bad).is_err(), "{bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn create_task_list_rejects_symlinked_root_escape() {
        // The tasks root itself may be a symlink out of the vault-shaped
        // parent; the pre-create assert must catch it (the leaf doesn't exist
        // yet, so only canonicalizing the nearest ancestor can see the link).
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let root = dir.path().join("vault").join("Tasks");
        std::fs::create_dir_all(root.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &root).unwrap();
        // assert_path_inside_vault(root, target) canonicalizes ancestors of
        // target against ROOT itself — a root that IS a link still resolves
        // under itself, so guard at the caller level: services passes the
        // VAULT path. Here we mirror that: vault-level containment.
        let target = root.join("Inbox");
        assert!(
            crate::capture_paths::assert_path_inside_vault(&dir.path().join("vault"), &target)
                .is_err()
        );
    }

    #[test]
    fn rename_task_list_moves_folder_and_refuses_existing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Inbox"), "t.md", TASK);
        assert_eq!(rename_task_list(&root, "Inbox", "Later").unwrap(), "Later");
        assert!(root.join("Later").join("t.md").exists());
        assert!(!root.join("Inbox").exists());
        // Same-parent nesting: renames the leaf only.
        write(&root.join("work/q3"), "x.md", TASK);
        assert_eq!(rename_task_list(&root, "work/q3", "q4").unwrap(), "work/q4");
        assert!(root.join("work/q4").join("x.md").exists());
        // Refuse an invalid name and a collision (never clobber).
        assert!(rename_task_list(&root, "Later", "a/b").is_err());
        std::fs::create_dir_all(root.join("Taken")).unwrap();
        assert!(rename_task_list(&root, "Later", "Taken").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rename_task_list_rejects_symlinked_source_escaping_root() {
        // Before the source-side containment fix, `from_dir.is_dir()` alone
        // gated the rename — and is_dir() follows symlinks, so a list-named
        // entry that is a symlink out of the tasks root passed it straight
        // through to std::fs::rename. POSIX rename() does not dereference
        // the SOURCE's final component, so that call renamed the symlink
        // ENTRY itself (from "Linked" to "Renamed", still inside root, still
        // pointing outside) — a real mutation. The function still ended up
        // returning Err, but only because the pre-existing DESTINATION
        // assert (assert_root_inside_vault) canonicalizes the *newly
        // renamed* entry, follows the symlink, and rejects it — an
        // undocumented accident, and only AFTER the rename() had already
        // executed. A resolved containment check on the SOURCE (mirroring
        // move_task_rejects_symlinked_source_escaping_root) must reject
        // before any rename() runs, so neither name is ever touched.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("Linked")).unwrap();
        assert!(rename_task_list(&root, "Linked", "Renamed").is_err());
        // The outside target directory itself is untouched — not
        // renamed/moved (rename() never dereferences the source leaf).
        assert!(outside.is_dir());
        // And no rename() must have executed at all: the source entry must
        // still be exactly where it was, under its original name, and no
        // destination entry may have been created. This is the assertion
        // that actually fails pre-fix — the accidental post-check above
        // only proves the FUNCTION reports Err, not that nothing moved.
        assert!(
            root.join("Linked").symlink_metadata().is_ok(),
            "the source symlink must not be renamed away on a rejected rename"
        );
        assert!(!root.join("Renamed").exists());
    }
}
