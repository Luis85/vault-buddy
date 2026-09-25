//! The PURE text half of search: which file names are notes, and how a
//! matching line is cut down to a display snippet. Everything here takes a
//! `&str` and returns an `Option`; nothing here touches a `Path`, the file
//! system or the content cache, which is the seam. `search.rs` keeps the
//! walk, the per-vault class-capped collection, the merge and the hit
//! assembly; this module only answers questions about strings.
//!
//! Split from `search.rs` at 778/800 nonblank lines (the shrink-only Rust
//! cap counts inline tests). The snippet tests moved here with the function
//! they test, unedited. `SNIPPET_CHARS` is re-exported as
//! `search::SNIPPET_CHARS` and `snippet_from_line` as
//! `search::snippet_from_line`, so no path a caller names has changed.

/// Max chars of a content snippet. One line of context is enough to decide
/// whether a hit is the right note; the panel is 360px wide.
pub const SNIPPET_CHARS: usize = 120;

/// Case-insensitive `.md` note check returning the stem. The suffix is 3
/// ASCII bytes, so the byte compare can't split a char boundary and
/// `len - 3` is a valid boundary; `> 3` keeps the stem non-empty (a bare
/// ".md"/".MD" is a dot-file anyway).
pub(super) fn md_stem(name: &str) -> Option<&str> {
    let b = name.as_bytes();
    if b.len() > 3 && b[b.len() - 3..].eq_ignore_ascii_case(b".md") {
        Some(&name[..name.len() - 3])
    } else {
        None
    }
}

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
