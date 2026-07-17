//! Disk operations: the sanctioned vault writes (collision-safe create;
//! surgical field/status update via `update_task_fields`) plus the pure
//! filename/render helpers they build on.

use super::writer::set_fields;
use crate::capture_note::yaml_quote;
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
/// priority means normal, and a bare `due:` is never emitted. `tags` renders
/// as a single canonical flow line (`tags: [a, b]`) after `due`/`priority`,
/// only when non-empty. When `task_id` is `Some((property, id))`, a
/// `<property>: <id>` line is written immediately after `created:`.
pub fn render_task(
    title: &str,
    created: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    task_id: Option<(&str, &str)>,
) -> String {
    let mut extra = String::new();
    // The generated ID (when enabled) sits right after `created`, before the
    // widened fields. The value is charset-safe base36; the property was
    // validated on save, so neither needs YAML quoting.
    if let Some((prop, id)) = task_id {
        extra.push_str(&format!("{prop}: {id}\n"));
    }
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
/// `tags` (already validated by the caller) is threaded through to
/// `render_task` verbatim. When `task_id` is `Some((property, id))`, a
/// `<property>: <id>` line is written immediately after `created:`.
pub fn create_task(
    root: &Path,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    task_id: Option<(&str, &str)>,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    crate::capture_note::write_note_collision_safe(
        &target,
        &render_task(title, today, due, priority, tags, task_id),
    )
}

/// Apply a surgical frontmatter patch to a task file on disk. Canonicalizes
/// `root` and `path` and requires containment — a lexical check can't see
/// through a symlink at the file or folder — then reads, applies `set_fields`,
/// and writes atomically (hidden `create_new` temp + fsync + REPLACING
/// rename). Replacing is correct here: the target is the `type: Task` file we
/// just read and are editing in place, touching only the named lines.
/// Returns the effective value of the FIRST `ensure_absent` key after the
/// operation — the value now on the file for that key (the freshly-stamped one
/// if it was absent, or the pre-existing value if already present), or `None`
/// when `ensure_absent` is empty or the key resolves to no readable value. In
/// practice that key is the generated task ID, and the caller uses this return
/// to reflect a just-stamped ID without a second read (Codex, PR #59).
pub fn update_task_fields(
    root: &Path,
    path: &Path,
    updates: &[(&str, Option<&str>)],
    ensure_absent: &[(&str, &str)],
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
    // Stamp-if-absent keys (the generated task ID): written only when the
    // property has no USABLE value yet — absent, OR present with a blank scalar.
    // A bare `task-id:` (an Obsidian property panel / template leaves the key
    // with no value) is not a usable id, so it must NOT suppress the stamp
    // (Codex, PR #59); an existing non-empty value (top-level, any casing) is
    // never overwritten, so IDs stay stable. `scalar_field_ci` is top-level
    // only, so a nested `metadata.task-id` never counts either.
    let mut effective: Vec<(&str, Option<&str>)> = updates.to_vec();
    // Owned on-disk key casings we stamp a BLANK line under, kept alive for
    // set_fields' `&str` refs below.
    let mut blank_stamp: Vec<(String, &str)> = Vec::new();
    for (key, val) in ensure_absent {
        match super::parse::frontmatter_scalar_ci(&content, key) {
            // Already has a usable id (any casing) → never overwritten.
            Some((_, v)) if !v.is_empty() => {}
            // Present but BLANK (a bare `task-id:` from an Obsidian property
            // panel / template): stamp it, rewriting the existing line under its
            // ON-DISK casing so set_fields (which matches case-sensitively)
            // replaces it — stamping the configured casing would insert a
            // case-mismatched DUPLICATE that scalar_field_ci's CI read then
            // shadows, hiding the id forever (Codex, PR #59).
            Some((on_disk, _)) => blank_stamp.push((on_disk, val)),
            // Absent → a new line under the configured property name.
            None => effective.push((key, Some(val))),
        }
    }
    for (k, v) in &blank_stamp {
        effective.push((k.as_str(), Some(v)));
    }
    // The id to report back: the first ensure key's usable value, else the one
    // we are about to stamp (fresh, for both the absent and blank cases).
    let ensured = ensure_absent.first().map(|(key, val)| {
        super::parse::scalar_field_ci(&content, key)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| (*val).to_string())
    });
    // Nothing to write (e.g. a move that only needs to stamp, but the id is
    // already present): skip the redundant atomic rewrite, still report the id.
    // update_task always passes a non-empty `updates`, so this only short-
    // circuits the ensure-only callers.
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

/// Set a task's `status:` frontmatter on disk (see `update_task_fields`). A
/// status toggle never stamps an ID (no ensure keys), so the id return is
/// discarded.
pub fn set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String> {
    update_task_fields(root, path, &[("status", Some(new_status))], &[]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_task_status_writes_an_arbitrary_status() {
        // set_task_status now takes a status string, so it can write archived
        // (and still new/done), not just a done bool.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "Buy milk", "2026-07-08", None, None, &[], None).unwrap();
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
        let doc = render_task("Buy milk", "2026-07-08", None, None, &[], None);
        assert_eq!(
            doc,
            "---\ntype: Task\nstatus: new\ntitle: \"Buy milk\"\ncreated: 2026-07-08\n---\n\n"
        );
    }

    #[test]
    fn render_quotes_a_colon_title() {
        // A colon in the title would break unquoted YAML — must be quoted.
        let doc = render_task("Ship: v1", "2026-07-08", None, None, &[], None);
        assert!(doc.contains("title: \"Ship: v1\"\n"));
    }

    #[test]
    fn render_quotes_and_escapes_special_title() {
        // A title with a quote and backslash must be escaped so it can't break
        // the frontmatter (read back by note_field).
        let doc = render_task("a\"b\\c", "2026-07-08", None, None, &[], None);
        assert!(doc.contains("title: \"a\\\"b\\\\c\"\n"));
    }

    #[test]
    fn create_task_writes_file_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");

        let p1 = create_task(&root, "Buy milk", "2026-07-08", None, None, &[], None).unwrap();
        assert_eq!(p1.file_name().unwrap(), "2026-07-08-buy-milk.md");
        let body = std::fs::read_to_string(&p1).unwrap();
        assert!(body.contains("type: Task"));
        assert!(body.contains("status: new"));
        assert!(body.contains("title: \"Buy milk\""));

        // Same title again → suffixed, original untouched (collision-safe).
        let p2 = create_task(&root, "Buy milk", "2026-07-08", None, None, &[], None).unwrap();
        assert_ne!(p1, p2);
        assert_eq!(p2.file_name().unwrap(), "2026-07-08-buy-milk (2).md");
        assert!(p1.exists() && p2.exists());
    }

    #[test]
    fn set_task_status_writes_and_rejects_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "Buy milk", "2026-07-08", None, None, &[], None).unwrap();

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
    fn render_includes_due_and_priority_only_when_present() {
        let plain = render_task("A", "2026-07-09", None, None, &[], None);
        assert_eq!(
            plain,
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-09\n---\n\n"
        ); // byte-identical to the pre-due/priority output
        let full = render_task(
            "A",
            "2026-07-09",
            Some("2026-07-15"),
            Some("high"),
            &[],
            None,
        );
        assert!(full.contains("created: 2026-07-09\ndue: 2026-07-15\npriority: high\n---\n"));
    }

    #[test]
    fn render_includes_flow_tags_only_when_present() {
        let plain = render_task("A", "2026-07-09", None, None, &[], None);
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
            None,
        );
        assert!(tagged.contains("due: 2026-07-15\ntags: [work, home/errands]\n---\n"));
    }

    #[test]
    fn render_writes_the_id_property_after_created_when_present() {
        let doc = render_task(
            "A",
            "2026-07-09",
            None,
            None,
            &[],
            Some(("task-id", "k3n7p2qz")),
        );
        assert!(doc.contains("created: 2026-07-09\ntask-id: k3n7p2qz\n"));
        // Absent → byte-identical to the pre-id output (no id line).
        let plain = render_task("A", "2026-07-09", None, None, &[], None);
        assert!(!plain.contains("task-id"));
        assert_eq!(
            plain,
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-09\n---\n\n"
        );
    }

    #[test]
    fn create_task_writes_the_id_property() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "Buy milk",
            "2026-07-08",
            None,
            None,
            &[],
            Some(("task-id", "abcd1234")),
        )
        .unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("task-id: abcd1234\n"));
    }

    #[test]
    fn update_task_fields_stamps_an_absent_ensure_key_but_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None).unwrap();
        // Absent → stamped alongside the edit, and the stamped id is returned.
        let stamped = update_task_fields(
            &root,
            &p,
            &[("status", Some("done"))],
            &[("task-id", "abcd1234")],
        )
        .unwrap();
        assert_eq!(stamped.as_deref(), Some("abcd1234"));
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("status: done\n"));
        assert!(body.contains("task-id: abcd1234\n"));
        // Present → never overwritten (a second stamp with a new id is a no-op),
        // and the EXISTING id is reported back, not the ignored candidate.
        let existing = update_task_fields(&root, &p, &[], &[("task-id", "zzzz9999")]).unwrap();
        assert_eq!(existing.as_deref(), Some("abcd1234"));
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("task-id: abcd1234\n"));
    }

    #[test]
    fn update_task_fields_ensure_absent_detects_an_existing_id_case_insensitively() {
        // Regression: scalar_field's exact-case match let a config using
        // "task-id" stamp a SECOND, conflicting id line onto a task already
        // carrying "Task-ID:" (e.g. stamped under a since-changed config
        // casing, or hand-authored). Obsidian folds frontmatter key case, so
        // the task would show a duplicate id. The case-insensitive
        // scalar_field_ci read must catch the existing key under any casing.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        let seeded = content.replacen(
            "created: 2026-07-08\n",
            "created: 2026-07-08\nTask-ID: existing123\n",
            1,
        );
        std::fs::write(&p, &seeded).unwrap();

        let reported = update_task_fields(
            &root,
            &p,
            &[("status", Some("done"))],
            &[("task-id", "new456")],
        )
        .unwrap();
        // The existing id (under its own casing) is reported, not the candidate.
        assert_eq!(reported.as_deref(), Some("existing123"));

        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("status: done\n"));
        assert!(body.contains("Task-ID: existing123\n"));
        assert!(!body.contains("new456"));
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
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        let seeded = content.replacen(
            "created: 2026-07-08\n",
            "created: 2026-07-08\ntask-id:\n",
            1,
        );
        std::fs::write(&p, &seeded).unwrap();

        let reported = update_task_fields(
            &root,
            &p,
            &[("status", Some("done"))],
            &[("task-id", "fresh777")],
        )
        .unwrap();
        // Blank → treated as missing → stamped, and the fresh id is returned.
        assert_eq!(reported.as_deref(), Some("fresh777"));
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("task-id: fresh777\n"));
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
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None).unwrap();
        let content = std::fs::read_to_string(&p).unwrap();
        let seeded = content.replacen(
            "created: 2026-07-08\n",
            "created: 2026-07-08\nTask-ID:\n",
            1,
        );
        std::fs::write(&p, &seeded).unwrap();

        let reported = update_task_fields(
            &root,
            &p,
            &[("status", Some("done"))],
            &[("task-id", "fresh777")],
        )
        .unwrap();
        // Blank (any casing) → stamped, fresh id reported.
        assert_eq!(reported.as_deref(), Some("fresh777"));
        let body = std::fs::read_to_string(&p).unwrap();
        // Rewritten in place under the ON-DISK casing — no lowercase duplicate.
        assert!(body.contains("Task-ID: fresh777\n"));
        assert!(!body.contains("task-id: fresh777\n"));
        // Exactly one id-ish line, case-insensitively — no conflicting second.
        let id_lines = body
            .lines()
            .filter(|l| l.trim_start().to_ascii_lowercase().starts_with("task-id:"))
            .count();
        assert_eq!(id_lines, 1);
    }

    #[test]
    fn set_task_status_does_not_stamp_any_id() {
        // A checkbox toggle is not an "edit": set_task_status passes no
        // ensure keys, so toggling never adds an id.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None).unwrap();
        set_task_status(&root, &p, "done").unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("task-id"));
    }
}
