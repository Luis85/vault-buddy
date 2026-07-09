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

/// A `type: Task` document with an empty body. `type`/`status`/`created` (and
/// the optional `due`/`priority`) are simple unquoted scalars; the
/// user-supplied title is quoted so a colon or quote can't break the
/// frontmatter. `due`/`priority` lines are written only when present — absent
/// priority means normal, and a bare `due:` is never emitted.
pub fn render_task(
    title: &str,
    created: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
) -> String {
    let mut extra = String::new();
    if let Some(d) = due {
        extra.push_str(&format!("due: {d}\n"));
    }
    if let Some(p) = priority {
        extra.push_str(&format!("priority: {p}\n"));
    }
    if !tags.is_empty() {
        // Canonical flow style: single-line, so the surgical writer can
        // rewrite it; charset-validated tags never need YAML quoting.
        extra.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    format!(
        "---\ntype: Task\nstatus: new\ntitle: {}\ncreated: {created}\n{extra}---\n\n",
        yaml_quote(title)
    )
}

/// Create a new task file under `root` (creating `root` if needed). Uses the
/// collision-safe atomic writer shared with the capture note, so it can never
/// overwrite an existing file — a name clash takes the ` (N)` suffix instead.
pub fn create_task(
    root: &Path,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    crate::capture_note::write_note_collision_safe(
        &target,
        &render_task(title, today, due, priority, tags),
    )
}

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

/// True iff `s` is a plain `YYYY-MM-DD` (digits and hyphens in position — no
/// calendar validity check; Obsidian tolerates e.g. 2026-02-31 and the UI
/// uses a native date picker). Shared by the shell's write validation and the
/// sort's "does this due count" test so they can never disagree.
pub fn is_valid_due(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// True iff `s` is a valid Obsidian tag: letters (any script), digits, `-`,
/// `_`, `/`, and at least one non-digit character. Shared by the lenient
/// read-side normalization (invalid entries are dropped) and the shell's
/// strict write validation (invalid entries are an error) so the two sides
/// can never disagree on what a tag is.
pub fn is_valid_tag(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '/'))
        && s.chars().any(|c| !c.is_ascii_digit())
}

/// Normalize one raw tag token from frontmatter: unquote, trim, strip a
/// leading `#`; None when the result fails `is_valid_tag` (dropped by the
/// lenient reader).
fn normalize_tag(raw: &str) -> Option<String> {
    let unquoted = crate::capture_note::unquote_yaml(raw.trim());
    let t = unquoted.trim();
    let t = t.strip_prefix('#').unwrap_or(t);
    is_valid_tag(t).then(|| t.to_string())
}

/// Case-insensitive dedupe preserving first-seen casing (Obsidian matches
/// tags case-insensitively but displays the authored case).
fn dedupe_tags(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in items {
        if seen.insert(t.to_lowercase()) {
            out.push(t);
        }
    }
    out
}

/// Parse one frontmatter tags-ish key. None when the key is absent; Some of
/// the normalized (possibly empty) list when present — so a present-but-empty
/// `tags:` still shadows the `tag:` alias.
fn parse_tags_key(content: &str, key: &str) -> Option<Vec<String>> {
    let mut lines = content.lines().peekable();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    while let Some(line) = lines.next() {
        if line.trim_end() == "---" {
            return None; // end of frontmatter — the body is never scanned
        }
        // Top-level keys only: an indented list item can't match (leading
        // space), same convention as note_field.
        let Some(rest) = line.strip_prefix(&prefix) else {
            continue;
        };
        let rest = rest.trim();
        let raw_items: Vec<&str> = if rest.is_empty() {
            // Block style: consume the following `- item` lines.
            let mut items = Vec::new();
            while let Some(next) = lines.peek() {
                if next.trim_end() == "---" {
                    break;
                }
                let Some(item) = next.trim_start().strip_prefix("- ") else {
                    break;
                };
                items.push(item);
                lines.next();
            }
            items
        } else if rest.starts_with('[') {
            // Flow `[a, b]` style: strip brackets, split only on commas.
            // An unquoted item with a space (e.g. `[a, two words]`) would fail
            // validation because space is not in the tag charset, so it's
            // dropped by the lenient reader.
            let inner = rest
                .strip_prefix('[')
                .and_then(|r| r.strip_suffix(']'))
                .unwrap_or(rest);
            inner.split(',').map(str::trim).collect()
        } else {
            // Legacy `a, b` / `a b` format: split on commas AND whitespace.
            rest.split(',').flat_map(str::split_whitespace).collect()
        };
        return Some(dedupe_tags(raw_items.into_iter().filter_map(normalize_tag)));
    }
    None
}

