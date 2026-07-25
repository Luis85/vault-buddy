//! Structural task writes that go beyond a field patch: permanently deleting
//! a task file, and duplicating one into a fresh copy. Same never-clobber /
//! containment discipline as `disk.rs`'s surgical writers, which these build
//! on (`set_fields`, `task_basename`, the collision-safe note writer).

use super::writer::set_fields;
use crate::yaml_scalar::yaml_quote;
use std::path::{Path, PathBuf};

/// Permanently delete a task file — the app's ONLY destructive vault write.
/// Canonicalizes `root` and `path` and requires containment (a symlink at the
/// file or folder can't be seen lexically), THEN re-reads the file and requires
/// it to be a `type: Task` document before removing. Task folders may
/// legitimately hold foreign files, and a listed row could be swapped for a
/// non-task file at the same path before the delete lands — so the document is
/// re-validated here, the same posture the move/field writers get from
/// `set_fields`' `type: Task` precondition (Codex P1, PR #76). The validated
/// bytes and the file's identity are taken from ONE open handle, and that
/// identity is re-verified at unlink time (`handle_identity` + `id_differs`) so
/// the object we remove is provably the object we validated — a swap during the
/// validation window is refused, not deleted. A missing file surfaces as an error
/// (the row the user clicked should exist), never a silent success.
pub fn delete_task(root: &Path, path: &Path) -> Result<(), String> {
    // A destructive write must NEVER follow a symlink at the leaf: if `path` is
    // a symlink to a different valid Task inside the root, canonicalize would
    // resolve to that target, `is_task` would pass on it, and `remove_file`
    // would delete the OTHER task instead of the entry the user clicked. Reject
    // a symlink at the original path (no-follow lstat) before any resolution
    // (Codex P1, PR #76). A missing path errors here too.
    let leaf =
        std::fs::symlink_metadata(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if leaf.file_type().is_symlink() {
        return Err("Refusing to delete: the task path is a symlink".to_string());
    }
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    // Read the bytes we validate AND the file's identity from ONE open handle, so
    // they provably describe the same file (no read-vs-stat race). The handle is
    // dropped at the end of this block, before any unlink, because Windows refuses
    // to remove a file that still has an open handle without FILE_SHARE_DELETE.
    let validated_id = {
        use std::io::Read as _;
        let mut file =
            std::fs::File::open(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
        let id = handle_identity(&file);
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|e| format!("Cannot read task: {e}"))?;
        if !super::doc::is_task(&content) {
            return Err("Refusing to delete: not a type: Task document".to_string());
        }
        id
    };
    // Verify stable file identity at unlink time (Codex P1, PR #76): a no-follow
    // re-stat rejects a swap to a symlink, and re-reading the file's identity
    // rejects a swap to any different file — both immediately before the
    // irreversible remove, so the object we unlink is provably the one we
    // validated. The residual re-open->remove window is irreducible in portable
    // std, which has no unlink-by-handle / funlinkat; on a single-user desktop
    // delete_task runs at machine speed with no user pause inside it (the confirm
    // happens client-side, before the IPC call), so the window is microseconds —
    // documented as a bounded gap (docs/Gaps.md, GAP-79).
    let now =
        std::fs::symlink_metadata(&canon_path).map_err(|e| format!("Cannot re-check task: {e}"))?;
    if now.file_type().is_symlink() {
        return Err(SWAPPED_MSG.to_string());
    }
    // canon_path is not a symlink per the check above, so this re-open cannot
    // follow one; a differing identity means the entry was swapped during the
    // validation window.
    let now_id = std::fs::File::open(&canon_path)
        .ok()
        .and_then(|f| handle_identity(&f));
    if id_differs(validated_id, now_id) {
        return Err(SWAPPED_MSG.to_string());
    }
    std::fs::remove_file(&canon_path).map_err(|e| format!("Cannot delete task: {e}"))
}

const SWAPPED_MSG: &str =
    "Refusing to delete: the task file changed on disk since it was validated; reopen it and retry";

/// Best-effort stable identity of an OPEN file — `(device, inode)` on Unix,
/// `(volume serial, file index)` on Windows — used by `delete_task` to detect a
/// swap between validating a task's bytes and unlinking it. `None` when the
/// platform can't supply it; callers treat a `None` on either side as "can't
/// prove a swap" (see `id_differs`), so missing identity never blocks a legitimate
/// delete (Codex P1 TOCTOU, PR #76).
#[cfg(unix)]
fn handle_identity(file: &std::fs::File) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = file.metadata().ok()?;
    Some((m.dev(), m.ino()))
}

