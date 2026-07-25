//! Disk operations: the sanctioned vault writes on an EXISTING task file —
//! the surgical field/status update via `update_task_fields` (and the
//! `backfill_task_id`/`set_task_status` wrappers over it). Task-document
//! CREATION (filename derivation + frontmatter rendering) lives in
//! `tasks::create`.

use super::writer::set_fields;
use std::path::Path;

/// Apply a surgical frontmatter patch to a task file on disk. Canonicalizes
/// `root` and `path` and requires containment — a lexical check can't see
/// through a symlink at the file or folder — then reads, applies `set_fields`,
/// and writes atomically (hidden `create_new` temp + fsync + REPLACING
/// rename). Replacing is correct here: the target is the `type: Task` file we
/// just read and are editing in place, touching only the named lines.
/// `ensure_id` names the vault's task-id property (`None` = ids off): when the
/// property has no USABLE value — absent, or present with a blank scalar (a
/// bare `task-id:` from an Obsidian property panel / template; Codex, PR #59)
/// — a fresh id is GENERATED HERE and stamped alongside the patch. Generating
/// inside this branch, rather than callers pre-drawing a candidate, means no
/// discarded CSPRNG draws on already-stamped tasks and no caller can get the
/// blank/casing rules wrong. An existing non-empty value (top-level, any
/// casing — `frontmatter_scalar_ci`; a nested `metadata.task-id` never
/// counts) is never overwritten, so IDs stay stable. Returns the property's
/// effective value after the write — freshly stamped or pre-existing — or
/// `None` when `ensure_id` is `None`; callers reflect a just-stamped ID
/// without a second read (Codex, PR #59).
pub fn update_task_fields(
    root: &Path,
    path: &Path,
    updates: &[(&str, Option<&str>)],
    ensure_id: Option<&str>,
) -> Result<Option<String>, String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    let mut effective: Vec<(&str, Option<&str>)> = updates.to_vec();
    // Owned storage for a freshly-generated id and the on-disk casing a blank
    // line is rewritten under — both must outlive `effective`'s borrows.
    let mut generated: Option<String> = None;
    let mut blank_casing: Option<String> = None;
    let ensured = ensure_id.and_then(|key| {
        // `id_property_unassignable` is the SINGLE-SOURCED gate
        // `services::tasks::parent`'s phase-1 pre-lock forecast also
        // consults (design spec §2) — see its own doc comment for why the
        // two must never be two independently-maintained notions of
        // "assignable". A BLOCK (a nested map/list under the key) or FLOW
        // (`key: {..}` / `[..]`) value is the USER'S frontmatter, not a
        // stamp target and not an id: set_fields' rewrite would
        // consume/rewrite it and delete their data (review, PR #59), and
        // reporting it as the effective id would let a duplicate that
        // preserved a flow value read as sharing the source's stable id
        // (Codex P2, PR #76). A present, non-blank scalar the strict reader
        // can't decode is likewise not a usable id. Leave either untouched
        // and report no id — the read (`scalar_id_ci`) agrees: non-scalar =
        // non-id.
        if super::parse::id_property_unassignable(&content, key) {
            return None;
        }
        match super::parse::frontmatter_scalar_ci(&content, key) {
            // Already has a usable PLAIN-SCALAR id (any casing) → never overwritten.
            // Decode it with the SAME strict reader `scalar_id_ci`/`list_tasks`
            // use, not the shallow `frontmatter_scalar_ci` value: for a quoted
            // hand-authored id like `task-id: 'a''b'` the shallow read yields
            // a''b while the list shows a'b, so callers that write this value
            // back (set_task_parent writes it as the child's `parent-id`) would
            // record a reference the parent does not answer to, and the
            // frontend's reflectStampedId would overwrite the correct row value
            // (Codex P2, PR #77). A value the strict reader cannot decode
            // reports no id — exactly as the list reports none (unreachable
            // here in practice: `id_property_unassignable` already ruled out
            // a non-empty value the strict reader rejects).
            Some((on_disk, v)) if !v.is_empty() => {
                super::parse::strict_scalar_field(&content, &on_disk, false)
            }
            // Truly blank or absent → generate + stamp. A BLANK line is
            // rewritten under its ON-DISK casing so set_fields (case-
            // sensitive) replaces it — stamping the configured casing would
            // insert a case-mismatched DUPLICATE that scalar_id_ci's CI
            // read then shadows, hiding the id forever (Codex, PR #59).
            // Absent stamps a new line under the configured property name.
            found => {
                blank_casing = found.map(|(on_disk, _)| on_disk);
                let id = super::id::new_task_id();
                generated = Some(id.clone());
                Some(id)
            }
        }
    });
    if let (Some(key), Some(id)) = (ensure_id, generated.as_deref()) {
        effective.push((blank_casing.as_deref().unwrap_or(key), Some(id)));
    }
    // Nothing to write (an ensure-only call — a move backfill — on a task
    // whose id is already present): skip the redundant atomic rewrite, still
    // report the id. update_task always passes a non-empty `updates`, so this
    // only short-circuits those callers.
    if effective.is_empty() {
        return Ok(ensured);
    }
    let updated = set_fields(&content, &effective).ok_or(
        "Task frontmatter could not be updated (not a type: Task document, or its frontmatter is malformed)",
    )?;
    crate::capture_note::write_atomic_replacing(&canon_path, &updated)
        .map_err(|e| format!("Cannot save task: {e}"))?;
    Ok(ensured)
}

