//! Task documents: `type: Task` markdown files under a vault's tasks folder.
//! Pure filename/render/parse logic + the two sanctioned vault writes
//! (collision-safe create; surgical `status:` flip). Same never-clobber
//! discipline as the capture note and transcript sidecar. See
//! docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md.

use crate::capture_note::yaml_quote;
use std::path::Path;

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
}
