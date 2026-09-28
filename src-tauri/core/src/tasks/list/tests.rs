//! `list.rs`'s tests, split out for the Rust LOC cap (list.rs stood at
//! 792/800 nonblank with these inline) — same module position
//! (`tasks::list::tests`), so every `super` path below is unchanged and the
//! tests still sit beside the code they pin. The seam is code vs. tests, the
//! `disk.rs`/`disk/tests.rs` precedent in this same directory: the read side
//! itself (`TaskItem`, the three scan entry points over one `ScanMode` walk,
//! the clock-free sort) is one unit, and every test here drives it through
//! the public entry points, so no split of the CODE would have let a test
//! move with the code it pins.

use super::*;

fn write(root: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join(name), body).unwrap();
}

#[test]
fn list_tasks_returns_only_type_task_files_sorted_open_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "2026-07-06-a.md",
        "---\ntype: Task\nstatus: done\ntitle: \"A done\"\ncreated: 2026-07-06\n---\n",
    );
    write(
        root,
        "2026-07-08-b.md",
        "---\ntype: Task\nstatus: new\ntitle: \"B open\"\ncreated: 2026-07-08\n---\n",
    );
    write(
        root,
        "2026-07-07-c.md",
        "---\ntype: Task\nstatus: new\ntitle: \"C open\"\ncreated: 2026-07-07\n---\n",
    );
    // Not a task — must be ignored even though it lives in the folder.
    write(
        root,
        "note.md",
        "---\ntype: Meeting\ntitle: \"Nope\"\n---\n",
    );
    // No frontmatter — ignored.
    write(root, "plain.md", "just text\n");

    let items = list_tasks(root, None);
    let titles: Vec<&str> = items.iter().map(|t| t.title.as_str()).collect();
    // Open tasks first, newest created first; the done task last.
    assert_eq!(titles, vec!["B open", "C open", "A done"]);
    assert!(!items[0].done);
    assert!(items[2].done);
    assert_eq!(items[0].status, "new");
    assert_eq!(items[2].created, "2026-07-06");
}

#[test]
fn list_tasks_excludes_archived() {
    // Archived tasks are removed from view — the list surfaces only open +
    // done, never archived (no show-archived surface this slice).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "open.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Open\"\ncreated: 2026-07-08\n---\n",
    );
    write(
        root,
        "done.md",
        "---\ntype: Task\nstatus: done\ntitle: \"Done\"\ncreated: 2026-07-07\n---\n",
    );
    write(
        root,
        "arch.md",
        "---\ntype: Task\nstatus: archived\ntitle: \"Arch\"\ncreated: 2026-07-06\n---\n",
    );
    let titles: Vec<String> = list_tasks(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(titles, vec!["Open", "Done"]); // archived is not surfaced
}

#[test]
fn list_tasks_missing_root_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(list_tasks(&dir.path().join("nope"), None).is_empty());
}

#[test]
fn list_tasks_walks_subdirectories_recursively() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "top.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Top\"\ncreated: 2026-07-08\n---\n",
    );
    write(
        &root.join("work"),
        "mid.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Mid\"\ncreated: 2026-07-07\n---\n",
    );
    write(
        &root.join("work/q3"),
        "deep.md",
        "---\ntype: Task\nstatus: done\ntitle: \"Deep\"\ncreated: 2026-07-06\n---\n",
    );
    let titles: Vec<String> = list_tasks(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    // All three found regardless of depth; open first (newest created), done last.
    assert_eq!(titles, vec!["Top", "Mid", "Deep"]);
}

#[test]
fn list_tasks_skips_dot_directories() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "real.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Real\"\ncreated: 2026-07-08\n---\n",
    );
    // A task in a hidden dir (e.g. .trash) must NOT be surfaced by the walk.
    write(
        &root.join(".trash"),
        "gone.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Gone\"\ncreated: 2026-07-08\n---\n",
    );
    let titles: Vec<String> = list_tasks(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(titles, vec!["Real"]);
}

#[cfg(unix)]
#[test]
fn list_tasks_does_not_follow_symlinked_subdir() {
    // A symlinked subdir pointing outside the tasks folder must not be
    // walked — dir_entries reports it as a symlink (not a dir), so the walk
    // skips it and can't leave the tasks folder.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    write(
        &root,
        "inside.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Inside\"\ncreated: 2026-07-08\n---\n",
    );
    // A real dir OUTSIDE the tasks folder, with a task in it, linked in.
    let outside = dir.path().join("outside");
    write(
        &outside,
        "escapee.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Escapee\"\ncreated: 2026-07-08\n---\n",
    );
    std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
    let titles: Vec<String> = list_tasks(&root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(titles, vec!["Inside"]); // Escapee is never followed
}

