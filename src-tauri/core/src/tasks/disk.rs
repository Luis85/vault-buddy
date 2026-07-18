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

/// The reserved task frontmatter keys: user extra frontmatter can never
/// redefine one of these (`sanitize_extra_frontmatter` drops the line), so
/// the surgical field writer (`set_fields`) is never confused about which key
/// it owns. The task-id property (when present) is appended to this set at
/// call time — it's per-vault configurable, so it can't be a `const`.
const RESERVED_TASK_KEYS: &[&str] = &[
    "type", "status", "title", "created", "due", "priority", "tags", "tag", "order",
];

/// A `type: Task` document. `type`/`status`/`created` (and the optional
/// `due`/`priority`) are simple unquoted scalars; the user-supplied title is
/// quoted so a colon or quote can't break the frontmatter. `due`/`priority`
/// lines are written only when present — absent priority means normal, and a
/// bare `due:` is never emitted. `tags` renders as a single canonical flow
/// line (`tags: [a, b]`) after `due`/`priority`, only when non-empty. When
/// `task_id` is `Some((property, id))`, a `<property>: <id>` line is written
/// immediately after `created:`.
///
/// `extra_frontmatter` is `{{title}}`/`{{date}}`/`{{due}}`/`{{priority}}`
/// substituted, then sanitized against the reserved keys above (plus the
/// task-id property, when present) and injected right before the closing
/// fence — same discipline as the capture-note renderer. `body_template`
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
    let vars = [
        ("title", title),
        ("date", created),
        ("due", due.unwrap_or("")),
        ("priority", priority.unwrap_or("")),
    ];
    if let Some(ef) = extra_frontmatter {
        let mut reserved: Vec<&str> = RESERVED_TASK_KEYS.to_vec();
        if let Some((prop, _)) = task_id {
            reserved.push(prop);
        }
        extra.push_str(&crate::template::sanitize_extra_frontmatter(
            &crate::template::substitute(ef, &vars),
            &reserved,
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
/// (see there for the substitution/sanitize contract).
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
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    crate::capture_note::write_note_collision_safe(
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
        ),
    )
}

/// Apply a surgical frontmatter patch to a task file on disk. Canonicalizes
/// `root` and `path` and requires containment — a lexical check can't see
/// through a symlink at the file or folder — then reads, applies `set_fields`,
/// and writes atomically (hidden `create_new` temp + fsync + REPLACING
/// rename). Replacing is correct here: the target is the `type: Task` file we
/// just read and are editing in place, touching only the named lines.
/// `ensure_id` names the vault's task-id property (`None` = ids off): when the
/// property has no USABLE value — absent, or present with a blank scalar (a
/// bare `task-id:` from an Obsidian property panel / template; Codex, PR #59)
/// — a fresh id is GENERATED HERE and stamped alongside the patch. Generating
/// inside this branch, rather than callers pre-drawing a candidate, means no
/// discarded CSPRNG draws on already-stamped tasks and no caller can get the
/// blank/casing rules wrong. An existing non-empty value (top-level, any
/// casing — `frontmatter_scalar_ci`; a nested `metadata.task-id` never
/// counts) is never overwritten, so IDs stay stable. Returns the property's
/// effective value after the write — freshly stamped or pre-existing — or
/// `None` when `ensure_id` is `None`; callers reflect a just-stamped ID
/// without a second read (Codex, PR #59).
pub fn update_task_fields(
    root: &Path,
    path: &Path,
    updates: &[(&str, Option<&str>)],
    ensure_id: Option<&str>,
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
    let mut effective: Vec<(&str, Option<&str>)> = updates.to_vec();
    // Owned storage for a freshly-generated id and the on-disk casing a blank
    // line is rewritten under — both must outlive `effective`'s borrows.
    let mut generated: Option<String> = None;
    let mut blank_casing: Option<String> = None;
    let ensured = ensure_id.and_then(|key| {
        match super::parse::frontmatter_scalar_ci(&content, key) {
            // Already has a usable id (any casing) → never overwritten.
            Some((_, v)) if !v.is_empty() => Some(v),
            // Empty-valued but opening a BLOCK (a nested map or block list
            // under the configured key): that is the USER'S frontmatter, not a
            // blank stamp target — set_fields' rewrite would consume the
            // indented block along with the key line, deleting their data
            // (review, PR #59). Leave it untouched; there is no usable id to
            // report (reads agree: scalar_field_ci yields "" → filtered).
            Some((on_disk, _)) if super::parse::key_opens_block(&content, &on_disk) => None,
            // Truly blank or absent → generate + stamp. A BLANK line is
            // rewritten under its ON-DISK casing so set_fields (case-
            // sensitive) replaces it — stamping the configured casing would
            // insert a case-mismatched DUPLICATE that scalar_field_ci's CI
            // read then shadows, hiding the id forever (Codex, PR #59).
            // Absent stamps a new line under the configured property name.
            found => {
                blank_casing = found.map(|(on_disk, _)| on_disk);
                let id = super::id::new_task_id();
                generated = Some(id.clone());
                Some(id)
            }
        }
    });
    if let (Some(key), Some(id)) = (ensure_id, generated.as_deref()) {
        effective.push((blank_casing.as_deref().unwrap_or(key), Some(id)));
    }
    // Nothing to write (an ensure-only call — a move backfill — on a task
    // whose id is already present): skip the redundant atomic rewrite, still
    // report the id. update_task always passes a non-empty `updates`, so this
    // only short-circuits those callers.
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

/// Best-effort id backfill on a task file a structural move just relocated
/// (drag / editor move, delete-list): stamp a missing/blank id under
/// `property` (`None` = ids off → no-op). The move already mutated the vault,
/// so a stamp failure only WARNS — it must never fail the move that carried
/// it (audio-first discipline, borrowed from the capture domain). Returns the
/// task's effective id — freshly stamped or already present — for callers
/// that reflect it without a reload.
pub fn backfill_task_id(root: &Path, path: &Path, property: Option<&str>) -> Option<String> {
    let prop = property?;
    match update_task_fields(root, path, &[], Some(prop)) {
        Ok(id) => id,
        Err(e) => {
            log::warn!("task id backfill on {path:?} failed: {e}");
            None
        }
    }
}

/// Set a task's `status:` frontmatter on disk (see `update_task_fields`). A
/// status toggle never stamps an ID (`ensure_id: None` — a checkbox click is
/// not an edit), so the id return is discarded.
pub fn set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String> {
    update_task_fields(root, path, &[("status", Some(new_status))], None).map(|_| ())
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
        let doc = render_task("Buy milk", "2026-07-08", None, None, &[], None, None, None);
        assert_eq!(
            doc,
            "---\ntype: Task\nstatus: new\ntitle: \"Buy milk\"\ncreated: 2026-07-08\n---\n\n"
        );
    }

    #[test]
    fn render_quotes_a_colon_title() {
        // A colon in the title would break unquoted YAML — must be quoted.
        let doc = render_task("Ship: v1", "2026-07-08", None, None, &[], None, None, None);
        assert!(doc.contains("title: \"Ship: v1\"\n"));
    }

    #[test]
    fn render_quotes_and_escapes_special_title() {
        // A title with a quote and backslash must be escaped so it can't break
        // the frontmatter (read back by note_field).
        let doc = render_task("a\"b\\c", "2026-07-08", None, None, &[], None, None, None);
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
        )
        .unwrap();
        assert_ne!(p1, p2);
        assert_eq!(p2.file_name().unwrap(), "2026-07-08-buy-milk (2).md");
        assert!(p1.exists() && p2.exists());
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
    fn render_includes_due_and_priority_only_when_present() {
        let plain = render_task("A", "2026-07-09", None, None, &[], None, None, None);
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
        );
        assert!(full.contains("created: 2026-07-09\ndue: 2026-07-15\npriority: high\n---\n"));
    }

    #[test]
    fn render_includes_flow_tags_only_when_present() {
        let plain = render_task("A", "2026-07-09", None, None, &[], None, None, None);
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
        );
        assert!(doc.contains("created: 2026-07-09\ntask-id: k3n7p2qz\n"));
        // Absent → byte-identical to the pre-id output (no id line).
        let plain = render_task("A", "2026-07-09", None, None, &[], None, None, None);
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
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None, None, None).unwrap();
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
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None, None, None).unwrap();
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
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None, None, None).unwrap();
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
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None, None, None).unwrap();
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
        // lines). frontmatter_scalar_ci reads that as an empty scalar, so the
        // blank-stamp branch would rewrite the key line — and set_fields'
        // block consumption would DELETE the user's nested data with it. A
        // non-scalar value is the user's frontmatter, never a stamp target:
        // the edit still applies, the block survives byte-for-byte, and no id
        // is reported.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        for (name, block) in [
            ("map", "task-id:\n  source: jira\n  ref: ABC-1\n"),
            ("list", "task-id:\n- a1\n- b2\n"),
        ] {
            let p =
                create_task(&root, name, "2026-07-08", None, None, &[], None, None, None).unwrap();
            let content = std::fs::read_to_string(&p).unwrap();
            let seeded = content.replacen(
                "created: 2026-07-08\n",
                &format!("created: 2026-07-08\n{block}"),
                1,
            );
            std::fs::write(&p, &seeded).unwrap();

            let reported =
                update_task_fields(&root, &p, &[("status", Some("done"))], Some("task-id"))
                    .unwrap();
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
        let p = create_task(&root, "A", "2026-07-08", None, None, &[], None, None, None).unwrap();
        set_task_status(&root, &p, "done").unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("task-id"));
    }

    #[test]
    fn task_default_output_is_byte_identical_with_no_template() {
        let out = render_task("Buy milk", "2026-07-08", None, None, &[], None, None, None);
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
        );
        assert!(out.contains("project: Alpha"));
        assert!(!out.contains("status: HIJACK"), "reserved dropped: {out}");
        assert!(out.contains("status: new"), "managed status intact");
        // Body after the fence, placeholders filled.
        assert!(out.ends_with("- [ ] Buy milk by 2026-07-08\n"), "{out}");
        // Still a valid task (closed fence + type: Task).
        assert!(out.contains("---\ntype: Task\n"));
    }
}
