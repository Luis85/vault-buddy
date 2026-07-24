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
/// redefine one of these (`render_extra_frontmatter` drops the key), so
/// the surgical field writer (`set_fields`) is never confused about which key
/// it owns. The task-id property (when present) is appended to this set at
/// call time — it's per-vault configurable, so it can't be a `const`.
// keep in sync with id.rs::RESERVED_TASK_KEYS. `description` is a MANAGED field the detail view owns via set_fields — templates must not seed it (a template block scalar would orphan on the first save), exactly as due/status/priority are managed + reserved (Codex PR #76).
const RESERVED_TASK_KEYS: &[&str] = &[
    "type",
    "status",
    "title",
    "created",
    "due",
    "scheduled",
    "priority",
    "tags",
    "tag",
    "order",
    "description",
];

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
        let mut reserved: Vec<&str> = RESERVED_TASK_KEYS.to_vec();
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
            scheduled,
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

/// Permanently delete a task file — the app's ONLY destructive vault write.
/// Canonicalizes `root` and `path` and requires containment (a symlink at the
/// file or folder can't be seen lexically), THEN re-reads the file and requires
/// it to be a `type: Task` document before removing. Task folders may
/// legitimately hold foreign files, and a listed row could be swapped for a
/// non-task file at the same path before the confirm lands — so identity is
/// re-validated immediately before this irreversible write, the same posture
/// the move/field writers get from `set_fields`' `type: Task` precondition
/// (Codex P1, PR #76). A missing file surfaces as an error (the row the user
/// clicked should exist), never a silent success.
#[allow(dead_code)]
pub fn delete_task(root: &Path, path: &Path) -> Result<(), String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    if !super::doc::is_task(&content) {
        return Err("Refusing to delete: not a type: Task document".to_string());
    }
    std::fs::remove_file(&canon_path).map_err(|e| format!("Cannot delete task: {e}"))
}

