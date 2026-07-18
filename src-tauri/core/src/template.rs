//! Additive-template primitives shared by the note/task/document renderers.
//! `substitute` fills `{{token}}` placeholders; `sanitize_extra_frontmatter`
//! makes a user's extra-frontmatter text safe to inject before a closing
//! `---` — it can never break the fence or redefine a managed key.

use serde_yaml_ng::{Mapping, Value};

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

/// A sentinel wrapping a Private-Use-Area delimiter (U+E000) — a valid YAML
/// plain-scalar character (YAML c-printable includes U+E000–U+FFFD) that cannot
/// occur in real input, so a `{{placeholder}}` parses as opaque structure and
/// its value is spliced back after parsing.
fn sentinel(i: usize) -> String {
    format!("\u{E000}{i}\u{E000}")
}

/// Walk `{{key}}` placeholders (whitespace inside the braces tolerated),
/// pushing `resolve(key)` for each. An unclosed `{{` is emitted literally.
/// UTF-8 safe. Shared by `substitute` (body templates) and `tokenize`
/// (frontmatter), so the two can never disagree on placeholder syntax.
fn expand_placeholders(template: &str, mut resolve: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            out.push_str(&resolve(after[..end].trim()));
            rest = &after[end + 2..];
        } else {
            out.push_str("{{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Replace every `{{key}}` (whitespace inside the braces tolerated) with its
/// value from `vars`. An unknown key renders empty. Unclosed `{{` is emitted
/// literally. UTF-8 safe. Used for BODY templates (raw markdown); frontmatter
/// templates go through `render_extra_frontmatter`, which parses the result.
pub fn substitute(template: &str, vars: &[(&str, &str)]) -> String {
    expand_placeholders(template, |key| {
        vars.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| (*v).to_string())
            .unwrap_or_default()
    })
}

/// Replace each KNOWN `{{key}}` with a sentinel scalar (recording its value at
/// that index) so the text parses as the structure the user intended; an
/// unknown key renders empty (matching `substitute`). Returns the tokenized
/// text and the values indexed by sentinel number.
fn tokenize(template: &str, vars: &[(&str, &str)]) -> (String, Vec<String>) {
    let mut values: Vec<String> = Vec::new();
    let text = expand_placeholders(template, |key| match vars.iter().find(|(k, _)| *k == key) {
        Some((_, v)) => {
            let idx = values.len();
            values.push((*v).to_string());
            sentinel(idx)
        }
        None => String::new(),
    });
    (text, values)
}

/// Splice recorded values back in place of their sentinels in every scalar
/// (mapping keys and values, recursively). A value lands as an opaque string,
/// so it can never inject YAML structure and a numeric-looking value stays a
/// string.
fn resolve_value(v: &mut Value, values: &[String]) {
    match v {
        Value::String(s) => {
            for (i, val) in values.iter().enumerate() {
                let token = sentinel(i);
                if s.contains(&token) {
                    *s = s.replace(&token, val);
                }
            }
        }
        Value::Sequence(seq) => seq.iter_mut().for_each(|e| resolve_value(e, values)),
        Value::Mapping(map) => {
            // Rebuild so KEYS are resolved too; the IndexMap-backed Mapping
            // preserves insertion order across the rebuild.
            let taken = std::mem::take(map);
            for (mut k, mut val) in taken {
                resolve_value(&mut k, values);
                resolve_value(&mut val, values);
                map.insert(k, val);
            }
        }
        _ => {}
    }
}

/// Render a per-vault extra-frontmatter template into mapping lines safe to
/// inject before a closing `---`. `{{placeholders}}` are resolved via a
/// sentinel round-trip, reserved top-level keys dropped, and the result
/// re-emitted as standard, Obsidian-compatible YAML (no document markers).
/// Malformed input, a non-mapping root, or an all-reserved block yields `""`
/// (logged) — never a broken fence or a `{}` literal. Replaces the former
/// substitute-then-sanitize pair.
pub fn render_extra_frontmatter(
    template: &str,
    vars: &[(&str, &str)],
    reserved: &[&str],
) -> String {
    let (tokenized, values) = tokenize(template, vars);
    if tokenized.trim().is_empty() {
        return String::new();
    }
    let mut root: Value = match serde_yaml_ng::from_str(&tokenized) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("extra frontmatter dropped: invalid YAML ({e})");
            return String::new();
        }
    };
    resolve_value(&mut root, &values);
    let Value::Mapping(map) = root else {
        log::warn!("extra frontmatter dropped: root is not a mapping");
        return String::new();
    };
    let kept: Mapping = map
        .into_iter()
        .filter(|(k, _)| match k {
            Value::String(s) => !reserved.iter().any(|r| r.eq_ignore_ascii_case(s)),
            _ => true,
        })
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let emitted = match serde_yaml_ng::to_string(&Value::Mapping(kept)) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("extra frontmatter dropped: emit failed ({e})");
            return String::new();
        }
    };
    // The serializer emits no document markers for a root mapping; strip any
    // bare ---/... line defensively (we inject INSIDE our own fence) and
    // normalize to exactly one trailing newline.
    let mut lines: Vec<&str> = emitted
        .lines()
        .filter(|l| {
            let t = l.trim();
            t != "---" && t != "..."
        })
        .collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut body = lines.join("\n");
    body.push('\n');
    body
}

