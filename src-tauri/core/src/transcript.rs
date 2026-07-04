//! Transcript sidecar: a `<base>.transcript.md` beside the recording that
//! the meeting note embeds. Written with the same never-clobber/atomic
//! discipline as the audio note. A `vault-buddy-transcript` frontmatter
//! marker (pending/failed/complete) is how the worker tells its own
//! regenerable sidecars from a finished transcript or a user's edits.

use crate::capture_note::{format_duration, write_note_atomic, yaml_quote, NOTE_TMP_SUFFIX};
use crate::capture_paths::is_capture_base;
use std::io::Write;
use std::path::{Path, PathBuf};

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

pub fn transcript_file_name(mp3_file_name: &str) -> String {
    let stem = mp3_file_name.strip_suffix(".mp3").unwrap_or(mp3_file_name);
    format!("{stem}.transcript.md")
}

pub fn transcript_path(mp3: &Path) -> PathBuf {
    let dir = mp3.parent().unwrap_or_else(|| Path::new("."));
    let name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.join(transcript_file_name(&name))
}

/// Missing sidecar, or one of our own not-yet-finished sidecars → work to do.
pub fn needs_transcription(mp3: &Path) -> bool {
    let path = transcript_path(mp3);
    match std::fs::read_to_string(&path) {
        Ok(content) => is_regenerable(&content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        // Unreadable (permissions/AV lock): don't spin on it this pass.
        Err(_) => false,
    }
}

/// Create the "transcribing…" placeholder so the note's embed never shows
/// "file not found". Idempotent: an existing sidecar (placeholder or real)
/// is left untouched — the reserved name means the exclusive-create wins on
/// the common path.
pub fn write_placeholder(mp3: &Path) -> std::io::Result<()> {
    let path = transcript_path(mp3);
    if path.exists() {
        return Ok(());
    }
    let name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match write_note_atomic(&path, &render_placeholder(&name)) {
        Ok(()) => Ok(()),
        // Raced by a concurrent writer — the sidecar exists, which is all we wanted.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

pub enum ReplaceOutcome {
    Written,
    SkippedForeign,
}

/// Atomically replace one of OUR regenerable sidecars (or write a missing
/// one). A finished transcript or a user-edited file is never overwritten.
/// Unlike the audio note, replacing here is intentional — but only ever our
/// own `pending`/`failed` output, verified before the move.
pub fn replace_if_ours(transcript_path: &Path, content: &str) -> std::io::Result<ReplaceOutcome> {
    match std::fs::read_to_string(transcript_path) {
        Ok(existing) if !is_regenerable(&existing) => return Ok(ReplaceOutcome::SkippedForeign),
        Ok(_) => {} // our placeholder/error — safe
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {} // fine, create it
        Err(e) => return Err(e),
    }
    let dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = transcript_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Exclusive-create an owned temp (numbered on collision), carrying the
    // ownership marker so recovery's cleanup can sweep it. Mirrors
    // capture_note::write_note_atomic deliberately — kept separate so the
    // audio-note writer (which must NEVER replace) is not touched.
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
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    drop(f);
    // Replacing rename is correct here: we verified the destination is our
    // own regenerable sidecar (or absent) above.
    let result = std::fs::rename(&tmp, transcript_path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| ReplaceOutcome::Written)
}

/// Capture MP3s under `<root>/YYYY/MM` that still need a transcript (missing
/// or one of our regenerable sidecars). Same layout discipline as recovery:
/// only `YYYY/MM`, only capture-named files, never follows symlinks.
pub fn pending_transcriptions(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (year, yft, yname) in dir_entries(root) {
        if !yft.is_dir() || !is_digit_dir(&yname, 4) {
            continue;
        }
        for (month, mft, mname) in dir_entries(&year) {
            if !mft.is_dir() || !is_digit_dir(&mname, 2) {
                continue;
            }
            for (path, fft, name) in dir_entries(&month) {
                if !fft.is_file() {
                    continue;
                }
                let Some(base) = name.strip_suffix(".mp3") else {
                    continue;
                };
                if !is_capture_base(base) {
                    continue;
                }
                if needs_transcription(&path) {
                    out.push(path);
                }
            }
        }
    }
    out
}

fn is_digit_dir(name: &str, len: usize) -> bool {
    name.len() == len && name.chars().all(|c| c.is_ascii_digit())
}

fn dir_entries(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, String)> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            // file_type() reads the dirent WITHOUT following symlinks — a
            // symlinked dir/junction must never let the scan escape the vault.
            if let Ok(ft) = entry.file_type() {
                let name = entry.file_name().to_string_lossy().into_owned();
                out.push((entry.path(), ft, name));
            }
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

    use std::path::Path;

    #[test]
    fn transcript_name_appends_transcript_before_md() {
        assert_eq!(
            transcript_file_name("2026-07-04 1405 Meeting.mp3"),
            "2026-07-04 1405 Meeting.transcript.md"
        );
    }

    #[test]
    fn transcript_path_sits_beside_the_mp3() {
        let p = transcript_path(Path::new("/v/Meetings/2026/07/b.mp3"));
        assert_eq!(p, Path::new("/v/Meetings/2026/07/b.transcript.md"));
    }

    #[test]
    fn write_placeholder_is_idempotent_and_needs_transcription_tracks_it() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("b.mp3");
        std::fs::write(&mp3, b"fake").unwrap();
        assert!(needs_transcription(&mp3), "no sidecar yet");
        write_placeholder(&mp3).unwrap();
        let side = transcript_path(&mp3);
        assert!(side.exists());
        assert!(needs_transcription(&mp3), "a placeholder still needs work");
        // second call must not error or clobber
        write_placeholder(&mp3).unwrap();
    }

    #[test]
    fn replace_overwrites_our_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("b.mp3");
        std::fs::write(&mp3, b"fake").unwrap();
        write_placeholder(&mp3).unwrap();
        let side = transcript_path(&mp3);
        let real = render_transcript(&meta(), &[seg(0, 1000, "done")]);
        assert!(matches!(
            replace_if_ours(&side, &real).unwrap(),
            ReplaceOutcome::Written
        ));
        let text = std::fs::read_to_string(&side).unwrap();
        assert!(text.contains("vault-buddy-transcript: complete"));
        assert!(!needs_transcription(&mp3), "a complete transcript is done");
    }

    #[test]
    fn replace_never_touches_a_user_owned_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("b.mp3");
        let side = transcript_path(&mp3);
        std::fs::write(&side, "my own hand-written transcript").unwrap();
        assert!(matches!(
            replace_if_ours(&side, "generated").unwrap(),
            ReplaceOutcome::SkippedForeign
        ));
        assert_eq!(
            std::fs::read_to_string(&side).unwrap(),
            "my own hand-written transcript"
        );
    }

    #[test]
    fn replace_writes_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("b.mp3");
        let side = transcript_path(&mp3);
        assert!(matches!(
            replace_if_ours(&side, "generated").unwrap(),
            ReplaceOutcome::Written
        ));
        assert_eq!(std::fs::read_to_string(&side).unwrap(), "generated");
    }

    #[test]
    fn replace_leaves_no_temp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let side = dir.path().join("b.transcript.md");
        replace_if_ours(&side, "generated").unwrap();
        let temps: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(temps.is_empty(), "temp not cleaned: {temps:?}");
    }

    fn month_dir(root: &Path) -> std::path::PathBuf {
        let d = root.join("2026").join("07");
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scan_finds_capture_mp3_without_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        let mp3 = month.join("2026-07-04 1405 Meeting.mp3");
        std::fs::write(&mp3, b"audio").unwrap();
        let pending = pending_transcriptions(dir.path());
        assert_eq!(pending, vec![mp3]);
    }

    #[test]
    fn scan_skips_completed_and_ignores_foreign_and_placeholders_are_pending() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        // completed → skipped
        let done = month.join("2026-07-04 1405 Meeting.mp3");
        std::fs::write(&done, b"audio").unwrap();
        std::fs::write(transcript_path(&done), render_transcript(&meta(), &[])).unwrap();
        // placeholder → still pending
        let pend = month.join("2026-07-04 1406 Meeting.mp3");
        std::fs::write(&pend, b"audio").unwrap();
        write_placeholder(&pend).unwrap();
        // foreign (not a capture base) → ignored
        std::fs::write(month.join("random.mp3"), b"audio").unwrap();

        let pending = pending_transcriptions(dir.path());
        assert_eq!(pending, vec![pend]);
    }

    #[test]
    fn scan_ignores_non_dated_and_root_level_files() {
        let dir = tempfile::tempdir().unwrap();
        // capture-named mp3 directly at the root (not under YYYY/MM)
        std::fs::write(dir.path().join("2026-07-04 1405 Meeting.mp3"), b"a").unwrap();
        let sub = dir.path().join("Project");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("2026-07-04 1405 Meeting.mp3"), b"a").unwrap();
        assert!(pending_transcriptions(dir.path()).is_empty());
    }
}
