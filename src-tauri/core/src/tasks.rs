//! Task documents: `type: Task` markdown files under a vault's tasks folder.
//! Pure filename/render/parse logic + the two sanctioned vault writes
//! (collision-safe create; surgical `status:` flip). Same never-clobber
//! discipline as the capture note and transcript sidecar. See
//! docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md.

use crate::capture_note::note_field;
use crate::capture_note::yaml_quote;
use crate::transcript::dir_entries;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Lower-case, collapse every run of non-alphanumeric chars to a single
/// hyphen, cap the length (so the filename component stays inside Windows'
/// 255-char segment / ~260-char MAX_PATH limits — the full title survives in
/// frontmatter), trim leading/trailing hyphens. Empty result → "task".
fn slugify(title: &str) -> String {
    const MAX_SLUG: usize = 80;
    let mut slug = String::new();
    let mut prev_hyphen = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.extend(ch.to_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    // slug is ASCII (alnum + '-'), so truncating by byte index is char-safe.
    slug.truncate(MAX_SLUG);
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "task".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `YYYY-MM-DD-<slug>` (no extension). `today` is supplied by the shell so
/// the core stays clock-free and testable.
pub fn task_basename(title: &str, today: &str) -> String {
    format!("{today}-{}", slugify(title))
}

/// A `type: Task` document with an empty body. `type`/`status`/`created` are
/// simple unquoted scalars; the user-supplied title is quoted so a colon or
/// quote can't break the frontmatter (read back by `capture_note::note_field`).
pub fn render_task(title: &str, created: &str) -> String {
    format!(
        "---\ntype: Task\nstatus: new\ntitle: {}\ncreated: {created}\n---\n\n",
        yaml_quote(title)
    )
}

/// Create a new task file under `root` (creating `root` if needed). Uses the
/// collision-safe atomic writer shared with the capture note, so it can never
/// overwrite an existing file — a name clash takes the ` (N)` suffix instead.
pub fn create_task(root: &Path, title: &str, today: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    crate::capture_note::write_note_collision_safe(&target, &render_task(title, today))
}

/// One task surfaced in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskItem {
    pub path: PathBuf,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
}

/// True iff the leading `---` frontmatter block is properly closed. A block
/// that opens but never closes is malformed: `note_field` would still read its
/// keys, but the surgical `set_status` write refuses it (no closing fence to
/// anchor an insert). Requiring closure keeps `is_task` consistent between the
/// list and the toggle — the list must not surface a row the toggle rejects.
fn has_closed_frontmatter(content: &str) -> bool {
    let mut lines = content.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    lines.any(|line| line.trim_end() == "---")
}

/// True iff the file's leading frontmatter declares `type: Task` AND that
/// frontmatter block is properly closed — a malformed, never-closed block is
/// not surfaced as a task (it can't be toggled either; see `set_status`).
pub fn is_task(content: &str) -> bool {
    has_closed_frontmatter(content) && note_field(content, "type").as_deref() == Some("Task")
}

/// Every `type: Task` file anywhere under `root`, best-effort — the configured
/// tasks folder is walked recursively so tasks organized into subfolders are
/// all surfaced. Open tasks (status != "done") first, newest `created` first
/// with title as tiebreaker; completed tasks after. A missing/unreadable root
/// or file degrades silently.
pub fn list_tasks(root: &Path) -> Vec<TaskItem> {
    let mut out = Vec::new();
    // Canonicalize the root so every descended subdirectory can be
    // containment-checked against it, and track walked dirs so a reparse-point
    // cycle can't loop forever. A missing/unresolvable root → empty list
    // (best-effort, unchanged).
    if let Ok(canon_root) = std::fs::canonicalize(root) {
        let mut walked = HashSet::new();
        collect_tasks(&canon_root, &canon_root, &mut walked, &mut out);
    }
    // Open first; within each group newest created first, then title. Sorting
    // once here (not per directory) orders the whole subtree as one list.
    out.sort_by(|a, b| {
        a.done
            .cmp(&b.done)
            .then(b.created.cmp(&a.created))
            .then(a.title.cmp(&b.title))
    });
    out
}

/// Recursively collect `type: Task` files under `dir` (a canonical path) into
/// `out`, best-effort. A subdirectory is descended only after canonicalizing it
/// and confirming it still resolves under `canon_root` — so a reparse point (a
/// symlink, or a Windows junction, which `dir_entries`' file type can report as
/// a plain directory) that leads OUTSIDE the tasks folder is never walked. Each
/// walked directory is recorded in `walked` so a reparse point pointing back
/// INSIDE the folder can't recurse forever. Dot-directories (`.obsidian`,
/// `.trash`, `.git`, …) are skipped so config dirs aren't walked and trashed
/// tasks aren't surfaced. Unreadable/unresolvable dirs and files degrade
/// silently.
fn collect_tasks(
    dir: &Path,
    canon_root: &Path,
    walked: &mut HashSet<PathBuf>,
    out: &mut Vec<TaskItem>,
) {
    if !walked.insert(dir.to_path_buf()) {
        return; // already walked — guards against a reparse-point cycle
    }
    for (path, ft, name) in dir_entries(dir) {
        if ft.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            // Resolve the child through any symlink/junction and require it to
            // stay inside the tasks folder before descending — the no-follow
            // dirent file type can't be trusted for a junction on Windows.
            // Unresolvable or escaping → skip.
            match std::fs::canonicalize(&path) {
                Ok(child) if child.starts_with(canon_root) => {
                    collect_tasks(&child, canon_root, walked, out)
                }
                _ => continue,
            }
            continue;
        }
        if !ft.is_file() || !name.ends_with(".md") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !is_task(&content) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
        let title = note_field(&content, "title").unwrap_or(stem);
        let status = note_field(&content, "status").unwrap_or_else(|| "new".to_string());
        let created = note_field(&content, "created").unwrap_or_default();
        let done = status == "done";
        out.push(TaskItem {
            path,
            title,
            status,
            created,
            done,
        });
    }
}

