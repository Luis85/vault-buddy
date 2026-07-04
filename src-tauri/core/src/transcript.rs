//! Transcript sidecar: a `<base>.transcript.md` beside the recording that
//! the meeting note embeds. Written with the same never-clobber/atomic
//! discipline as the audio note. A `vault-buddy-transcript` frontmatter
//! marker (pending/failed/complete) is how the worker tells its own
//! regenerable sidecars from a finished transcript or a user's edits.

use crate::capture_note::{format_duration, yaml_quote};

/// Frontmatter marker line values. `pending`/`failed` sidecars are ours to
/// (re)write; `complete` — and any file without the marker — is left alone.
const MARKER_PENDING: &str = "vault-buddy-transcript: pending";
const MARKER_FAILED: &str = "vault-buddy-transcript: failed";
const MARKER_COMPLETE: &str = "vault-buddy-transcript: complete";

pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub struct TranscriptMeta {
    pub mp3_file_name: String,
    pub model_label: String,
    pub language: Option<String>,
    pub duration_secs: u64,
    pub generated_at: String,
    pub timestamps: bool,
}

/// `[HH:MM:SS]` — meetings can exceed an hour, so always render hours.
pub fn format_timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("[{h:02}:{m:02}:{s:02}]")
}

/// A sidecar we may (re)write: our own not-yet-finished output. A finished
/// (`complete`) transcript or a file a user has taken over must never match.
pub fn is_regenerable(content: &str) -> bool {
    content.contains(MARKER_PENDING) || content.contains(MARKER_FAILED)
}

pub fn render_placeholder(mp3_file_name: &str) -> String {
    format!(
        "---\n{MARKER_PENDING}\ntranscript-of: {}\ncreated-by: Vault Buddy\n---\n\n*Transcribing…*\n",
        yaml_quote(mp3_file_name)
    )
}

pub fn render_error(mp3_file_name: &str, message: &str) -> String {
    // Message is flattened to one line so the callout can't be broken out of.
    let flat = message.replace(['\n', '\r'], " ");
    format!(
        "---\n{MARKER_FAILED}\ntranscript-of: {}\ncreated-by: Vault Buddy\n---\n\n\
         > [!warning] Transcription failed\n> {flat}\n>\n> This will be retried automatically.\n",
        yaml_quote(mp3_file_name)
    )
}

pub fn render_transcript(meta: &TranscriptMeta, segments: &[Segment]) -> String {
    let mut out = String::from("---\n");
    out.push_str(MARKER_COMPLETE);
    out.push('\n');
    out.push_str(&format!(
        "transcript-of: {}\n",
        yaml_quote(&meta.mp3_file_name)
    ));
    out.push_str(&format!("model: {}\n", yaml_quote(&meta.model_label)));
    let lang = meta.language.as_deref().unwrap_or("auto");
    out.push_str(&format!("language: {}\n", yaml_quote(lang)));
    out.push_str(&format!(
        "duration: {}\n",
        yaml_quote(&format_duration(meta.duration_secs))
    ));
    out.push_str(&format!("generated: {}\n", yaml_quote(&meta.generated_at)));
    out.push_str("created-by: Vault Buddy\n---\n\n");
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        if meta.timestamps {
            out.push_str(&format!("{} {text}\n\n", format_timestamp(s.start_ms)));
        } else {
            out.push_str(&format!("{text}\n\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: u64, end_ms: u64, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms,
            text: text.into(),
        }
    }

    fn meta() -> TranscriptMeta {
        TranscriptMeta {
            mp3_file_name: "2026-07-04 1405 Meeting.mp3".into(),
            model_label: "whisper-small".into(),
            language: Some("es".into()),
            duration_secs: 3723,
            generated_at: "2026-07-04T15:10:00+02:00".into(),
            timestamps: true,
        }
    }

    #[test]
    fn timestamp_is_hms() {
        assert_eq!(format_timestamp(0), "[00:00:00]");
        assert_eq!(format_timestamp(12_000), "[00:00:12]");
        assert_eq!(format_timestamp(3_723_000), "[01:02:03]");
    }

    #[test]
    fn placeholder_is_regenerable_and_names_the_audio() {
        let p = render_placeholder("2026-07-04 1405 Meeting.mp3");
        assert!(p.starts_with("---\n"));
        assert!(p.contains("vault-buddy-transcript: pending"));
        assert!(p.contains(r#"transcript-of: "2026-07-04 1405 Meeting.mp3""#));
        assert!(is_regenerable(&p));
    }

    #[test]
    fn error_is_regenerable_and_carries_message() {
        let e = render_error("x.mp3", "model download failed");
        assert!(e.contains("vault-buddy-transcript: failed"));
        assert!(e.contains("model download failed"));
        assert!(is_regenerable(&e));
    }

    #[test]
    fn real_transcript_is_complete_not_regenerable() {
        let t = render_transcript(&meta(), &[seg(0, 12_000, "Hola a todos")]);
        assert!(t.contains("vault-buddy-transcript: complete"));
        assert!(t.contains(r#"model: "whisper-small""#));
        assert!(t.contains(r#"language: "es""#));
        assert!(t.contains(r#"duration: "1:02:03""#));
        assert!(t.contains("[00:00:00] Hola a todos"));
        assert!(
            !is_regenerable(&t),
            "a finished transcript must never be overwritten"
        );
    }

    #[test]
    fn timestamps_can_be_disabled() {
        let mut m = meta();
        m.timestamps = false;
        let t = render_transcript(&m, &[seg(0, 1000, "one"), seg(1000, 2000, "two")]);
        assert!(!t.contains("[00:00:00]"), "no timestamps when disabled");
        assert!(t.contains("one"));
        assert!(t.contains("two"));
    }

    #[test]
    fn language_none_renders_auto() {
        let mut m = meta();
        m.language = None;
        assert!(render_transcript(&m, &[]).contains(r#"language: "auto""#));
    }

    #[test]
    fn frontmatter_injection_is_escaped() {
        // mp3 name is derived from a filesystem name; a crafted name must
        // not break or inject frontmatter.
        let p = render_placeholder("evil\"\ninjected: true.mp3");
        assert!(
            !p.contains("\ninjected:"),
            "newline must not inject a field"
        );
    }

    #[test]
    fn user_edited_sidecar_is_not_regenerable() {
        assert!(!is_regenerable("just some notes the user typed"));
    }
}
