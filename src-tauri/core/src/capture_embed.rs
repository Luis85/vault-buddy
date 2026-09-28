//! The capture note's embed line: writing it, and retargeting it when the
//! recording is renamed.
//!
//! The two halves live in ONE module on purpose. `render_note` emits the
//! embed and `capture::rename` rewrites it, and the rewrite matches the line
//! the writer produced — so a change to either shape that misses the other
//! makes a rename silently stop following the file, which is a worse failure
//! than the dead link the shape exists to cure (docs/Gaps.md GAP-153).

use crate::obsidian_link::{escape_label, is_wikilink_safe};

/// The one embed line a capture note may carry, for either of its two
/// targets: the audio file (`<base>.mp3`) or the transcript sidecar, whose
/// wikilink target is `<base>.transcript` for the `<base>.transcript.md` on
/// disk.
///
/// A wikilink is used whenever the name can carry one. `capture_paths::
/// sanitize_title` filters what is illegal in a FILE NAME (`/ \ < > : " | ? *`
/// and controls) and deliberately lets `#`, `^`, `[` and `]` through — they
/// are legal in a file name, and mapping them would damage the base a
/// recording is ADDRESSED by (`is_capture_base`, `rename_plan`, the transcript
/// sidecar's derived name) to repair a rendering concern. But inside `[[...]]`
/// Obsidian splits at `#` and resolves a heading in a file that does not
/// exist, so the embed is dead while the `.mp3` beside the note is named
/// correctly — and a wikilink offers no escape for any of them. Only the note
/// is wrong, so only the note changes: those names fall back to
/// `screen_note::embed`'s shape, a percent-encoded markdown embed with a
/// backslash-escaped label, sharing its character set through
/// `crate::obsidian_link`.
///
/// A capture's note and its audio/transcript are always siblings in the same
/// directory, so the destination is the file name alone — no `../` depth to
/// compute. (GAP-153; GAP-149 is the same defect on the screen-capture note.)
pub(crate) fn embed_line(link_name: &str) -> String {
    if is_wikilink_safe(link_name) {
        return format!("![[{link_name}]]");
    }
    // The markdown destination must name the file as it really is on disk: a
    // wikilink target of `<base>.transcript` stands for `<base>.transcript.md`.
    let file_name = match link_name.strip_suffix(TRANSCRIPT_LINK_SUFFIX) {
        Some(_) => format!("{link_name}.md"),
        None => link_name.to_string(),
    };
    // Keep the final extension literal (screen_note::embed's convention): it
    // is what makes the destination read as a file.
    let (stem, ext) = file_name
        .rsplit_once('.')
        .unwrap_or((file_name.as_str(), ""));
    let mut dest = crate::uri::encode(stem);
    if !ext.is_empty() {
        dest.push('.');
        dest.push_str(ext);
    }
    format!("![{}]({dest})", escape_label(link_name))
}

/// What a transcript sidecar's wikilink target ends with — the `.md` is
/// implied. `capture::rename` composes the same `{stem}.transcript` name when
/// it retargets the second embed.
pub(crate) const TRANSCRIPT_LINK_SUFFIX: &str = ".transcript";

