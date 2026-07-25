//! The per-file parse: what ONE task file yields. Read a candidate `.md`,
//! keep it only if it is a `type: Task` document, and map it to a
//! `TaskItem`. `list.rs` owns the walk — which files get visited, and in
//! what order; this module only ever sees one file at a time.

use super::doc::{is_markdown_name, is_task};
use super::list::{ScanMode, TaskItem};
use super::parse::{is_valid_due, note_tags, scalar_field, scalar_id_ci};
use crate::capture_note::note_field;
use std::path::Path;

/// The per-file half of the shared walk: read, keep `type: Task` files, map
/// to a TaskItem. In View mode (`mode.strict()` false), an unreadable file or
/// a non-task degrades silently, matching `list_tasks`'s historical behavior
/// exactly. In Structural mode, a non-task still degrades silently (it isn't
/// a hierarchy edge), but an UNREADABLE file records the first error into
/// `first_error` — the caller aborts the walk as soon as it sees one set.
pub(super) fn collect_task_file(
    path: &Path,
    name: &str,
    canon_root: &Path,
    id_property: Option<&str>,
    mode: ScanMode,
    first_error: &mut Option<String>,
    out: &mut Vec<TaskItem>,
) {
    // Case-insensitive, matching search.rs's own note scan (AGENTS.md:
    // "notes are any-case `.md`") — an exact-case compare here made a
    // hand-authored `B.MD` Task invisible to the STRUCTURAL scan too, so its
    // `parent-id` edges didn't exist as far as the cycle guard was
    // concerned (review finding 4).
    if !is_markdown_name(name) {
        return;
    }
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            if mode.strict() {
                // We can't tell whether this was a Task without reading it, so
                // treat any unreadable `.md` as a POSSIBLE Task rather than
                // silently dropping a possible `parent-id` edge (Codex P2,
                // PR #77). Only the FIRST error is kept — the caller stops the
                // walk as soon as it sees one, so there is never a second.
                first_error.get_or_insert_with(|| format!("{}: {e}", path.display()));
            }
            return;
        }
    };
    if !is_task(&content) {
        return;
    }
    // `Path::file_stem` splits on the last `.` structurally, so it strips
    // any-case `.md`/`.MD`/`.Md` alike — unlike the case-SENSITIVE
    // `strip_suffix(".md")` this replaced, which left a mixed-case
    // extension in the fallback title even though `is_markdown_name` above
    // already treats the file as a task. Matches every sibling file_stem
    // fallback in this domain (`duplicate_task` in structural.rs, the
    // list-move stem preserve in lists/relocate.rs, the announce-title
    // fallback in services/tasks/mod.rs, and `read_title` in
    // services/tasks/parent/mod.rs) so one file gets one title everywhere.
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    let title = note_field(&content, "title").unwrap_or(stem);
    let status = scalar_field(&content, "status").unwrap_or_else(|| "new".to_string());
    // Archived tasks are removed from the VIEW — never surfaced by
    // `list_tasks`. The structural scan keeps them: their files still carry
    // `parent-id`, and a cycle routed through one must still be visible.
    if status == "archived" && !mode.include_archived() {
        return;
    }
    let created = scalar_field(&content, "created").unwrap_or_default();
    let due = scalar_field(&content, "due");
    // Filter through the date validator so a malformed value (e.g. "next week")
    // becomes None at the DTO/MCP boundary, honoring the "invalid → None"
    // contract in CORE — not only in the frontend's scheduledOf (Codex, PR #75).
    let scheduled = scalar_field(&content, "scheduled").filter(|s| is_valid_due(s));
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
    let parent_id = super::parent::parent_id_field(&content);
    let parent_link = super::parent::parent_link_field(&content);
    // Case-insensitive, top-level-only via `scalar_id_ci`, which agrees with
    // the id-stamp path: a task stamped under a different casing still surfaces
    // (Codex review, PR #59), a blank `task-id:` counts as ABSENT, and a
    // NON-SCALAR value (a block or flow collection) is NOT surfaced as an id —
    // else a duplicate that preserved a flow-valued property (never-clobber)
    // would read as sharing the source's stable id (Codex P2, PR #76).
    let id = id_property.and_then(|p| scalar_id_ci(&content, p));
    out.push(TaskItem {
        path: path.to_path_buf(),
        title,
        status,
        created,
        done,
        due,
        scheduled,
        priority,
        tags,
        list,
        order,
        id,
        description: super::description::description_field(&content),
        parent_id,
        parent_link,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    // list_tasks lives in the sibling `list` module. `super::list_tasks` won't
    // resolve HERE: inside this nested `tests` module `super` means `collect`,
    // not `tasks` (the same nesting gotcha disk.rs's tests document) — so this
    // reaches it via the crate-root re-export instead.
    use crate::tasks::list_tasks;

    fn write(root: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(name), body).unwrap();
    }

    #[test]
    fn title_fallback_strips_a_mixed_case_md_extension() {
        // `is_markdown_name` (doc.rs) was made case-insensitive so a
        // hand-authored "Upper.MD" is walked as a task at all (AGENTS.md:
        // "notes are any-case `.md`", the same rule search.rs's own note
        // scan applies) — but this file's TITLE FALLBACK still stripped
        // ".md" case-sensitively, so an untitled "Upper.MD" surfaced as
        // "Upper.MD" instead of "Upper". That disagrees with every sibling
        // file_stem-based fallback in this domain that already strips
        // any-case extensions by construction: `duplicate_task`
        // (structural.rs), the list-move stem preserve (lists/relocate.rs),
        // the announce-title fallback (services/tasks/mod.rs), and the
        // parent-link label fallback (`read_title`,
        // services/tasks/parent/mod.rs) — so the same file was labelled two
        // different ways in two different places.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "Upper.MD", "---\ntype: Task\nstatus: new\n---\n");
        let items = list_tasks(root, None);
        assert_eq!(items[0].title, "Upper");
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
        let items = list_tasks(root, None);
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
        let titles: Vec<String> = list_tasks(root, None)
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(titles, vec!["Good"]);
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
        let items = list_tasks(root, None);
        assert_eq!(items[0].due.as_deref(), Some("2026-07-15"));
        assert_eq!(items[0].priority.as_deref(), Some("high"));
    }

    #[test]
    fn list_tasks_reads_scheduled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "t.md",
            "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\nscheduled: 2026-07-20\n---\n",
        );
        write(
            root,
            "u.md",
            "---\ntype: Task\nstatus: new\ntitle: \"U\"\ncreated: 2026-07-08\n---\n",
        );
        // A malformed value must degrade to None IN CORE (not just the
        // frontend) so TaskDto/MCP never expose it (Codex, PR #75).
        write(
            root,
            "m.md",
            "---\ntype: Task\nstatus: new\ntitle: \"M\"\ncreated: 2026-07-08\nscheduled: next week\n---\n",
        );
        let items = list_tasks(root, None);
        let sched = |title: &str| {
            items
                .iter()
                .find(|t| t.title == title)
                .unwrap()
                .scheduled
                .clone()
        };
        assert_eq!(sched("T"), Some("2026-07-20".to_string()));
        assert_eq!(sched("U"), None); // absent → None
        assert_eq!(sched("M"), None); // malformed → None (filtered in core)
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
        let items = list_tasks(root, None);
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
        assert_eq!(list_tasks(root, None)[0].tags, vec!["work"]);
    }

    #[test]
    fn list_tasks_reads_the_configured_id_property_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "t.md", "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\ntask-id: abc12345\n---\n");
        assert_eq!(
            list_tasks(root, Some("task-id"))[0].id.as_deref(),
            Some("abc12345")
        );
        assert_eq!(list_tasks(root, None)[0].id, None); // off → no read
    }

    #[test]
    fn list_tasks_reads_the_id_property_case_insensitively() {
        // Codex PR #59: the id STAMP path detects an existing id under ANY
        // casing (via scalar_field_ci), but list_tasks once read it with
        // scalar_field's exact-case lookup — so a task stamped `Task-ID:` while
        // the vault resolves the property to `task-id` had a stable id on disk
        // that was invisible in TaskDto.id (dead to the UI/MCP and the copy-id
        // feature). Both now share scalar_field_ci, so read agrees with write.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "upper.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Upper\"\ncreated: 2026-07-08\nTask-ID: abc12345\n---\n",
        );
        write(
            root,
            "exact.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Exact\"\ncreated: 2026-07-08\ntask-id: xyz\n---\n",
        );
        // A NESTED indented `task-id` under a mapping is NOT the top-level
        // property — scalar_field_ci is top-level-only, the same discipline the
        // id-stamp uses — so this file carries no usable id at all.
        write(
            root,
            "nested.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Nested\"\ncreated: 2026-07-08\nmetadata:\n  task-id: nope\n---\n",
        );

        let items = list_tasks(root, Some("task-id"));
        let id_of = |title: &str| items.iter().find(|t| t.title == title).unwrap().id.clone();
        assert_eq!(id_of("Upper"), Some("abc12345".to_string())); // case-insensitive win
        assert_eq!(id_of("Exact"), Some("xyz".to_string())); // exact case: no regression
        assert_eq!(id_of("Nested"), None); // nested key never counts as top-level

        // Feature off (None property) never reads, regardless of on-disk casing.
        let off = list_tasks(root, None);
        assert_eq!(off.iter().find(|t| t.title == "Upper").unwrap().id, None);
    }

    #[test]
    fn list_tasks_treats_a_blank_id_property_as_absent() {
        // A bare `task-id:` (an Obsidian property panel / template leaves the
        // key valueless) reads as Some("") through scalar_id_ci's inner read.
        // The STAMP path treats that as missing and generates; the read must
        // agree — surfacing "" as TaskDto.id would hand the UI/MCP an unusable
        // id until the next edit (review, PR #59).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "blank.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Blank\"\ncreated: 2026-07-08\ntask-id:\n---\n",
        );
        assert_eq!(list_tasks(root, Some("task-id"))[0].id, None);
    }

    #[test]
    fn list_tasks_surfaces_the_parent_pair() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "c.md", "---\ntype: Task\nstatus: new\ntitle: \"C\"\ncreated: 2026-07-25\nparent-id: ab12cd34\nparent: \"[[Tasks/p]]\"\n---\n");
        let out = list_tasks(root, None);
        assert_eq!(out[0].parent_id.as_deref(), Some("ab12cd34"));
        assert_eq!(out[0].parent_link.as_deref(), Some("[[Tasks/p]]"));
    }

    #[test]
    fn parent_id_is_surfaced_even_when_ids_are_disabled() {
        // `id_property = None` (feature off) must NOT suppress parent-id: the
        // service validates against dormant ids, and the row still needs to
        // know it has a parent. Only the task's OWN id is gated.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "c.md", "---\ntype: Task\nstatus: new\ntitle: \"C\"\ncreated: 2026-07-25\ntask-id: aaa11111\nparent-id: ab12cd34\n---\n");
        let out = list_tasks(root, None);
        assert_eq!(out[0].id, None); // own id gated, as today
        assert_eq!(out[0].parent_id.as_deref(), Some("ab12cd34")); // parent NOT gated
    }

    #[test]
    fn parent_index_resolves_an_edge_through_matching_quoted_ids() {
        // Defect A regression: a task's OWN id is read via `scalar_id_ci`
        // (parse.rs), a `parent-id` reference via `parent::parent_id_field`
        // (parent.rs) — the two must decode an identical on-disk YAML scalar
        // to the identical string, or `parent_index` can never resolve the
        // edge. `'a''b'` is the YAML doubled-single-quote escape for `a'b`;
        // the old `scalar_id_ci` (via `scalar_field`'s shallow one-layer
        // quote strip) decoded it to `a''b` instead, so it never matched the
        // parent-id side's full decode and the edge silently vanished.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "parent.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Parent\"\ncreated: 2026-07-25\ntask-id: 'a''b'\n---\n",
        );
        write(
            root,
            "child.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Child\"\ncreated: 2026-07-25\nparent-id: 'a''b'\n---\n",
        );
        let items = list_tasks(root, Some("task-id"));
        let parent_item = items.iter().find(|t| t.title == "Parent").unwrap();
        let child_item = items.iter().find(|t| t.title == "Child").unwrap();
        // Both sides must decode to the SAME string.
        assert_eq!(parent_item.id.as_deref(), Some("a'b"));
        assert_eq!(child_item.parent_id.as_deref(), Some("a'b"));

        let idx = crate::tasks::parent_index(&items);
        assert_eq!(
            idx.get(child_item.path.as_path()),
            Some(&parent_item.path.as_path()),
            "the child must resolve a real edge to the parent"
        );
    }

    #[test]
    fn list_tasks_does_not_surface_a_non_scalar_id_as_an_id() {
        // A block- or flow-valued id property is the user's structure, not a
        // stable id — it must read as None so a duplicate that preserved a flow
        // value can't appear to share the source's id (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "flow.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Flow\"\ncreated: 2026-07-08\ntask-id: {source: jira}\n---\n",
        );
        write(
            root,
            "block.md",
            "---\ntype: Task\nstatus: new\ntitle: \"Block\"\ncreated: 2026-07-08\ntask-id:\n  source: jira\n---\n",
        );
        let items = list_tasks(root, Some("task-id"));
        assert_eq!(items.len(), 2);
        for t in &items {
            assert_eq!(t.id, None, "{}: a non-scalar id must not surface", t.title);
        }
    }
}
