//! The Screen Capture companion note: managed frontmatter, the video embed,
//! and the vault's additive template content around them.
//!
//! Mirrors `capture_note::render_note` exactly in discipline — the managed
//! keys are always emitted and are never user-removable, and the vault's
//! extra frontmatter is filtered against them before injection, so a template
//! can neither break the fence nor shadow a key the writer depends on
//! (spec §9.1).

use crate::capture_note::format_duration;
use crate::template::{render_extra_frontmatter, substitute};
use crate::yaml_scalar::yaml_quote;

/// Frontmatter keys this renderer owns. A user template naming one of these
/// has that key dropped rather than honoured.
pub const RESERVED_SCREEN_NOTE_KEYS: &[&str] = &[
    "type",
    "recorded",
    "duration",
    "source",
    "inputs",
    "resolution",
    "vault",
    "created-by",
];

#[derive(Debug, Clone)]
pub struct ScreenNoteMeta {
    pub recorded_at: String,
    pub duration_secs: u64,
    pub vault_name: String,
    /// The window title or screen label that was captured.
    pub source: String,
    pub input_devices: Vec<String>,
    pub width: u32,
    pub height: u32,
    /// Additive per-vault template content. None → the template-free output.
    pub extra_frontmatter: Option<String>,
    pub body_template: Option<String>,
}

