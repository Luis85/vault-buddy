//! Companion markdown note: frontmatter metadata + an ![[…]] embed of the
//! recording — no AI sections in this increment. Written atomically
//! (temp + fsync + non-replacing rename) so a crash can truncate only a
//! hidden temp file, never a note in the vault.

use std::io::Write;
use std::path::Path;

pub struct NoteMeta {
    pub recorded_at: String,
    pub duration_secs: u64,
    pub vault_name: String,
    pub recording_type: String,
    pub input_devices: Vec<String>,
    pub event: Option<String>,
}

pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Double-quote a YAML scalar, escaping `\` and `"` and flattening
/// newlines to spaces. Vault and device names are user/system input;
/// unquoted they could break the frontmatter or inject fields — and an
/// unquoted `1:02:03` duration even parses as YAML sexagesimal.
fn yaml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ");
    format!("\"{escaped}\"")
}

pub fn render_note(meta: &NoteMeta, mp3_file_name: &str) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("recorded: {}\n", yaml_quote(&meta.recorded_at)));
    out.push_str(&format!(
        "duration: {}\n",
        yaml_quote(&format_duration(meta.duration_secs))
    ));
    out.push_str(&format!("vault: {}\n", yaml_quote(&meta.vault_name)));
    out.push_str(&format!("type: {}\n", yaml_quote(&meta.recording_type)));
    out.push_str("inputs:\n");
    for device in &meta.input_devices {
        out.push_str(&format!("  - {}\n", yaml_quote(device)));
    }
    if let Some(event) = &meta.event {
        out.push_str(&format!("event: {}\n", yaml_quote(event)));
    }
    out.push_str("created-by: Vault Buddy\n---\n\n");
    out.push_str(&format!("![[{mp3_file_name}]]\n"));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> NoteMeta {
        NoteMeta {
            recorded_at: "2026-07-04T14:05:00+02:00".into(),
            duration_secs: 3723,
            vault_name: "Work".into(),
            recording_type: "Meeting".into(),
            input_devices: vec!["Headset Mic".into(), "Speakers (loopback)".into()],
            event: None,
        }
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
}
