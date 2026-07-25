//! YAML frontmatter scalar quoting/unquoting primitives shared by every managed-field renderer (the note/task/document frontmatter writers).

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

/// True for a char that must be escaped in the single-line double-quoted YAML
/// scalar `yaml_quote_multiline` produces: any C0/C1 control or DEL
/// (`0x00–0x1F`, `0x7F–0x9F`), a line/paragraph separator (`U+2028`/`U+2029`),
/// or a BMP noncharacter (`U+FFFE`/`U+FFFF`). NEL (`U+0085`) falls in the
/// `0x7F–0x9F` range and so is escaped too — it is a YAML-1.1 line break whose
/// folding could otherwise silently change the value. `\n`/`\t`/`\r` are handled
/// by earlier match arms before this predicate is consulted.
fn multiline_needs_escape(c: char) -> bool {
    let u = c as u32;
    u < 0x20 || (0x7f..=0x9f).contains(&u) || matches!(u, 0x2028 | 0x2029 | 0xfffe | 0xffff)
}

/// Double-quote a scalar PRESERVING newlines as `\n` escapes (unlike
/// `yaml_quote`, which flattens them to spaces for single-line managed
/// fields). Produces a valid one-physical-line YAML double-quoted scalar so a
/// multi-line value (the task `description`) rides the line-oriented surgical
/// writer untouched. Escapes `\` and `"`, encodes newline as `\n` and tab as
/// `\t`, encodes CR as `\r` (so it round-trips exactly, not silently dropped —
/// `yaml_unquote_multiline` decodes `\r` back to CR), and escapes every code point
/// that is not a safe, non-folding member of YAML's `c-printable` set
/// (`multiline_needs_escape`) as `\xXX` (≤ U+00FF) or `\uXXXX` (above it): the
/// other C0 controls, DEL, the C1 controls (incl. NEL), the LS/PS line
/// separators, and the U+FFFE/U+FFFF noncharacters. A bare one of these is
/// forbidden or ambiguous in a double-quoted scalar and would invalidate or
/// silently refold the frontmatter, so the "valid, exact value for any input"
/// claim holds for pasted control/line-break characters too (Codex PR #76).
pub fn yaml_quote_multiline(value: &str) -> String {
    let mut inner = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '\\' => inner.push_str("\\\\"),
            '"' => inner.push_str("\\\""),
            '\n' => inner.push_str("\\n"),
            '\t' => inner.push_str("\\t"),
            '\r' => inner.push_str("\\r"), // exact round-trip: the decoder maps \r back to CR
            c if multiline_needs_escape(c) => {
                let u = c as u32;
                if u <= 0xff {
                    inner.push_str(&format!("\\x{u:02x}"));
                } else {
                    inner.push_str(&format!("\\u{u:04x}"));
                }
            }
            other => inner.push(other),
        }
    }
    format!("\"{inner}\"")
}

/// Read `count` hex digits following a `\x`/`\u`/`\U` escape `marker` and push
/// the decoded Unicode scalar onto `out`. A malformed (non-hex), truncated, or
/// non-scalar (surrogate / out-of-range) escape degrades to verbatim so no
/// bytes are ever lost — the defensive-read posture, matching how an unknown
/// escape is preserved below.
fn push_hex_escape(chars: &mut std::str::Chars<'_>, marker: char, count: usize, out: &mut String) {
    let mut digits = String::with_capacity(count);
    for _ in 0..count {
        match chars.next() {
            Some(d) if d.is_ascii_hexdigit() => digits.push(d),
            trailing => {
                out.push('\\');
                out.push(marker);
                out.push_str(&digits);
                if let Some(t) = trailing {
                    out.push(t);
                }
                return;
            }
        }
    }
    match u32::from_str_radix(&digits, 16)
        .ok()
        .and_then(char::from_u32)
    {
        Some(ch) => out.push(ch),
        None => {
            out.push('\\');
            out.push(marker);
            out.push_str(&digits);
        }
    }
}