#[cfg(unix)]
#[test]
fn list_tasks_terminates_on_a_directory_cycle() {
    // A link pointing back to an ancestor inside the folder must not loop,
    // and the task must be counted once. Guards the walked-set + canonical
    // containment (the same guard catches a Windows junction cycle).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    write(
        &root,
        "a.md",
        "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-08\n---\n",
    );
    // Tasks/sub/loop -> Tasks — a cycle back to an ancestor, still inside root.
    std::os::unix::fs::symlink(&root, root.join("sub").join("loop")).unwrap();
    let titles: Vec<String> = list_tasks(&root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(titles, vec!["A"]); // terminates; A counted exactly once
}

#[test]
fn list_tasks_ties_break_on_title_when_created_matches() {
    // Two open tasks sharing the same created date must fall back to the
    // title tiebreak (`.then(a.title.cmp(&b.title))`) — ascending order.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "2026-07-08-z.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Zebra\"\ncreated: 2026-07-08\n---\n",
    );
    write(
        root,
        "2026-07-08-a.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Apple\"\ncreated: 2026-07-08\n---\n",
    );

    let items = list_tasks(root, None);
    let titles: Vec<&str> = items.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, vec!["Apple", "Zebra"]);
}

#[test]
fn list_tasks_sorts_by_due_then_priority_then_created() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mk = |name: &str, extra: &str, title: &str, created: &str| {
        write(
            root,
            name,
            &format!("---\ntype: Task\nstatus: new\ntitle: \"{title}\"\ncreated: {created}\n{extra}---\n"),
        )
    };
    mk("a.md", "", "NoDue", "2026-07-09");
    mk("b.md", "due: 2026-07-20\n", "Later", "2026-07-01");
    mk("c.md", "due: 2026-07-10\n", "Sooner", "2026-07-01");
    mk(
        "d.md",
        "due: 2026-07-10\npriority: high\n",
        "SoonerHigh",
        "2026-07-01",
    );
    mk("e.md", "due: tomorrow\n", "BadDue", "2026-07-08"); // unparseable → no-date
    write(
        root,
        "z.md",
        "---\ntype: Task\nstatus: done\ntitle: \"Done\"\ncreated: 2026-07-09\ndue: 2026-07-01\n---\n",
    );
    let titles: Vec<String> = list_tasks(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    // dated (due asc, high before normal) → no-date (created desc) → done last
    // (done ignores its overdue due — done sorts by created).
    assert_eq!(
        titles,
        vec!["SoonerHigh", "Sooner", "Later", "NoDue", "BadDue", "Done"]
    );
}

#[test]
fn list_tasks_derives_list_from_subfolder() {
    // A List IS a folder: the task's list is its parent folder relative to
    // the tasks root — "" at the root, `/`-joined at any depth (never the
    // platform separator; the identity crosses IPC and merges across OSes).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "top.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Top\"\ncreated: 2026-07-08\n---\n",
    );
    write(
        &root.join("work"),
        "mid.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Mid\"\ncreated: 2026-07-07\n---\n",
    );
    write(
        &root.join("work/q3"),
        "deep.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Deep\"\ncreated: 2026-07-06\n---\n",
    );
    let items = list_tasks(root, None);
    let lists: Vec<(&str, &str)> = items
        .iter()
        .map(|t| (t.title.as_str(), t.list.as_str()))
        .collect();
    assert_eq!(
        lists,
        vec![("Top", ""), ("Mid", "work"), ("Deep", "work/q3")]
    );
}

// Review finding 4: `collect_task_file`'s own `.md` match was
// case-SENSITIVE, while `search.rs` already treats notes as any-case
// `.md` (AGENTS.md's search section: "notes are any-case `.md`"). A
// hand-authored `Upper.MD` task must be visible to BOTH the view
// (list_tasks) and the hierarchy guard (list_tasks_structural) — the
// latter matters most: an invisible task's `parent-id` edges don't
// exist as far as cycle validation is concerned (see the companion test
// in services/tasks/parent/tests.rs for the concrete cycle this let
// through).
#[test]
fn list_tasks_and_structural_scan_surface_uppercase_md_extensions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "Upper.MD",
        "---\ntype: Task\nstatus: new\ntitle: \"Upper\"\ncreated: 2026-07-25\n---\n",
    );
    let titles: Vec<String> = list_tasks(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(
        titles,
        vec!["Upper"],
        "the VIEW must surface an uppercase .MD task"
    );
    let structural = list_tasks_structural(root, None).unwrap();
    assert_eq!(
        structural.len(),
        1,
        "the STRUCTURAL scan must surface it too"
    );
    assert_eq!(structural[0].title, "Upper");
}

