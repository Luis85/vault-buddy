//! The Tutorial companion note (Task 48; F-43; ADR R13; A13, A21): the
//! markdown a publication writes beside the video it copies into a vault --
//! the TENTH sanctioned vault write's second file.
//!
//! The retired phase-5 Screen Capture note's discipline exactly: the
//! managed keys are always emitted and never user-removable, the vault's
//! `screen_extra_frontmatter` is filtered against them before injection
//! (`RESERVED_TUTORIAL_NOTE_KEYS`), every string is YAML-quoted, and the
//! embed goes through `screen_note::embed` -- wikilink-metacharacter safe,
//! the one piece of that note Task 59 kept.
//!
//! **The embed names the FINAL file (A21).** `render_tutorial_note` takes
//! the name the video actually landed under -- the caller renders only
//! after the commit, whose ` (N)` retry can move past the name reserved
//! before the copy began. A note rendered from the reservation would embed
//! a file that is not the one beside it.
//!
//! **Chapters are OUTPUT time of the rendered range** (A13):
//! `render_plan::chapters_for` over the product's frozen edit and range,
//! so a chapter at 01:05 of the project reads 00:05 in a render starting at
//! 01:00, and one before the range is not listed. A chapter title is user
//! text written into MARKDOWN (not YAML), so it is escaped as Markdown --
//! `[[c]]` must not become a link and `#d` must not become a tag.

use crate::capture_note::format_duration;
use crate::screen_note::embed;
use crate::template::{render_extra_frontmatter, substitute};
use crate::yaml_scalar::yaml_quote;

/// Frontmatter keys this renderer owns. A vault template naming one of
/// these has that key dropped rather than honoured.
pub const RESERVED_TUTORIAL_NOTE_KEYS: &[&str] = &[
    "type",
    "created-by",
    "recorded",
    "duration",
    "product",
    "revision",
    "range",
    "chapters",
];

/// Everything the note says about one published product.
#[derive(Debug, Clone)]
pub struct TutorialNoteMeta {
    /// When the product was rendered (its ledger `createdAt`).
    pub recorded_at: String,
    /// The product's own duration -- the rendered range's, never the
    /// project's.
    pub duration_ms: u64,
    /// The product's display name.
    pub product: String,
    /// The project revision it was rendered from.
    pub revision: u64,
    /// The rendered output range, `None` for the whole project.
    pub range: Option<(u64, u64)>,
    /// `(output ms relative to the range start, title)`, sorted --
    /// `render_plan::chapters_for`.
    pub chapters: Vec<(u64, String)>,
    /// Additive per-vault template content (the vault's screen-capture
    /// note fields). `None` → the template-free output.
    pub extra_frontmatter: Option<String>,
    pub body_template: Option<String>,
}

