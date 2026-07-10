//! Document identity: what counts as a task. The two primitives BOTH the
//! writer and the list depend on, so they can never disagree.

use crate::capture_note::note_field;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
