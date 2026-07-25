//! Lenient readers for the two parent keys. `parent-id` is authoritative for
//! hierarchy resolution; `parent` is an Obsidian link carried for navigation
//! only and is never parsed for meaning.

/// The raw `parent-id` scalar, or `None` when absent/empty/non-scalar. Lenient
/// like every other widened field: a block (`|`/`>`) or flow (`[..]`/`{..}`)
/// value degrades to None rather than surfacing a partial value.
pub(super) fn parent_id_field(content: &str) -> Option<String> {
    scalar(content, "parent-id")
}

/// The raw `parent` link scalar. Carried through to the DTO verbatim — the app
/// never interprets it.
pub(super) fn parent_link_field(content: &str) -> Option<String> {
    scalar(content, "parent")
}

/// STRICT optional-field decode — deliberately NOT `decode_scalar_lenient`.
/// That decoder exists for TITLES, where falling back to raw text is right
/// because a title must never vanish. A parent reference is the opposite: a
/// wrong value manufactures a phantom relationship and would make
/// `vault_has_parent_links` block ID settings forever. So unsupported and
/// null-ish forms yield None, matching `description_field`'s rules (Codex P2,
/// PR #77).
fn scalar(content: &str, key: &str) -> Option<String> {
    let raw = crate::capture_note::raw_scalar_field(content, key)?.trim();
    if raw.is_empty() {
        return None;
    }
    // A block (`|`/`>`) or flow (`{..}`) value is the user's own structure, not
    // our scalar. `[[wikilink]]` is exempt: it is the form users type for the
    // `parent` link, and that value is never parsed for meaning.
    if raw.starts_with(['|', '>', '{']) || (raw.starts_with('[') && !raw.starts_with("[[")) {
        return None;
    }
    // A leading `#` is a YAML comment — the property is null.
    if raw.starts_with('#') {
        return None;
    }
    let decoded = if raw.starts_with('"') {
        // An unterminated quoted scalar is multi-line; reject rather than
        // surfacing its first line.
        crate::yaml_scalar::yaml_unquote_multiline(super::description::double_quoted_slice(raw)?)
    } else if raw.starts_with('\'') {
        super::description::decode_single_quoted(raw)?
    } else {
        let stripped = super::description::strip_inline_comment(raw).trim();
        if matches!(stripped, "null" | "Null" | "NULL" | "~") {
            return None;
        }
        stripped.to_string()
    };
    (!decoded.trim().is_empty()).then_some(decoded)
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
    fn null_comment_and_unterminated_forms_read_as_no_parent() {
        // A parent reference is a REFERENCE: a wrong value is worse than none.
        // These would otherwise become phantom ids and permanently block the
        // ID-settings guard (Codex P2, PR #77).
        for body in [
            "parent-id: # note",
            "parent-id: null",
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
}
