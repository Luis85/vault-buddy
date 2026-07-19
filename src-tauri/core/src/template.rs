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

/// Count up to `max` consecutive ASCII digits at `start`.
fn count_digits(b: &[u8], start: usize, max: usize) -> usize {
    let mut c = 0;
    while c < max && matches!(b.get(start + c), Some(d) if d.is_ascii_digit()) {
        c += 1;
    }
    c
}

/// `^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$` — js-yaml's date-only
/// timestamp regex (whole string, 4-2-2 digits with literal dashes).
fn is_date_only(b: &[u8]) -> bool {
    b.len() == 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5].is_ascii_digit()
        && b[6].is_ascii_digit()
        && b[7] == b'-'
        && b[8].is_ascii_digit()
        && b[9].is_ascii_digit()
}

/// js-yaml's date-time timestamp regex, whole-string:
/// `[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)`
/// `[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?`
/// `(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?`.
/// Hand-rolled (no regex dependency); each field width mirrors the pattern
/// exactly so it neither under- nor over-matches js-yaml.
fn is_date_time(b: &[u8]) -> bool {
    let n = b.len();
    let mut i = 0usize;
    // 4-digit year, then '-'
    if count_digits(b, i, 4) != 4 {
        return false;
    }
    i += 4;
    if b.get(i) != Some(&b'-') {
        return false;
    }
    i += 1;
    // 1-2 digit month, then '-'
    let m = count_digits(b, i, 2);
    if m == 0 {
        return false;
    }
    i += m;
    if b.get(i) != Some(&b'-') {
        return false;
    }
    i += 1;
    // 1-2 digit day
    let d = count_digits(b, i, 2);
    if d == 0 {
        return false;
    }
    i += d;
    // separator: a single T/t, or one-or-more space/tab
    match b.get(i) {
        Some(&c) if c == b'T' || c == b't' => i += 1,
        Some(&c) if c == b' ' || c == b'\t' => {
            i += 1;
            while matches!(b.get(i), Some(&c) if c == b' ' || c == b'\t') {
                i += 1;
            }
        }
        _ => return false,
    }
    // 1-2 digit hour, ':', 2-digit minute, ':', 2-digit second
    let h = count_digits(b, i, 2);
    if h == 0 {
        return false;
    }
    i += h;
    if b.get(i) != Some(&b':') {
        return false;
    }
    i += 1;
    if count_digits(b, i, 2) != 2 {
        return false;
    }
    i += 2;
    if b.get(i) != Some(&b':') {
        return false;
    }
    i += 1;
    if count_digits(b, i, 2) != 2 {
        return false;
    }
    i += 2;
    // optional fraction: '.' then zero-or-more digits
    if b.get(i) == Some(&b'.') {
        i += 1;
        i += count_digits(b, i, usize::MAX - i);
    }
    // optional timezone group `[ \t]*(Z|[-+][0-9][0-9]?(:[0-9][0-9])?)`, whole
    // group optional — so trailing whitespace with no Z/offset must NOT be
    // consumed (it would then fail the end anchor). Snapshot before the
    // whitespace and restore if no zone follows.
    let before_ws = i;
    while matches!(b.get(i), Some(&c) if c == b' ' || c == b'\t') {
        i += 1;
    }
    match b.get(i) {
        Some(&b'Z') => i += 1,
        Some(&c) if c == b'+' || c == b'-' => {
            i += 1;
            let oh = count_digits(b, i, 2);
            if oh == 0 {
                return false;
            }
            i += oh;
            if b.get(i) == Some(&b':') {
                i += 1;
                if count_digits(b, i, 2) != 2 {
                    return false;
                }
                i += 2;
            }
        }
        _ => i = before_ws,
    }
    i == n
}

/// Whether `s` would be implicitly resolved to a timestamp by js-yaml's DEFAULT
/// schema (the one implicit type serde_yaml_ng — YAML 1.2 core — does NOT guard,
/// so serde emits such a string BARE and Obsidian then reads it as a Date).
/// Matches js-yaml's two timestamp regexes on the WHOLE string; a string that
/// merely contains a date substring is not a timestamp.
fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    is_date_only(b) || is_date_time(b)
}

/// A timestamp value's stand-in in the emitted YAML: the same collision-proof
/// private-use sentinel scheme `sentinel` uses, which serde emits as a bare
/// plain scalar (verified), so it can be swapped for the force-quoted form
/// after serialization. Distinct `ts` body from the placeholder sentinels
/// (already resolved away by this point) for clarity.
fn ts_sentinel(i: usize) -> String {
    format!("\u{E000}ts{i}\u{E000}")
}