#[cfg(windows)]
fn handle_identity(file: &std::fs::File) -> Option<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    // std's Metadata::file_index()/volume_serial_number() are still unstable
    // (windows_by_handle, rust-lang/rust#63010), so read the identity through
    // GetFileInformationByHandle — windows-sys is already a core dependency (it
    // backs rename_noreplace's MoveFileExW fallback).
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `file` owns a valid handle for the duration of the call, and `info`
    // is a live, correctly-typed out-parameter. RawHandle and windows-sys HANDLE
    // are both `*mut c_void`, so the handle passes with no cast (the MoveFileExW
    // call site in capture_paths.rs relies on the same pointer-alias identity).
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) };
    if ok == 0 {
        return None;
    }
    let index = ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64;
    Some((info.dwVolumeSerialNumber as u64, index))
}

#[cfg(not(any(unix, windows)))]
fn handle_identity(_file: &std::fs::File) -> Option<(u64, u64)> {
    None
}

/// True only when two identity snapshots PROVE the path now points at a different
/// file. A `None` on either side (identity unavailable) never fires, so the guard
/// fires on a proven swap, never on absence of proof.
fn id_differs(a: Option<(u64, u64)>, b: Option<(u64, u64)>) -> bool {
    matches!((a, b), (Some(x), Some(y)) if x != y)
}

