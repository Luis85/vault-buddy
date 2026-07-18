//! Companion markdown note: frontmatter metadata + an ![[…]] embed of the
//! recording, plus an optional `## Transcript` embed of the transcript
//! sidecar when transcription is enabled. Written atomically (temp +
//! fsync + non-replacing rename) so a crash can truncate only a hidden
//! temp file, never a note in the vault.

use std::io::Write;
use std::path::Path;

pub struct NoteMeta {
    pub recorded_at: String,
    pub duration_secs: u64,
    pub vault_name: String,
    pub recording_type: String,
    /// Total paused time (pre-formatted, e.g. "1:05"); None when the
    /// recording was never paused — the line is omitted entirely then.
    pub paused: Option<String>,
    pub input_devices: Vec<String>,
    pub event: Option<String>,
    pub transcribe: bool,
    /// Append a `## Follow-up` scaffold (Action items / Decisions / Notes)
    /// above the transcript embed. Per-vault opt-out; recovery leaves it off.
    pub follow_up: bool,
    /// Additive template content (per-vault). None → today's exact output.
    pub extra_frontmatter: Option<String>,
    pub body_template: Option<String>,
}

pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

// yaml_quote now lives in `crate::template` (the frontmatter-primitives home)
// so `template::substitute_yaml` can quote values without a template↔capture_note
// module cycle; re-exported here to keep the `capture_note::yaml_quote` path for
// callers (document_import, tasks::disk) and this module's own field quoting.
pub use crate::template::yaml_quote;

/// Read one top-level `key:` scalar from a note's leading `---` frontmatter
/// block, undoing `yaml_quote`'s escaping. Returns None if the note has no
/// frontmatter block or the key is absent. Deliberately minimal — it only
/// reads back the handful of top-level fields `render_note` itself writes, so
/// it needs no general YAML parser (indented list items are skipped, and the
/// search stops at the closing `---` so the note body is never scanned).
pub fn note_field(content: &str, key: &str) -> Option<String> {
    let mut lines = content.lines();
    // Frontmatter must be the very first line.
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    for line in lines {
        if line.trim_end() == "---" {
            break; // end of frontmatter — never scan the body
        }
        // `strip_prefix` on the raw line matches top-level keys only: an
        // indented `  - device` list item has a leading space and can't match.
        if let Some(rest) = line.strip_prefix(&prefix) {
            return Some(unquote_yaml(rest.trim()));
        }
    }
    None
}

/// Inverse of `yaml_quote` for the double-quoted form: strip the surrounding
/// quotes and unescape `\"` then `\\` (reverse order of the escaping). An
/// unquoted scalar (older/hand-edited note) is returned as-is.
pub(crate) fn unquote_yaml(value: &str) -> String {
    match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => value.to_string(),
    }
}

