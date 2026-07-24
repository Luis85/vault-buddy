//! Reading the task `description:` frontmatter field: decode each single-line
//! YAML scalar form exactly as Obsidian's js-yaml does; reject the multi-line
//! forms (block / line-spanning quoted) rather than expose a partial value.

use super::parse::strip_inline_comment;
use crate::yaml_scalar::yaml_unquote_multiline;

/// Extract the `"..."` span of a double-quoted scalar starting at `s[0] == '"'`,
/// through its closing quote (the first `"` not escaped by a preceding `\`).
/// `"` and `\` are ASCII, so byte-scanning can never mismatch inside a
/// multi-byte char; any trailing ` # comment` after the close is left out.
/// None when the scalar is unterminated.
fn double_quoted_slice(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    let mut i = 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2, // skip the escaped char (its bytes are never " or \)
            b'"' => return Some(&s[..=i]),
            _ => i += 1,
        }
    }
    None
}

/// Decode a single-quoted YAML scalar starting at `s[0] == '\''`: the content up
/// to the closing quote (a `'` that is NOT doubled), collapsing each `''` to one
/// `'`. A trailing ` # comment` after the close is dropped. None when
/// unterminated.
fn decode_single_quoted(s: &str) -> Option<String> {
    let inner = &s[1..];
    let b = inner.as_bytes();
    let mut out = String::with_capacity(inner.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\'' {
            if b.get(i + 1) == Some(&b'\'') {
                out.push('\'');
                i += 2;
            } else {
                return Some(out); // closing quote — the rest is a comment
            }
        } else {
            let ch = inner[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    None
}

/// True when `value` begins a QUOTED scalar (`"`/`'`) whose closing quote is NOT
/// on this same physical line — i.e. a multi-line quoted scalar, whose
/// continuation lives on the following indented lines. Both the reader (which
/// rejects it) and `set_fields` (which consumes its continuation on a rewrite so
/// nothing orphans) key off this, so they agree (Codex P2, PR #76).
pub(super) fn opens_multiline_quoted(value: &str) -> bool {
    (value.starts_with('"') && double_quoted_slice(value).is_none())
        || (value.starts_with('\'') && decode_single_quoted(value).is_none())
}

/// Read the top-level `description:` free-text field, decoded by its YAML scalar
/// form so a value reads exactly as Obsidian's js-yaml does (Codex P2, PR #76):
/// a DOUBLE-quoted scalar (`"…"`, optionally followed by a ` # comment`) is
/// unescaped via `yaml_unquote_multiline` on just its span; a SINGLE-quoted
/// scalar (`'…'`, `''` → `'`, optional trailing comment) is decoded likewise. An
/// UNQUOTED value has its YAML comment stripped as Obsidian would: a `#` that
/// STARTS the value or is whitespace-preceded begins a comment (`# foo` reads as
/// none, `bar # baz` reads as `bar`), while a `#` glued to non-whitespace
/// (`bar#baz`) stays content; a YAML null form (`null`/`~`) reads as none. A
/// BLOCK scalar (`description: |` / `>` + indented lines) is REJECTED (`None`),
/// as are a multi-line quoted scalar and a flow collection (`[..]`/`{..}`): we
/// store/read single-line scalars only, and surfacing a partial value or a bare
/// `|`/`>` marker would be wrong (Codex P2, PR #76) — the block itself stays
/// safe, since `set_fields` consumes it whole on a rewrite. Returns `None` when
/// absent or empty. Top-level only (an
/// indented `  description:` never matches), stops at the closing fence,
/// mirroring `note_field`.
pub(super) fn description_field(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        if line.trim_end() == "---" {
            break; // end of frontmatter — never scan the body
        }
        if let Some(rest) = line.strip_prefix("description:") {
            let raw = rest.trim();
            if raw.starts_with(['|', '>']) {
                return None; // block scalar — not a single-line value we expose
            }
            if raw.starts_with(['[', '{']) {
                // A flow collection (`[a, b]` / `{k: v}`) is structured YAML, not a
                // scalar string — Obsidian/js-yaml parses it as a sequence/mapping.
                // Surfacing the raw syntax as text would misreport the value AND a
                // later detail save would canonicalize it into quoted text, so
                // degrade to no description like the other unsupported forms (Codex
                // P2, PR #76). Quoted values start with `"`/`'`, so a bracket INSIDE
                // quotes (`"[not a list]"`) still reaches the quoted branch below.
                return None;
            }
            if raw.starts_with('#') {
                // A value that STARTS with `#` is a YAML comment — the property is
                // null. `strip_inline_comment` (shared with the tags parser)
                // deliberately KEEPS a leading `#` (a `#tag` token), so it would
                // otherwise expose the comment as the description; a description has
                // no such exception (Codex P2, PR #76). Quoted values start with
                // `"`/`'`, so a quoted `"#hashtag"` still reaches the branch below.
                return None;
            }
            let decoded = if raw.starts_with('"') {
                // A double-quoted scalar that doesn't close on THIS physical line
                // is multi-line (its continuation is on the following indented
                // lines) — `double_quoted_slice` returns None, so `?` rejects it
                // rather than exposing the unterminated first line (Codex P2,
                // PR #76). set_fields consumes the continuation on a rewrite, so
                // the block stays safe.
                yaml_unquote_multiline(double_quoted_slice(raw)?)
            } else if raw.starts_with('\'') {
                // Same for a single-quoted scalar that spans lines.
                decode_single_quoted(raw)?
            } else {
                // Plain (unquoted) scalar: a whitespace-preceded `#` is a YAML
                // comment (matching Obsidian — a literal `#` must be QUOTED),
                // and a YAML null form reads as no description (Codex P2, PR #76).
                let stripped = strip_inline_comment(raw).trim();
                if matches!(stripped, "null" | "Null" | "NULL" | "~") {
                    return None;
                }
                stripped.to_string()
            };
            return (!decoded.is_empty()).then_some(decoded);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn description_field_decodes_a_multiline_scalar_and_ignores_comment_hash() {
        let content = "---\ntype: Task\ndescription: \"fix bug #42\\nsee notes\"\n---\n\nbody\n";
        assert_eq!(
            super::description_field(content),
            Some("fix bug #42\nsee notes".to_string())
        );
    }

    #[test]
    fn description_field_is_none_when_absent_or_empty() {
        assert_eq!(super::description_field("---\ntype: Task\n---\n"), None);
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: \"\"\n---\n"),
            None
        );
    }

    #[test]
    fn description_field_decodes_quoted_forms_and_drops_trailing_comments() {
        // A hand-authored description can use any YAML scalar form; each must
        // read as Obsidian's js-yaml decodes it (Codex P2, PR #76).
        // Double-quoted with a trailing comment → the # after the close is a comment.
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: \"done\" # note\n---\n")
                .as_deref(),
            Some("done")
        );
        // Single-quoted, with the YAML `''` → `'` escape.
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: 'it''s done'\n---\n")
                .as_deref(),
            Some("it's done")
        );
        // Single-quoted with a trailing comment.
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: 'plain' # c\n---\n").as_deref(),
            Some("plain")
        );
        // A `#` INSIDE double quotes is content, not a comment.
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: \"has #hash inside\"\n---\n")
                .as_deref(),
            Some("has #hash inside")
        );
        // An UNQUOTED description strips a whitespace-preceded `#` comment,
        // matching Obsidian — a literal `#` must be QUOTED (see the
        // "has #hash inside" double-quoted case above).
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: use #tag today\n---\n")
                .as_deref(),
            Some("use")
        );
    }

    #[test]
    fn description_field_reads_a_plain_scalar_like_obsidian() {
        // A whitespace-preceded `#` in a plain scalar is a YAML comment (Codex
        // P2, PR #76).
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: call Bob # private\n---\n")
                .as_deref(),
            Some("call Bob")
        );
        // YAML null forms read as no description.
        for n in ["null", "Null", "NULL", "~"] {
            assert_eq!(
                super::description_field(&format!("---\ntype: Task\ndescription: {n}\n---\n")),
                None,
                "`{n}` is YAML null"
            );
        }
        // A plain value with no comment/null is kept verbatim (incl. digits).
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: 12345\n---\n").as_deref(),
            Some("12345")
        );
    }

    #[test]
    fn description_field_rejects_a_block_scalar() {
        // A block scalar (`|` literal / `>` folded, with any chomping/indent
        // suffix) is not a single-line value we expose — reading it must NOT
        // surface the bare marker as the description (Codex P2, PR #76).
        for marker in ["|", "|-", "|+", ">", ">-", ">2"] {
            let doc = format!(
                "---\ntype: Task\ndescription: {marker}\n  indented body line\n  second line\n---\n"
            );
            assert_eq!(
                super::description_field(&doc),
                None,
                "block scalar `{marker}` must read as no description"
            );
        }
    }

    #[test]
    fn description_field_rejects_a_multiline_quoted_scalar() {
        // A quoted scalar whose close is on a LATER line is multi-line — reading
        // must reject it, not surface the unterminated first physical line
        // (Codex P2, PR #76).
        let dq = "---\ntype: Task\ndescription: \"first line\n  second line\"\n---\n";
        assert_eq!(super::description_field(dq), None);
        let sq = "---\ntype: Task\ndescription: 'first line\n  second line'\n---\n";
        assert_eq!(super::description_field(sq), None);
        // A single-line quoted value still decodes (the close IS on the line).
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: \"all one line\"\n---\n")
                .as_deref(),
            Some("all one line")
        );
    }

    #[test]
    fn description_field_treats_a_leading_hash_as_a_comment() {
        // A value starting with `#` is a YAML comment → the property is null. The
        // tags parser keeps a leading `#` (a tag token), but a description must not
        // (Codex P2, PR #76).
        for v in ["#private", "# private", "#tag then words"] {
            assert_eq!(
                super::description_field(&format!("---\ntype: Task\ndescription: {v}\n---\n")),
                None,
                "`{v}` is a YAML comment"
            );
        }
        // A `#` glued to non-whitespace content (not a comment) is still kept.
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: color#3\n---\n").as_deref(),
            Some("color#3")
        );
        // A `#` inside quotes is content, not a comment.
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: \"#hashtag\"\n---\n")
                .as_deref(),
            Some("#hashtag")
        );
    }

    #[test]
    fn description_field_rejects_flow_collections() {
        // A flow sequence/mapping is structured YAML, not a scalar string —
        // Obsidian parses it as a list/map, so exposing the raw syntax as text
        // would misreport the value and a later save would canonicalize the
        // structure into quoted text (Codex P2, PR #76).
        for v in ["[first, second]", "{text: note}", "[]", "{}", "[a"] {
            assert_eq!(
                super::description_field(&format!("---\ntype: Task\ndescription: {v}\n---\n")),
                None,
                "flow collection `{v}` must read as no description"
            );
        }
        // A bracket INSIDE a quoted scalar is content, not a flow collection.
        assert_eq!(
            super::description_field("---\ntype: Task\ndescription: \"[not a list]\"\n---\n")
                .as_deref(),
            Some("[not a list]")
        );
    }
}
