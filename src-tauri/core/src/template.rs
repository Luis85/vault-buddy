//! Additive-template primitives shared by the note/task/document renderers.
//! `substitute` fills `{{token}}` placeholders in body templates;
//! `render_extra_frontmatter` renders a frontmatter template safely — it
//! tokenizes placeholders, parses with a real YAML library, drops reserved
//! managed keys, and re-emits Obsidian-compatible mapping lines, so a user's
//! extra frontmatter can never break the fence or redefine a managed key.

use serde_yaml_ng::{Mapping, Value};

/// Double-quote a YAML scalar, escaping `\` and `"` and flattening newlines to
/// spaces. The home for the app's frontmatter quoting: `render_note`/
/// `render_task`/`render_frontmatter`'s managed fields all use it, and
/// `capture_note` re-exports it so its existing callers keep the
/// `capture_note::yaml_quote` path.
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
            // Splice each recorded value in place of its sentinel, in index order.
            // A recorded value that itself contained a later index's sentinel text
            // could cross-substitute, but values are app/user text where a literal
            // U+E000 delimiter is effectively impossible and the worst case is one
            // value replacing another — never a structure breakout. Bounded, not guarded.
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
        Value::Tagged(_) => {
            // A custom YAML tag (`!x …`) parses into Tagged without error, but
            // Obsidian's js-yaml throws on an unknown tag — injected inside the
            // managed fence, that invalidates the whole block. Resolve the inner
            // value and STRIP the tag (unwrap to inner) so output stays
            // Obsidian-parseable and no sentinel leaks. (Standard tags like `!!str`
            // are already resolved to String/Number by serde before we get here.)
            if let Value::Tagged(mut t) = std::mem::take(v) {
                resolve_value(&mut t.value, values);
                *v = t.value;
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
            Value::String(s) => {
                // Drop a YAML merge key (`<<`) unconditionally. serde_yaml_ng keeps
                // it literal (no merge expansion), but Obsidian's js-yaml honors it,
                // which would promote a nested reserved key to the top level and
                // evade this filter. Merge anchors have no use in additive frontmatter.
                s != "<<" && !reserved.iter().any(|r| r.eq_ignore_ascii_case(s))
            }
            _ => true,
        })
        .collect();
    if kept.is_empty() {
        log::warn!("extra frontmatter dropped: all keys reserved");
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
            // Exact column-0 match only: serde emits document markers at column 0,
            // while a value's block-scalar content is always indented, so an
            // indented `---` inside a multiline value is preserved, not stripped.
            *l != "---" && *l != "..."
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

    #[test]
    fn render_drops_a_merge_key_to_block_reserved_evasion() {
        // serde_yaml_ng keeps `<<` literal; Obsidian's js-yaml would honor it and
        // promote the nested reserved key. The filter drops a top-level `<<` outright.
        let out = render_extra_frontmatter("<<: {type: evil}\nkeep: kept", &[], &["type"]);
        assert!(
            !out.contains("type"),
            "merge-key reserved evasion blocked: {out}"
        );
        assert!(out.contains("keep: kept"), "{out}");
    }

    #[test]
    fn render_strips_a_custom_tag_and_resolves_its_placeholder() {
        // A custom tag would break Obsidian's js-yaml and hide a sentinel; the tag is
        // stripped and the inner placeholder resolved.
        let out = render_extra_frontmatter("foo: !x {{title}}", &[("title", "hi")], &[]);
        assert_eq!(out, "foo: hi\n");
        assert!(!out.contains('\u{E000}'), "no sentinel leaks: {out}");
    }

    #[test]
    fn render_drops_a_placeholder_key_that_resolves_to_a_reserved_name() {
        // Resolve-before-filter: a placeholder in KEY position resolving to a reserved
        // name is still dropped.
        assert_eq!(
            render_extra_frontmatter("{{k}}: evil\nkeep: kept", &[("k", "type")], &["type"]),
            "keep: kept\n"
        );
    }

    #[test]
    fn render_keeps_a_value_with_quotes_and_backslash_as_one_scalar() {
        // Arbitrary substituted metacharacters stay inside one scalar.
        let out = render_extra_frontmatter("note: {{t}}", &[("t", "a \"q\" \\ b")], &[]);
        assert_eq!(out.lines().count(), 1, "one scalar line: {out}");
        assert!(out.starts_with("note:"), "{out}");
    }
}