/// `MM:SS`, minutes unbounded (a two-hour project reads `120:00`).
fn clock(ms: u64) -> String {
    let secs = ms / 1_000;
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// Backslash-escape every character Markdown (and Obsidian) reads as
/// syntax, so a title is shown as the text it is.
fn escape_markdown(text: &str) -> String {
    let flat = text.replace(['\n', '\r'], " ");
    let mut out = String::with_capacity(flat.len());
    for c in flat.chars() {
        if matches!(
            c,
            '\\' | '`'
                | '*'
                | '_'
                | '['
                | ']'
                | '#'
                | '<'
                | '>'
                | '|'
                | '!'
                | '^'
                | '='
                | '~'
                | '$'
                | '%'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The companion note for `meta`, embedding `mp4_file_name` -- the name
/// the video LANDED under (A21; module doc).
pub fn render_tutorial_note(meta: &TutorialNoteMeta, mp4_file_name: &str) -> String {
    let duration = format_duration(meta.duration_ms / 1_000);
    let revision = meta.revision.to_string();
    let range = meta
        .range
        .map(|(start, end)| format!("{}-{}", clock(start), clock(end)));

    let mut out = String::from("---\n");
    out.push_str("type: Tutorial\n");
    out.push_str("created-by: vault-buddy\n");
    out.push_str(&format!("recorded: {}\n", yaml_quote(&meta.recorded_at)));
    out.push_str(&format!("duration: {}\n", yaml_quote(&duration)));
    out.push_str(&format!("product: {}\n", yaml_quote(&meta.product)));
    out.push_str(&format!("revision: {revision}\n"));
    if let Some(range) = &range {
        out.push_str(&format!("range: {}\n", yaml_quote(range)));
    }

    let date = meta.recorded_at.split(['T', ' ']).next().unwrap_or("");
    let vars = [
        ("recordedAt", meta.recorded_at.as_str()),
        ("date", date),
        ("duration", duration.as_str()),
        ("product", meta.product.as_str()),
        ("revision", revision.as_str()),
        ("range", range.as_deref().unwrap_or("")),
    ];
    if let Some(template) = &meta.extra_frontmatter {
        // `""` or newline-terminated mapping lines.
        out.push_str(&render_extra_frontmatter(
            template,
            &vars,
            RESERVED_TUTORIAL_NOTE_KEYS,
        ));
    }
    out.push_str("---\n\n");
    out.push_str(&embed(mp4_file_name));

    if !meta.chapters.is_empty() {
        out.push_str("\n## Chapters\n\n");
        for (at, title) in &meta.chapters {
            out.push_str(&format!("- {} {}\n", clock(*at), escape_markdown(title)));
        }
    }
    if let Some(template) = &meta.body_template {
        let body = substitute(template, &vars);
        if !body.trim().is_empty() {
            out.push('\n');
            out.push_str(body.trim_end());
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model::{AssetKind, TrackKind};
    use crate::editor::model_cues::Marker;
    use crate::editor::render_plan::chapters_for;
    use crate::editor::test_support::{asset, clip, minimal_project, track};
    use crate::editor::Map;

    fn meta() -> TutorialNoteMeta {
        TutorialNoteMeta {
            recorded_at: "2026-09-24T10:15:00+02:00".into(),
            duration_ms: 197_400,
            product: "Walkthrough v2".into(),
            revision: 14,
            range: None,
            chapters: Vec::new(),
            extra_frontmatter: None,
            body_template: None,
        }
    }

    fn marker(id: &str, source_ms: u64, title: &str) -> Marker {
        Marker {
            id: id.into(),
            clip_id: "c1".into(),
            source_ms,
            title: title.into(),
            extra: Map::new(),
        }
    }

    #[test]
    fn the_managed_frontmatter_the_embed_and_nothing_else() {
        assert_eq!(
            render_tutorial_note(&meta(), "2026-09-24 1015 Walkthrough v2.mp4"),
            concat!(
                "---\n",
                "type: Tutorial\n",
                "created-by: vault-buddy\n",
                "recorded: \"2026-09-24T10:15:00+02:00\"\n",
                "duration: \"3:17\"\n",
                "product: \"Walkthrough v2\"\n",
                "revision: 14\n",
                "---\n",
                "\n",
                "![[2026-09-24 1015 Walkthrough v2.mp4]]\n",
            )
        );
    }

    // A13: a chapter's time in the note is its time in the RENDERED FILE.
    // The clip starts at output 2 000 and plays source from 1 000 at 2x, so
    // the three markers sit at output 4 000, 62 000 and 122 000; the render
    // covers [60 000, 180 000), so the first is outside and the others read
    // 00:02 and 01:02 -- asymmetric, so neither the unshifted project time
    // nor the source time can pass.
    #[test]
    fn note_lists_chapters_at_range_relative_time() {
        let mut project = minimal_project();
        project.tracks = vec![track("v1", TrackKind::Video, false)];
        project.assets = vec![asset("a1", AssetKind::Video, 600_000)];
        let mut c1 = clip("c1", "v1", "a1", 2_000, 1_000, 401_000);
        c1.speed = Some(crate::editor::Num::from(2));
        project.clips = vec![c1];
        project.markers = vec![
            marker("m1", 5_000, "Before the range"),
            marker("m2", 121_000, "Setup"),
            marker("m3", 241_000, "Deploy"),
        ];
        let range = Some((60_000, 180_000));
        let mut m = meta();
        m.range = range;
        m.chapters = chapters_for(&project, range).unwrap();
        let note = render_tutorial_note(&m, "x.mp4");
        assert!(note.contains("range: \"01:00-03:00\"\n"), "{note}");
        assert!(
            note.contains("\n## Chapters\n\n- 00:02 Setup\n- 01:02 Deploy\n"),
            "{note}"
        );
        assert!(!note.contains("Before the range"), "{note}");
    }

    // A title is user text in two places: YAML (the product) and Markdown
    // (a chapter). Each gets its own escaping, and neither can break out:
    // the frontmatter stays one quoted scalar, the chapter line links
    // nothing and tags nothing.
    #[test]
    fn note_escapes_markdown_and_yaml() {
        let hostile = r#"a: "b" [[c]] #d"#;
        let mut m = meta();
        m.product = hostile.into();
        m.chapters = vec![(5_000, hostile.into())];
        let note = render_tutorial_note(&m, "x.mp4");
        assert!(
            note.contains("product: \"a: \\\"b\\\" [[c]] #d\"\n"),
            "{note}"
        );
        assert!(
            note.contains("- 00:05 a: \"b\" \\[\\[c\\]\\] \\#d\n"),
            "{note}"
        );
        let front = note.split("---\n").nth(1).expect("a frontmatter block");
        assert!(
            front.lines().all(|l| l.contains(": ")),
            "one key per line, none broken: {front}"
        );
    }

    // The vault's template adds, never overrides: a template key naming a
    // managed one is dropped, the body lands after the chapters.
    #[test]
    fn the_vault_template_is_additive() {
        let mut m = meta();
        m.chapters = vec![(0, "Intro".into())];
        m.extra_frontmatter =
            Some("type: Hijacked\nproduct: other\ntopic: \"{{product}}\"\n".into());
        m.body_template = Some("Notes for {{product}} r{{revision}}".into());
        let note = render_tutorial_note(&m, "x.mp4");
        assert!(note.contains("type: Tutorial\n"));
        assert!(!note.contains("Hijacked"));
        assert!(!note.contains("product: other"));
        assert!(note.contains("topic: Walkthrough v2\n"), "{note}");
        let chapters = note.find("## Chapters").unwrap();
        let body = note.find("Notes for Walkthrough v2 r14").unwrap();
        assert!(chapters < body, "{note}");
    }

    // The embed is `screen_note::embed`'s: a wikilink metacharacter in the
    // landed name falls back to a percent-encoded markdown embed.
    #[test]
    fn a_wikilink_unsafe_name_embeds_as_a_markdown_link() {
        let note = render_tutorial_note(&meta(), "C# [demo].mp4");
        assert!(
            note.contains("![C# \\[demo\\].mp4](C%23%20%5Bdemo%5D.mp4)\n"),
            "{note}"
        );
    }
}