/// Replace every timestamp-shaped string in a VALUE position (mapping values,
/// sequence items, and their nested descendants — never keys) with a unique
/// sentinel, recording the original. The recorded originals are re-emitted
/// force-quoted after serialization so Obsidian keeps them as text, closing the
/// one implicit-type gap serde leaves open. Non-timestamp scalars are untouched,
/// so their bytes stay exactly as serde emits them.
/// Pull every timestamp-shaped string VALUE out into `out`, leaving a
/// `ts_sentinel(i)` marker in its place so the post-serialization pass can
/// re-emit it double-quoted (serde emits it bare, which Obsidian's js-yaml
/// resolves to a Date). Mapping values, sequence items, and nested structures
/// are covered; mapping KEYS are intentionally excluded — the Obsidian
/// date-property concern (and the Codex finding) is about value positions, and
/// a date used as a property NAME is not a real case.
fn extract_timestamp_values(v: &mut Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if is_timestamp(s) => {
            let idx = out.len();
            out.push(std::mem::take(s));
            *v = Value::String(ts_sentinel(idx));
        }
        Value::Sequence(seq) => seq
            .iter_mut()
            .for_each(|e| extract_timestamp_values(e, out)),
        Value::Mapping(map) => map
            .values_mut()
            .for_each(|val| extract_timestamp_values(val, out)),
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
    // serde_yaml_ng (YAML 1.2 core) has no timestamp type, so an ISO-date-shaped
    // string is emitted BARE — but Obsidian's js-yaml resolves a bare date to a
    // Date, turning a text value into a Date property (and dropping an
    // explicitly-quoted value's quotes). Swap each timestamp-shaped value for a
    // sentinel, then re-emit it force-quoted after serialization so it stays
    // text. serde already quotes every other implicit type (int/float/bool/null);
    // timestamp is the one remaining gap.
    let mut root = Value::Mapping(kept);
    let mut ts_values: Vec<String> = Vec::new();
    extract_timestamp_values(&mut root, &mut ts_values);
    let mut emitted = match serde_yaml_ng::to_string(&root) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("extra frontmatter dropped: emit failed ({e})");
            return String::new();
        }
    };
    for (i, val) in ts_values.iter().enumerate() {
        emitted = emitted.replace(&ts_sentinel(i), &yaml_quote(val));
    }
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
            // `when` is force-quoted: a bare ISO date would be a Date in Obsidian.
            "name: Buy milk\nwhen: \"2026-07-18\"\n"
        );
    }

    #[test]
    fn render_quotes_a_date_only_value_so_obsidian_keeps_it_text() {
        // serde (YAML 1.2 core) emits a bare `2026-07-18`, which Obsidian's
        // js-yaml resolves to a Date; force-quote it so it stays text.
        let out = render_extra_frontmatter("label: {{t}}", &[("t", "2026-07-18")], &[]);
        assert_eq!(out, "label: \"2026-07-18\"\n");
        assert!(
            !out.contains("label: 2026-07-18"),
            "must not be bare: {out}"
        );
    }

    #[test]
    fn render_quotes_a_full_datetime_value() {
        assert_eq!(
            render_extra_frontmatter("when: {{t}}", &[("t", "2026-07-18T12:00:00Z")], &[]),
            "when: \"2026-07-18T12:00:00Z\"\n"
        );
    }

    #[test]
    fn render_does_not_over_quote_non_timestamp_strings() {
        // A plain word stays bare exactly as serde emits it.
        assert_eq!(
            render_extra_frontmatter("project: {{t}}", &[("t", "Alpha")], &[]),
            "project: Alpha\n"
        );
        // A string that merely CONTAINS a date substring is not a pure
        // timestamp (whole-string match), so it stays unchanged/bare.
        assert_eq!(
            render_extra_frontmatter("ref: {{t}}", &[("t", "/x/a.docx (docx, 2026-07-10)")], &[]),
            "ref: /x/a.docx (docx, 2026-07-10)\n"
        );
        // Single-digit fields don't match js-yaml's strict date-only 4-2-2 regex.
        assert_eq!(
            render_extra_frontmatter("d: {{t}}", &[("t", "2026-7-8")], &[]),
            "d: 2026-7-8\n"
        );
    }

    #[test]
    fn render_quotes_a_single_digit_datetime_value() {
        // js-yaml's date-TIME resolver allows single-digit month/day/hour
        // (`[0-9]{1,2}`), unlike the strict 4-2-2 date-only form — so a
        // single-digit datetime IS a timestamp and must be force-quoted, even
        // though the bare single-digit DATE above is not. Locks the delicate
        // `{1,2}`-field branch of the hand-ported matcher.
        assert_eq!(
            render_extra_frontmatter("d: {{t}}", &[("t", "2026-7-8 12:00:00")], &[]),
            "d: \"2026-7-8 12:00:00\"\n"
        );
    }

    #[test]
    fn render_quotes_timestamps_in_nested_and_sequence_positions() {
        // Nested mapping value.
        assert_eq!(
            render_extra_frontmatter("meta:\n  when: {{d}}", &[("d", "2026-07-18")], &[]),
            "meta:\n  when: \"2026-07-18\"\n"
        );
        // Block-sequence items, date-only and full datetime.
        assert_eq!(
            render_extra_frontmatter(
                "dates: [{{a}}, {{b}}]",
                &[("a", "2026-07-18"), ("b", "2026-01-01T00:00:00Z")],
                &[]
            ),
            "dates:\n- \"2026-07-18\"\n- \"2026-01-01T00:00:00Z\"\n"
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