/// Return the lines of `text` safe to inject into a frontmatter block:
/// - a `---`/`...` line (a fence) is dropped, and so is any indented block
///   under it — user frontmatter can never break out of the block;
/// - a top-level line whose key (before the first `:`) is in `reserved`
///   (case-insensitive, surrounding quotes stripped so `"type":` can't evade
///   it) is dropped along with its indented continuation lines, so a managed
///   key can't be redefined;
/// - a top-level line that is not a real `key: value` / `key:` mapping entry —
///   a bare scalar (including one whose colon is not a `: ` separator, like
///   `https://x` or `project:Alpha`), or a `- list` sequence entry (even with
///   an inline `- key: value`) — is dropped: in a mapping block it is invalid YAML;
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
        // A top-level line must be a REAL YAML mapping entry: a `: ` (colon +
        // space) separator, or a trailing `:` (empty value / block opener). A
        // colon that is not a separator is part of a scalar (`https://x`,
        // `project:Alpha`) — NOT a mapping — so drop the line and its block
        // rather than inject a bare scalar that breaks the whole block.
        let mapping = if let Some(i) = line.find(": ") {
            Some((&line[..i], &line[i + 1..]))
        } else {
            line.trim_end().strip_suffix(':').map(|k| (k, ""))
        };
        let Some((raw_key, value)) = mapping else {
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
    fn yaml_crate_preserves_key_order_and_emits_no_doc_markers() {
        // Gate-zero behaviour the design depends on: parse→emit keeps insertion
        // order and does not wrap a root mapping in ---/... markers.
        let v: serde_yaml_ng::Value = serde_yaml_ng::from_str("b: 2\na: 1\n").unwrap();
        let out = serde_yaml_ng::to_string(&v).unwrap();
        assert_eq!(out, "b: 2\na: 1\n");
        assert!(!out.contains("---") && !out.contains("..."));
    }

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
    fn sanitize_drops_scalars_whose_colon_is_not_a_mapping_separator() {
        // A colon not followed by a space (a URL, or a missing-space typo) is
        // part of a scalar, not a mapping separator — drop the line rather than
        // inject a bare scalar that breaks the whole block (Codex P2).
        assert_eq!(sanitize_extra_frontmatter("https://example.com", &[]), "");
        assert_eq!(sanitize_extra_frontmatter("project:Alpha", &[]), "");
        // A real `: ` separator and a trailing `:` (block opener) are kept; a
        // URL sitting as a VALUE (after a real separator) is fine.
        assert_eq!(
            sanitize_extra_frontmatter("home: https://example.com", &[]),
            "home: https://example.com\n"
        );
        assert_eq!(
            sanitize_extra_frontmatter("attendees:", &["type"]),
            "attendees:\n"
        );
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

    #[test]
    fn render_resolves_placeholders_and_preserves_key_order() {
        let vars = [("title", "Buy milk"), ("date", "2026-07-18")];
        assert_eq!(
            render_extra_frontmatter("name: {{title}}\nwhen: {{date}}", &vars, &[]),
            "name: Buy milk\nwhen: 2026-07-18\n"
        );
    }

    #[test]
    fn render_keeps_a_colon_value_as_one_safe_scalar() {
        // A substituted value with a colon-space would read as a nested mapping if
        // injected raw; via the parsed tree it is one quoted scalar.
        assert_eq!(
            render_extra_frontmatter("summary: {{t}}", &[("t", "Ship: v1")], &[]),
            "summary: 'Ship: v1'\n"
        );
    }

    #[test]
    fn render_keeps_bracket_and_numeric_values_as_strings() {
        // `[draft]` stays a string (not a flow sequence); a numeric placeholder
        // value stays a string so Obsidian reads it as text, not a number.
        assert_eq!(
            render_extra_frontmatter("label: {{t}}", &[("t", "[draft]")], &[]),
            "label: '[draft]'\n"
        );
        assert_eq!(
            render_extra_frontmatter("year: {{t}}", &[("t", "2026")], &[]),
            "year: '2026'\n"
        );
    }

    #[test]
    fn render_keeps_a_literal_number_as_a_number() {
        assert_eq!(render_extra_frontmatter("count: 5", &[], &[]), "count: 5\n");
    }

    #[test]
    fn render_drops_reserved_keys_plain_quoted_and_case_insensitive() {
        assert_eq!(
            render_extra_frontmatter("type: Evil\nkeep: kept", &[], &["type"]),
            "keep: kept\n"
        );
        assert_eq!(
            render_extra_frontmatter("\"type\": Evil\nkeep: kept", &[], &["type"]),
            "keep: kept\n"
        );
        assert_eq!(
            render_extra_frontmatter("Type: Evil\nkeep: kept", &[], &["type"]),
            "keep: kept\n"
        );
        // A non-reserved key is kept.
        assert_eq!(
            render_extra_frontmatter("project: Alpha", &[], &["type"]),
            "project: Alpha\n"
        );
    }

    #[test]
    fn render_resolves_a_placeholder_inside_a_sequence() {
        // Capability the line heuristic never had: placeholders inside collections.
        assert_eq!(
            render_extra_frontmatter(
                "people: [{{a}}, {{b}}]",
                &[("a", "Alex"), ("b", "Sam")],
                &[]
            ),
            "people:\n- Alex\n- Sam\n"
        );
    }

    #[test]
    fn render_drops_injected_markers_sequences_and_bare_scalars() {
        // A stray fence makes it multi-document → dropped; a sequence or scalar
        // root is not a mapping → dropped.
        assert_eq!(
            render_extra_frontmatter("owner: me\n---\nsneaky: 1", &[], &[]),
            ""
        );
        assert_eq!(render_extra_frontmatter("- a\n- b", &[], &[]), "");
        assert_eq!(render_extra_frontmatter("just text", &[], &[]), "");
    }

    #[test]
    fn render_empty_and_all_reserved_yield_empty_never_brace() {
        assert_eq!(render_extra_frontmatter("", &[], &["type"]), "");
        assert_eq!(render_extra_frontmatter("   \n  ", &[], &["type"]), "");
        // Every key reserved → empty mapping → "" (never the serializer's `{}`).
        assert_eq!(
            render_extra_frontmatter("type: a\ntags: b", &[], &["type", "tags"]),
            ""
        );
    }

    #[test]
    fn render_malformed_yaml_drops_the_block() {
        assert_eq!(render_extra_frontmatter("key: [unclosed", &[], &[]), "");
    }

    #[test]
    fn render_output_is_obsidian_safe_no_markers_no_tabs() {
        let out = render_extra_frontmatter("a: 1\nb: {{t}}", &[("t", "x")], &["z"]);
        assert!(!out.contains('\t'), "no tabs: {out}");
        for line in out.lines() {
            assert!(
                line.trim() != "---" && line.trim() != "...",
                "no markers: {out}"
            );
        }
    }
}