/// A task's tags from frontmatter, in every form Obsidian accepts (see
/// parse_tags_key). `tags:` wins; the `tag:` singular alias is read only
/// when `tags:` is absent. Body `#hashtags` are deliberately out of scope —
/// the scanner stays frontmatter-only like the rest of the vault domain.
pub fn note_tags(content: &str) -> Vec<String> {
    parse_tags_key(content, "tags")
        .or_else(|| parse_tags_key(content, "tag"))
        .unwrap_or_default()
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
    // Canonicalize the root so every descended subdirectory can be
    // containment-checked against it, and track walked dirs so a reparse-point
    // cycle can't loop forever. A missing/unresolvable root → empty list
    // (best-effort, unchanged).
    if let Ok(canon_root) = std::fs::canonicalize(root) {
        let mut walked = HashSet::new();
        collect_tasks(&canon_root, &canon_root, &mut walked, &mut out);
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
        // Archived tasks are removed from view — never surfaced in the list.
        if status == "archived" {
            continue;
        }
        let created = note_field(&content, "created").unwrap_or_default();
        let due = note_field(&content, "due");
        let priority = note_field(&content, "priority");
        let tags = note_tags(&content);
        let done = status == "done";
        out.push(TaskItem {
            path,
            title,
            status,
            created,
            done,
            due,
            priority,
            tags,
        });
    }
}

