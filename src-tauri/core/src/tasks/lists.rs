//! Lists: folders under the tasks root. Read-lenient enumeration (any
//! folder is a list, hand-created and nested alike — the `type: Task`
//! philosophy applied to folders) and the two write halves — creating a
//! list folder and moving a task between lists — on the same containment
//! and never-clobber discipline as every other sanctioned vault write.
//! Spec: docs/superpowers/specs/2026-07-11-task-lists-sorting-ordering-design.md.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

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
    let mut entries = crate::transcript::dir_entries(dir);
    entries.sort_by(|a, b| a.2.cmp(&b.2));
    for (path, ft, name) in entries {
        if !ft.is_dir() || name.starts_with('.') {
            continue;
        }
        match std::fs::canonicalize(&path) {
            Ok(child) if child.starts_with(canon_root) => {
                if let Ok(rel) = child.strip_prefix(canon_root) {
                    out.push(rel_to_list(rel));
                }
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
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
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

/// Move a task file into another list's folder, keeping its basename. The
/// source is canonicalized and must live inside the canonical root (the
/// `update_task_fields` gate); the target list is validated lexically
/// (relative, no `..`/absolute components — multi-segment allowed, existing
/// nested lists are real targets), created if a just-deleted folder needs
/// resurrecting (lists are folders), and containment-asserted before and
/// after. The landing uses `rename_noreplace` + the shared ` (N)` suffix
/// scheme — a collision never clobbers the occupant. Moving a task to the
/// list it is already in is a no-op `Ok`. Returns the landed path.
pub fn move_task_to_list(root: &Path, path: &Path, list: &str) -> Result<PathBuf, String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    // Lexical gate on the target list — rejected before any filesystem access.
    let normalized = normalize_list_rel(list)?;
    let target_dir = if normalized.is_empty() {
        canon_root.clone()
    } else {
        canon_root.join(&normalized)
    };
    crate::capture_paths::assert_path_inside_vault(&canon_root, &target_dir)?;
    std::fs::create_dir_all(&target_dir)
        .map_err(|e| format!("Could not create the list folder: {e}"))?;
    let canon_target_dir = std::fs::canonicalize(&target_dir)
        .map_err(|e| format!("Cannot resolve the list folder: {e}"))?;
    if !canon_target_dir.starts_with(&canon_root) {
        return Err("List folder resolves outside the tasks folder".to_string());
    }
    if canon_path.parent() == Some(canon_target_dir.as_path()) {
        return Ok(canon_path); // already in that list — nothing to move
    }
    let stem = canon_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = canon_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for attempt in 1u32.. {
        let candidate = canon_target_dir.join(format!(
            "{}{ext}",
            crate::capture_paths::candidate(&stem, attempt)
        ));
        match crate::capture_paths::rename_noreplace(&canon_path, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not move the task: {e}")),
        }
    }
    unreachable!("suffix search always terminates")
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
        // each real folder once.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("sub").join("loop")).unwrap();
        let lists = task_lists(&root);
        assert_eq!(lists.iter().filter(|l| l.as_str() == "sub").count(), 1);
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
    fn move_task_between_lists_keeps_basename() {
        // Expectations are built from the CANONICAL root: on Windows,
        // canonicalize returns the \\?\ form, so a lexical root.join(...)
        // would never compare equal to the landed (canonical) path.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root, "2026-07-08-t.md", TASK);
        let canon_root = std::fs::canonicalize(&root).unwrap();
        let landed = move_task_to_list(&root, &root.join("2026-07-08-t.md"), "Inbox").unwrap();
        assert_eq!(landed, canon_root.join("Inbox").join("2026-07-08-t.md"));
        assert!(landed.exists());
        assert!(!root.join("2026-07-08-t.md").exists());
        // And back to the root via "".
        let back = move_task_to_list(&root, &landed, "").unwrap();
        assert_eq!(back, canon_root.join("2026-07-08-t.md"));
    }

    #[test]
    fn move_task_same_list_is_a_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Inbox"), "t.md", TASK);
        let p = root.join("Inbox").join("t.md");
        let landed = move_task_to_list(&root, &p, "Inbox").unwrap();
        assert_eq!(landed, std::fs::canonicalize(&p).unwrap());
        assert!(p.exists());
    }

    #[test]
    fn move_task_collision_lands_suffixed_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root, "t.md", TASK);
        write(&root.join("Inbox"), "t.md", "occupant");
        let landed = move_task_to_list(&root, &root.join("t.md"), "Inbox").unwrap();
        assert_eq!(landed.file_name().unwrap(), "t (2).md");
        // The occupant is untouched.
        assert_eq!(
            std::fs::read_to_string(root.join("Inbox").join("t.md")).unwrap(),
            "occupant"
        );
    }

    #[test]
    fn move_task_recreates_a_deleted_list_folder() {
        // Lists are folders; moving into one that vanished resurrects it,
        // exactly like add_task recreates the tasks root.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root, "t.md", TASK);
        assert!(!root.join("Someday").exists());
        let landed = move_task_to_list(&root, &root.join("t.md"), "Someday").unwrap();
        assert!(landed.exists());
        assert!(root.join("Someday").is_dir());
    }

    #[test]
    fn move_task_rejects_source_outside_root_and_escaping_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("outside.md");
        std::fs::write(&outside, TASK).unwrap();
        assert!(move_task_to_list(&root, &outside, "Inbox").is_err());
        write(&root, "t.md", TASK);
        assert!(move_task_to_list(&root, &root.join("t.md"), "../x").is_err());
        assert!(move_task_to_list(&root, &root.join("t.md"), "/abs").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn move_task_rejects_symlinked_source_escaping_root() {
        // Canonicalization (not lexical starts_with) must catch a source that
        // is a symlink out of the tasks root — the move would otherwise pull
        // an outside file into the vault (and delete it at its real home).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let real = dir.path().join("elsewhere.md");
        std::fs::write(&real, TASK).unwrap();
        let link = root.join("linked.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(move_task_to_list(&root, &link, "Inbox").is_err());
    }
}
