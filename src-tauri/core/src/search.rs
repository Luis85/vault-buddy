//! Read-only cross-vault search backing the panel's Search view. Walks every
//! registered vault on demand (no index, no background state) with the same
//! reparse-safe discipline as the tasks scan, matching case-insensitive
//! substrings against note stems + note text and attachment filenames. Never
//! writes; opening a hit is delegated to Obsidian via `obsidian://`. See
//! docs/superpowers/specs/2026-07-09-vault-search-design.md.

/// Max chars of a content snippet. One line of context is enough to decide
/// whether a hit is the right note; the panel is 360px wide.
pub const SNIPPET_CHARS: usize = 120;

/// A ~SNIPPET_CHARS-char window of `line` around its first case-insensitive
/// occurrence of `query_lower` (already lowercased by the caller), with `…`
/// marking trimmed ends. `None` when the line doesn't contain the query.
///
/// Char-boundary safe by construction: the window is cut from a `Vec<char>`
/// of the ORIGINAL line, never by byte-slicing. Centering maps the byte index
/// found in the lowercased line back to a char position, which is only
/// reliable when lowercasing changed neither the byte nor the char length
/// (`ẞ`→`ß`, `İ`→`i̇` do); when it did, the window falls back to the line
/// start rather than risk mis-centering — best-effort placement, never a
/// panic.
// TEMPORARY dead_code allow: consumed by `search_vaults` in the next commit;
// until then only the tests call it and clippy's -D warnings would fail.
#[allow(dead_code)]
pub(crate) fn snippet_from_line(line: &str, query_lower: &str) -> Option<String> {
    let trimmed = line.trim();
    let lower = trimmed.to_lowercase();
    let byte_idx = lower.find(query_lower)?;
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= SNIPPET_CHARS {
        return Some(trimmed.to_string());
    }
    let match_char = if lower.len() == trimmed.len() && lower.chars().count() == chars.len() {
        lower[..byte_idx].chars().count()
    } else {
        0
    };
    // Put the match roughly a third in, so context before AND after survives.
    let start = match_char.saturating_sub(SNIPPET_CHARS / 3);
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let start = end.saturating_sub(SNIPPET_CHARS);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_returns_whole_short_line_trimmed() {
        assert_eq!(
            snippet_from_line("  Project Alpha kickoff  ", "alpha").as_deref(),
            Some("Project Alpha kickoff")
        );
    }

    #[test]
    fn snippet_is_none_without_a_match() {
        assert_eq!(snippet_from_line("nothing here", "alpha"), None);
    }

    #[test]
    fn snippet_matches_case_insensitively_preserving_original_case() {
        let s = snippet_from_line("loud ALPHA text", "alpha").unwrap();
        assert!(s.contains("ALPHA"), "got: {s}");
    }

    #[test]
    fn snippet_windows_a_long_line_around_the_match() {
        let line = format!("{}needle{}", "x".repeat(200), "y".repeat(200));
        let s = snippet_from_line(&line, "needle").unwrap();
        assert!(s.contains("needle"), "got: {s}");
        assert!(s.starts_with('…') && s.ends_with('…'), "got: {s}");
        // 120 window chars + 2 ellipses
        assert!(
            s.chars().count() <= SNIPPET_CHARS + 2,
            "got len {}",
            s.chars().count()
        );
    }

    #[test]
    fn snippet_never_panics_on_multibyte_text() {
        // Regression guard: a byte-sliced window would panic on a non-char
        // boundary in multi-byte text; the char-vec window must not.
        let line = format!("{}NEEDLE{}", "ä".repeat(150), "ö".repeat(150));
        let s = snippet_from_line(&line, "needle").unwrap();
        assert!(s.contains("NEEDLE"), "got: {s}");
    }

    #[test]
    fn snippet_falls_back_to_line_start_when_lowercasing_shifts_length() {
        // 'İ' lowercases to a two-char sequence, so byte positions in the
        // lowered string can't be mapped back — window anchors at the start.
        let line = format!("İ{}needle{}", "x".repeat(200), "y".repeat(10));
        // Returning Some without panicking is the contract; the window is
        // start-anchored (best-effort), so it may not include the match.
        let s = snippet_from_line(&line, "needle").unwrap();
        assert!(s.starts_with('İ'), "got: {s}");
        assert!(s.ends_with('…'), "got: {s}");
    }
}
