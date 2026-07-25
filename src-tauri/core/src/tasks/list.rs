//! The read side: scan the tasks folder, map files to `TaskItem`s, and the
//! clock-free sort ("overdue"/"today" need a clock, so date-bucket grouping
//! is deliberately the frontend's job, not the sort's).

use super::collect::collect_task_file;
use super::parse::is_valid_due;
use std::path::{Path, PathBuf};

/// One task surfaced in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskItem {
    pub path: PathBuf,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
    pub due: Option<String>,
    /// The do/plan date (`YYYY-MM-DD`) — when the user plans to WORK the task,
    /// distinct from `due` (the deadline). Read then validator-filtered, so it is
    /// `None` when absent OR unparseable (an honest DTO/MCP boundary).
    pub scheduled: Option<String>,
    pub priority: Option<String>,
    pub tags: Vec<String>,
    /// The task's List = its parent folder relative to the tasks root, always
    /// `/`-joined ("" at the root) — the identity crosses IPC and merges
    /// across platforms, so it never carries the OS separator.
    pub list: String,
    /// Manual rank from the `order:` frontmatter number; lenient read —
    /// unparseable/non-finite is unranked, never an error.
    pub order: Option<f64>,
    /// The generated id read from the vault's configured id property, when
    /// the vault has task IDs enabled; `None` when the feature is off (the
    /// property is never read) or the file simply has no value there.
    pub id: Option<String>,
    /// Free-text detail, decoded from the `description:` frontmatter scalar
    /// (multi-line, `#`-tolerant). `None` when absent/empty.
    pub description: Option<String>,
    /// The parent Task's stable id, read from `parent-id`. Authoritative for
    /// hierarchy resolution. NOT gated on the vault's id feature — a task's own
    /// id is (it is read under the configured property), but this is a plain
    /// key that always means the same thing.
    pub parent_id: Option<String>,
    /// The parent's Obsidian link (`parent`), carried verbatim for navigation.
    /// Never parsed for meaning.
    pub parent_link: Option<String>,
}

/// Sort tier for a priority value: high first, low last, anything else
/// (normal, absent, hand-authored unknown) in the middle.
pub fn priority_rank(p: Option<&str>) -> u8 {
    match p {
        Some("high") => 0,
        Some("low") => 2,
        _ => 1,
    }
}

/// (has-no-valid-due, due) — tuple compare puts valid dues first, ascending;
/// an unparseable hand-authored due sorts with the undated.
fn due_key(t: &TaskItem) -> (bool, &str) {
    match t.due.as_deref().filter(|d| is_valid_due(d)) {
        Some(d) => (false, d),
        None => (true, ""),
    }
}

/// Every `type: Task` file anywhere under `root`, best-effort — the configured
/// tasks folder is walked recursively so tasks organized into subfolders are
/// all surfaced. Open tasks (status != "done") first — sorted by due
/// ascending (no/unparseable due last), then priority tier, then newest
/// `created`, then title; completed tasks after, sorted by newest `created`
/// then title. A missing/unreadable root or file degrades silently.
///
/// `id_property` is the vault's configured task-id frontmatter key, or
/// `None` when task IDs are off — the property is then never read, so a
/// disabled vault pays no extra cost and `TaskItem.id` is always `None`.
///
/// A PRESENTATION function in two ways that make it wrong for a hierarchy
/// guard: `status: archived` Tasks are dropped, and a file that can't be read
/// is silently skipped. A guard (the cycle index, the id-settings guard) must
/// see every `parent-id` edge or a cycle can slip through validation — use
/// `list_tasks_structural` there instead.
pub fn list_tasks(root: &Path, id_property: Option<&str>) -> Vec<TaskItem> {
    // `scan` only ever returns Err in Structural mode — View never fails.
    let mut out = scan(root, id_property, ScanMode::View).unwrap_or_default();
    sort_tasks(&mut out);
    out
}

