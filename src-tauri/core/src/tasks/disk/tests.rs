//! `disk.rs`'s tests, split out for the Rust LOC cap — same module position
//! (`tasks::disk::tests`), so every `super` path below is unchanged and the
//! tests still sit beside the code they pin (the `services::tasks::parent`
//! mod.rs/tests.rs precedent this mirrors).

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
            update_task_fields(&root, &p, &[("status", Some("done"))], Some("task-id")).unwrap();
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
fn concurrent_update_task_fields_on_the_same_file_never_loses_an_edit() {
    // TASK 6f. Without a per-path lock, `update_task_fields` is a plain
    // read -> surgical-edit -> atomic-replacing-write with nothing
    // serializing two callers on the SAME file: both read v1, both edit
    // their own key against that same v1, and whichever writes second
    // silently overwrites the first writer's key with its own (correct
    // for ITS key, stale for everything else it also rewrote from v1) —
    // a textbook lost update. Two threads, two DIFFERENT keys, on one
    // file: with the lock, both survive every time; without it, this
    // reproduced on 300/300 iterations across three separate runs in
    // the environment this test was authored in (not a rare
    // interleaving that needed a sleep or an injected hook to hit) —
    // see the task report for the full measurement. 50 iterations here
    // is a wide safety margin over that observed rate, not a minimum.
    for i in 0..50 {
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
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let thread_a = {
            let p = p.clone();
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_task_fields(&root, &p, &[("due", Some("2026-07-20"))], None)
            })
        };
        let thread_b = {
            let p = p.clone();
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_task_fields(&root, &p, &[("priority", Some("high"))], None)
            })
        };
        thread_a.join().unwrap().unwrap();
        thread_b.join().unwrap().unwrap();

        let body = std::fs::read_to_string(&p).unwrap();
        assert!(
            body.contains("due: 2026-07-20\n"),
            "iteration {i}: thread A's `due` edit was lost, got: {body}"
        );
        assert!(
            body.contains("priority: high\n"),
            "iteration {i}: thread B's `priority` edit was lost, got: {body}"
        );
    }
}

#[test]
fn concurrent_parent_id_stamp_and_status_flip_never_lose_either() {
    // TASK 6f's motivating regression: `resolve_parent_for_write`'s
    // phase 3a stamps the PARENT's own Task ID via `update_task_fields`
    // with no ordinary field update (`&[]`, `ensure_id: Some(prop)`) —
    // exactly what this reproduces on thread A. A concurrent
    // `set_task_status` on that SAME parent file (a user marking it
    // done at the same moment it is adopted as a parent, or the
    // embedded MCP server's `set_task_status` landing mid-adoption) is
    // thread B: `&[("status", ...)]`, `ensure_id: None` (a checkbox
    // click never stamps). Pre-fix this loses the id 300/300 times
    // across three runs in the authoring environment — B reads the
    // pre-stamp content, A stamps and writes, B overwrites A's stamp
    // with its own stale (unstamped) snapshot, silently orphaning the
    // child that was told to point at this id. 50 iterations is a wide
    // margin over that observed rate; see the task report.
    for i in 0..50 {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "Parent",
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
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let thread_stamp = {
            let p = p.clone();
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_task_fields(&root, &p, &[], Some("task-id"))
            })
        };
        let thread_status = {
            let p = p.clone();
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_task_fields(&root, &p, &[("status", Some("done"))], None)
            })
        };
        let stamped = thread_stamp
            .join()
            .unwrap()
            .unwrap()
            .expect("an absent id is always stamped");
        thread_status.join().unwrap().unwrap();

        let body = std::fs::read_to_string(&p).unwrap();
        assert!(
            body.contains(&format!("task-id: {stamped}\n")),
            "iteration {i}: the freshly-stamped parent id was erased by the \
             concurrent status flip, got: {body}"
        );
        assert!(
            body.contains("status: done\n"),
            "iteration {i}: the concurrent status flip was itself lost, got: {body}"
        );
    }
}