#[test]
fn structural_scan_keeps_archived_rows() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "a.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Open\"\n---\n",
    );
    write(
        root,
        "b.md",
        "---\ntype: Task\nstatus: archived\ntitle: \"Arch\"\nparent-id: x\n---\n",
    );
    assert_eq!(list_tasks(root, None).len(), 1); // presentation: archived hidden
    let all = list_tasks_structural(root, None).unwrap();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|t| t.parent_id.as_deref() == Some("x")));
}

// Fix 1 (subtasks-vault-UX-polish increment): an archived task can still
// be somebody's PARENT. The hierarchy read that decides whether that
// relationship exists must see it, exactly as the structural guard does —
// but it is still a best-effort VIEW (no write to protect), not a guard,
// so it must NOT inherit list_tasks_structural's abort-on-unreadable-file
// posture. These three tests pin that exact split.
#[test]
fn list_tasks_including_archived_keeps_archived_rows_but_list_tasks_still_hides_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "open.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Open\"\ncreated: 2026-07-08\n---\n",
    );
    write(
        root,
        "arch.md",
        "---\ntype: Task\nstatus: archived\ntitle: \"Arch\"\ncreated: 2026-07-06\n---\n",
    );
    // The main todo-list presentation is UNCHANGED by this fix.
    assert_eq!(list_tasks(root, None).len(), 1);
    let titles: Vec<String> = list_tasks_including_archived(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(titles, vec!["Open", "Arch"]);
}

#[test]
fn list_tasks_including_archived_missing_root_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(list_tasks_including_archived(&dir.path().join("nope"), None).is_empty());
}

#[cfg(unix)]
#[test]
fn list_tasks_including_archived_degrades_on_an_unreadable_file_unlike_the_structural_guard() {
    // The precise behavior that must NOT be inherited from
    // list_tasks_structural: an unreadable sibling file must not blank
    // this read entirely — it has no write to protect, so there is
    // nothing to refuse. Same fixture shape as
    // structural_scan_errors_on_an_unreadable_task above, opposite
    // expectation.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "a.md",
        "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
    );
    let locked = root.join("b.md");
    std::fs::write(&locked, "---\ntype: Task\nstatus: new\ntitle: \"B\"\n---\n").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let bypassed = std::fs::read_to_string(&locked).is_ok();
    if bypassed {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }
    let titles: Vec<String> = list_tasks_including_archived(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        titles,
        vec!["A"],
        "an unreadable file degrades rather than failing the whole read"
    );
}

// uid-independent counterpart, mirroring
// structural_scan_errors_on_a_non_utf8_task_file: read_to_string fails
// with InvalidData for non-UTF-8 bytes regardless of uid/permissions, so
// this exercises the lenient-degrade guarantee even when tests run as
// root (the chmod-based test above self-skips there).
#[test]
fn list_tasks_including_archived_degrades_on_a_non_utf8_task_file_unlike_the_structural_guard() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "a.md",
        "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
    );
    std::fs::write(root.join("b.md"), [0xff, 0xfe]).unwrap();
    let titles: Vec<String> = list_tasks_including_archived(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    assert_eq!(
        titles,
        vec!["A"],
        "a non-UTF-8 file degrades rather than failing the whole read"
    );
}

