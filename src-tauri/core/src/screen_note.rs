//! The embed line a companion note uses for a video it sits beside.
//!
//! Until Task 59 this module rendered the phase-5 export's whole Screen
//! Capture note; the export is retired, and the tutorial editor's
//! publication renders its own note (`editor::note::render_tutorial_note`)
//! through the one piece that outlived it: `embed`, the
//! wikilink-metacharacter-safe video embed.

use crate::obsidian_link::{escape_label, is_wikilink_safe};

/// The video embed line.
///
/// **Fixed HERE, in the note, and deliberately NOT in
/// `staging_title::sanitize_title` — the two are different trades and the
/// wrong one renames people's files.**
/// A capture's base comes from a scraped window title, and `sanitize_title`
/// maps only the characters that would make the name unsafe AS A PATH
/// (`: \ / ? * " < > |` and controls). `#`, `^`, `[` and `]` are perfectly
/// legal in a Windows or POSIX file name, and they are also the characters
/// that make `![[...]]` mean something else — Obsidian splits at `#` and
/// resolves a heading inside a file that does not exist, so the embed is
/// dead while the `.mp4` sitting beside the note is named correctly. Mapping
/// them in `sanitize_title` would fix the note by damaging the FILE NAME for
/// every user, throwing away title fidelity (`C++ [Debug]`) to repair a
/// rendering concern — and it would change the base a capture is ADDRESSED
/// by, which is the identity `is_capture_base`, the sidecar's own `base`
/// round-trip and `screen_recovery::classify` all key on. Only the note is
/// wrong, so only the note is changed.
///
/// The fallback is `tasks::parent_link`'s, sharing its character set and its
/// label escape: a percent-encoded markdown embed. The note and the video
/// are always siblings in the same directory (`commit_screen_capture` names
/// the note from where the video landed), so the destination is the file
/// name alone — no `../` depth to compute. The extension is left literal,
/// `parent_link`'s own convention for the `.md` it appends.
pub(crate) fn embed(mp4_file_name: &str) -> String {
    if is_wikilink_safe(mp4_file_name) {
        return format!("![[{mp4_file_name}]]\n");
    }
    let (stem, ext) = mp4_file_name
        .rsplit_once('.')
        .unwrap_or((mp4_file_name, ""));
    let mut dest = crate::uri::encode(stem);
    if !ext.is_empty() {
        dest.push('.');
        dest.push_str(ext);
    }
    format!("![{}]({dest})\n", escape_label(mp4_file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_file_name_is_a_wikilink_embed() {
        assert_eq!(
            embed("2026-09-18 1432 Figma walkthrough.mp4"),
            "![[2026-09-18 1432 Figma walkthrough.mp4]]\n"
        );
    }

    // REGRESSION (fix wave): a window title is whatever the recorded
    // application put in its title bar, and `staging_title::sanitize_title`
    // maps only the Windows-reserved set — `#`, `^`, `[` and `]` all survive into
    // the base, and all four are WIKILINK metacharacters. `![[... Issue #42
    // ...]]` makes Obsidian split at the `#` and resolve a heading inside a
    // file that does not exist, so the embed is silently dead beside a video
    // that is named perfectly well. The note is the only thing wrong, so the
    // note is the only thing this fixes.
    #[test]
    fn a_wikilink_metacharacter_in_the_file_name_falls_back_to_a_markdown_embed() {
        let name = "2026-09-20 1432 Issue #42 - GitHub.mp4";
        let out = embed(name);
        assert!(!out.contains("![["), "still a wikilink, got: {out}");
        assert!(
            out.contains(&format!("![{name}](")),
            "the label is not the file name, got: {out}"
        );
        // The `#` is encoded, so nothing downstream can read it as a heading.
        assert!(
            !out.contains("](2026-09-20 1432 Issue #42"),
            "raw destination: {out}"
        );
        // Pinned EXACTLY, not by a `contains("%23")` that a half-applied
        // encoding would also satisfy.
        assert_eq!(
            out.lines().find(|l| l.starts_with("![")),
            Some("![2026-09-20 1432 Issue #42 - GitHub.mp4](2026%2D09%2D20%201432%20Issue%20%2342%20%2D%20GitHub.mp4)")
        );
    }

    // The other three survivors of `sanitize_title`, and the label escape.
    // A one-character fixture per metacharacter, because a fixture carrying
    // all four at once proves only that SOME of them trips the branch.
    #[test]
    fn every_surviving_wikilink_metacharacter_trips_the_markdown_fallback() {
        for c in ['#', '^', '[', ']'] {
            let name = format!("2026-09-20 1432 a{c}b.mp4");
            let out = embed(&name);
            assert!(!out.contains("![["), "{c} stayed a wikilink: {out}");
        }
        // `]` in the LABEL would terminate it early and leave the rest as
        // literal text beside a broken embed.
        let out = embed("2026-09-20 1432 a]b.mp4");
        assert!(
            out.contains(r"![2026-09-20 1432 a\]b.mp4]("),
            "the label is unescaped, got: {out}"
        );
    }
}
