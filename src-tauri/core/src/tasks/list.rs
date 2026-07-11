//! The read side: scan the tasks folder, map files to `TaskItem`s, and the
//! clock-free sort ("overdue"/"today" need a clock, so date-bucket grouping
//! is deliberately the frontend's job, not the sort's).

use super::doc::is_task;
use super::parse::{is_valid_due, note_tags, scalar_field};
use crate::capture_note::note_field;
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
    pub priority: Option<String>,
    pub tags: Vec<String>,
    /// The task's List = its parent folder relative to the tasks root, always
    /// `/`-joined ("" at the root) — the identity crosses IPC and merges
    /// across platforms, so it never carries the OS separator.
    pub list: String,
    /// Manual rank from the `order:` frontmatter number; lenient read —
    /// unparseable/non-finite is unranked, never an error.
    pub order: Option<f64>,
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
pub fn list_tasks(root: &Path) -> Vec<TaskItem> {
    let mut out = Vec::new();
    // The walk discipline (canonical containment, cycle set, dot-dir skip)
    // lives in vault_walk, single-sourced with the search scan. A missing/
    // unresolvable root → empty list (best-effort, unchanged).
    if let Ok(canon_root) = std::fs::canonicalize(root) {
        crate::vault_walk::walk_vault(&canon_root, &mut |path, name| {
            collect_task_file(path, name, &canon_root, &mut out);
            crate::vault_walk::Flow::Continue
        });
    }
    // Open first. Open tasks: due ascending (no/invalid due last), then
    // priority tier, then newest created, then title. Done tasks ignore due —
    // newest created first, then title. Clock-free: "overdue"/"today" need a
    // clock, so bucketing is the frontend's job, not the sort's.
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
    out
}