/// Best-effort id backfill on a task file a structural move just relocated
/// (drag / editor move, delete-list): stamp a missing/blank id under
/// `property` (`None` = ids off → no-op). The move already mutated the vault,
/// so a stamp failure only WARNS — it must never fail the move that carried
/// it (audio-first discipline, borrowed from the capture domain). Returns the
/// task's effective id — freshly stamped or already present — for callers
/// that reflect it without a reload.
pub fn backfill_task_id(root: &Path, path: &Path, property: Option<&str>) -> Option<String> {
    let prop = property?;
    match update_task_fields(root, path, &[], Some(prop)) {
        Ok(id) => id,
        Err(e) => {
            log::warn!("task id backfill on {path:?} failed: {e}");
            None
        }
    }
}

/// Set a task's `status:` frontmatter on disk (see `update_task_fields`). A
/// status toggle never stamps an ID (`ensure_id: None` — a checkbox click is
/// not an edit), so the id return is discarded.
pub fn set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String> {
    update_task_fields(root, path, &[("status", Some(new_status))], None).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    // create_task now lives in tasks::create, not disk — most of these tests
    // still build a task file on disk via it. `super::create::create_task`
    // won't resolve HERE: inside this nested `tests` module `super` means
    // `disk`, not `tasks` (the same nesting gotcha the description-field
    // note below documents), so this reaches it via the crate-root re-export
    // instead.
    use crate::tasks::create_task;

    #[test]
    fn set_task_status_writes_an_arbitrary_status() {
        // set_task_status now takes a status string, so it can write archived
        // (and still new/done), not just a done bool.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "Buy milk",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        set_task_status(&root, &p, "archived").unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("status: archived\n"));
        set_task_status(&root, &p, "done").unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("status: done\n"));
    }

    #[test]
    fn set_task_status_writes_and_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "Buy milk",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        set_task_status(&root, &p, "done").unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("status: done\n"));
        set_task_status(&root, &p, "new").unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("status: new\n"));

        // A path outside the root is refused.
        let outside = dir.path().join("outside.md");
        std::fs::write(&outside, "---\ntype: Task\nstatus: new\n---\n").unwrap();
        assert!(set_task_status(&root, &outside, "done").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn set_task_status_rejects_symlinked_file_escaping_root() {
        // Canonicalization (not a lexical starts_with) must catch a task file that
        // is a symlink pointing outside the tasks root — the write would otherwise
        // land outside the vault.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let real = dir.path().join("elsewhere.md");
        std::fs::write(&real, "---\ntype: Task\nstatus: new\n---\n").unwrap();
        let link = root.join("2026-07-08-linked.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(set_task_status(&root, &link, "done").is_err());
    }

    #[test]
    fn update_task_fields_sets_rewrites_and_clears_scheduled() {
        // `scheduled` rides the same generic surgical writer as `due`/`tags` —
        // no new write machinery — but the spec promised an explicit
        // scheduled-named regression test pinning the set/rewrite/clear
        // round-trip on disk (not just render_task's in-memory output).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "A",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("scheduled"));

        // Set: absent → inserted at the closing fence.
        update_task_fields(&root, &p, &[("scheduled", Some("2026-07-20"))], None).unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("scheduled: 2026-07-20\n"));

        // Rewrite: existing line replaced in place, not duplicated.
        update_task_fields(&root, &p, &[("scheduled", Some("2026-07-25"))], None).unwrap();
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("scheduled: 2026-07-25\n"));
        assert!(!body.contains("2026-07-20"));
        assert_eq!(body.matches("scheduled:").count(), 1);

        // Clear: None removes the line entirely.
        update_task_fields(&root, &p, &[("scheduled", None)], None).unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("scheduled"));
    }

    #[test]
    fn update_task_fields_stamps_an_absent_ensure_key_but_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "A",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        // Absent → a fresh id is generated INTERNALLY, stamped alongside the
        // edit, and returned (shape-asserted: generation is random now).
        let stamped = update_task_fields(&root, &p, &[("status", Some("done"))], Some("task-id"))
            .unwrap()
            .expect("an absent id must be stamped");
        assert_eq!(stamped.len(), 8);
        assert!(stamped
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()));
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("status: done\n"));
        assert!(body.contains(&format!("task-id: {stamped}\n")));
        // Present → never overwritten (a second ensure is a no-op), and the
        // EXISTING id is reported back, not a fresh draw.
        let existing = update_task_fields(&root, &p, &[], Some("task-id")).unwrap();
        assert_eq!(existing.as_deref(), Some(stamped.as_str()));
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains(&format!("task-id: {stamped}\n")));
    }

    #[test]
    fn update_task_fields_detects_an_existing_id_case_insensitively() {
        // Regression: scalar_field's exact-case match let a config using
        // "task-id" stamp a SECOND, conflicting id line onto a task already
        // carrying "Task-ID:" (e.g. stamped under a since-changed config
        // casing, or hand-authored). Obsidian folds frontmatter key case, so
        // the task would show a duplicate id. The case-insensitive
        // scalar_field_ci read must catch the existing key under any casing.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "A",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        let seeded = content.replacen(
            "created: 2026-07-08\n",
            "created: 2026-07-08\nTask-ID: existing123\n",
            1,
        );
        std::fs::write(&p, &seeded).unwrap();

        let reported =
            update_task_fields(&root, &p, &[("status", Some("done"))], Some("task-id")).unwrap();
        // The existing id (under its own casing) is reported — no fresh stamp.
        assert_eq!(reported.as_deref(), Some("existing123"));

        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("status: done\n"));
        assert!(body.contains("Task-ID: existing123\n"));
        // Exactly one id-ish line, case-insensitively — never a second,
        // conflicting one under a different casing.
        let id_lines = body
            .lines()
            .filter(|l| l.trim_start().to_ascii_lowercase().starts_with("task-id:"))
            .count();
        assert_eq!(id_lines, 1);
    }

    #[test]
    fn update_task_fields_stamps_over_a_blank_id_property() {
        // Codex PR #59: a bare `task-id:` (an Obsidian property panel/template
        // leaves the key valueless) is NOT a usable id — the presence-only
        // predecessor treated it as present and suppressed the stamp forever.
        // The non-empty check now stamps it and reports the fresh id.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "A",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        let seeded = content.replacen(
            "created: 2026-07-08\n",
            "created: 2026-07-08\ntask-id:\n",
            1,
        );
        std::fs::write(&p, &seeded).unwrap();

        let reported = update_task_fields(&root, &p, &[("status", Some("done"))], Some("task-id"))
            .unwrap()
            .expect("a blank id must be stamped");
        // Blank → treated as missing → a fresh id generated + returned.
        assert_eq!(reported.len(), 8);
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains(&format!("task-id: {reported}\n")));
        // The blank line was rewritten in place, not duplicated.
        let id_lines = body.lines().filter(|l| l.starts_with("task-id:")).count();
        assert_eq!(id_lines, 1);
    }

    #[test]
    fn update_task_fields_stamps_a_blank_id_under_its_on_disk_casing() {
        // Codex PR #59: the blank-id stamp must rewrite the EXISTING line, not
        // add a second one under the configured casing. `set_fields` matches
        // keys case-sensitively, so stamping the config's `task-id` onto a file
        // whose blank line is `Task-ID:` (Obsidian folds key case; a property
        // panel / template can leave either casing) would INSERT a duplicate —
        // and `scalar_field_ci`'s case-insensitive read would then return the
        // first (blank) line, hiding the id forever. The stamp must land on the
        // on-disk key name.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "A",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        let seeded = content.replacen(
            "created: 2026-07-08\n",
            "created: 2026-07-08\nTask-ID:\n",
            1,
        );
        std::fs::write(&p, &seeded).unwrap();

        let reported = update_task_fields(&root, &p, &[("status", Some("done"))], Some("task-id"))
            .unwrap()
            .expect("a blank id must be stamped");
        // Blank (any casing) → stamped, fresh id reported.
        assert_eq!(reported.len(), 8);
        let body = std::fs::read_to_string(&p).unwrap();
        // Rewritten in place under the ON-DISK casing — no lowercase duplicate.
        assert!(body.contains(&format!("Task-ID: {reported}\n")));
        assert!(!body.contains("task-id:"));
        // Exactly one id-ish line, case-insensitively — no conflicting second.
        let id_lines = body
            .lines()
            .filter(|l| l.trim_start().to_ascii_lowercase().starts_with("task-id:"))
            .count();
        assert_eq!(id_lines, 1);
    }

    #[test]
    fn update_task_fields_never_stamps_over_a_non_scalar_id_property() {
        // review, PR #59: a configured id property can collide with a key the
        // user already owns as a nested MAP or block LIST (`uid:` + indented
        // lines), and (Codex P2, PR #76) an inline FLOW map/seq
        // (`uid: {..}`/`[..]`). frontmatter_scalar_ci reads a block as an empty
        // scalar and a flow as a NON-empty one, but neither is an id: stamping
        // would rewrite the key line (deleting a block's nested data), and
        // reporting a flow value as the id would let a duplicate read as sharing
        // it. A non-scalar value is the user's frontmatter, never a stamp
        // target: the edit still applies, the value survives byte-for-byte, and
        // no id is reported.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        for (name, block) in [
            ("map", "task-id:\n  source: jira\n  ref: ABC-1\n"),
            ("list", "task-id:\n- a1\n- b2\n"),
            ("flow-map", "task-id: {source: jira, ref: ABC-1}\n"),
            ("flow-seq", "task-id: [a1, b2]\n"),
        ] {
            let p = create_task(
                &root,
                name,
                "2026-07-08",
                None,
                None,
                &[],
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let content = std::fs::read_to_string(&p).unwrap();
            let seeded = content.replacen(
                "created: 2026-07-08\n",
                &format!("created: 2026-07-08\n{block}"),
                1,
            );
            std::fs::write(&p, &seeded).unwrap();

            let reported =
                update_task_fields(&root, &p, &[("status", Some("done"))], Some("task-id"))
                    .unwrap();
            assert_eq!(reported, None, "{name}: no usable id to report");
            let body = std::fs::read_to_string(&p).unwrap();
            assert!(body.contains("status: done\n"), "{name}: the edit applied");
            assert!(
                body.contains(block),
                "{name}: the user's block survives byte-for-byte, got: {body}"
            );
        }
    }

    #[test]
    fn set_task_status_does_not_stamp_any_id() {
        // A checkbox toggle is not an "edit": set_task_status passes no
        // ensure keys, so toggling never adds an id.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "A",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        set_task_status(&root, &p, "done").unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("task-id"));
    }

    #[test]
    fn update_task_fields_sets_rewrites_and_clears_description() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let p = root.join("t.md");
        std::fs::write(&p, "---\ntype: Task\nstatus: new\ntitle: X\n---\n\nbody\n").unwrap();
        let quoted = crate::yaml_scalar::yaml_quote_multiline("hi\nthere #42");
        update_task_fields(&root, &p, &[("description", Some(quoted.as_str()))], None).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        // NOTE (brief deviation): the brief's literal `super::description::…`
        // does not resolve here — `super` inside this nested `tests` module
        // means `disk`, not `tasks` (that shorthand only works from disk.rs's
        // own top-level functions, or from a sibling module like list.rs, one
        // nesting level shallower). Fully qualifying from the crate root
        // reaches the same `pub(super)` item — still visible, since
        // `tasks::disk::tests` is a descendant of `tasks` — without changing
        // `description_field`'s visibility or touching any other call site.
        assert_eq!(
            crate::tasks::description::description_field(&after),
            Some("hi\nthere #42".to_string())
        );
        assert!(after.contains("\nbody\n")); // body untouched
        update_task_fields(&root, &p, &[("description", None)], None).unwrap();
        assert_eq!(
            crate::tasks::description::description_field(&std::fs::read_to_string(&p).unwrap()),
            None
        );
    }

    #[test]
    fn effective_id_return_uses_the_strict_decode_like_the_list_reader() {
        // A quoted hand-authored id decodes to a'b for list_tasks (scalar_id_ci
        // -> strict_scalar_field). The RETURN value must agree: set_task_parent
        // writes it as the child's `parent-id`, so a shallow a''b here would
        // record a reference the parent does not answer to, and the frontend's
        // reflectStampedId would overwrite the correct row value (Codex P2, PR #77).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let p = root.join("t.md");
        std::fs::write(
            &p,
            "---\ntype: Task\nstatus: new\ntitle: \"T\"\ntask-id: 'a''b'\n---\n",
        )
        .unwrap();
        let returned = update_task_fields(root, &p, &[("status", Some("done"))], Some("task-id"))
            .unwrap()
            .expect("an existing id is reported back");
        assert_eq!(returned, "a'b", "must match what list_tasks surfaces");
        // And the existing id was NOT overwritten.
        let after = std::fs::read_to_string(&p).unwrap();
        assert!(after.contains("task-id: 'a''b'"), "got {after}");
    }
}