/// The STRUCTURAL counterpart of `list_tasks`, for a hierarchy guard: the
/// SAME walk (never copied — thread new modes through `ScanMode` instead),
/// but it INCLUDES `status: archived` Tasks (their files still carry
/// `parent-id`, and a cycle routed through one must still be visible to the
/// guard) and FAILS the whole scan — naming the offending path — when any
/// `.md` file cannot be read, rather than silently dropping it. A file's
/// `type:` can't be checked without reading it, so an unreadable `.md` is
/// treated as a POSSIBLE Task: dropping it would drop a possible hierarchy
/// edge, and a missing edge is exactly what lets a cycle pass validation and
/// get written (Codex P2, PR #77). The rule: a view may degrade; a guard must
/// refuse.
pub fn list_tasks_structural(
    root: &Path,
    id_property: Option<&str>,
) -> Result<Vec<TaskItem>, String> {
    let mut out = scan(root, id_property, ScanMode::Structural)?;
    sort_tasks(&mut out);
    Ok(out)
}

/// Which of the two callers is walking. One enum instead of two independent
/// bools, so the two combinations actually used — lenient+presentation,
/// strict+structural — can't drift apart from a third nobody asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScanMode {
    View,
    Structural,
}

impl ScanMode {
    /// Structural keeps `status: archived` Tasks; View drops them.
    pub(super) fn include_archived(self) -> bool {
        self == ScanMode::Structural
    }

    /// Structural aborts the whole scan on the first unreadable `.md` file;
    /// View skips it and keeps going (today's degrade-silently behavior).
    pub(super) fn strict(self) -> bool {
        self == ScanMode::Structural
    }
}

/// The ONE walk both `list_tasks` and `list_tasks_structural` drive over
/// `crate::vault_walk` (canonical containment, cycle set, dot-dir skip,
/// single-sourced with the search scan) — do not copy it; a future mode
/// belongs in `ScanMode`. Structural mode stops at the first unreadable file
/// (`Flow::Stop`) instead of scanning the rest of a vault it already knows it
/// must reject.
fn scan(root: &Path, id_property: Option<&str>, mode: ScanMode) -> Result<Vec<TaskItem>, String> {
    let canon_root = match std::fs::canonicalize(root) {
        Ok(p) => p,
        // A missing tasks folder is legitimately empty in EITHER mode — a
        // vault that has never created one has no graph to protect yet
        // (finding 1). Any OTHER root failure (EACCES, an unavailable
        // network share) must not read as "no tasks" in Structural mode: a
        // settings guard would then conclude no parent links exist and
        // permit an unsafe id-property change on the strength of a scan
        // that never actually ran. View mode keeps today's exact
        // best-effort/empty behavior regardless of the error kind.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound || !mode.strict() => {
            return Ok(Vec::new())
        }
        Err(e) => return Err(format!("Cannot resolve tasks folder: {e}")),
    };
    let mut out = Vec::new();
    let mut first_error: Option<String> = None;
    let unreadable_dirs = crate::vault_walk::walk_vault(&canon_root, &mut |path, name| {
        collect_task_file(
            path,
            name,
            &canon_root,
            id_property,
            mode,
            &mut first_error,
            &mut out,
        );
        if first_error.is_some() {
            crate::vault_walk::Flow::Stop
        } else {
            crate::vault_walk::Flow::Continue
        }
    });
    if let Some(e) = first_error {
        return Err(e);
    }
    // A directory the walk could not fully enumerate hides possible
    // `parent-id` edges just as an unreadable FILE does — in Structural mode
    // that must refuse rather than report a silently-partial graph as
    // complete (finding 2). View mode (list_tasks) ignores it, same as
    // always.
    if mode.strict() {
        if let Some(first) = unreadable_dirs.first() {
            return Err(format!("Cannot fully scan the tasks folder: {first}"));
        }
    }
    Ok(out)
}

/// Open first. Open tasks: due ascending (no/invalid due last), then
/// priority tier, then newest created, then title. Done tasks ignore due —
/// newest created first, then title. Clock-free: "overdue"/"today" need a
/// clock, so bucketing is the frontend's job, not the sort's. Shared by both
/// entry points so they can never disagree on order.
fn sort_tasks(out: &mut [TaskItem]) {
    out.sort_by(|a, b| {
        a.done.cmp(&b.done).then_with(|| {
            if a.done {
                b.created
                    .cmp(&a.created)
                    .then_with(|| a.title.cmp(&b.title))
            } else {
                due_key(a)
                    .cmp(&due_key(b))
                    .then_with(|| {
                        priority_rank(a.priority.as_deref())
                            .cmp(&priority_rank(b.priority.as_deref()))
                    })
                    .then_with(|| b.created.cmp(&a.created))
                    .then_with(|| a.title.cmp(&b.title))
            }
        })
    });
}

#[cfg(test)]
mod tests {
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
}
