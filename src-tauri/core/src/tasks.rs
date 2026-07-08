//! Task documents: `type: Task` markdown files under a vault's tasks folder.
//! Pure filename/render/parse logic + the two sanctioned vault writes
//! (collision-safe create; surgical `status:` flip). Same never-clobber
//! discipline as the capture note and transcript sidecar. See
//! docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md.

use crate::capture_note::note_field;
use crate::capture_note::yaml_quote;
use crate::transcript::dir_entries;
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
pub fn create_task(root: &Path, title: &str, today: &str) -> std::io::Result<std::path::PathBuf> {
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

/// Every `type: Task` file directly under `root`, best-effort. Open tasks
/// (status != "done") first, newest `created` first with title as tiebreaker;
/// completed tasks after. A missing/unreadable root or file degrades silently.
pub fn list_tasks(root: &Path) -> Vec<TaskItem> {
    let mut out = Vec::new();
    for (path, ft, name) in dir_entries(root) {
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
    // Open first; within each group newest created first, then title.
    out.sort_by(|a, b| {
        a.done
            .cmp(&b.done)
            .then(b.created.cmp(&a.created))
            .then(a.title.cmp(&b.title))
    });
    out
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
        if in_frontmatter && trimmed == "---" {
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

    let dir = canon_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = canon_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Owned temp with suffix-retry — a crash between create_new and rename can
    // strand a predictable temp; without retry every future toggle of the same
    // task would fail AlreadyExists on that stranded temp and be permanently
    // stuck. Mirrors write_note_atomic's numbered-temp loop; the marker suffix
    // lets recovery clean strays.
    use std::io::Write;
    let (tmp, mut f) = {
        let mut attempt = 0u32;
        loop {
            let candidate = if attempt == 0 {
                dir.join(format!(
                    ".{file_name}{}",
                    crate::capture_note::NOTE_TMP_SUFFIX
                ))
            } else {
                dir.join(format!(
                    ".{file_name}.{attempt}{}",
                    crate::capture_note::NOTE_TMP_SUFFIX
                ))
            };
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(f) => break (candidate, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
                Err(e) => return Err(format!("Cannot stage task write: {e}")),
            }
        }
    };
    f.write_all(updated.as_bytes())
        .map_err(|e| format!("Cannot write task: {e}"))?;
    f.sync_all()
        .map_err(|e| format!("Cannot flush task: {e}"))?;
    drop(f);
    let result = std::fs::rename(&tmp, &canon_path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(|e| format!("Cannot save task: {e}"))
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
