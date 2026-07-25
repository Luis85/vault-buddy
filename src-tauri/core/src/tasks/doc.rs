//! Document identity: what counts as a task. The two primitives BOTH the
//! writer and the list depend on, so they can never disagree.

use super::parse::scalar_field;

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
/// The `type` scalar is read through `scalar_field` (comment strip + quote
/// unwrap), not a raw `note_field` compare, so a valid `type: Task # note` or
/// `type: 'Task'` still counts — the same lenient read the structured task
/// fields use, keeping the list and the writer in agreement (Codex, PR #46).
pub fn is_task(content: &str) -> bool {
    has_closed_frontmatter(content) && scalar_field(content, "type").as_deref() == Some("Task")
}

/// True for a name ending `.md` CASE-INSENSITIVELY (`.md`/`.MD`/`.Md`/…) —
/// the same rule `search.rs`'s own note scan applies (AGENTS.md's search
/// section: "notes are any-case `.md`"). Shared by `collect.rs` (the
/// list/structural walk) and `lists/relocate.rs` (delete-list's direct-task
/// scan) so the two can't independently drift back to a case-sensitive
/// compare — a case-sensitive miss here is not cosmetic: a hand-authored
/// `B.MD` invisible to the STRUCTURAL scan is a `parent-id` edge that does
/// not exist as far as the cycle guard is concerned (see
/// `services::tasks::parent`'s own tests for the concrete cycle that let
/// through). The suffix is 3 ASCII bytes, so the byte compare can't split a
/// UTF-8 char boundary; `>= 3` matches `str::ends_with(".md")`'s own
/// acceptance of a bare `.md` name exactly, just case-insensitively.
pub(super) fn is_markdown_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() >= 3 && b[b.len() - 3..].eq_ignore_ascii_case(b".md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_markdown_name_is_case_insensitive() {
        for name in [".md", "a.md", "a.MD", "a.Md", "a.mD", "A.B.md"] {
            assert!(is_markdown_name(name), "{name} must count as markdown");
        }
        for name in ["", "a", "md", "a.mdx", "a.txt", ".m"] {
            assert!(!is_markdown_name(name), "{name} must not count as markdown");
        }
    }

    #[test]
    fn is_task_only_true_for_type_task() {
        assert!(is_task("---\ntype: Task\nstatus: new\n---\n"));
        assert!(is_task("---\ntype: \"Task\"\n---\n")); // quoted also fine
        assert!(!is_task("---\ntype: Meeting\n---\n"));
        assert!(!is_task("no frontmatter"));
    }

    #[test]
    fn is_task_tolerates_commented_and_single_quoted_type() {
        // Codex review, PR #46 round 4: the identity check must read the
        // `type` scalar the same lenient way the structured fields do.
        // Otherwise a hand-authored `type: Task # chores` or `type: 'Task'`
        // (both valid YAML that Obsidian reads as `Task`) fails the exact
        // compare — the task vanishes from list_tasks AND the same is_task
        // guard makes a later status/field edit refuse the file as non-task.
        assert!(is_task("---\ntype: Task # chores\n---\n"));
        assert!(is_task("---\ntype: 'Task'\n---\n"));
        // A commented-out non-task type is still not a task.
        assert!(!is_task("---\ntype: Meeting # x\n---\n"));
    }

    #[test]
    fn is_task_false_for_unterminated_frontmatter() {
        // A type: Task block that never closes is malformed: set_status refuses
        // to toggle it, so the list must not surface it as a task either — the
        // list and the toggle must agree on what counts as a task.
        assert!(!is_task("---\ntype: Task\ntitle: \"x\"\n"));
    }
}