/// Inverse of `yaml_quote_multiline`. A double-quoted value is unescaped in a
/// SINGLE left-to-right pass (so `\\` consumes both chars before an `n` could
/// be misread as a newline). An unquoted value (hand-authored / older file) is
/// returned verbatim — the defensive-read posture of the rest of the vault
/// domain. Our encoder emits only a subset of escapes, but a hand-authored
/// description can carry ANY standard YAML double-quoted escape, so the decoder
/// accepts the full set — the single-char escapes plus `\x`/`\u`/`\U` — and
/// reads a value exactly as Obsidian's js-yaml does (Codex PR #76). An escape
/// outside that set keeps its backslash verbatim.
pub fn yaml_unquote_multiline(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some(' ') => out.push(' '),
            Some('0') => out.push('\0'),
            Some('a') => out.push('\u{07}'),
            Some('b') => out.push('\u{08}'),
            Some('f') => out.push('\u{0c}'),
            Some('v') => out.push('\u{0b}'),
            Some('e') => out.push('\u{1b}'),
            Some('N') => out.push('\u{85}'),
            Some('_') => out.push('\u{a0}'),
            Some('L') => out.push('\u{2028}'),
            Some('P') => out.push('\u{2029}'),
            Some('x') => push_hex_escape(&mut chars, 'x', 2, &mut out),
            Some('u') => push_hex_escape(&mut chars, 'u', 4, &mut out),
            Some('U') => push_hex_escape(&mut chars, 'U', 8, &mut out),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_quote_multiline_roundtrips_newlines_quotes_backslashes() {
        let s = "line one\nline \"two\"\twith a \\ backslash";
        let quoted = yaml_quote_multiline(s);
        // Single physical line, double-quoted, no raw newline.
        assert!(quoted.starts_with('"') && quoted.ends_with('"'));
        assert!(!quoted.contains('\n'));
        assert_eq!(yaml_unquote_multiline(&quoted), s);
    }

    #[test]
    fn yaml_quote_multiline_round_trips_a_carriage_return() {
        // CR encodes as `\r` (not dropped), so it round-trips through the decoder —
        // a CR in a duplicated title must survive, not vanish (Codex P2, PR #76).
        let quoted = yaml_quote_multiline("first\rsecond");
        assert_eq!(quoted, "\"first\\rsecond\"");
        assert!(!quoted.contains('\r')); // no raw CR in the scalar
        assert_eq!(yaml_unquote_multiline(&quoted), "first\rsecond");
    }

    #[test]
    fn yaml_unquote_multiline_passes_through_unquoted_and_handles_literal_backslash_n() {
        // Hand-authored unquoted scalar → verbatim.
        assert_eq!(
            yaml_unquote_multiline("hello # not a comment"),
            "hello # not a comment"
        );
        // A user who literally typed backslash-n must NOT get a newline: the
        // single-pass decoder consumes `\\` before it can see `n`.
        let s = "a\\nb"; // the three chars: a, backslash, n, b
        let quoted = yaml_quote_multiline(s);
        assert_eq!(yaml_unquote_multiline(&quoted), s);
    }

    #[test]
    fn yaml_quote_multiline_hex_escapes_control_characters() {
        // Pasted control chars (NUL, backspace, form-feed, ESC) are forbidden
        // bare in a YAML double-quoted scalar; they must be `\xXX`-escaped so the
        // frontmatter stays valid, and round-trip exactly (Codex PR #76).
        let s = "a\u{0}b\u{8}c\u{c}d\u{1b}e";
        let quoted = yaml_quote_multiline(s);
        assert!(
            !quoted.chars().any(|c| (c as u32) < 0x20),
            "no raw control chars survive in the quoted scalar"
        );
        assert!(quoted.contains("\\x00"));
        assert!(quoted.contains("\\x08"));
        assert!(quoted.contains("\\x0c"));
        assert!(quoted.contains("\\x1b"));
        assert_eq!(yaml_unquote_multiline(&quoted), s);
    }

    #[test]
    fn yaml_quote_multiline_escapes_c1_controls() {
        // C1 controls U+0080–U+0084 and U+0086–U+009F are OUTSIDE YAML's
        // c-printable set, so a bare one invalidates the frontmatter just like a
        // C0 control — Obsidian can reject the Task's whole property block. They
        // must be `\xXX`-escaped and round-trip (Codex PR #76). (NEL U+0085 is
        // exercised by the line-break test below.)
        let s = "a\u{80}b\u{84}c\u{86}d\u{9f}e";
        let quoted = yaml_quote_multiline(s);
        assert!(
            !quoted
                .chars()
                .any(|c| matches!(c as u32, 0x80..=0x84 | 0x86..=0x9f)),
            "no raw forbidden C1 control survives in the quoted scalar"
        );
        assert!(quoted.contains("\\x80"));
        assert!(quoted.contains("\\x9f"));
        assert_eq!(yaml_unquote_multiline(&quoted), s);
    }

    #[test]
    fn yaml_quote_multiline_escapes_line_breaks_and_noncharacters() {
        // NEL (U+0085), LS (U+2028), PS (U+2029) are line breaks under YAML 1.1
        // (which Obsidian's js-yaml largely follows) — a bare one can FOLD and
        // silently change the value. U+FFFE / U+FFFF are not c-printable at all
        // and can invalidate the frontmatter. None may reach the raw output; all
        // must round-trip exactly (Codex PR #76).
        let s = "a\u{85}b\u{2028}c\u{2029}d\u{fffe}e\u{ffff}f";
        let quoted = yaml_quote_multiline(s);
        assert!(quoted.contains("\\x85")); // NEL (<= 0xFF → \xXX)
        assert!(quoted.contains("\\u2028")); // LS  (> 0xFF → \uXXXX)
        assert!(quoted.contains("\\u2029")); // PS
        assert!(quoted.contains("\\ufffe")); // BMP noncharacter
        assert!(quoted.contains("\\uffff"));
        assert!(
            !quoted
                .chars()
                .any(|c| matches!(c as u32, 0x85 | 0x2028 | 0x2029 | 0xfffe | 0xffff)),
            "no raw line-break or noncharacter survives in the quoted scalar"
        );
        assert_eq!(yaml_unquote_multiline(&quoted), s);
    }

    #[test]
    fn yaml_unquote_multiline_decodes_the_standard_yaml_escape_set() {
        // A hand-authored double-quoted description can carry any standard YAML
        // escape, not just the subset our encoder emits. We must read it exactly
        // as Obsidian's js-yaml does, or `list_tasks` (and its MCP DTO) would
        // expose the raw backslash sequence and a detail resave would
        // canonicalize the wrong literal (Codex PR #76).
        assert_eq!(yaml_unquote_multiline("\"caf\\u00e9\""), "café");
        assert_eq!(yaml_unquote_multiline("\"\\U0001F600!\""), "😀!");
        // The single-char escapes YAML defines for a double-quoted scalar.
        assert_eq!(
            yaml_unquote_multiline("\"\\0\\a\\b\\f\\v\\e\\/\\N\\_\\L\\P\\ end\""),
            "\u{0}\u{7}\u{8}\u{c}\u{b}\u{1b}/\u{85}\u{a0}\u{2028}\u{2029} end"
        );
        // A malformed/truncated Unicode escape degrades to verbatim (never lose
        // bytes) — the defensive-read posture, matching the `\x` handling.
        assert_eq!(yaml_unquote_multiline("\"\\uZZ\""), "\\uZZ");
        assert_eq!(yaml_unquote_multiline("\"\\u12\""), "\\u12");
    }
}