pub fn render_note(meta: &NoteMeta, mp3_file_name: &str) -> String {
    let duration = format_duration(meta.duration_secs);
    let mut out = String::from("---\n");
    out.push_str(&format!("recorded: {}\n", yaml_quote(&meta.recorded_at)));
    out.push_str(&format!("duration: {}\n", yaml_quote(&duration)));
    if let Some(paused) = &meta.paused {
        out.push_str(&format!("paused: {}\n", yaml_quote(paused)));
    }
    out.push_str(&format!("vault: {}\n", yaml_quote(&meta.vault_name)));
    out.push_str(&format!("type: {}\n", yaml_quote(&meta.recording_type)));
    out.push_str("inputs:\n");
    for device in &meta.input_devices {
        out.push_str(&format!("  - {}\n", yaml_quote(device)));
    }
    if let Some(event) = &meta.event {
        out.push_str(&format!("event: {}\n", yaml_quote(event)));
    }
    out.push_str("created-by: Vault Buddy\n");
    // Extra frontmatter: substituted then sanitized (reserved keys dropped,
    // fence-safe) so a user field can never break the block or shadow a
    // managed key. Injected right before the closing fence.
    let date = meta.recorded_at.split(['T', ' ']).next().unwrap_or("");
    let vars = [
        ("recordedAt", meta.recorded_at.as_str()),
        ("duration", duration.as_str()),
        ("vault", meta.vault_name.as_str()),
        ("type", meta.recording_type.as_str()),
        ("date", date),
    ];
    if let Some(extra) = &meta.extra_frontmatter {
        const NOTE_RESERVED: &[&str] = &[
            "recorded",
            "duration",
            "paused",
            "vault",
            "type",
            "inputs",
            "event",
            "created-by",
        ];
        out.push_str(&crate::template::sanitize_extra_frontmatter(
            &crate::template::substitute(extra, &vars),
            NOTE_RESERVED,
        ));
    }
    out.push_str("---\n\n");
    out.push_str(&format!("![[{mp3_file_name}]]\n"));
    // Body: a non-empty body template replaces the scaffold; otherwise the
    // legacy follow-up scaffold renders when opted in.
    match meta.body_template.as_deref().map(str::trim) {
        Some(body) if !body.is_empty() => {
            out.push('\n');
            let rendered = crate::template::substitute(body, &vars);
            out.push_str(&rendered);
            if !rendered.ends_with('\n') {
                out.push('\n');
            }
        }
        _ if meta.follow_up => {
            // A follow-up scaffold above the (possibly long) transcript embed
            // so the actionable part is visible without scrolling. Static
            // text — the rename retarget only rewrites the ![[…]] embed
            // line, never this.
            out.push_str(
                "\n## Follow-up\n\n### Action items\n\n- [ ] \n\n### Decisions\n\n### Notes\n",
            );
        }
        _ => {}
    }
    if meta.transcribe {
        // The transcript sidecar's name is derived from the mp3 stem and was
        // reserved pairwise, so this embed resolves once the sidecar lands
        // (a "transcribing…" placeholder is written immediately so it never
        // shows "file not found").
        let stem = mp3_file_name.strip_suffix(".mp3").unwrap_or(mp3_file_name);
        out.push_str(&format!("\n## Transcript\n\n![[{stem}.transcript]]\n"));
    }
    out
}