/// Duplicate a task file into the same folder, faithfully: the source bytes
/// are copied (body, extra frontmatter, description, unknown keys all
/// preserved), then only the identity fields are rewritten surgically via
/// `set_fields` — title → "<title> (copy)", status → new, and the id handled so
/// no two tasks ever share one. `id_property` is the vault's configured id key
/// ONLY when it is a valid, non-reserved property (else `None`, so a
/// foreign/reserved key is never touched). When present, the copy's id is
/// REGENERATED (`ids_enabled == true`) or STRIPPED (`false`): stripping matters
/// because leaving the source id on the copy would collide with the original if
/// the user later re-enables IDs, and the ensure-id path never overwrites an
/// existing value (Codex P2, PR #76). Written through the collision-safe
/// never-clobber writer, so a name clash takes the ` (N)` suffix. `today` names
/// the new file (clock-free core).
#[allow(dead_code)]
pub fn duplicate_task(
    root: &Path,
    path: &Path,
    today: &str,
    id_property: Option<&str>,
    ids_enabled: bool,
) -> Result<PathBuf, String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    // Title fallback matches list_tasks' display: an untitled hand-authored
    // task shows its filename stem, so the copy must too (Codex P2, PR #76).
    let stem = canon_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let title = crate::capture_note::note_field(&content, "title").unwrap_or(stem);
    let new_title = format!("{title} (copy)");
    let quoted = yaml_quote(&new_title);
    // A fresh id when IDs are on; `None` strips the configured property so the
    // copy can never inherit the source id.
    let new_id = if ids_enabled {
        Some(super::id::new_task_id())
    } else {
        None
    };
    // Resolve the id key to its ON-DISK casing (case-insensitive lookup) so the
    // case-sensitive `set_fields` matches a differently-cased existing id — else
    // a strip misses `Task-ID:` and a regenerate inserts a SECOND, differently-
    // cased id (Codex P2, PR #76). With no id line on disk, add one under the
    // configured name ONLY when regenerating.
    let id_key: Option<String> =
        id_property.and_then(
            |prop| match super::parse::frontmatter_scalar_ci(&content, prop) {
                // A block-valued (nested map/list under the key) OR flow-valued
                // (inline `{..}` / `[..]`) id property is the USER's frontmatter,
                // not a scalar id — set_fields would consume the block, or
                // rewrite the single flow line, and destroy it either way. Leave
                // it untouched (Codex P2, PR #76); update_task_fields' ensure_id
                // never rewrites a present value, so only duplicate is exposed.
                Some((on_disk, _))
                    if super::parse::key_opens_block(&content, &on_disk)
                        || super::parse::key_opens_flow(&content, &on_disk) =>
                {
                    None
                }
                Some((on_disk, _)) => Some(on_disk),
                None => new_id.as_ref().map(|_| prop.to_string()),
            },
        );
    let mut updates: Vec<(&str, Option<&str>)> =
        vec![("title", Some(quoted.as_str())), ("status", Some("new"))];
    if let Some(key) = id_key.as_deref() {
        updates.push((key, new_id.as_deref()));
    }
    let rewritten =
        set_fields(&content, &updates).ok_or("Source is not a valid type: Task document")?;
    let parent = canon_path.parent().unwrap_or(&canon_root);
    let target = parent.join(format!("{}.md", task_basename(&new_title, today)));
    crate::capture_note::write_note_collision_safe(&target, &rewritten)
        .map_err(|e| format!("Cannot write duplicate: {e}"))
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
    fn update_task_fields_sets_rewrites_and_clears_scheduled() {
        // `scheduled` rides the same generic surgical writer as `due`/`tags` —
        // no new write machinery — but the spec promised an explicit
        // scheduled-named regression test pinning the set/rewrite/clear
        // round-trip on disk (not just render_task's in-memory output).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "A",
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
        assert!(!std::fs::read_to_string(&p).unwrap().contains("scheduled"));

        // Set: absent → inserted at the closing fence.
        update_task_fields(&root, &p, &[("scheduled", Some("2026-07-20"))], None).unwrap();
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("scheduled: 2026-07-20\n"));

        // Rewrite: existing line replaced in place, not duplicated.
        update_task_fields(&root, &p, &[("scheduled", Some("2026-07-25"))], None).unwrap();
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("scheduled: 2026-07-25\n"));
        assert!(!body.contains("2026-07-20"));
        assert_eq!(body.matches("scheduled:").count(), 1);

        // Clear: None removes the line entirely.
        update_task_fields(&root, &p, &[("scheduled", None)], None).unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("scheduled"));
    }

    #[test]
    fn update_task_fields_stamps_an_absent_ensure_key_but_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        let p = create_task(
            &root,
            "A",
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
        let p = create_task(
            &root,
            "A",
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
        let p = create_task(
            &root,
            "A",
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
        let p = create_task(
            &root,
            "A",
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
            let p = create_task(
                &root,
                name,
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
        let p = create_task(
            &root,
            "A",
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
        set_task_status(&root, &p, "done").unwrap();
        assert!(!std::fs::read_to_string(&p).unwrap().contains("task-id"));
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
    fn delete_task_removes_a_task_refuses_outside_and_refuses_a_non_task() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let p = root.join("t.md");
        std::fs::write(&p, "---\ntype: Task\nstatus: new\ntitle: X\n---\n").unwrap();
        // Happy path: a real task is removed.
        assert!(delete_task(&root, &p).is_ok());
        assert!(!p.exists());
        // A path outside the tasks root is refused (write a sibling file to delete).
        let outside = dir.path().join("outside.md");
        std::fs::write(&outside, "x").unwrap();
        assert!(delete_task(&root, &outside).is_err());
        assert!(outside.exists());
        // A FOREIGN (non-task) file INSIDE the tasks root is refused — task folders
        // may legitimately hold non-task files, and this first destructive write
        // must never remove one (Codex P1, PR #76).
        let foreign = root.join("notes.md");
        std::fs::write(&foreign, "# just some notes, not a task\n").unwrap();
        assert!(delete_task(&root, &foreign).is_err());
        assert!(foreign.exists());
    }

    #[test]
    fn duplicate_task_copies_body_and_resets_identity_with_fresh_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: done\ntitle: \"Buy milk\"\ncreated: 2026-07-01\ntask-id: aaa11111\ndue: 2026-07-10\n---\n\nThe body stays.\n",
        )
        .unwrap();
        let new = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
        assert!(new.exists() && new != src);
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("title: \"Buy milk (copy)\""));
        assert!(out.contains("status: new")); // reset
        assert!(out.contains("The body stays.")); // body preserved
        assert!(out.contains("due: 2026-07-10")); // other fields preserved
        assert!(!out.contains("task-id: aaa11111")); // id regenerated, not shared
        assert!(out.contains("task-id: ")); // a fresh id is present
                                            // IDs off → the configured id property is STRIPPED, not inherited: leaving
                                            // the source id on the copy would collide with the original if IDs are
                                            // later re-enabled, and ensure-id never overwrites an existing value
                                            // (Codex P2, PR #76).
        let new2 = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), false).unwrap();
        let out2 = std::fs::read_to_string(&new2).unwrap();
        assert!(!out2.contains("task-id")); // stripped when ids are off
        assert!(out2.contains("title: \"Buy milk (copy)\""));
    }

    #[test]
    fn duplicate_task_uses_the_filename_stem_when_the_source_has_no_title() {
        // An untitled hand-authored task lists under its filename stem, so the copy
        // must too — not an empty " (copy)" (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("My hand note.md");
        std::fs::write(&src, "---\ntype: Task\nstatus: new\n---\n\nbody\n").unwrap();
        let new = duplicate_task(&root, &src, "2026-07-24", None, false).unwrap();
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("title: \"My hand note (copy)\""));
    }

    #[test]
    fn duplicate_task_rewrites_the_on_disk_id_key_casing() {
        // Source key `Task-ID:`, configured property `task-id`: the copy must
        // rewrite/strip THAT on-disk key, never insert a second differently-cased
        // id or miss the strip (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: new\ntitle: X\nTask-ID: aaa11111\n---\n",
        )
        .unwrap();
        // IDs on → the existing `Task-ID:` line is rewritten to a fresh id (no
        // second `task-id:` line, exactly one id key remains).
        let on = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
        let out_on = std::fs::read_to_string(&on).unwrap();
        assert!(!out_on.contains("aaa11111")); // old id replaced
        assert_eq!(out_on.matches("Task-ID:").count(), 1); // rewritten in place
        assert!(!out_on.to_lowercase().contains("task-id: aaa")); // no stale value
        assert_eq!(out_on.to_lowercase().matches("task-id:").count(), 1); // exactly one id key
                                                                          // IDs off → the `Task-ID:` line is stripped despite the casing mismatch.
        let off = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), false).unwrap();
        let out_off = std::fs::read_to_string(&off).unwrap();
        assert!(!out_off.to_lowercase().contains("task-id"));
    }

    #[test]
    fn duplicate_task_leaves_a_block_valued_id_property_untouched() {
        // A block-valued id property (nested map under the key) is the user's
        // frontmatter, not a scalar id — duplicating must not consume/delete the
        // indented block (Codex P2, PR #76), mirroring update_task_fields.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: new\ntitle: X\ntask-id:\n  source: jira\n  ref: ABC-1\n---\n",
        )
        .unwrap();
        // IDs on: the block-valued property is NOT treated as a scalar id, so the
        // nested lines survive and no fresh id is jammed onto the block key.
        let new = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("source: jira") && out.contains("ref: ABC-1"));
        assert!(out.contains("title: \"X (copy)\""));
    }

    #[test]
    fn duplicate_task_leaves_a_flow_valued_id_property_untouched() {
        // A FLOW-valued id property (inline map/list on one line) is the user's
        // frontmatter, not a scalar id. Unlike the block case it has no indented
        // lines, so key_opens_block misses it — key_opens_flow catches it. Both
        // ids-on (no fresh id jammed over it) and ids-off (not stripped) must
        // leave the inline structure intact (Codex P2, PR #76).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("orig.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: done\ntitle: X\ntask-id: {source: jira, ref: ABC-1}\n---\n",
        )
        .unwrap();
        let on = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
        let out_on = std::fs::read_to_string(&on).unwrap();
        assert!(out_on.contains("task-id: {source: jira, ref: ABC-1}"));
        assert!(out_on.contains("title: \"X (copy)\""));
        assert!(out_on.contains("status: new"));
        let off = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), false).unwrap();
        let out_off = std::fs::read_to_string(&off).unwrap();
        assert!(out_off.contains("task-id: {source: jira, ref: ABC-1}"));
    }

    #[test]
    fn update_task_fields_sets_rewrites_and_clears_description() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let p = root.join("t.md");
        std::fs::write(&p, "---\ntype: Task\nstatus: new\ntitle: X\n---\n\nbody\n").unwrap();
        let quoted = crate::template::yaml_quote_multiline("hi\nthere #42");
        update_task_fields(&root, &p, &[("description", Some(quoted.as_str()))], None).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        // NOTE (brief deviation): the brief's literal `super::parse::…` does not
        // resolve here — `super` inside this nested `tests` module means `disk`,
        // not `tasks` (that shorthand only works from disk.rs's own top-level
        // functions, or from a sibling module like list.rs, one nesting level
        // shallower). Fully qualifying from the crate root reaches the same
        // `pub(super)` item — still visible, since `tasks::disk::tests` is a
        // descendant of `tasks` — without changing `description_field`'s
        // visibility or touching any other call site.
        assert_eq!(
            crate::tasks::parse::description_field(&after),
            Some("hi\nthere #42".to_string())
        );
        assert!(after.contains("\nbody\n")); // body untouched
        update_task_fields(&root, &p, &[("description", None)], None).unwrap();
        assert_eq!(
            crate::tasks::parse::description_field(&std::fs::read_to_string(&p).unwrap()),
            None
        );
    }
}