pub fn render_screen_note(meta: &ScreenNoteMeta, mp4_file_name: &str) -> String {
    let duration = format_duration(meta.duration_secs);
    let resolution = format!("{}x{}", meta.width, meta.height);

    let mut out = String::from("---\n");
    out.push_str("type: \"Screen Capture\"\n");
    out.push_str(&format!("recorded: {}\n", yaml_quote(&meta.recorded_at)));
    out.push_str(&format!("duration: {}\n", yaml_quote(&duration)));
    out.push_str(&format!("source: {}\n", yaml_quote(&meta.source)));
    if meta.input_devices.is_empty() {
        // A silent capture is legal (spec §7.2). Flow style, so the key is
        // never left dangling with no list under it.
        out.push_str("inputs: []\n");
    } else {
        out.push_str("inputs:\n");
        for device in &meta.input_devices {
            out.push_str(&format!("  - {}\n", yaml_quote(device)));
        }
    }
    out.push_str(&format!("resolution: {}\n", yaml_quote(&resolution)));
    out.push_str(&format!("vault: {}\n", yaml_quote(&meta.vault_name)));
    out.push_str("created-by: Vault Buddy\n");

    let date = meta.recorded_at.split(['T', ' ']).next().unwrap_or("");
    let vars = [
        ("recordedAt", meta.recorded_at.as_str()),
        ("date", date),
        ("duration", duration.as_str()),
        ("source", meta.source.as_str()),
        ("resolution", resolution.as_str()),
        ("vault", meta.vault_name.as_str()),
    ];

    if let Some(template) = &meta.extra_frontmatter {
        let extra = render_extra_frontmatter(template, &vars, RESERVED_SCREEN_NOTE_KEYS);
        if !extra.is_empty() {
            // render_extra_frontmatter returns either "" or a string whose
            // last emitted mapping line is newline-terminated, so no
            // `if !out.ends_with('\n')` fixup is reachable here.
            out.push_str(&extra);
        }
    }

    out.push_str("---\n\n");
    out.push_str(&format!("![[{mp4_file_name}]]\n"));

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

    fn meta() -> ScreenNoteMeta {
        ScreenNoteMeta {
            recorded_at: "2026-09-18 14:32".into(),
            duration_secs: 197,
            vault_name: "Engineering".into(),
            source: "Figma — Design System".into(),
            input_devices: vec!["Microphone (Yeti)".into()],
            width: 1920,
            height: 1080,
            extra_frontmatter: None,
            body_template: None,
        }
    }

    #[test]
    fn renders_the_managed_frontmatter_and_the_embed() {
        let out = render_screen_note(&meta(), "2026-09-18 1432 Figma walkthrough.mp4");
        assert_eq!(
            out,
            concat!(
                "---\n",
                "type: \"Screen Capture\"\n",
                "recorded: \"2026-09-18 14:32\"\n",
                "duration: \"3:17\"\n",
                "source: \"Figma — Design System\"\n",
                "inputs:\n",
                "  - \"Microphone (Yeti)\"\n",
                "resolution: \"1920x1080\"\n",
                "vault: \"Engineering\"\n",
                "created-by: Vault Buddy\n",
                "---\n",
                "\n",
                "![[2026-09-18 1432 Figma walkthrough.mp4]]\n",
            )
        );
    }

    #[test]
    fn a_silent_capture_emits_an_empty_inputs_list() {
        let mut m = meta();
        m.input_devices = vec![];
        let out = render_screen_note(&m, "x.mp4");
        assert!(
            out.contains("inputs: []\n"),
            "flow-style empty list, never a dangling key"
        );
    }

    // A window title is user data and can contain YAML metacharacters. An
    // unquoted colon or backslash produces a malformed frontmatter block that
    // Obsidian refuses to parse.
    #[test]
    fn a_hostile_source_title_is_quoted_and_escaped() {
        let mut m = meta();
        m.source = "C:\\Users\\a: \"weird\" title".into();
        let out = render_screen_note(&m, "x.mp4");
        assert!(
            out.contains(r#"source: "C:\\Users\\a: \"weird\" title""#),
            "got: {out}"
        );
    }

    #[test]
    fn extra_frontmatter_is_injected_after_created_by() {
        let mut m = meta();
        m.extra_frontmatter = Some("project: acme".into());
        let out = render_screen_note(&m, "x.mp4");
        let created = out
            .find("created-by: Vault Buddy")
            .expect("created-by present");
        let project = out
            .find("project: acme")
            .expect("extra frontmatter present");
        assert!(
            created < project,
            "extra frontmatter follows the managed keys"
        );
        assert!(
            project < out.find("---\n\n![[").expect("closing fence"),
            "and precedes the fence"
        );
    }

    // A user template must never be able to redefine a managed key: the
    // writer and every reader would then disagree about the note's identity.
    #[test]
    fn a_template_cannot_override_a_managed_key() {
        let mut m = meta();
        m.extra_frontmatter = Some("type: Note\nvault: Other\nproject: acme".into());
        let out = render_screen_note(&m, "x.mp4");
        assert!(out.contains("type: \"Screen Capture\""));
        assert!(!out.contains("type: Note"), "reserved key dropped");
        assert!(!out.contains("vault: Other"), "reserved key dropped");
        assert!(out.contains("project: acme"), "non-reserved key survives");
        assert_eq!(out.matches("vault:").count(), 1, "exactly one vault key");
    }

    #[test]
    fn template_placeholders_resolve() {
        let mut m = meta();
        m.extra_frontmatter = Some("title: \"{{source}}\"\nlen: \"{{duration}}\"".into());
        m.body_template = Some("Recorded {{date}} from {{source}} ({{duration}}).".into());
        let out = render_screen_note(&m, "x.mp4");
        assert!(
            out.contains("Figma — Design System"),
            "source placeholder resolved"
        );
        assert!(out.contains("Recorded 2026-09-18 from Figma — Design System (3:17)."));
    }

    #[test]
    fn a_body_template_follows_the_embed() {
        let mut m = meta();
        m.body_template = Some("## Notes".into());
        let out = render_screen_note(&m, "x.mp4");
        let embed = out.find("![[x.mp4]]").expect("embed present");
        assert!(embed < out.find("## Notes").expect("body present"));
        assert!(out.ends_with('\n'), "file ends with a newline");
    }

    // Regression: an empty or unset template must reproduce the template-free
    // output byte-for-byte, so a vault that never opts in sees no change.
    #[test]
    fn blank_templates_are_byte_identical_to_none() {
        let baseline = render_screen_note(&meta(), "x.mp4");
        let mut m = meta();
        m.extra_frontmatter = Some("   ".into());
        m.body_template = Some("".into());
        assert_eq!(render_screen_note(&m, "x.mp4"), baseline);
    }

    #[test]
    fn malformed_template_yaml_degrades_to_nothing_and_never_breaks_the_fence() {
        let mut m = meta();
        m.extra_frontmatter = Some("this: is: not: valid: yaml: [".into());
        let out = render_screen_note(&m, "x.mp4");
        assert_eq!(out.matches("---\n").count(), 2, "exactly two fence lines");
        assert!(out.contains("![[x.mp4]]"));
    }
}
