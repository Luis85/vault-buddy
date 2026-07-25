//! Task-document creation: filename derivation (slugify/task_basename) and
//! frontmatter rendering (render_task) + the collision-safe create_task write.

use crate::capture_note::{write_note_collision_safe, yaml_quote};
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

/// A `type: Task` document. `type`/`status`/`created` (and the optional
/// `due`/`priority`) are simple unquoted scalars; the user-supplied title is
/// quoted so a colon or quote can't break the frontmatter. `due`/`priority`
/// lines are written only when present — absent priority means normal, and a
/// bare `due:` is never emitted. `tags` renders as a single canonical flow
/// line (`tags: [a, b]`) after `due`/`priority`, only when non-empty. When
/// `task_id` is `Some((property, id))`, a `<property>: <id>` line is written
/// immediately after `created:`. `scheduled` (the last param) is emitted
/// after `due`, only when present.
///
/// `extra_frontmatter` is `{{title}}`/`{{date}}`/`{{due}}`/`{{priority}}`
/// rendered via `render_extra_frontmatter`, which resolves the placeholders,
/// parses the result as YAML, and drops the reserved keys above (plus the
/// task-id property, when present) before injecting right before the closing
/// fence — same pipeline as the capture-note renderer. `body_template`
/// (same placeholders), when non-empty after trimming, becomes the task
/// body — tasks have none today, so any non-empty template is new content,
/// not a scaffold replacement. Both default to a no-op with `None`/empty, so
/// the historical byte-for-byte output is unchanged when a vault opts into
/// neither.
#[allow(clippy::too_many_arguments)]
pub fn render_task(
    title: &str,
    created: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    task_id: Option<(&str, &str)>,
    extra_frontmatter: Option<&str>,
    body_template: Option<&str>,
    scheduled: Option<&str>,
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
    if let Some(s) = scheduled {
        extra.push_str(&format!("scheduled: {s}\n"));
    }
    if let Some(p) = priority {
        extra.push_str(&format!("priority: {p}\n"));
    }
    if !tags.is_empty() {
        // Canonical flow style: single-line, so the surgical writer can
        // rewrite it; charset-validated tags never need YAML quoting.
        extra.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    let vars = [
        ("title", title),
        ("date", created),
        ("due", due.unwrap_or("")),
        ("priority", priority.unwrap_or("")),
    ];
    if let Some(ef) = extra_frontmatter {
        let mut reserved: Vec<&str> = super::RESERVED_TASK_KEYS.to_vec();
        if let Some((prop, _)) = task_id {
            reserved.push(prop);
        }
        extra.push_str(&crate::template::render_extra_frontmatter(
            ef, &vars, &reserved,
        ));
    }
    let body = match body_template.map(str::trim) {
        Some(b) if !b.is_empty() => {
            let rendered = crate::template::substitute(b, &vars);
            if rendered.ends_with('\n') {
                rendered
            } else {
                format!("{rendered}\n")
            }
        }
        _ => String::new(),
    };
    format!(
        "---\ntype: Task\nstatus: new\ntitle: {}\ncreated: {created}\n{extra}---\n\n{body}",
        yaml_quote(title)
    )
}

/// Create a new task file under `root` (creating `root` if needed). Uses the
/// collision-safe atomic writer shared with the capture note, so it can never
/// overwrite an existing file — a name clash takes the ` (N)` suffix instead.
/// `tags` (already validated by the caller) is threaded through to
/// `render_task` verbatim. When `task_id` is `Some((property, id))`, a
/// `<property>: <id>` line is written immediately after `created:`.
/// `extra_frontmatter`/`body_template` pass straight through to `render_task`
/// (see there for the placeholder-rendering contract).
#[allow(clippy::too_many_arguments)]
pub fn create_task(
    root: &Path,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    task_id: Option<(&str, &str)>,
    extra_frontmatter: Option<&str>,
    body_template: Option<&str>,
    scheduled: Option<&str>,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    write_note_collision_safe(
        &target,
        &render_task(
            title,
            today,
            due,
            priority,
            tags,
            task_id,
            extra_frontmatter,
            body_template,
            scheduled,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let doc = render_task(
            "Buy milk",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            doc,
            "---\ntype: Task\nstatus: new\ntitle: \"Buy milk\"\ncreated: 2026-07-08\n---\n\n"
        );
    }

    #[test]
    fn render_quotes_a_colon_title() {
        // A colon in the title would break unquoted YAML — must be quoted.
        let doc = render_task(
            "Ship: v1",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        assert!(doc.contains("title: \"Ship: v1\"\n"));
    }

    #[test]
    fn render_quotes_and_escapes_special_title() {
        // A title with a quote and backslash must be escaped so it can't break
        // the frontmatter (read back by note_field).
        let doc = render_task(
            "a\"b\\c",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        assert!(doc.contains("title: \"a\\\"b\\\\c\"\n"));
    }

    #[test]
    fn create_task_writes_file_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");

        let p1 = create_task(
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
        )
        .unwrap();
        assert_eq!(p1.file_name().unwrap(), "2026-07-08-buy-milk.md");
        let body = std::fs::read_to_string(&p1).unwrap();
        assert!(body.contains("type: Task"));
        assert!(body.contains("status: new"));
        assert!(body.contains("title: \"Buy milk\""));

        // Same title again → suffixed, original untouched (collision-safe).
        let p2 = create_task(
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
        )
        .unwrap();
        assert_ne!(p1, p2);
        assert_eq!(p2.file_name().unwrap(), "2026-07-08-buy-milk (2).md");
        assert!(p1.exists() && p2.exists());
    }

    #[test]
    fn render_includes_due_and_priority_only_when_present() {
        let plain = render_task("A", "2026-07-09", None, None, &[], None, None, None, None);
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
            None,
            None,
            None,
        );
        assert!(full.contains("created: 2026-07-09\ndue: 2026-07-15\npriority: high\n---\n"));
    }

    #[test]
    fn render_includes_scheduled_after_due_only_when_present() {
        // Absent → byte-identical to the pre-scheduled output (no scheduled line).
        let plain = render_task(
            "A",
            "2026-07-09",
            Some("2026-07-15"),
            Some("high"),
            &[],
            None,
            None,
            None,
            None,
        );
        assert!(plain.contains("due: 2026-07-15\npriority: high\n"));
        assert!(!plain.contains("scheduled"));
        // Present → emitted right after due, before priority.
        let sched = render_task(
            "A",
            "2026-07-09",
            Some("2026-07-15"),
            Some("high"),
            &[],
            None,
            None,
            None,
            Some("2026-07-20"),
        );
        assert!(sched.contains("due: 2026-07-15\nscheduled: 2026-07-20\npriority: high\n"));
        // Scheduled with no due lands right after created.
        let no_due = render_task(
            "A",
            "2026-07-09",
            None,
            None,
            &[],
            None,
            None,
            None,
            Some("2026-07-20"),
        );
        assert!(no_due.contains("created: 2026-07-09\nscheduled: 2026-07-20\n---\n"));
    }

    #[test]
    fn render_includes_flow_tags_only_when_present() {
        let plain = render_task("A", "2026-07-09", None, None, &[], None, None, None, None);
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
            None,
            None,
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
            None,
            None,
            None,
        );
        assert!(doc.contains("created: 2026-07-09\ntask-id: k3n7p2qz\n"));
        // Absent → byte-identical to the pre-id output (no id line).
        let plain = render_task("A", "2026-07-09", None, None, &[], None, None, None, None);
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
            None,
            None,
            None,
        )
        .unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("task-id: abcd1234\n"));
    }

    #[test]
    fn task_default_output_is_byte_identical_with_no_template() {
        let out = render_task(
            "Buy milk",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            None,
            None,
            None,
        );
        assert_eq!(
            out,
            "---\ntype: Task\nstatus: new\ntitle: \"Buy milk\"\ncreated: 2026-07-08\n---\n\n"
        );
    }

    #[test]
    fn task_extra_frontmatter_and_body_apply_and_reserved_dropped() {
        let out = render_task(
            "Buy milk",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            Some("project: Alpha\nstatus: HIJACK"),
            Some("- [ ] {{title}} by {{date}}"),
            None,
        );
        assert!(out.contains("project: Alpha"));
        assert!(!out.contains("status: HIJACK"), "reserved dropped: {out}");
        assert!(out.contains("status: new"), "managed status intact");
        // Body after the fence, placeholders filled.
        assert!(out.ends_with("- [ ] Buy milk by 2026-07-08\n"), "{out}");
        // Still a valid task (closed fence + type: Task).
        assert!(out.contains("---\ntype: Task\n"));
    }

    #[test]
    fn task_extra_frontmatter_drops_the_parent_keys() {
        // finding 3: parent-id/parent are reserved (RESERVED_TASK_KEYS) —
        // a user template seeding either must never smuggle a fake parent
        // link past the surgical writer, mirroring the status: HIJACK case
        // above.
        let out = render_task(
            "Buy milk",
            "2026-07-08",
            None,
            None,
            &[],
            None,
            Some("project: Alpha\nparent-id: fake0000\nparent: \"[[Nope]]\""),
            None,
            None,
        );
        assert!(out.contains("project: Alpha"));
        assert!(
            !out.contains("parent-id: fake0000"),
            "reserved dropped: {out}"
        );
        assert!(
            !out.contains("parent: \"[[Nope]]\""),
            "reserved dropped: {out}"
        );
    }
}
