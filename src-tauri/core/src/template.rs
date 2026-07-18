//! Additive-template primitives shared by the note/task/document renderers.
//! `substitute` fills `{{token}}` placeholders; `sanitize_extra_frontmatter`
//! makes a user's extra-frontmatter text safe to inject before a closing
//! `---` — it can never break the fence or redefine a managed key.

/// Double-quote a YAML scalar, escaping `\` and `"` and flattening newlines to
/// spaces. The home for the app's frontmatter quoting: `render_note`/
/// `render_task`/`render_frontmatter`'s managed fields and this module's
/// `sanitize_extra_frontmatter` value-quoting all use it, and `capture_note`
/// re-exports it so its existing callers keep the `capture_note::yaml_quote`
/// path.
pub fn yaml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ");
    format!("\"{escaped}\"")
}

/// Replace every `{{key}}` (whitespace inside the braces tolerated) with its
/// value from `vars`. An unknown key renders empty (the available keys are
/// documented in the UI). Unclosed `{{` is emitted literally. UTF-8 safe.
/// Values are inserted RAW; for frontmatter templates,
/// `sanitize_extra_frontmatter` quotes any resulting value that would be an
/// unsafe YAML scalar, so callers substitute the same way for body and
/// frontmatter.
pub fn substitute(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let key = after[..end].trim();
            if let Some((_, val)) = vars.iter().find(|(k, _)| *k == key) {
                out.push_str(val);
            }
            rest = &after[end + 2..];
        } else {
            out.push_str("{{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Return the lines of `text` safe to inject into a frontmatter block:
/// - a `---`/`...` line (a fence) is dropped, and so is any indented block
///   under it — user frontmatter can never break out of the block;
/// - a top-level line whose key (before the first `:`) is in `reserved`
///   (case-insensitive, surrounding quotes stripped so `"type":` can't evade
///   it) is dropped along with its indented continuation lines, so a managed
///   key can't be redefined;
/// - a top-level line that is not a `key: value` mapping — a bare scalar, or a
///   `- list` sequence entry (even one with an inline `- key: value` mapping) —
///   is dropped: injected into a mapping block it would be invalid YAML;
/// - blank lines are dropped.
///
/// Everything else is kept verbatim, newline-terminated.
pub fn sanitize_extra_frontmatter(text: &str, reserved: &[&str]) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Do NOT reset `skipping` here: a blank line inside a dropped
            // block's continuation must not resurrect the rest of that block
            // (a following TOP-LEVEL line already clears the skip below). YAML
            // block sequences legitimately contain a blank line between items.
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if !skipping {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if trimmed == "---" || trimmed == "..." {
            skipping = true;
            continue;
        }
        // A top-level YAML block-sequence entry (`- item`, or `- key: val` with
        // an inline mapping) is invalid inside the managed mapping block — reject
        // it even when it carries a colon, which would otherwise pass the mapping
        // gate below with a bogus `- key`. Codex P2 follow-up.
        if trimmed == "-" || trimmed.starts_with("- ") || trimmed.starts_with("-\t") {
            skipping = true;
            continue;
        }
        // A top-level line must be a `key: value` mapping. A line with no colon
        // (a bare scalar) would inject malformed YAML — drop it and its block.
        let Some((raw_key, value)) = line.split_once(':') else {
            skipping = true;
            continue;
        };
        // Unquote the key before the reserved check so a QUOTED reserved key
        // (`"type": Note`, which YAML/Obsidian still read as `type`) can't evade
        // it and redefine a managed field. An empty key is invalid YAML too.
        let key = unquote_key(raw_key.trim());
        if key.is_empty() || reserved.iter().any(|r| r.eq_ignore_ascii_case(key)) {
            skipping = true;
            continue;
        }
        skipping = false;
        // Quote a value that would be an unsafe YAML plain scalar — a `: `
        // (colon-space) or trailing `:` inside it (e.g. a substituted title
        // `Ship: v1`) reads as a nested mapping and corrupts the frontmatter.
        out.push_str(&emit_line(raw_key, value));
        out.push('\n');
    }
    out
}

/// Emit a kept `key: value` frontmatter line, double-quoting the value when it
/// would otherwise be an invalid YAML plain scalar — it contains a `: `
/// (colon-space, which YAML reads as a nested mapping) or ends with `:` — unless
/// the value is empty (a block/mapping opener like `key:`), already quoted, or a
/// flow collection (`[…]`/`{…}`). `raw_key`/`value` are the two halves of the
/// line's first `:`, so a value with no risky content is emitted byte-identical.
fn emit_line(raw_key: &str, value: &str) -> String {
    let v = value.trim();
    let needs_quote = !v.is_empty()
        && !v.starts_with(['"', '\'', '[', '{'])
        && (v.contains(": ") || v.ends_with(':'));
    if needs_quote {
        format!("{raw_key}: {}", yaml_quote(v))
    } else {
        format!("{raw_key}:{value}")
    }
}

/// Strip a single matched pair of surrounding ASCII quotes (`"` or `'`) from a
/// frontmatter key, so a quoted key compares equal to its bare form. The quote
/// chars are ASCII, so the byte-index slice stays on char boundaries.
fn unquote_key(key: &str) -> &str {
    for q in ['"', '\''] {
        if key.len() >= 2 && key.starts_with(q) && key.ends_with(q) {
            return key[1..key.len() - 1].trim();
        }
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_fills_known_and_empties_unknown() {
        let vars = [("title", "Buy milk"), ("date", "2026-07-18")];
        assert_eq!(
            substitute("# {{title}} ({{date}})", &vars),
            "# Buy milk (2026-07-18)"
        );
        assert_eq!(substitute("{{ title }}", &vars), "Buy milk"); // whitespace tolerated
        assert_eq!(substitute("x {{nope}} y", &vars), "x  y"); // unknown → empty
    }

    #[test]
    fn substitute_is_utf8_safe_and_tolerates_unclosed() {
        assert_eq!(substitute("café {{x}}", &[]), "café ");
        assert_eq!(substitute("a {{ open", &[]), "a {{ open");
    }

    #[test]
    fn sanitize_quotes_a_value_that_would_break_the_scalar() {
        // A value with a colon-space (e.g. a substituted title `Ship: v1`) would
        // read as a nested mapping — quote the whole value (Codex P2). A value
        // with no risky content, an already-quoted value, and a flow collection
        // are left byte-identical.
        assert_eq!(
            sanitize_extra_frontmatter("summary: Ship: v1", &[]),
            "summary: \"Ship: v1\"\n"
        );
        assert_eq!(
            sanitize_extra_frontmatter("ref: /x/a.docx (docx, 2026-07-10)", &[]),
            "ref: /x/a.docx (docx, 2026-07-10)\n"
        );
        assert_eq!(
            sanitize_extra_frontmatter("tags: [a, b]", &["type"]),
            "tags: [a, b]\n"
        );
        assert_eq!(
            sanitize_extra_frontmatter("note: \"a: b\"", &[]),
            "note: \"a: b\"\n"
        );
        // A trailing-colon value (`due:` as a value) is also quoted.
        assert_eq!(sanitize_extra_frontmatter("x: a:", &[]), "x: \"a:\"\n");
    }

    #[test]
    fn sanitize_drops_fences_and_reserved_keys() {
        let text = "project: Alpha\ntype: Evil\n---\nowner: me\ntags:\n  - x\n  - y\nnote: keep";
        let reserved = ["type", "tags"];
        let out = sanitize_extra_frontmatter(text, &reserved);
        assert!(out.contains("project: Alpha"));
        assert!(out.contains("note: keep"));
        assert!(!out.contains("type: Evil"), "reserved key dropped: {out}");
        assert!(!out.contains("---"), "fence dropped: {out}");
        assert!(!out.contains("- x"), "reserved block items dropped: {out}");
        // `owner: me` sits after a fence line → the fence starts a skip block,
        // but a following TOP-LEVEL key resets it and is kept.
        assert!(out.contains("owner: me"), "{out}");
    }

    #[test]
    fn sanitize_empty_in_empty_out() {
        assert_eq!(sanitize_extra_frontmatter("", &["type"]), "");
        assert_eq!(sanitize_extra_frontmatter("\n\n", &["type"]), "");
    }

    #[test]
    fn sanitize_blank_line_inside_a_dropped_block_does_not_leak_it() {
        // A blank line between items of a reserved key's block must NOT end the
        // skip — otherwise the trailing list items leak in as a dangling
        // fragment with no owning key, which could attach to an unrelated key
        // when reinjected. Regression for the Task 6 review finding.
        let text = "tags:\n  - x\n\n  - y\nnote: keep";
        let out = sanitize_extra_frontmatter(text, &["tags"]);
        assert!(!out.contains("- x"), "first item dropped: {out}");
        assert!(
            !out.contains("- y"),
            "item after the blank must NOT leak: {out}"
        );
        assert_eq!(out, "note: keep\n");
    }

    #[test]
    fn sanitize_reserved_is_case_insensitive_and_dotdotdot_fence_drops() {
        // Reserved-key match ignores casing; the `...` document-end fence is
        // dropped like `---`, along with its indented block.
        let text = "Type: Evil\n...\n  leaked: 1\nkeep: yes";
        let out = sanitize_extra_frontmatter(text, &["type"]);
        assert!(
            !out.contains("Type: Evil"),
            "case-insensitive reserved drop: {out}"
        );
        assert!(
            !out.contains("leaked"),
            "... fence + its indented block dropped: {out}"
        );
        assert_eq!(out, "keep: yes\n");
    }

    #[test]
    fn sanitize_drops_top_level_sequence_entries_and_bare_scalars() {
        // A bare scalar and a top-level `- list` entry are invalid in a mapping
        // block. `- project: Alpha` is the Codex follow-up case: it carries a
        // colon but is still a sequence entry, not a `key: value` mapping.
        let text = "- personal\n- project: Alpha\n-\njust some text\nvalid: yes";
        let out = sanitize_extra_frontmatter(text, &[]);
        assert!(
            !out.contains("- personal"),
            "no-colon list item dropped: {out}"
        );
        assert!(
            !out.contains("- project"),
            "colon-bearing list item dropped: {out}"
        );
        assert!(
            !out.contains("just some text"),
            "bare scalar dropped: {out}"
        );
        assert_eq!(out, "valid: yes\n");
    }

    #[test]
    fn sanitize_drops_a_quoted_reserved_key() {
        // A QUOTED reserved key must not evade the reserved check — YAML/Obsidian
        // still read `"type": Note` as `type`, redefining the managed field.
        assert_eq!(sanitize_extra_frontmatter("\"type\": Note", &["type"]), "");
        assert_eq!(sanitize_extra_frontmatter("'type': Note", &["type"]), "");
        // A quoted NON-reserved key stays (valid YAML, kept verbatim).
        assert_eq!(
            sanitize_extra_frontmatter("\"project\": Alpha", &["type"]),
            "\"project\": Alpha\n"
        );
    }
}
