//! The one place that knows which characters a wikilink cannot carry, and
//! how to escape a markdown label.
//!
//! Two callers had the same problem for the same reason: a name that this
//! app does not choose — a List FOLDER the user created (`tasks::parent_link`)
//! and a file named from a scraped WINDOW TITLE (`screen_note`) — ends up
//! inside `[[...]]`, where `#`, `|`, `[`, `]` and `^` all change the link's
//! meaning and a wikilink offers no escape for any of them. The answer in
//! both cases is the same: fall back to a percent-encoded markdown link
//! whose label is backslash-escaped.
//!
//! It lives here rather than in either caller because the CHARACTER SET is
//! the part that must not drift: a set that gained a character in one file
//! and not the other would make one surface silently start emitting dead
//! links again, which is exactly the failure this module exists to prevent.

/// Characters that change a wikilink's meaning: `#` starts a heading target,
/// `|` an alias, `[`/`]` can terminate it, `^` a block ref.
pub const WIKILINK_UNSAFE: [char; 5] = ['#', '|', '[', ']', '^'];

/// Can `text` be put inside `[[...]]` and mean what it says?
pub fn is_wikilink_safe(text: &str) -> bool {
    !text.contains(WIKILINK_UNSAFE)
}

/// Backslash-escape the characters that would break a markdown link label.
///
/// YAML quoting (where a caller applies one) protects the surrounding
/// scalar, not the Markdown parsed after YAML decoding.
pub fn escape_label(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_wikilink_metacharacter_is_reported_unsafe() {
        for c in WIKILINK_UNSAFE {
            let name = format!("Demo{c}1");
            assert!(!is_wikilink_safe(&name), "{name} passed as wikilink-safe");
        }
        assert!(is_wikilink_safe("2026-09-20 1432 Figma — Design (v2).mp4"));
    }

    #[test]
    fn a_label_escapes_only_the_markdown_link_metacharacters() {
        assert_eq!(
            escape_label(r#"we [need] this \ now"#),
            r#"we \[need\] this \\ now"#
        );
        // `#`, `|` and `^` are harmless inside a label and are left alone, so
        // the rendered text still reads like the name the user recognises.
        assert_eq!(escape_label("Issue #42 | a^b"), "Issue #42 | a^b");
    }
}