/// Duplicate a task file into the same folder, faithfully: the source bytes
/// are copied (body, extra frontmatter, description, unknown keys all
/// preserved), then only the identity fields are rewritten surgically via
/// `set_fields` — title → "<title> (copy)", status → new, and the id handled so
/// no two tasks ever share one. `id_property` is the vault's configured id key
/// ONLY when it is a valid, non-reserved property (else `None`, so a
/// foreign/reserved key is never touched). When present, the copy's id is
/// REGENERATED (`ids_enabled == true`) or STRIPPED (`false`): stripping matters
/// because leaving the source id on the copy would collide with the original if
/// the user later re-enables IDs, and the ensure-id path never overwrites an
/// existing value (Codex P2, PR #76). Written through the collision-safe
/// never-clobber writer, so a name clash takes the ` (N)` suffix. `today` names
/// the new file (clock-free core).
pub fn duplicate_task(
    root: &Path,
    path: &Path,
    today: &str,
    id_property: Option<&str>,
    ids_enabled: bool,
) -> Result<PathBuf, String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    // Title fallback matches list_tasks' display: an untitled hand-authored
    // task shows its filename stem, so the copy must too — including a stem that
    // is not valid UTF-8, where the list uses a LOSSY conversion (Codex P2,
    // PR #76). `to_str()` would drop such a stem to "".
    let stem = canon_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Decode the title's YAML scalar form from the RAW value (quotes/escapes
    // intact) so a hand-authored quoted / commented / escaped title copies
    // faithfully — `"café"` → `café`, `'it''s ready'` → `it's ready` — not
    // with the syntax baked in. Reading via `note_field` first would strip the
    // quotes and decode only `\"`/`\\`, losing `\u` and a `#` inside quotes
    // (Codex P2, PR #76).
    let title = crate::capture_note::raw_scalar_field(&content, "title")
        .map(super::description::decode_scalar_lenient)
        .unwrap_or(stem);
    let new_title = format!("{title} (copy)");
    let quoted = yaml_quote(&new_title);
    // A fresh id when IDs are on; `None` strips the configured property so the
    // copy can never inherit the source id.
    let new_id = if ids_enabled {
        Some(super::id::new_task_id())
    } else {
        None
    };
    // Resolve the id key to its ON-DISK casing (case-insensitive lookup) so the
    // case-sensitive `set_fields` matches a differently-cased existing id — else
    // a strip misses `Task-ID:` and a regenerate inserts a SECOND, differently-
    // cased id (Codex P2, PR #76). With no id line on disk, add one under the
    // configured name ONLY when regenerating.
    let id_key: Option<String> =
        id_property.and_then(
            |prop| match super::parse::frontmatter_scalar_ci(&content, prop) {
                // A block-valued (nested map/list under the key) OR flow-valued
                // (inline `{..}` / `[..]`) id property is the USER's frontmatter,
                // not a scalar id — set_fields would consume the block, or
                // rewrite the single flow line, and destroy it either way. Leave
                // it untouched (Codex P2, PR #76); update_task_fields' ensure_id
                // never rewrites a present value, so only duplicate is exposed.
                Some((on_disk, _))
                    if super::parse::key_opens_block(&content, &on_disk)
                        || super::parse::key_opens_flow(&content, &on_disk) =>
                {
                    None
                }
                Some((on_disk, _)) => Some(on_disk),
                None => new_id.as_ref().map(|_| prop.to_string()),
            },
        );
    let mut updates: Vec<(&str, Option<&str>)> =
        vec![("title", Some(quoted.as_str())), ("status", Some("new"))];
    if let Some(key) = id_key.as_deref() {
        updates.push((key, new_id.as_deref()));
    }
    let rewritten =
        set_fields(&content, &updates).ok_or("Source is not a valid type: Task document")?;
    let parent = canon_path.parent().unwrap_or(&canon_root);
    let target = parent.join(format!("{}.md", super::task_basename(&new_title, today)));
    crate::capture_note::write_note_collision_safe(&target, &rewritten)
        .map_err(|e| format!("Cannot write duplicate: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_task_removes_a_task_refuses_outside_and_refuses_a_non_task() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let p = root.join("t.md");
        std::fs::write(&p, "---\ntype: Task\nstatus: new\ntitle: X\n---\n").unwrap();
        // Happy path: a real task is removed.
        assert!(delete_task(&root, &p).is_ok());
        assert!(!p.exists());
        // A path outside the tasks root is refused (write a sibling file to delete).
        let outside = dir.path().join("outside.md");
        std::fs::write(&outside, "x").unwrap();
        assert!(delete_task(&root, &outside).is_err());
        assert!(outside.exists());
        // A FOREIGN (non-task) file INSIDE the tasks root is refused — task folders
        // may legitimately hold non-task files, and this first destructive write
        // must never remove one (Codex P1, PR #76).
        let foreign = root.join("notes.md");
        std::fs::write(&foreign, "# just some notes, not a task\n").unwrap();
        assert!(delete_task(&root, &foreign).is_err());
        assert!(foreign.exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_task_refuses_a_symlink_leaf_and_never_deletes_the_target() {
        // A symlink whose target is a real Task inside the root must NOT be
        // followed: canonicalize would resolve to the target and remove_file
        // would delete the WRONG task. delete_task must refuse, leaving the real
        // task (and the symlink) intact (Codex P1, PR #76).
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let real = root.join("real.md");
        std::fs::write(&real, "---\ntype: Task\nstatus: new\ntitle: Real\n---\n").unwrap();
        let link = root.join("link.md");
        symlink(&real, &link).unwrap();
        assert!(delete_task(&root, &link).is_err());
        assert!(real.exists(), "the symlink's target task must survive");
        assert!(
            link.symlink_metadata().is_ok(),
            "the symlink itself is untouched"
        );
    }

    #[test]
    fn identity_flags_distinct_files_and_matches_the_same_file() {
        // delete_task re-reads the file's identity immediately before unlink and
        // refuses on a mismatch (a swap during the validation window). Two distinct
        // files must read as different; the SAME file opened twice must NOT — else
        // every delete would falsely refuse (Codex P1 TOCTOU, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let ida1 = handle_identity(&std::fs::File::open(&a).unwrap());
        let ida2 = handle_identity(&std::fs::File::open(&a).unwrap());
        let idb = handle_identity(&std::fs::File::open(&b).unwrap());
        assert!(ida1.is_some(), "identity is available on this platform");
        assert!(
            !id_differs(ida1, ida2),
            "the same file opened twice is not a swap"
        );
        assert!(id_differs(ida1, idb), "two distinct files are a swap");
    }

    #[test]
    fn duplicate_task_copies_body_and_resets_identity_with_fresh_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: done\ntitle: \"Buy milk\"\ncreated: 2026-07-01\ntask-id: aaa11111\ndue: 2026-07-10\n---\n\nThe body stays.\n",
        )
        .unwrap();
        let new = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
        assert!(new.exists() && new != src);
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("title: \"Buy milk (copy)\""));
        assert!(out.contains("status: new")); // reset
        assert!(out.contains("The body stays.")); // body preserved
        assert!(out.contains("due: 2026-07-10")); // other fields preserved
        assert!(!out.contains("task-id: aaa11111")); // id regenerated, not shared
        assert!(out.contains("task-id: ")); // a fresh id is present
                                            // IDs off → the configured id property is STRIPPED, not inherited: leaving
                                            // the source id on the copy would collide with the original if IDs are
                                            // later re-enabled, and ensure-id never overwrites an existing value
                                            // (Codex P2, PR #76).
        let new2 = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), false).unwrap();
        let out2 = std::fs::read_to_string(&new2).unwrap();
        assert!(!out2.contains("task-id")); // stripped when ids are off
        assert!(out2.contains("title: \"Buy milk (copy)\""));
    }

    #[test]
    fn duplicate_task_uses_the_filename_stem_when_the_source_has_no_title() {
        // An untitled hand-authored task lists under its filename stem, so the copy
        // must too — not an empty " (copy)" (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("My hand note.md");
        std::fs::write(&src, "---\ntype: Task\nstatus: new\n---\n\nbody\n").unwrap();
        let new = duplicate_task(&root, &src, "2026-07-24", None, false).unwrap();
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("title: \"My hand note (copy)\""));
    }

    #[test]
    fn duplicate_task_decodes_a_hand_authored_quoted_title() {
        // A single-quoted title must copy as its DECODED value + " (copy)", not
        // with the YAML quotes/escape baked in (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: new\ntitle: 'it''s ready'\n---\n",
        )
        .unwrap();
        let new = duplicate_task(&root, &src, "2026-07-25", None, false).unwrap();
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("title: \"it's ready (copy)\""), "got: {out}");
    }

    #[test]
    fn duplicate_task_decodes_an_escaped_double_quoted_title() {
        // `note_field` only handles `\"`/`\\`; a `\u` escape must still decode
        // (café), which requires reading the RAW quoted value (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: new\ntitle: \"caf\\u00e9\"\n---\n",
        )
        .unwrap();
        let new = duplicate_task(&root, &src, "2026-07-25", None, false).unwrap();
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("title: \"café (copy)\""), "got: {out}");
    }

    #[cfg(unix)]
    #[test]
    fn duplicate_task_uses_a_lossy_stem_for_a_non_utf8_untitled_filename() {
        // An untitled task whose filename is not valid UTF-8 lists under a LOSSY
        // stem, so the copy's title must too — `to_str()` would drop it to an
        // empty " (copy)" (Codex P2, PR #76).
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let name = std::ffi::OsStr::from_bytes(b"bad\xffname.md");
        let src = root.join(name);
        std::fs::write(&src, "---\ntype: Task\nstatus: new\n---\n").unwrap();
        let new = duplicate_task(&root, &src, "2026-07-24", None, false).unwrap();
        let out = std::fs::read_to_string(&new).unwrap();
        // U+FFFD replaces the bad byte; the title is non-empty and " (copy)".
        assert!(out.contains("(copy)\""));
        assert!(
            !out.contains("title: \" (copy)\""),
            "the stem must not be dropped to empty"
        );
    }

    #[test]
    fn duplicate_task_rewrites_the_on_disk_id_key_casing() {
        // Source key `Task-ID:`, configured property `task-id`: the copy must
        // rewrite/strip THAT on-disk key, never insert a second differently-cased
        // id or miss the strip (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: new\ntitle: X\nTask-ID: aaa11111\n---\n",
        )
        .unwrap();
        // IDs on → the existing `Task-ID:` line is rewritten to a fresh id (no
        // second `task-id:` line, exactly one id key remains).
        let on = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
        let out_on = std::fs::read_to_string(&on).unwrap();
        assert!(!out_on.contains("aaa11111")); // old id replaced
        assert_eq!(out_on.matches("Task-ID:").count(), 1); // rewritten in place
        assert!(!out_on.to_lowercase().contains("task-id: aaa")); // no stale value
        assert_eq!(out_on.to_lowercase().matches("task-id:").count(), 1); // exactly one id key
                                                                          // IDs off → the `Task-ID:` line is stripped despite the casing mismatch.
        let off = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), false).unwrap();
        let out_off = std::fs::read_to_string(&off).unwrap();
        assert!(!out_off.to_lowercase().contains("task-id"));
    }

    #[test]
    fn duplicate_task_leaves_a_block_valued_id_property_untouched() {
        // A block-valued id property (nested map under the key) is the user's
        // frontmatter, not a scalar id — duplicating must not consume/delete the
        // indented block (Codex P2, PR #76), mirroring update_task_fields.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: new\ntitle: X\ntask-id:\n  source: jira\n  ref: ABC-1\n---\n",
        )
        .unwrap();
        // IDs on: the block-valued property is NOT treated as a scalar id, so the
        // nested lines survive and no fresh id is jammed onto the block key.
        let new = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("source: jira") && out.contains("ref: ABC-1"));
        assert!(out.contains("title: \"X (copy)\""));
    }

    #[test]
    fn duplicate_task_leaves_a_flow_valued_id_property_untouched() {
        // A FLOW-valued id property (inline map/list on one line) is the user's
        // frontmatter, not a scalar id. Unlike the block case it has no indented
        // lines, so key_opens_block misses it — key_opens_flow catches it. Both
        // ids-on (no fresh id jammed over it) and ids-off (not stripped) must
        // leave the inline structure intact (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: done\ntitle: X\ntask-id: {source: jira, ref: ABC-1}\n---\n",
        )
        .unwrap();
        let on = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
        let out_on = std::fs::read_to_string(&on).unwrap();
        assert!(out_on.contains("task-id: {source: jira, ref: ABC-1}"));
        assert!(out_on.contains("title: \"X (copy)\""));
        assert!(out_on.contains("status: new"));
        let off = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), false).unwrap();
        let out_off = std::fs::read_to_string(&off).unwrap();
        assert!(out_off.contains("task-id: {source: jira, ref: ABC-1}"));
    }
}