/// Return `content` with the named frontmatter lines updated, preserving every
/// other line and its exact ending. For each `(key, value)`: `Some(v)` rewrites
/// the existing `key:` line in place (first occurrence) or inserts `key: v` at
/// the closing fence; `None` removes the line (a missing line is a no-op).
/// Values are written VERBATIM — the caller quotes user text (`yaml_quote`).
/// `None` result iff the file is not `type: Task` or its frontmatter never
/// closes (no safe anchor; the caller skips + warns) — same contract as the
/// old single-key set_status.
pub fn set_fields(content: &str, updates: &[(&str, Option<&str>)]) -> Option<String> {
    if !is_task(content) {
        return None;
    }
    // Inserted lines need their own terminator so they can't glue onto a
    // fence that lacks a trailing newline. Match the document's convention.
    let nl = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::with_capacity(content.len() + 32 * updates.len());
    let mut handled = vec![false; updates.len()];
    let mut in_frontmatter = false;
    let mut seen_open = false;
    let mut closed = false;
    // True while consuming the `- item` lines of a block-style value whose
    // key was just rewritten/removed — the items belong to the replaced
    // value, so they are dropped with it. Also consumes indented continuation
    // lines (nested-mapping items), since YAML block items can span multiple
    // lines when they are mappings. Cleared by the first non-item,
    // non-indented line (including the closing fence), so body bullets are
    // never at risk: the fence always clears the flag before the body starts,
    // and top-level frontmatter keys and the fence are never indented.
    let mut skip_list_items = false;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if skip_list_items {
            let starts_indented = line.starts_with([' ', '\t']);
            if starts_indented || trimmed.trim_start().starts_with("- ") {
                continue; // item or its indented continuation — part of the consumed value
            }
            skip_list_items = false;
        }
        if !seen_open {
            // First line is the opening `---` (guaranteed by is_task).
            seen_open = true;
            in_frontmatter = true;
            out.push_str(line);
            continue;
        }
        // trim_end() so a closing fence with trailing whitespace is accepted,
        // matching is_task/note_field — the list and the writer must agree.
        if in_frontmatter && trimmed.trim_end() == "---" {
            // Closing fence: insert every not-yet-handled Set here; a pending
            // removal of a line that never existed is simply done.
            for (i, (key, value)) in updates.iter().enumerate() {
                if !handled[i] {
                    if let Some(v) = value {
                        out.push_str(&format!("{key}: {v}{nl}"));
                    }
                    handled[i] = true;
                }
            }
            in_frontmatter = false;
            closed = true;
            out.push_str(line);
            continue;
        }
        if in_frontmatter {
            // Key match requires the colon right after the key so `due` can't
            // rewrite `duedate:`. Only the first occurrence of a key is edited.
            let matched = updates.iter().enumerate().find(|(i, (key, _))| {
                !handled[*i]
                    && trimmed
                        .strip_prefix(*key)
                        .is_some_and(|rest| rest.starts_with(':'))
            });
            if let Some((i, (key, value))) = matched {
                // `key:` with nothing after the colon means the value is a
                // block-style list on the following lines — consume it along
                // with the key line (rewrite and removal alike), so a
                // hand-authored block list round-trips to one flow line
                // instead of leaving orphaned `- item` lines.
                let rest = &trimmed[key.len() + 1..];
                if rest.trim().is_empty() {
                    skip_list_items = true;
                }
                if let Some(v) = value {
                    let ending = &line[trimmed.len()..]; // "\r\n", "\n", or ""
                    out.push_str(&format!("{key}: {v}{ending}"));
                }
                // drop the line (its newline goes with it) if value is None
                handled[i] = true;
                continue;
            }
        }
        out.push_str(line);
    }
    closed.then_some(out)
}

/// Single-key convenience over `set_fields` — kept because the status toggle
/// is the hot path and its list/toggle-agreement tests pin the contract.
pub fn set_status(content: &str, new_status: &str) -> Option<String> {
    set_fields(content, &[("status", Some(new_status))])
}

/// Apply a surgical frontmatter patch to a task file on disk. Canonicalizes
/// `root` and `path` and requires containment — a lexical check can't see
/// through a symlink at the file or folder — then reads, applies `set_fields`,
/// and writes atomically (hidden `create_new` temp + fsync + REPLACING
/// rename). Replacing is correct here: the target is the `type: Task` file we
/// just read and are editing in place, touching only the named lines.
pub fn update_task_fields(
    root: &Path,
    path: &Path,
    updates: &[(&str, Option<&str>)],
) -> Result<(), String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    let updated = set_fields(&content, updates).ok_or(
        "Task frontmatter could not be updated (not a type: Task document, or its frontmatter is malformed)",
    )?;
    crate::capture_note::write_atomic_replacing(&canon_path, &updated)
        .map_err(|e| format!("Cannot save task: {e}"))
}

