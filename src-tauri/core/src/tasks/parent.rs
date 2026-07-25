//! Lenient readers for the two parent keys. `parent-id` is authoritative for
//! hierarchy resolution; `parent` is an Obsidian link carried for navigation
//! only and is never parsed for meaning.

/// The raw `parent-id` scalar, or `None` when absent/empty/non-scalar. Lenient
/// like every other widened field: a block (`|`/`>`) or flow (`[..]`/`{..}`)
/// value degrades to None rather than surfacing a partial value. NOT the link
/// field — a `[[wikilink]]`-shaped value here is not exempt (finding 5): an
/// id must be a plain scalar, and a wikilink is exactly the kind of flow
/// value the block/flow guard exists to reject.
pub(super) fn parent_id_field(content: &str) -> Option<String> {
    scalar(content, "parent-id", false)
}

/// The raw `parent` link scalar. Carried through to the DTO verbatim — the app
/// never interprets it. The only caller that gets the `[[wikilink]]` flow
/// exemption (see `scalar`'s `link` parameter).
pub(super) fn parent_link_field(content: &str) -> Option<String> {
    scalar(content, "parent", true)
}

/// STRICT optional-field decode — deliberately NOT `decode_scalar_lenient`.
/// That decoder exists for TITLES, where falling back to raw text is right
/// because a title must never vanish. A parent reference is the opposite: a
/// wrong value manufactures a phantom relationship and would make
/// `vault_has_parent_links` block ID settings forever. So unsupported and
/// null-ish forms yield None, matching `description_field`'s rules (Codex P2,
/// PR #77).
///
/// `link` gates the `[[wikilink]]` flow-sequence exemption: only
/// `parent_link_field` passes `true` (that IS the form users type for
/// `parent`, never parsed for meaning). `parent_id_field` passes `false` — an
/// id must be a plain scalar, so `parent-id: [[Some Task]]` is rejected like
/// any other flow value, not silently accepted as if it were the link
/// (finding 5: the exemption previously applied to both keys because they
/// shared one undiscriminated `scalar` helper).
///
/// The decode itself now lives in `parse::strict_scalar_field` (Defect A): a
/// task's OWN id (`scalar_id_ci`) must decode a quoted YAML scalar
/// IDENTICALLY to a `parent-id` reference, or the two sides of an id
/// comparison can never agree — see that function's doc comment. This is a
/// thin named wrapper so `parent_id_field`/`parent_link_field` read
/// naturally; it carries no logic of its own anymore.
fn scalar(content: &str, key: &str, link: bool) -> Option<String> {
    super::parse::strict_scalar_field(content, key, link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plain_and_quoted_values() {
        let c = "---\ntype: Task\nparent-id: ab12cd34\nparent: \"[[Tasks/Work/p]]\"\n---\n";
        assert_eq!(parent_id_field(c), Some("ab12cd34".to_string()));
        assert_eq!(parent_link_field(c), Some("[[Tasks/Work/p]]".to_string()));
    }

    #[test]
    fn absent_empty_and_non_scalar_read_as_none() {
        assert_eq!(parent_id_field("---\ntype: Task\n---\n"), None);
        assert_eq!(parent_id_field("---\ntype: Task\nparent-id:\n---\n"), None);
        // A block or flow value is the user's own frontmatter, not our scalar.
        assert_eq!(
            parent_id_field("---\ntype: Task\nparent-id:\n  a: b\n---\n"),
            None
        );
        assert_eq!(
            parent_id_field("---\ntype: Task\nparent-id: {a: b}\n---\n"),
            None
        );
    }

    #[test]
    fn a_continued_plain_scalar_reads_as_no_parent() {
        // YAML folds `parent-id: abc` + an indented `def` into "abc def", so
        // returning "abc" would report a value Obsidian disagrees with — and
        // "abc" may be ANOTHER task's id, resolving the hierarchy to the wrong
        // parent (Codex P2, PR #77). Reject, like the other multi-line forms.
        let c = "---\ntype: Task\nparent-id: abc\n  def\ntitle: T\n---\n";
        assert_eq!(parent_id_field(c), None);
        // A BLANK line does not end a plain scalar either — YAML folds
        // `abc`, blank, indented `def` into "abc\ndef" (Codex P2, PR #77).
        let blank = "---\ntype: Task\nparent-id: abc\n\n  def\ntitle: T\n---\n";
        assert_eq!(parent_id_field(blank), None);
        // A single-line plain value is of course still fine.
        let ok = "---\ntype: Task\nparent-id: abc\ntitle: T\n---\n";
        assert_eq!(parent_id_field(ok), Some("abc".to_string()));
        // …and a blank line followed by a TOP-LEVEL key is not a continuation.
        let sep = "---\ntype: Task\nparent-id: abc\n\ntitle: T\n---\n";
        assert_eq!(parent_id_field(sep), Some("abc".to_string()));
    }

    #[test]
    fn null_comment_and_unterminated_forms_read_as_no_parent() {
        // A parent reference is a REFERENCE: a wrong value is worse than none.
        // These would otherwise become phantom ids and permanently block the
        // ID-settings guard (Codex P2, PR #77).
        for body in [
            "parent-id: # note",
            "parent-id: null",
            "parent-id: Null",
            "parent-id: ~",
            "parent-id: NULL",
            "parent-id: \"unterminated",
        ] {
            let c = format!("---\ntype: Task\n{body}\n---\n");
            assert_eq!(parent_id_field(&c), None, "{body} must read as no parent");
        }
    }

    #[test]
    fn an_unquoted_wikilink_still_reads_as_a_link() {
        // Hand-authored `parent: [[X]]` is a YAML flow sequence, but it is the
        // form users type; read it rather than dropping it. It is never parsed
        // for meaning, so a lenient read costs nothing.
        let c = "---\ntype: Task\nparent: [[Tasks/p]]\n---\n";
        assert_eq!(parent_link_field(c), Some("[[Tasks/p]]".to_string()));
    }

    #[test]
    fn a_wikilink_is_not_a_valid_parent_id() {
        // finding 5: the `[[…]]` exemption belongs to the `parent` LINK field
        // only — a task id must be a plain scalar, so a wikilink-shaped
        // `parent-id` must read as None, unlike `parent` itself.
        let c = "---\ntype: Task\nparent-id: [[Some Task]]\nparent: [[Some Task]]\n---\n";
        assert_eq!(parent_id_field(c), None);
        assert_eq!(parent_link_field(c), Some("[[Some Task]]".to_string()));
    }

    #[test]
    fn reads_a_single_quoted_value() {
        let c = "---\ntype: Task\nparent-id: 'ab12cd34'\n---\n";
        assert_eq!(parent_id_field(c), Some("ab12cd34".to_string()));
    }

    #[test]
    fn strips_a_trailing_inline_comment() {
        let c = "---\ntype: Task\nparent-id: abc # was xyz\n---\n";
        assert_eq!(parent_id_field(c), Some("abc".to_string()));
    }

    #[test]
    fn a_flow_sequence_is_rejected_for_the_link_field_too() {
        // Not a wikilink form (single bracket, not double) — the block/flow
        // rejection applies to `parent` exactly as it does to `parent-id`.
        let c = "---\ntype: Task\nparent: [a, b]\n---\n";
        assert_eq!(parent_link_field(c), None);
    }

    #[test]
    fn parent_id_only_document_leaves_parent_link_field_none() {
        // The two keys are kept apart solely by raw_scalar_field's exact
        // "parent:"-prefix match (parent-id's line starts with "parent-",
        // never "parent:") — nothing else pins that a `parent-id`-only
        // document can't bleed into `parent_link_field` (finding 8).
        let c = "---\ntype: Task\nparent-id: ab12cd34\n---\n";
        assert_eq!(parent_link_field(c), None);
    }
}