/// The per-file half of the old recursive collector: read, keep `type: Task`
/// files, map to a TaskItem. Unreadable files and non-tasks degrade silently.
fn collect_task_file(path: &Path, name: &str, canon_root: &Path, out: &mut Vec<TaskItem>) {
    if !name.ends_with(".md") {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    if !is_task(&content) {
        return;
    }
    let stem = name.strip_suffix(".md").unwrap_or(name).to_string();
    let title = note_field(&content, "title").unwrap_or(stem);
    let status = scalar_field(&content, "status").unwrap_or_else(|| "new".to_string());
    // Archived tasks are removed from view — never surfaced in the list.
    if status == "archived" {
        return;
    }
    let created = scalar_field(&content, "created").unwrap_or_default();
    let due = scalar_field(&content, "due");
    let priority = scalar_field(&content, "priority");
    let tags = note_tags(&content);
    let done = status == "done";
    // The walk hands canonical paths under the canonical root, so the parent
    // dir's strip_prefix is the task's List for free (no extra I/O).
    let list = path
        .parent()
        .and_then(|dir| dir.strip_prefix(canon_root).ok())
        .map(|rel| {
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let order = scalar_field(&content, "order")
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|f| f.is_finite());
    out.push(TaskItem {
        path: path.to_path_buf(),
        title,
        status,
        created,
        done,
        due,
        priority,
        tags,
        list,
        order,
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

        let items = list_tasks(root);
        let titles: Vec<&str> = items.iter().map(|t| t.title.as_str()).collect();
        // Open tasks first, newest created first; the done task last.
        assert_eq!(titles, vec!["B open", "C open", "A done"]);
        assert!(!items[0].done);
        assert!(items[2].done);
        assert_eq!(items[0].status, "new");
        assert_eq!(items[2].created, "2026-07-06");
    }

    #[test]
    fn list_tasks_strips_inline_comments_from_structured_scalars() {
        // Codex review, PR #46: `due: 2026-07-15 # client` read the comment
        // into the value, so a due Obsidian's Properties UI shows failed
        // is_valid_due and bucketed as no-date; `priority: high # urgent`
        // degraded to normal; `status: done # shipped` counted as open and
        // `status: archived # old` stayed listed. Structured scalars strip
        // comments like the tags reader does. Titles stay raw (free text).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "a.md",
            "---\ntype: Task\nstatus: done # shipped\ntitle: \"A\"\ncreated: 2026-07-06 # early\ndue: 2026-07-15 # client\npriority: high # urgent\n---\n",
        );
        write(
            root,
            "b.md",
            "---\ntype: Task\nstatus: archived # old\ntitle: \"B\"\n---\n",
        );
        // Quoted-then-commented corner: the comment strip must also unwrap
        // the remaining quote pair.
        write(
            root,
            "c.md",
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\ndue: \"2026-07-16\" # quoted\n---\n",
        );
        let items = list_tasks(root);
        let titles: Vec<&str> = items.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["C", "A"]); // archived B gone; done A last
        assert_eq!(items[0].due.as_deref(), Some("2026-07-16"));
        assert!(items[1].done);
        assert_eq!(items[1].status, "done");
        assert_eq!(items[1].created, "2026-07-06");
        assert_eq!(items[1].due.as_deref(), Some("2026-07-15"));
        assert_eq!(items[1].priority.as_deref(), Some("high"));
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
        let titles: Vec<String> = list_tasks(root).into_iter().map(|t| t.title).collect();
        assert_eq!(titles, vec!["Open", "Done"]); // archived is not surfaced
    }

    #[test]
    fn list_tasks_missing_root_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_tasks(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn list_tasks_skips_unterminated_frontmatter() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "2026-07-08-good.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Good\"\ncreated: 2026-07-08\n---\n",
        );
        // Opens `---\ntype: Task` but never closes the block — must not appear.
        write(
            root,
            "2026-07-08-bad.md",
            "---\ntype: Task\ntitle: \"Bad\"\n",
        );
        let titles: Vec<String> = list_tasks(root).into_iter().map(|t| t.title).collect();
        assert_eq!(titles, vec!["Good"]);
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
        let titles: Vec<String> = list_tasks(root).into_iter().map(|t| t.title).collect();
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
        let titles: Vec<String> = list_tasks(root).into_iter().map(|t| t.title).collect();
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
        let titles: Vec<String> = list_tasks(&root).into_iter().map(|t| t.title).collect();
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
        let titles: Vec<String> = list_tasks(&root).into_iter().map(|t| t.title).collect();
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

        let items = list_tasks(root);
        let titles: Vec<&str> = items.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["Apple", "Zebra"]);
    }

    #[test]
    fn list_tasks_reads_due_and_priority() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "t.md",
            "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\ndue: 2026-07-15\npriority: high\n---\n",
        );
        let items = list_tasks(root);
        assert_eq!(items[0].due.as_deref(), Some("2026-07-15"));
        assert_eq!(items[0].priority.as_deref(), Some("high"));
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
        let titles: Vec<String> = list_tasks(root).into_iter().map(|t| t.title).collect();
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
        let items = list_tasks(root);
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
    fn list_tasks_reads_order_leniently() {
        // `order:` is the manual rank — lenient read like every widened field:
        // integers and floats parse, anything else (or absence) is unranked
        // (None), never an error.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-08\norder: 1536\n---\n",
        );
        write(
            root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ncreated: 2026-07-08\norder: 1536.5\n---\n",
        );
        write(
            root,
            "c.md",
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\ncreated: 2026-07-08\norder: soon\n---\n",
        );
        write(
            root,
            "d.md",
            "---\ntype: Task\nstatus: new\ntitle: \"D\"\ncreated: 2026-07-08\n---\n",
        );
        let items = list_tasks(root);
        let by_title = |t: &str| items.iter().find(|i| i.title == t).unwrap().order;
        assert_eq!(by_title("A"), Some(1536.0));
        assert_eq!(by_title("B"), Some(1536.5));
        assert_eq!(by_title("C"), None);
        assert_eq!(by_title("D"), None);
    }

    #[test]
    fn list_tasks_reads_tags() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "t.md",
            "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\ntags:\n- work\n---\n",
        );
        assert_eq!(list_tasks(root)[0].tags, vec!["work"]);
    }
}