#[cfg(unix)]
#[test]
fn concurrent_updates_through_a_symlink_and_the_real_path_still_serialize() {
    // FILE_LOCKS is keyed on the CANONICAL path specifically so two
    // spellings of the same file share one lock — a lexical key (the
    // caller's raw `path` argument, before canonicalization) would let a
    // caller going through a symlink race a caller going through the
    // real path as if they were two different files, and the lock would
    // be decorative for exactly the aliasing it exists to catch. Same
    // shape as concurrent_update_task_fields_on_the_same_file_never_
    // loses_an_edit, but thread B addresses the file through a symlink
    // INSIDE the tasks root pointing at thread A's real path.
    for i in 0..50 {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let real = create_task(
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
        let linked = root.join("linked.md");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let thread_real = {
            let real = real.clone();
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_task_fields(&root, &real, &[("due", Some("2026-07-20"))], None)
            })
        };
        let thread_linked = {
            let linked = linked.clone();
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                update_task_fields(&root, &linked, &[("priority", Some("high"))], None)
            })
        };
        thread_real.join().unwrap().unwrap();
        thread_linked.join().unwrap().unwrap();

        let body = std::fs::read_to_string(&real).unwrap();
        assert!(
            body.contains("due: 2026-07-20\n"),
            "iteration {i}: the real-path edit was lost, got: {body}"
        );
        assert!(
            body.contains("priority: high\n"),
            "iteration {i}: the symlink-path edit was lost, got: {body}"
        );
    }
}

#[test]
fn with_task_file_lock_prunes_dead_entries_so_the_map_does_not_grow_unboundedly() {
    // FILE_LOCKS must not become a forever-growing record of every task
    // file this process has ever touched (its own doc comment). Touch
    // many DISTINCT, never-reused paths SEQUENTIALLY — each acquire
    // fully completes and releases before the next starts, so nothing
    // is ever concurrently in flight. `with_task_file_lock` never
    // touches the filesystem itself (it is a pure path-keyed map), so a
    // nonexistent path is fine here — this test is about the map's
    // bookkeeping, not file I/O.
    //
    // Measured as a DELTA, not an absolute size: FILE_LOCKS is one
    // process-wide static, and `cargo test`'s default parallel runner
    // shares this one process across every test in this file — a
    // concurrency test elsewhere in this module can genuinely be
    // mid-`with_task_file_lock` (an Arc legitimately still alive) at the
    // same instant this one runs, which would make an absolute "must be
    // <= 1" assertion flaky on cross-test noise that has nothing to do
    // with THIS test's own 500 iterations. The delta isolates what this
    // loop itself left behind regardless of what else shares the map.
    let before = file_locks().lock().unwrap_or_else(|e| e.into_inner()).len();
    for i in 0..500 {
        let p = std::path::PathBuf::from(format!("/nonexistent/task-lock-probe/{i}"));
        with_task_file_lock(&p, || {});
    }
    let after = file_locks().lock().unwrap_or_else(|e| e.into_inner()).len();
    let grew_by = after.saturating_sub(before);
    assert!(
        grew_by <= 5,
        "500 distinct, sequential (never concurrent) acquisitions grew the lock map by \
         {grew_by} entries (before={before}, after={after}) — pruning on acquire is what \
         keeps this bounded by in-flight writes rather than by how many files this \
         process has ever touched; a small constant growth is tolerable cross-test noise, \
         but growth anywhere near 500 means pruning regressed"
    );
}

#[test]
fn ensure_id_never_overwrites_an_anchored_existing_id_and_reports_it_stripped() {
    // Behavior change, stated plainly (task report): before this fix, the
    // effective id returned for `task-id: &stable abc` was the literal
    // "&stable abc" (strict_scalar_field did not strip the anchor). It is
    // now "abc" — matching what list_tasks/scalar_id_ci report, and what
    // mirror_id_reference already mirrors into a child. The never-overwrite
    // invariant is UNAFFECTED either way: this property was already
    // classified as "has a usable value" (Some, never None) before this fix
    // too, so ensure_id never stamped over it pre- or post-fix — only the
    // STRING it reports changed, not whether it counts as usable.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    let p = root.join("t.md");
    std::fs::write(
        &p,
        "---\ntype: Task\nstatus: new\ntitle: \"T\"\ntask-id: &stable abc\n---\n",
    )
    .unwrap();
    let reported = update_task_fields(&root, &p, &[("status", Some("done"))], Some("task-id"))
        .unwrap()
        .expect("an existing anchored id is reported back, not treated as absent");
    assert_eq!(reported, "abc");
    // Never overwritten: the source line is untouched, byte for byte.
    let after = std::fs::read_to_string(&p).unwrap();
    assert!(after.contains("task-id: &stable abc\n"), "got {after}");
    assert_eq!(
        after.matches("task-id:").count(),
        1,
        "no second id line stamped"
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