/// Rewrite exactly the embed line(s) naming `old_mp3` — in EITHER shape
/// `embed_line` can produce — to point at the new file name. Line-anchored on
/// purpose: an audio note may have been hand-edited between the write and the
/// rename, so prose mentioning the old name (even quoting the embed) stays
/// untouched and only the embed our own `render_note` wrote may change.
pub fn retarget_embed(note: &str, old_mp3: &str, new_mp3: &str) -> String {
    // BOTH shapes, in the same change as the writer (GAP-153). Every note
    // already on disk carries the wikilink form; a note written after the
    // fallback landed — and any second rename of one — carries the markdown
    // form, and a retarget blind to it would silently stop following the file,
    // which is worse than the dead link the fallback cures. `embed_line` is
    // the single authority for the markdown shape, so the two can never drift.
    let old_wikilink = format!("![[{old_mp3}]]");
    let old_markdown = embed_line(old_mp3);
    let new_line = embed_line(new_mp3);
    let mut out = String::with_capacity(note.len());
    for line in note.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        if body == old_wikilink || body == old_markdown {
            out.push_str(&new_line);
            out.push_str(&line[body.len()..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_note::{render_note, NoteMeta};

    fn meta() -> NoteMeta {
        NoteMeta {
            recorded_at: "2026-07-04T14:05:00+02:00".into(),
            duration_secs: 3723,
            vault_name: "Work".into(),
            recording_type: "Meeting".into(),
            paused: None,
            input_devices: vec!["Headset Mic".into()],
            event: None,
            transcribe: false,
            follow_up: false,
            extra_frontmatter: None,
            body_template: None,
        }
    }

    #[test]
    fn retarget_rewrites_only_the_embed_line() {
        let note = "---\nvault: \"W\"\n---\n\nSee old.mp3 in prose.\n![[old.mp3]]\n";
        let out = retarget_embed(note, "old.mp3", "new.mp3");
        assert!(out.contains("![[new.mp3]]"));
        assert!(!out.contains("![[old.mp3]]"));
        assert!(
            out.contains("See old.mp3 in prose."),
            "prose mention untouched: {out}"
        );
    }

    #[test]
    fn retarget_preserves_crlf_line_endings() {
        let note = "a\r\n![[old.mp3]]\r\nb\r\n";
        let out = retarget_embed(note, "old.mp3", "new.mp3");
        assert_eq!(out, "a\r\n![[new.mp3]]\r\nb\r\n");
    }

    #[test]
    fn retarget_without_a_match_returns_the_note_unchanged() {
        let note = "no embed here\n![[other.mp3]]\n";
        assert_eq!(retarget_embed(note, "old.mp3", "new.mp3"), note);
    }

    #[test]
    fn retarget_handles_a_note_without_trailing_newline() {
        let out = retarget_embed("![[old.mp3]]", "old.mp3", "new.mp3");
        assert_eq!(out, "![[new.mp3]]");
    }

    // ---- GAP-153: wikilink metacharacters in a renamed capture's base ----

    /// Resolve every embed line in `note` to the file an Obsidian READER would
    /// open. Deliberately models the reader rather than re-calling the writer,
    /// so these tests cannot pass by agreeing with a broken implementation:
    /// a wikilink's target ends at the first `]]` and is then cut at the first
    /// `#` (heading), `^` (block ref) or `|` (alias) — which is precisely how a
    /// `#` in a renamed base turns a correct file name into a dead embed.
    /// (`[`/`]` are unsafe too, for the termination reason above; every unsafe
    /// fixture here also carries a `#` or `^`, so the cut is what bites.)
    /// A markdown embed resolves to its percent-DECODED destination.
    fn embedded_files(note: &str) -> Vec<String> {
        let mut found = Vec::new();
        for line in note.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(inner) = line
                .strip_prefix("![[")
                .and_then(|r| r.split_once("]]"))
                .map(|(target, _)| target.split(['#', '^', '|']).next().unwrap_or(target))
            {
                // A wikilink names a note without its extension unless the
                // target carries a real one (`.mp3`); the transcript sidecar
                // is `<base>.transcript.md` on disk.
                if inner.ends_with(".transcript") {
                    found.push(format!("{inner}.md"));
                } else {
                    found.push(inner.to_string());
                }
            } else if line.starts_with("![") && line.ends_with(')') {
                let dest = line.rsplit_once("](").expect("markdown embed").1;
                let dest = dest.trim_end_matches(')');
                found.push(
                    percent_encoding::percent_decode_str(dest)
                        .decode_utf8()
                        .expect("valid utf-8 destination")
                        .into_owned(),
                );
            }
        }
        found
    }

    fn transcribing_meta() -> NoteMeta {
        let mut m = meta();
        m.transcribe = true;
        m
    }

    #[test]
    fn a_plain_base_still_renders_the_historical_wikilink_embeds() {
        // REGRESSION (GAP-153): the metacharacter fallback must change NOTHING
        // for an ordinary title. Byte-exact on purpose — a "near enough"
        // rewrite would churn every note already on disk on the next rename.
        let note = render_note(&transcribing_meta(), "2026-07-04 1405 Team sync.mp3");
        assert!(
            note.contains("\n![[2026-07-04 1405 Team sync.mp3]]\n"),
            "{note}"
        );
        assert!(
            note.contains("\n## Transcript\n\n![[2026-07-04 1405 Team sync.transcript]]\n"),
            "{note}"
        );
        assert!(
            !note.contains("]("),
            "no markdown fallback for a safe name: {note}"
        );
    }

    #[test]
    fn a_metacharacter_base_embeds_the_audio_as_an_encoded_markdown_link() {
        // REGRESSION (GAP-153): `sanitize_title` lets `#`, `^`, `[` and `]`
        // through, so `rename_capture` can land a base Obsidian splits at the
        // `#` inside `![[...]]` — a dead embed beside a correctly named .mp3,
        // with no error anywhere.
        let name = "2026-07-04 1405 Sprint #4 retro.mp3";
        let note = render_note(&meta(), name);
        assert!(
            !note.contains("![["),
            "an unsafe name must not be emitted as a wikilink: {note}"
        );
        assert_eq!(embedded_files(&note), vec![name.to_string()], "{note}");
        // The `#` is encoded, so nothing downstream can read it as a heading.
        assert!(note.contains("%23"), "{note}");
    }

    #[test]
    fn a_metacharacter_base_gets_the_same_fallback_for_the_transcript_embed() {
        // REGRESSION (GAP-153): the transcript embed on the next line breaks
        // identically, and fixing only the audio one leaves it dead.
        let name = "2026-07-04 1405 Q3 [final] #2.mp3";
        let note = render_note(&transcribing_meta(), name);
        assert!(note.contains("## Transcript"), "{note}");
        assert_eq!(
            embedded_files(&note),
            vec![
                name.to_string(),
                "2026-07-04 1405 Q3 [final] #2.transcript.md".to_string(),
            ],
            "{note}"
        );
    }

    /// The rename's own retarget sequence (`capture::rename`): the audio embed
    /// first, then the transcript sidecar's on the same note.
    fn rename(note: &str, old_base: &str, new_base: &str) -> String {
        let out = retarget_embed(note, &format!("{old_base}.mp3"), &format!("{new_base}.mp3"));
        retarget_embed(
            &out,
            &format!("{old_base}.transcript"),
            &format!("{new_base}.transcript"),
        )
    }

    #[test]
    fn retarget_follows_the_file_through_two_metacharacter_renames() {
        // REGRESSION (GAP-153, the trap): after the FIRST rename to a
        // metacharacter title the embed is in markdown form, so a retarget
        // that only matches the literal `![[old]]` line silently stops
        // following the file on the SECOND rename — worse than the dead link
        // the writer fix set out to cure.
        let first = "2026-07-04 1405 Meeting";
        let second = "2026-07-04 1405 Sprint #4 retro";
        let third = "2026-07-04 1405 Q3 [final] ^2";

        let note = render_note(&transcribing_meta(), &format!("{first}.mp3"));
        let once = rename(&note, first, second);
        assert_eq!(
            embedded_files(&once),
            vec![format!("{second}.mp3"), format!("{second}.transcript.md"),],
            "after the first rename: {once}"
        );

        let twice = rename(&once, second, third);
        assert_eq!(
            embedded_files(&twice),
            vec![format!("{third}.mp3"), format!("{third}.transcript.md")],
            "after the second rename: {twice}"
        );
        assert!(
            !twice.contains("Sprint"),
            "no trace of the superseded name: {twice}"
        );
    }

    #[test]
    fn retarget_rewrites_a_pre_existing_wikilink_embed_to_either_form() {
        // REGRESSION (GAP-153): every note already on disk carries the
        // wikilink form. Teaching the retarget the markdown shape must not
        // cost it the one it has always rewritten.
        let on_disk =
            "---\nvault: \"W\"\n---\n\n![[old.mp3]]\n\n## Transcript\n\n![[old.transcript]]\n";

        let plain = rename(on_disk, "old", "new");
        assert_eq!(
            embedded_files(&plain),
            vec!["new.mp3".to_string(), "new.transcript.md".to_string()]
        );
        assert!(plain.contains("![[new.mp3]]"), "{plain}");

        let unsafe_name = rename(on_disk, "old", "new #1");
        assert_eq!(
            embedded_files(&unsafe_name),
            vec!["new #1.mp3".to_string(), "new #1.transcript.md".to_string()],
            "{unsafe_name}"
        );
        assert!(!unsafe_name.contains("![["), "{unsafe_name}");
    }

    #[test]
    fn retarget_of_a_markdown_embed_leaves_prose_and_line_endings_alone() {
        // REGRESSION (GAP-153): an audio note may have been hand-edited
        // between the write and the rename, so the retarget stays line-
        // anchored for the markdown shape exactly as it is for the wikilink.
        let note = render_note(&meta(), "2026-07-04 1405 Sprint #4 retro.mp3");
        let embed = note
            .lines()
            .find(|l| l.starts_with("!["))
            .expect("an embed line")
            .to_string();
        let hand_edited = format!("{note}\nI mentioned {embed} in prose.\r\nkeep me\r\n");
        let out = retarget_embed(
            &hand_edited,
            "2026-07-04 1405 Sprint #4 retro.mp3",
            "2026-07-04 1405 Plain.mp3",
        );
        assert!(
            out.contains(&format!("I mentioned {embed} in prose.\r\n")),
            "a prose mention is not an embed line: {out}"
        );
        assert!(out.contains("keep me\r\n"), "{out}");
        assert!(out.contains("![[2026-07-04 1405 Plain.mp3]]"), "{out}");
    }
}