#[cfg(unix)]
#[test]
fn structural_scan_errors_on_an_unreadable_task() {
    // One unreadable Task in a network vault must ABORT the scan, not vanish
    // from the graph — a missing edge lets a cycle through (Codex P2, PR #77).
    // Root bypasses DAC, so probe and skip under root; CI's rust-core runs
    // non-root and exercises the assertions (same pattern as
    // move_task_fails_and_rolls_back_when_source_cannot_be_removed in
    // lists.rs).
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "a.md",
        "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
    );
    let locked = root.join("b.md");
    std::fs::write(&locked, "---\ntype: Task\nstatus: new\ntitle: \"B\"\n---\n").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // If a read still succeeds despite the mode, perms are being bypassed
    // (root) and the wall this test relies on doesn't hold — skip.
    let bypassed = std::fs::read_to_string(&locked).is_ok();
    if bypassed {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }
    // BOTH scans run while `b.md` is still unreadable — restoring first would
    // let the view read it, making the "degrades gracefully" assertion pass
    // for the wrong reason (its sibling directory test failed in CI exactly
    // this way).
    let out = list_tasks_structural(root, None);
    let view: Vec<String> = list_tasks(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    // Restore before asserting so the tempdir can clean up either way.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        out.is_err(),
        "an unreadable task must fail the structural scan"
    );
    // The VIEW degrades gracefully: it skips what it cannot read and returns
    // the rest. Asserting the exact set (not just non-empty) is what makes
    // this a real test of the lenient path.
    assert_eq!(view, vec!["A"], "the VIEW skips only the unreadable task");
}

// uid-independent counterpart to structural_scan_errors_on_an_unreadable_task
// above: read_to_string fails with InvalidData for non-UTF-8 bytes
// regardless of uid/permissions, so this exercises the strict-error
// guarantee even when tests run as root (the chmod test above self-skips
// there) — finding 4.
#[test]
fn structural_scan_errors_on_a_non_utf8_task_file() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "a.md",
        "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
    );
    std::fs::write(root.join("b.md"), [0xff, 0xfe]).unwrap();
    let out = list_tasks_structural(root, None);
    assert!(
        out.is_err(),
        "a non-UTF-8 task file must fail the structural scan"
    );
    assert!(!list_tasks(root, None).is_empty()); // the VIEW still degrades gracefully
}

// finding 1: a missing tasks folder is legitimately empty in EITHER mode
// — the guard has no graph to protect yet. Also covers finding 9 (no
// prior test of list_tasks_structural on a missing root).
#[test]
fn list_tasks_structural_missing_root_is_ok_empty() {
    let dir = tempfile::tempdir().unwrap();
    let out = list_tasks_structural(&dir.path().join("nope"), None).unwrap();
    assert!(out.is_empty());
}

// finding 1: `canonicalize` succeeds here (the path exists) — it is
// finding 2's directory-error reporting that must catch a root that
// exists but cannot be enumerated as a directory, not finding 1's
// canonicalize-error branch (which never sees an Err in this case).
// Verifying the REAL mechanism, not assuming which one fires, per the
// review's own caution.
#[test]
fn list_tasks_structural_errors_when_root_is_not_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("not-a-dir");
    std::fs::write(&root, b"i am a file, not the tasks folder").unwrap();
    let out = list_tasks_structural(&root, None);
    assert!(
        out.is_err(),
        "a non-directory root must fail the structural scan"
    );
}

// finding 2: a failed directory READ must not silently drop the whole
// subtree — a cycle routed through a task inside it would be invisible
// to the guard. The VIEW walk keeps today's lenient behavior: list_tasks
// still returns whatever it could reach.
#[cfg(unix)]
#[test]
fn structural_scan_errors_on_an_unreadable_subdirectory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "top.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Top\"\n---\n",
    );
    write(
        &root.join("Sub"),
        "hidden.md",
        "---\ntype: Task\nstatus: new\ntitle: \"Hidden\"\n---\n",
    );
    let sub = root.join("Sub");
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o000)).unwrap();
    // If a read still succeeds despite the mode, perms are being bypassed
    // (root) and the wall this test relies on doesn't hold — skip. Root
    // bypasses DAC, so probe and skip under root; CI's rust-core runs
    // non-root and exercises the assertions (same idiom as
    // move_task_fails_and_rolls_back_when_source_cannot_be_removed in
    // tasks/lists.rs).
    let bypassed = std::fs::read_dir(&sub).is_ok();
    if bypassed {
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    // BOTH scans must run while `Sub` is still unreadable — restoring first
    // would let the view walk into it and see "Hidden", which is exactly how
    // this test failed in CI (it skips under root, so only CI runs it).
    let structural = list_tasks_structural(root, None);
    let view: Vec<String> = list_tasks(root, None)
        .into_iter()
        .map(|t| t.title)
        .collect();
    // Restore before asserting so the tempdir can clean up either way.
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        structural.is_err(),
        "an unreadable subdirectory must fail the structural scan"
    );
    assert_eq!(
        view,
        vec!["Top"],
        "the VIEW still returns the tasks it could reach"
    );
}