/// Return `content` with the frontmatter `status:` line set to `new_status`,
/// preserving every other line and its exact ending. If the frontmatter has no
/// `status:` line, insert one at the closing fence (a hand-authored `type:
/// Task` file the list surfaces must stay toggleable). `None` if the file is
/// not `type: Task`, or if its frontmatter block never closes (malformed) —
/// in that case there is no safe insertion point, so we refuse rather than
/// guess — then the caller skips + warns.
pub fn set_status(content: &str, new_status: &str) -> Option<String> {
    if !is_task(content) {
        return None;
    }
    // The inserted status line needs its own terminator so it can't glue onto
    // the closing fence when that fence lacks a trailing newline. Match the
    // document's existing convention.
    let nl = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    // Split keeping line endings so CRLF is preserved verbatim.
    let mut out = String::with_capacity(content.len() + 16);
    let mut in_frontmatter = false;
    let mut seen_open = false;
    let mut done = false;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if !seen_open {
            // First line is the opening `---` (guaranteed by is_task).
            seen_open = true;
            in_frontmatter = true;
            out.push_str(line);
            continue;
        }
        // `trimmed.trim_end()` (not just the CR/LF-stripped `trimmed`) so a
        // closing fence with trailing whitespace (`---  `) is recognized here
        // too — `is_task`/`note_field` accept it, so the toggle must agree or a
        // listed row becomes un-toggleable.
        if in_frontmatter && trimmed.trim_end() == "---" {
            // Closing fence: if no status line was found, insert one now. The
            // inserted line gets its own `nl` terminator — never the fence
            // line's ending — so it can't glue onto the fence when the fence
            // has no trailing newline (e.g. end of file).
            if !done {
                out.push_str(&format!("status: {new_status}{nl}"));
                done = true;
            }
            in_frontmatter = false;
            out.push_str(line);
            continue;
        }
        if in_frontmatter && !done && trimmed.starts_with("status:") {
            let ending = &line[trimmed.len()..]; // "\r\n", "\n", or ""
            out.push_str(&format!("status: {new_status}{ending}"));
            done = true;
            continue;
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// Flip a task's completion status on disk. Canonicalizes `root` and `path`
/// and requires containment — a lexical check can't see through a symlink at
/// the file or folder — then reads, applies `set_status`, and writes atomically
/// (hidden `create_new` temp + fsync + REPLACING rename). Replacing is correct
/// here: the target is the `type: Task` file we just read and are editing in
/// place, and we touch only its status line (see the spec's surgical-write rule).
pub fn set_task_status(root: &Path, path: &Path, done: bool) -> Result<(), String> {
    // Canonical containment: resolve both and require the file under the root.
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    let updated = set_status(&content, if done { "done" } else { "new" }).ok_or(
        "Task frontmatter could not be updated (not a type: Task document, or its frontmatter is malformed)",
    )?;
    // The REPLACING atomic write (temp + fsync + rename) is shared with the
    // transcript sidecar; replacing is correct here because `canon_path` is the
    // `type: Task` file we just read and are editing in place, touching only its
    // status line.
    crate::capture_note::write_atomic_replacing(&canon_path, &updated)
        .map_err(|e| format!("Cannot save task: {e}"))
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
    fn list_tasks_missing_root_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(list_tasks(&dir.path().join("nope")).is_empty());
    }

    #[test]
    fn is_task_only_true_for_type_task() {
        assert!(is_task("---\ntype: Task\nstatus: new\n---\n"));
        assert!(is_task("---\ntype: \"Task\"\n---\n")); // quoted also fine
        assert!(!is_task("---\ntype: Meeting\n---\n"));
        assert!(!is_task("no frontmatter"));
    }

    #[test]
    fn is_task_false_for_unterminated_frontmatter() {
        // A type: Task block that never closes is malformed: set_status refuses
        // to toggle it, so the list must not surface it as a task either — the
        // list and the toggle must agree on what counts as a task.
        assert!(!is_task("---\ntype: Task\ntitle: \"x\"\n"));
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
    fn basename_slugifies_title_with_date() {
        assert_eq!(
            task_basename("Buy milk", "2026-07-08"),
            "2026-07-08-buy-milk"
        );
        assert_eq!(
            task_basename("  Prepare  Release: cutover!! ", "2026-07-08"),
            "2026-07-08-prepare-release-cutover"
        );
    }

    #[test]
    fn basename_empty_slug_falls_back_to_task() {
        // A title of only punctuation must still yield a usable filename.
        assert_eq!(task_basename("!!!", "2026-07-08"), "2026-07-08-task");
    }

    #[test]
    fn basename_caps_long_slug_for_filesystem_limits() {
        // A very long title must not overflow a Windows path component (255)
        // and blow the ~260-char default MAX_PATH. Slug is capped; the full
        // title still lives in frontmatter (render_task, not the filename).
        let base = task_basename(&"a".repeat(300), "2026-07-08");
        let slug = base.strip_prefix("2026-07-08-").unwrap();
        assert!(
            slug.len() <= 80,
            "slug should be capped, got {}",
            slug.len()
        );
        assert!(slug.chars().all(|c| c == 'a'));
    }

    #[test]
    fn render_writes_type_task_status_new_quoted_title() {
        let doc = render_task("Buy milk", "2026-07-08");
        assert_eq!(
            doc,
            "---\ntype: Task\nstatus: new\ntitle: \"Buy milk\"\ncreated: 2026-07-08\n---\n\n"
        );
    }

    #[test]
    fn render_quotes_a_colon_title() {
        // A colon in the title would break unquoted YAML — must be quoted.
        let doc = render_task("Ship: v1", "2026-07-08");
        assert!(doc.contains("title: \"Ship: v1\"\n"));
    }

    #[test]
    fn render_quotes_and_escapes_special_title() {
        // A title with a quote and backslash must be escaped so it can't break
        // the frontmatter (read back by note_field).
        let doc = render_task("a\"b\\c", "2026-07-08");
        assert!(doc.contains("title: \"a\\\"b\\\\c\"\n"));
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
    fn create_task_writes_file_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");

        let p1 = create_task(&root, "Buy milk", "2026-07-08").unwrap();
        assert_eq!(p1.file_name().unwrap(), "2026-07-08-buy-milk.md");
        let body = std::fs::read_to_string(&p1).unwrap();
        assert!(body.contains("type: Task"));
        assert!(body.contains("status: new"));
        assert!(body.contains("title: \"Buy milk\""));

        // Same title again → suffixed, original untouched (collision-safe).
        let p2 = create_task(&root, "Buy milk", "2026-07-08").unwrap();
        assert_ne!(p1, p2);
        assert_eq!(p2.file_name().unwrap(), "2026-07-08-buy-milk (2).md");
        assert!(p1.exists() && p2.exists());
    }

    #[test]
    fn set_status_flips_only_the_status_line_preserving_body() {
        let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-08\n---\n\nSome body\n- [ ] sub\n";
        let flipped = set_status(doc, "done").unwrap();
        assert!(flipped.contains("status: done\n"));
        assert!(!flipped.contains("status: new\n"));
        // Everything else byte-for-byte intact.
        assert!(flipped.contains("title: \"A\"\n"));
        assert!(flipped.contains("created: 2026-07-08\n"));
        assert!(flipped.contains("\nSome body\n- [ ] sub\n"));
    }

    #[test]
    fn set_status_preserves_crlf_endings() {
        let doc = "---\r\ntype: Task\r\nstatus: new\r\ntitle: \"A\"\r\n---\r\n\r\nbody\r\n";
        let flipped = set_status(doc, "done").unwrap();
        assert!(flipped.contains("status: done\r\n"));
        assert!(flipped.contains("body\r\n"));
    }

    #[test]
    fn set_status_refuses_non_task() {
        // Not our document — never rewrite it.
        assert!(set_status("---\ntype: Meeting\nstatus: new\n---\n", "done").is_none());
        assert!(set_status("no frontmatter", "done").is_none());
    }

    #[test]
    fn set_status_inserts_line_when_missing() {
        // A hand-authored type: Task with no status line is surfaced in the list
        // as an unchecked row, so it MUST become toggleable — insert the status.
        let doc = "---\ntype: Task\ntitle: \"x\"\n---\n\nbody\n";
        let out = set_status(doc, "done").unwrap();
        assert!(out.contains("status: done\n"));
        assert!(out.contains("title: \"x\"\n"));
        assert!(out.contains("\nbody\n"));
    }

    #[test]
    fn set_status_inserts_line_when_missing_no_trailing_newline() {
        // Regression: a hand-authored task with no status line AND no trailing
        // newline after the closing fence must not glue status onto the fence.
        let doc = "---\ntype: Task\ntitle: \"x\"\n---";
        let out = set_status(doc, "done").unwrap();
        assert_eq!(out, "---\ntype: Task\ntitle: \"x\"\nstatus: done\n---");
    }

    #[test]
    fn set_status_insert_preserves_crlf_when_missing() {
        let doc = "---\r\ntype: Task\r\ntitle: \"x\"\r\n---\r\n";
        let out = set_status(doc, "done").unwrap();
        assert!(out.contains("status: done\r\n"));
        assert!(out.contains("---\r\n"));
    }

    #[test]
    fn set_status_none_for_unterminated_frontmatter() {
        // Opening fence + type: Task but no closing --- : malformed; refuse
        // rather than guess where to insert (documented narrow contract).
        let doc = "---\ntype: Task\ntitle: \"x\"\n";
        assert!(set_status(doc, "done").is_none());
    }

    #[test]
    fn set_status_inserts_when_closing_fence_has_trailing_whitespace() {
        // Regression: is_task/note_field accept a closing fence with trailing
        // whitespace, so set_status must too — otherwise a listed status-less
        // task would be un-toggleable (set_status returns None, toggle errors).
        let doc = "---\ntype: Task\ntitle: \"x\"\n---  \n";
        assert!(is_task(doc)); // the list surfaces it…
        let out = set_status(doc, "done").unwrap(); // …so the toggle must accept it
        assert!(out.contains("status: done\n"));
        assert!(out.contains("---  \n")); // fence preserved verbatim
    }

    #[test]
    fn set_task_status_writes_and_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "Buy milk", "2026-07-08").unwrap();

        set_task_status(&root, &p, true).unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("status: done\n"));
        set_task_status(&root, &p, false).unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("status: new\n"));

        // A path outside the root is refused.
        let outside = dir.path().join("outside.md");
        std::fs::write(&outside, "---\ntype: Task\nstatus: new\n---\n").unwrap();
        assert!(set_task_status(&root, &outside, true).is_err());
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
        assert!(set_task_status(&root, &link, true).is_err());
    }
}
