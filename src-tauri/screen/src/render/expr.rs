//! ffmpeg filtergraph text: escaping and number formatting (tutorial-editor
//! Task 42). PURE.
//!
//! **Escaping takes two passes, and one is a defect.** ffmpeg reads a
//! filter option value TWICE: the graph parser consumes one level of
//! backslash escaping for `\ ' [ ] , ;` (the characters that separate
//! filters, chains and link labels), then the option parser consumes
//! another for `\ ' :` (the option separator). A value escaped once, with
//! every one of those eight characters behind a single backslash, loses its
//! first `:` to the option split -- measured against ffmpeg 9.0.1, a single
//! pass turned `C:\a'b,c` into the value `C` and a parse error. So
//! `escape_filter_value` applies the option level first and the graph level
//! over it, and the result decoded back byte-for-byte through `drawtext` on
//! the same build.
//!
//! **Numbers are formatted, never `Display`ed raw.** A float's `Display`
//! can print `0.30000000000000004`; `num` rounds to six decimals and trims.
//! Times go through `ffmpeg_args::ms_to_ffmpeg_seconds`' integer arithmetic
//! (that module's trap 2), so a cut never lands a millisecond off.

use crate::ffmpeg_args::ms_to_ffmpeg_seconds;

/// What the option parser unescapes (the inner level).
const OPTION_LEVEL: [char; 3] = ['\\', '\'', ':'];
/// What the graph parser unescapes (the outer level).
const GRAPH_LEVEL: [char; 6] = ['\\', '\'', '[', ']', ',', ';'];

/// `s` escaped for BOTH levels ffmpeg reads a `filter_complex` option value
/// at (see the module doc): safe to paste after `option=` in a graph.
pub fn escape_filter_value(s: &str) -> String {
    escape(&escape(s, &OPTION_LEVEL), &GRAPH_LEVEL)
}

fn escape(s: &str, special: &[char]) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        if special.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Milliseconds as the exact decimal seconds a filter takes.
pub fn seconds(ms: u64) -> String {
    ms_to_ffmpeg_seconds(ms)
}

/// Signed milliseconds as exact decimal seconds (`-0.200`).
pub fn signed_seconds(ms: i64) -> String {
    let magnitude = seconds(ms.unsigned_abs());
    if ms < 0 {
        format!("-{magnitude}")
    } else {
        magnitude
    }
}

/// `+x` / `-x` seconds, to append to an expression that shifts by `ms`.
pub fn plus_seconds(ms: i64) -> String {
    if ms < 0 {
        signed_seconds(ms)
    } else {
        format!("+{}", seconds(ms.unsigned_abs()))
    }
}

/// A plain number: at most six decimals, trailing zeros trimmed, never
/// `-0` -- what a filter option expects for a factor or an anchor.
pub fn num(v: f64) -> String {
    let fixed = format!("{v:.6}");
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".into()
    } else {
        trimmed.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ffmpeg reads a filter option TWICE: the graph parser unescapes
    // `\ ' [ ] , ;`, then the option parser unescapes `\ ' :`. A value
    // escaped only once loses its drive colon to the option split (measured
    // against ffmpeg 9.0.1: a single pass cut `C:...` to `C`), so both
    // levels are applied, innermost first -- the result below decoded back
    // to the original through drawtext on the same build.
    #[test]
    fn escape_filter_value_escapes_colons_and_quotes() {
        assert_eq!(
            escape_filter_value(r"C:\a'b,c;d[e]"),
            r"C\\:\\\\a\\\'b\,c\;d\[e\]"
        );
        assert_eq!(escape_filter_value("plain.ass"), "plain.ass");
    }

    #[test]
    fn seconds_are_exact_decimal_seconds() {
        assert_eq!(seconds(4_500), "4.500");
        assert_eq!(seconds(0), "0.000");
        assert_eq!(signed_seconds(-200), "-0.200");
        assert_eq!(plus_seconds(-200), "-0.200");
        assert_eq!(plus_seconds(1_000), "+1.000");
    }

    #[test]
    fn numbers_are_short_and_never_negative_zero() {
        assert_eq!(num(2.0), "2");
        assert_eq!(num(0.1 + 0.2), "0.3");
        assert_eq!(num(0.25), "0.25");
        assert_eq!(num(-0.000_000_1), "0");
        assert_eq!(num(-1.5), "-1.5");
    }
}