/// Set a task's `status:` frontmatter on disk (see `update_task_fields`).
pub fn set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String> {
    update_task_fields(root, path, &[("status", Some(new_status))])
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
    fn set_task_status_writes_an_arbitrary_status() {
        // set_task_status now takes a status string, so it can write archived
        // (and still new/done), not just a done bool.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "Buy milk", "2026-07-08", None, None, &[]).unwrap();
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
        let doc = render_task("Buy milk", "2026-07-08", None, None, &[]);
        assert_eq!(
            doc,
            "---\ntype: Task\nstatus: new\ntitle: \"Buy milk\"\ncreated: 2026-07-08\n---\n\n"
        );
    }

    #[test]
    fn render_quotes_a_colon_title() {
        // A colon in the title would break unquoted YAML — must be quoted.
        let doc = render_task("Ship: v1", "2026-07-08", None, None, &[]);
        assert!(doc.contains("title: \"Ship: v1\"\n"));
    }

    #[test]
    fn render_quotes_and_escapes_special_title() {
        // A title with a quote and backslash must be escaped so it can't break
        // the frontmatter (read back by note_field).
        let doc = render_task("a\"b\\c", "2026-07-08", None, None, &[]);
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

        let p1 = create_task(&root, "Buy milk", "2026-07-08", None, None, &[]).unwrap();
        assert_eq!(p1.file_name().unwrap(), "2026-07-08-buy-milk.md");
        let body = std::fs::read_to_string(&p1).unwrap();
        assert!(body.contains("type: Task"));
        assert!(body.contains("status: new"));
        assert!(body.contains("title: \"Buy milk\""));

        // Same title again → suffixed, original untouched (collision-safe).
        let p2 = create_task(&root, "Buy milk", "2026-07-08", None, None, &[]).unwrap();
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
        let p = create_task(&root, "Buy milk", "2026-07-08", None, None, &[]).unwrap();

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
    fn set_fields_updates_multiple_keys_in_one_pass() {
        let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-08\ndue: 2026-07-10\n---\n\nbody\n";
        let out = set_fields(
            doc,
            &[
                ("title", Some("\"B\"")),
                ("due", Some("2026-07-20")),
                ("priority", Some("high")),
            ],
        )
        .unwrap();
        assert!(out.contains("title: \"B\"\n"));
        assert!(out.contains("due: 2026-07-20\n"));
        assert!(out.contains("priority: high\n")); // inserted at the fence
        assert!(out.contains("status: new\n")); // untouched key preserved
        assert!(out.contains("created: 2026-07-08\n"));
        assert!(out.contains("\nbody\n")); // body byte-for-byte
    }

    #[test]
    fn set_fields_removes_a_line_with_none() {
        let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\ndue: 2026-07-10\npriority: low\n---\n\nbody\n";
        let out = set_fields(doc, &[("due", None), ("priority", None)]).unwrap();
        assert!(!out.contains("due:"));
        assert!(!out.contains("priority:"));
        assert!(out.contains("title: \"A\"\n"));
        assert!(out.contains("\nbody\n"));
    }

    #[test]
    fn set_fields_removing_a_missing_key_is_a_no_op() {
        let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n";
        assert_eq!(set_fields(doc, &[("due", None)]).unwrap(), doc);
    }

    #[test]
    fn set_fields_preserves_crlf_and_unknown_keys() {
        let doc = "---\r\ntype: Task\r\nstatus: new\r\ncustom: keep-me\r\n---\r\n\r\nbody\r\n";
        let out = set_fields(doc, &[("due", Some("2026-07-20"))]).unwrap();
        assert!(out.contains("due: 2026-07-20\r\n")); // inserted line matches CRLF
        assert!(out.contains("custom: keep-me\r\n"));
        assert!(out.contains("body\r\n"));
    }

    #[test]
    fn set_fields_refuses_non_task_and_unclosed_fence() {
        assert!(set_fields("---\ntype: Meeting\n---\n", &[("due", Some("x"))]).is_none());
        assert!(set_fields("---\ntype: Task\ntitle: \"x\"\n", &[("due", Some("x"))]).is_none());
    }

    #[test]
    fn set_fields_does_not_match_a_key_prefix() {
        // "due" must not rewrite a "duedate:" line — key match requires the colon
        // immediately after the key.
        let doc = "---\ntype: Task\nstatus: new\nduedate: keep\n---\n";
        let out = set_fields(doc, &[("due", Some("2026-07-20"))]).unwrap();
        assert!(out.contains("duedate: keep\n"));
        assert!(out.contains("due: 2026-07-20\n")); // inserted, not substituted
    }

    #[test]
    fn render_includes_due_and_priority_only_when_present() {
        let plain = render_task("A", "2026-07-09", None, None, &[]);
        assert_eq!(
            plain,
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-09\n---\n\n"
        ); // byte-identical to the pre-due/priority output
        let full = render_task("A", "2026-07-09", Some("2026-07-15"), Some("high"), &[]);
        assert!(full.contains("created: 2026-07-09\ndue: 2026-07-15\npriority: high\n---\n"));
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
    fn is_valid_due_accepts_only_plain_dates() {
        assert!(is_valid_due("2026-07-15"));
        assert!(!is_valid_due("2026-7-15"));
        assert!(!is_valid_due("tomorrow"));
        assert!(!is_valid_due("2026-07-15T10:00"));
        assert!(!is_valid_due(""));
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
    fn is_valid_tag_accepts_obsidian_charset_and_rejects_the_rest() {
        for ok in ["work", "home/errands", "a-b_c", "año", "q3-2026", "1-2"] {
            assert!(is_valid_tag(ok), "{ok} should be valid");
        }
        // all-digits, empty, spaces, punctuation → invalid
        for bad in ["123", "", "two words", "a.b", "#work", "a,b"] {
            assert!(!is_valid_tag(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn note_tags_parses_flow_block_and_legacy_forms() {
        let flow = "---\ntype: Task\ntags: [work, home/errands]\n---\n";
        assert_eq!(note_tags(flow), vec!["work", "home/errands"]);
        let block = "---\ntype: Task\ntags:\n  - work\n  - \"home/errands\"\n---\n";
        assert_eq!(note_tags(block), vec!["work", "home/errands"]);
        let legacy = "---\ntype: Task\ntags: work, home/errands\n---\n";
        assert_eq!(note_tags(legacy), vec!["work", "home/errands"]);
        let spaces = "---\ntype: Task\ntags: work home/errands\n---\n";
        assert_eq!(note_tags(spaces), vec!["work", "home/errands"]);
    }

    #[test]
    fn note_tags_normalizes_and_dedupes() {
        // `#` stripped, invalid entries dropped, case-insensitive dedupe keeps
        // the first-seen casing — lenient read, never an error.
        let doc = "---\ntype: Task\ntags: [#Work, work, 123, two words, urgent]\n---\n";
        assert_eq!(note_tags(doc), vec!["Work", "urgent"]);
    }

    #[test]
    fn note_tags_reads_the_tag_alias_only_when_tags_is_absent() {
        let alias = "---\ntype: Task\ntag: work\n---\n";
        assert_eq!(note_tags(alias), vec!["work"]);
        let both = "---\ntype: Task\ntags: [a1]\ntag: b1\n---\n";
        assert_eq!(note_tags(both), vec!["a1"]); // tags: wins
    }

    #[test]
    fn note_tags_is_empty_without_frontmatter_or_key_and_never_reads_the_body() {
        assert!(note_tags("no frontmatter").is_empty());
        assert!(note_tags("---\ntype: Task\n---\n").is_empty());
        // A `tags:`-looking line in the body must not be read.
        assert!(note_tags("---\ntype: Task\n---\ntags: [body]\n").is_empty());
        // Block list stops at the closing fence.
        let fenced = "---\ntype: Task\ntags:\n- work\n---\n- not-a-tag\n";
        assert_eq!(note_tags(fenced), vec!["work"]);
    }

    #[test]
    fn set_fields_rewrites_a_block_list_to_one_flow_line() {
        // A hand-authored block-style tags list must round-trip to the canonical
        // flow line — orphaned `- item` lines would corrupt the frontmatter.
        let doc =
            "---\ntype: Task\nstatus: new\ntags:\n  - work\n  - home\ntitle: \"A\"\n---\nbody\n";
        let out = set_fields(doc, &[("tags", Some("[urgent]"))]).unwrap();
        assert!(out.contains("tags: [urgent]\n"));
        assert!(!out.contains("- work"));
        assert!(!out.contains("- home"));
        assert!(out.contains("title: \"A\"\n")); // key after the block untouched
        assert!(out.contains("\nbody\n"));
    }

    #[test]
    fn set_fields_removes_a_block_list_entirely() {
        let doc = "---\ntype: Task\nstatus: new\ntags:\n- work\n- home\n---\n";
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert!(!out.contains("tags"));
        assert!(!out.contains("- work"));
        assert!(out.contains("status: new\n"));
    }

    #[test]
    fn set_fields_block_consumption_preserves_crlf() {
        let doc = "---\r\ntype: Task\r\nstatus: new\r\ntags:\r\n  - work\r\n---\r\n";
        let out = set_fields(doc, &[("tags", Some("[home]"))]).unwrap();
        assert!(out.contains("tags: [home]\r\n"));
        assert!(!out.contains("- work"));
        assert!(out.contains("status: new\r\n"));
    }

    #[test]
    fn set_fields_block_list_running_to_the_fence_keeps_the_fence() {
        let doc = "---\ntype: Task\nstatus: new\ntags:\n- work\n---\nbody\n";
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert_eq!(out, "---\ntype: Task\nstatus: new\n---\nbody\n");
    }

    #[test]
    fn set_fields_empty_value_key_without_items_consumes_nothing() {
        // A bare `tags:` with no list following: rewrite it in place, and the
        // next line (a real key) must not be swallowed.
        let doc = "---\ntype: Task\nstatus: new\ntags:\ntitle: \"A\"\n---\n";
        let out = set_fields(doc, &[("tags", Some("[x1]"))]).unwrap();
        assert!(out.contains("tags: [x1]\n"));
        assert!(out.contains("title: \"A\"\n"));
    }

    #[test]
    fn set_fields_body_bullets_are_never_consumed() {
        // Removing an inline-valued key must not touch `- ` bullet lines in the
        // body — consumption applies only to a block list directly under an
        // empty-valued matched key inside the frontmatter.
        let doc = "---\ntype: Task\nstatus: new\ndue: 2026-07-10\n---\n- a body bullet\n";
        let out = set_fields(doc, &[("due", None)]).unwrap();
        assert!(out.contains("- a body bullet\n"));
        assert!(!out.contains("due:"));
    }

    #[test]
    fn set_fields_consumes_nested_mapping_items_without_orphans() {
        // Regression: a block item that is a mapping has indented continuation
        // lines ("  role: owner") that don't start with "- " — the consumption
        // must take them too or the removal leaves orphaned lines that corrupt
        // the frontmatter structure.
        let doc = "---\ntype: Task\nstatus: new\ntags:\n- name: Alice\n  role: owner\n- name: Bob\ntitle: \"A\"\n---\n";
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert!(!out.contains("Alice"));
        assert!(!out.contains("role: owner"));
        assert!(!out.contains("Bob"));
        assert!(out.contains("title: \"A\"\n")); // next top-level key survives
        assert!(out.contains("status: new\n"));
    }

    #[test]
    fn set_fields_consumed_block_still_lets_a_later_key_be_rewritten() {
        // A key matched AFTER a consumed block must still be rewritable in the
        // same call (the flag must not swallow or skip it).
        let doc = "---\ntype: Task\nstatus: new\ntags:\n- work\ndue: 2026-07-10\n---\n";
        let out = set_fields(doc, &[("tags", None), ("due", Some("2026-08-01"))]).unwrap();
        assert!(!out.contains("- work"));
        assert!(!out.contains("tags"));
        assert!(out.contains("due: 2026-08-01\n"));
    }

    #[test]
    fn render_includes_flow_tags_only_when_present() {
        let plain = render_task("A", "2026-07-09", None, None, &[]);
        assert_eq!(
            plain,
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-09\n---\n\n"
        ); // byte-identical to the pre-tags output
        let tagged = render_task(
            "A",
            "2026-07-09",
            Some("2026-07-15"),
            None,
            &["work".to_string(), "home/errands".to_string()],
        );
        assert!(tagged.contains("due: 2026-07-15\ntags: [work, home/errands]\n---\n"));
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