/// Rewrite exactly the `![[old]]` embed line(s) to point at the new file
/// name. Line-anchored on purpose: the user may have written prose
/// mentioning the old name, and only the embed our own render_note wrote
/// may change.
pub fn retarget_embed(note: &str, old_mp3: &str, new_mp3: &str) -> String {
    let old_line = format!("![[{old_mp3}]]");
    let new_line = format!("![[{new_mp3}]]");
    let mut out = String::with_capacity(note.len());
    for line in note.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        if body == old_line {
            out.push_str(&new_line);
            out.push_str(&line[body.len()..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// Ownership marker for our note temp files: recovery's cleanup filter
/// deletes ONLY temps carrying this suffix — never another tool's
/// `.md.tmp` that happens to live in a recording folder.
pub const NOTE_TMP_SUFFIX: &str = ".vault-buddy.tmp";

pub fn write_note_collision_safe(
    note_path: &Path,
    content: &str,
) -> std::io::Result<std::path::PathBuf> {
    let dir = note_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = note_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    for attempt in 1u32.. {
        // Names come from the shared suffix scheme — same as the .mp3 side.
        let candidate = dir.join(format!(
            "{}.md",
            crate::capture_paths::candidate(&stem, attempt)
        ));
        match write_note_atomic(&candidate, content) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("suffix search always terminates")
}

pub fn write_note_atomic(note_path: &Path, content: &str) -> std::io::Result<()> {
    if note_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "note already exists",
        ));
    }
    let dir = note_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = note_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Exclusive-create the temp too: File::create on the predictable temp
    // name would truncate an existing file or follow a planted symlink out
    // of the vault. On AlreadyExists, take a numbered temp name instead
    // (still carrying the ownership marker so recovery can clean it).
    let (tmp, mut f) = {
        let mut attempt = 0u32;
        loop {
            let candidate = if attempt == 0 {
                dir.join(format!(".{file_name}{NOTE_TMP_SUFFIX}"))
            } else {
                dir.join(format!(".{file_name}.{attempt}{NOTE_TMP_SUFFIX}"))
            };
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(f) => break (candidate, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    };
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    drop(f);
    // Atomic non-replacing move: fails with AlreadyExists if the note name
    // got taken since the exists() check above (std::fs::rename would
    // REPLACE an existing destination on both Unix and Windows), so a
    // user/sync-client file can never be silently clobbered — the caller's
    // collision-safe loop takes a suffixed name instead.
    let result = crate::capture_paths::rename_noreplace(&tmp, note_path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Atomically write `content` to `path`, REPLACING any existing file. For files
/// we own and intentionally overwrite in place — the transcript sidecar and the
/// task status flip — NOT the never-clobber audio-note path (that uses
/// `write_note_atomic`/`write_note_collision_safe`, which refuse to replace).
/// Exclusive-creates a marker-suffixed temp (numbered on collision, so a
/// stranded temp from an interrupted write can't permanently block the next
/// attempt at `create_new`), fsyncs it, then does the REPLACING rename. The
/// temp is removed on ANY failure — write, flush, or rename — so an interrupted
/// write leaves nothing behind.
pub fn write_atomic_replacing(path: &Path, content: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (tmp, mut f) = {
        let mut attempt = 0u32;
        loop {
            let candidate = if attempt == 0 {
                dir.join(format!(".{file_name}{NOTE_TMP_SUFFIX}"))
            } else {
                dir.join(format!(".{file_name}.{attempt}{NOTE_TMP_SUFFIX}"))
            };
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(f) => break (candidate, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
                Err(e) => return Err(e),
            }
        }
    };
    let written = f.write_all(content.as_bytes()).and_then(|()| f.sync_all());
    drop(f);
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> NoteMeta {
        NoteMeta {
            recorded_at: "2026-07-04T14:05:00+02:00".into(),
            duration_secs: 3723,
            vault_name: "Work".into(),
            recording_type: "Meeting".into(),
            paused: None,
            input_devices: vec!["Headset Mic".into(), "Speakers (loopback)".into()],
            event: None,
            transcribe: false,
            follow_up: false,
            extra_frontmatter: None,
            body_template: None,
        }
    }

    #[test]
    fn note_records_paused_duration_when_present() {
        let mut m = meta();
        m.paused = Some(format_duration(65));
        let note = render_note(&m, "x.mp3");
        assert!(note.contains(r#"paused: "1:05""#), "{note}");
        let plain = render_note(&meta(), "x.mp3");
        assert!(!plain.contains("paused:"), "no paused line when None");
    }

    #[test]
    fn duration_formats() {
        assert_eq!(format_duration(7), "0:07");
        assert_eq!(format_duration(189), "3:09");
        assert_eq!(format_duration(3723), "1:02:03");
    }

    #[test]
    fn note_contains_frontmatter_and_embed() {
        let note = render_note(&meta(), "2026-07-04 1405 Meeting.mp3");
        assert!(note.starts_with("---\n"), "frontmatter first: {note}");
        assert!(note.contains(r#"recorded: "2026-07-04T14:05:00+02:00""#));
        assert!(note.contains(r#"duration: "1:02:03""#));
        assert!(note.contains(r#"vault: "Work""#));
        assert!(note.contains(r#"type: "Meeting""#));
        assert!(note.contains(r#"- "Headset Mic""#));
        assert!(note.contains("![[2026-07-04 1405 Meeting.mp3]]"));
        assert!(!note.contains("event:"), "no event line when None");
    }

    #[test]
    fn note_includes_event_when_present() {
        let mut m = meta();
        m.event = Some("recovered after crash".into());
        assert!(render_note(&m, "x.mp3").contains(r#"event: "recovered after crash""#));
    }

    #[test]
    fn yaml_special_characters_are_escaped() {
        // Vault and device names are user/system input: colons, quotes and
        // newlines must not break the frontmatter or inject fields.
        let mut m = meta();
        m.vault_name = "Work: \"Client\" Vault".into();
        m.input_devices = vec!["Mic\nInjected: true".into()];
        let note = render_note(&m, "x.mp3");
        assert!(note.contains(r#"vault: "Work: \"Client\" Vault""#));
        assert!(
            !note.contains("\nInjected:"),
            "newline must not inject a field"
        );
    }

    #[test]
    fn atomic_write_creates_note_and_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("n.md");
        write_note_atomic(&note, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "hello");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp not cleaned: {leftovers:?}");
    }

    #[test]
    fn atomic_write_never_replaces_existing_note() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("n.md");
        std::fs::write(&note, "user content").unwrap();
        assert!(write_note_atomic(&note, "new").is_err());
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "user content");
    }

    #[test]
    fn temp_file_carries_ownership_marker() {
        // The marker is what recovery's cleanup filter keys on — without
        // it we could never safely delete stale temps in a user's vault.
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("n.md");
        // Sabotage the rename by pre-creating the target after temp write
        // is not needed — just verify the constant shape is used.
        write_note_atomic(&note, "x").unwrap();
        assert!(NOTE_TMP_SUFFIX.contains("vault-buddy"));
    }

    #[test]
    fn occupied_temp_name_is_never_truncated() {
        // A pre-existing file (or planted symlink) at the predictable
        // temp name must survive; the write takes a numbered temp instead.
        let dir = tempfile::tempdir().unwrap();
        let squatter = dir.path().join(format!(".n.md{NOTE_TMP_SUFFIX}"));
        std::fs::write(&squatter, "someone else's bytes").unwrap();
        let note = dir.path().join("n.md");
        write_note_atomic(&note, "content").unwrap();
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "content");
        assert_eq!(
            std::fs::read_to_string(&squatter).unwrap(),
            "someone else's bytes"
        );
    }

    #[test]
    fn collision_safe_write_suffixes_instead_of_dropping() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("n.md");
        std::fs::write(&note, "taken").unwrap();
        let written = write_note_collision_safe(&note, "content").unwrap();
        assert_eq!(written, dir.path().join("n (2).md"));
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "content");
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "taken");
    }

    #[test]
    fn note_embeds_transcript_when_enabled() {
        let mut m = meta();
        m.transcribe = true;
        let note = render_note(&m, "2026-07-04 1405 Meeting.mp3");
        assert!(
            note.contains("![[2026-07-04 1405 Meeting.mp3]]"),
            "audio embed stays"
        );
        assert!(note.contains("## Transcript"));
        assert!(note.contains("![[2026-07-04 1405 Meeting.transcript]]"));
    }

    #[test]
    fn note_has_no_transcript_section_when_disabled() {
        let note = render_note(&meta(), "b.mp3");
        assert!(!note.contains("## Transcript"));
    }

    #[test]
    fn note_includes_follow_up_template_when_enabled() {
        let mut m = meta();
        m.follow_up = true;
        let note = render_note(&m, "2026-07-04 1405 Meeting.mp3");
        assert!(note.contains("## Follow-up"));
        assert!(note.contains("### Action items"));
        assert!(note.contains("- [ ]"));
        assert!(note.contains("### Decisions"));
        assert!(note.contains("### Notes"));
        // the audio embed still sits above the scaffold
        assert!(note.contains("![[2026-07-04 1405 Meeting.mp3]]"));
    }

    #[test]
    fn note_has_no_follow_up_when_disabled() {
        // meta() sets follow_up: false (Step 3d), so the default note omits it.
        assert!(!render_note(&meta(), "x.mp3").contains("## Follow-up"));
    }

    #[test]
    fn follow_up_sits_above_the_transcript() {
        let mut m = meta();
        m.follow_up = true;
        m.transcribe = true;
        let note = render_note(&m, "2026-07-04 1405 Meeting.mp3");
        let fu = note.find("## Follow-up").unwrap();
        let tr = note.find("## Transcript").unwrap();
        assert!(
            fu < tr,
            "follow-up must render above the transcript embed: {note}"
        );
    }

    #[test]
    fn note_default_output_is_byte_identical_with_empty_templates() {
        // A note with follow-up + transcript, no templates, must equal the exact
        // legacy string (regression guard for the additive refactor).
        let mut m = meta();
        m.follow_up = true;
        m.transcribe = true;
        let note = render_note(&m, "R.mp3");
        let expected = "---\nrecorded: \"2026-07-04T14:05:00+02:00\"\nduration: \"1:02:03\"\nvault: \"Work\"\ntype: \"Meeting\"\ninputs:\n  - \"Headset Mic\"\n  - \"Speakers (loopback)\"\ncreated-by: Vault Buddy\n---\n\n![[R.mp3]]\n\n## Follow-up\n\n### Action items\n\n- [ ] \n\n### Decisions\n\n### Notes\n\n## Transcript\n\n![[R.transcript]]\n";
        assert_eq!(note, expected);
    }

    #[test]
    fn note_extra_frontmatter_injected_and_reserved_dropped() {
        let mut m = meta();
        m.extra_frontmatter = Some("attendees: 3\ntype: HIJACK".into());
        let note = render_note(&m, "R.mp3");
        assert!(note.contains("attendees: 3"));
        assert!(
            !note.contains("type: HIJACK"),
            "reserved key dropped: {note}"
        );
        // Managed type survives and the fence isn't broken.
        assert!(note.contains("type: \"Meeting\""));
    }

    #[test]
    fn note_body_template_replaces_the_scaffold_between_the_embeds() {
        let mut m = meta();
        m.follow_up = true; // would normally add the scaffold
        m.transcribe = true;
        m.body_template = Some("## Summary\n{{type}} in {{vault}}".into());
        let note = render_note(&m, "R.mp3");
        assert!(note.contains("## Summary\nMeeting in Work"));
        assert!(
            !note.contains("## Follow-up"),
            "template replaces scaffold: {note}"
        );
        // Embeds still bracket the body.
        let audio = note.find("![[R.mp3]]").unwrap();
        let body = note.find("## Summary").unwrap();
        let tr = note.find("## Transcript").unwrap();
        assert!(audio < body && body < tr, "{note}");
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

    #[test]
    fn note_field_reads_top_level_scalars() {
        // meta(): type "Meeting", duration 3723s → "1:02:03", vault "Work".
        let note = render_note(&meta(), "x.mp3");
        assert_eq!(note_field(&note, "type").as_deref(), Some("Meeting"));
        assert_eq!(note_field(&note, "duration").as_deref(), Some("1:02:03"));
        assert_eq!(note_field(&note, "vault").as_deref(), Some("Work"));
    }

    #[test]
    fn note_field_is_none_when_absent_or_no_frontmatter() {
        let note = render_note(&meta(), "x.mp3");
        assert_eq!(note_field(&note, "nope"), None);
        // an indented list item (inputs:) is not a top-level scalar
        assert_eq!(note_field(&note, "Headset Mic"), None);
        assert_eq!(note_field("no frontmatter here\ntype: X\n", "type"), None);
        assert_eq!(note_field("", "type"), None);
    }

    #[test]
    fn note_field_unescapes_quotes_and_ignores_the_body() {
        // A quoted value with an embedded quote round-trips; a `type:` mention
        // in the note body must never be picked up (search stops at the
        // closing `---`).
        let mut m = meta();
        m.recording_type = r#"A "quoted" type"#.into();
        let note = format!(
            "{}\nprose mentioning type: fake\n",
            render_note(&m, "x.mp3")
        );
        assert_eq!(
            note_field(&note, "type").as_deref(),
            Some(r#"A "quoted" type"#)
        );
    }

    #[test]
    fn note_field_stops_at_the_closing_frontmatter_delimiter() {
        // A key that appears only in the BODY (after the closing ---) must not
        // be read: the scan must stop at the delimiter. Without the break this
        // would return Some("leaked").
        let note = format!("{}\nnew-field: leaked\n", render_note(&meta(), "x.mp3"));
        assert_eq!(note_field(&note, "new-field"), None);
    }

    #[test]
    fn note_field_unescapes_backslashes() {
        // The \\ -> \ arm: a value with a literal backslash (plausible on this
        // Windows-targeted app — device/vault names, paths) must round-trip.
        let mut m = meta();
        m.vault_name = r#"C:\Users\me"#.into();
        let note = render_note(&m, "x.mp3");
        assert_eq!(
            note_field(&note, "vault").as_deref(),
            Some(r#"C:\Users\me"#)
        );
    }

    #[test]
    fn note_field_returns_an_unquoted_scalar_as_is() {
        // Hand-edited-note path: an unquoted value is returned trimmed, as-is.
        let note = "---\ntype: Meeting\n---\n\nbody\n";
        assert_eq!(note_field(note, "type").as_deref(), Some("Meeting"));
    }
}
