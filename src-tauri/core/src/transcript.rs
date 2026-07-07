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
const MARKER_CANCELLED: &str = "vault-buddy-transcript: cancelled";

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
    pub processing_secs: u64,
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

/// A deliberately-cancelled sidecar. Non-regenerable (like `complete`, unlike
/// `pending`/`failed`), so the startup scan never re-queues it — but a forced
/// re-transcribe overwrites it. Same frontmatter/`yaml_quote` discipline.
pub fn render_cancelled(mp3_file_name: &str) -> String {
    format!(
        "---\n{MARKER_CANCELLED}\ntranscript-of: {}\ncreated-by: Vault Buddy\n---\n\n\
         > [!note] Transcription cancelled\n> Re-transcribe from the Recordings list to run it again.\n",
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
    let mut wrote_any = false;
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        wrote_any = true;
        if meta.timestamps {
            out.push_str(&format!("{} {text}\n\n", format_timestamp(s.start_ms)));
        } else {
            out.push_str(&format!("{text}\n\n"));
        }
    }
    if !wrote_any {
        // Zero segments (or all-empty) is a valid whisper result for silence —
        // a complete transcript with an honest notice, not a blank body.
        out.push_str("_No speech detected._\n\n");
    }
    out.push_str(&render_stats(meta, segments));
    out
}

/// The `## Statistics` footer: metadata that's otherwise hidden in the note's
/// frontmatter embed, plus figures computed from the transcript. Pure — every
/// value comes from `meta`/`segments`, so it's deterministic and unit-tested.
pub fn render_stats(meta: &TranscriptMeta, segments: &[Segment]) -> String {
    let mut words = 0usize;
    let mut segment_count = 0usize;
    for s in segments {
        let t = s.text.trim();
        if t.is_empty() {
            continue;
        }
        segment_count += 1;
        words += t.split_whitespace().count();
    }
    // checked_div returns None on zero duration — the divide-by-zero guard,
    // in the form clippy's manual_checked_ops lint wants.
    let speaking_rate = match (words as u64 * 60).checked_div(meta.duration_secs) {
        Some(wpm) => format!("{wpm} wpm"),
        None => "—".to_string(),
    };
    let language = meta.language.as_deref().unwrap_or("auto");
    format!(
        "## Statistics\n\n\
         | Metric | Value |\n\
         | --- | --- |\n\
         | Duration | {} |\n\
         | Words | {words} |\n\
         | Segments | {segment_count} |\n\
         | Speaking rate | {speaking_rate} |\n\
         | Model | {} |\n\
         | Language | {language} |\n\
         | Processing time | {} |\n\
         | Generated | {} |\n",
        format_duration(meta.duration_secs),
        meta.model_label,
        format_duration(meta.processing_secs),
        meta.generated_at,
    )
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

/// The state of a recording's transcript sidecar, for the Recordings list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptStatus {
    Missing,
    Pending,
    Failed,
    Complete,
    Cancelled,
}

impl TranscriptStatus {
    /// Lowercased wire form for the frontend (`Missing` → "none").
    pub fn as_dto_str(&self) -> &'static str {
        match self {
            TranscriptStatus::Missing => "none",
            TranscriptStatus::Pending => "pending",
            TranscriptStatus::Failed => "failed",
            TranscriptStatus::Complete => "complete",
            TranscriptStatus::Cancelled => "cancelled",
        }
    }
}

/// Classify a recording's sidecar. A non-regenerable file (the `complete`
/// marker, or a user's hand-edit) reads as `Complete` so the re-transcribe
/// confirm fires before it is overwritten. Unreadable → `Missing` (best-effort).
pub fn transcript_status(mp3: &Path) -> TranscriptStatus {
    match std::fs::read_to_string(transcript_path(mp3)) {
        Ok(c) if c.contains(MARKER_PENDING) => TranscriptStatus::Pending,
        Ok(c) if c.contains(MARKER_FAILED) => TranscriptStatus::Failed,
        Ok(c) if c.contains(MARKER_CANCELLED) => TranscriptStatus::Cancelled,
        Ok(_) => TranscriptStatus::Complete,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TranscriptStatus::Missing,
        Err(_) => TranscriptStatus::Missing,
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
    write_sidecar_atomic(transcript_path, content).map(|()| ReplaceOutcome::Written)
}

/// Forced atomic overwrite of a transcript sidecar, skipping the never-clobber
/// guard. ONLY for the explicit `retranscribe` command — the user asked to
/// regenerate this sidecar. Still touches nothing but the sidecar.
pub fn force_write_sidecar(transcript_path: &Path, content: &str) -> std::io::Result<()> {
    write_sidecar_atomic(transcript_path, content)
}

/// The atomic temp + fsync + REPLACING-rename shared by `replace_if_ours` and
/// `force_write_sidecar`. Exclusive-creates a marker-suffixed temp (numbered on
/// collision) so recovery's cleanup can sweep it; mirrors capture_note's writer
/// deliberately so the never-replace audio writer is untouched.
fn write_sidecar_atomic(transcript_path: &Path, content: &str) -> std::io::Result<()> {
    let dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = transcript_path
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
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    drop(f);
    let result = std::fs::rename(&tmp, transcript_path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
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

pub(crate) fn is_digit_dir(name: &str, len: usize) -> bool {
    name.len() == len && name.chars().all(|c| c.is_ascii_digit())
}

pub(crate) fn dir_entries(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, String)> {
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
            processing_secs: 47,
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
    fn transcript_ends_with_a_stats_table() {
        // meta(): model "whisper-small", language "es", duration 3723s.
        let t = render_transcript(
            &meta(),
            &[
                seg(0, 1000, "hola a todos"), // 3 words
                seg(1000, 2000, "que tal"),   // 2 words
                seg(2000, 2500, "   "),       // empty → skipped
            ],
        );
        assert!(t.contains("## Statistics"));
        assert!(t.contains("| Words | 5 |"));
        assert!(t.contains("| Segments | 2 |"));
        assert!(t.contains("| Model | whisper-small |"));
        assert!(t.contains("| Language | es |"));
        assert!(t.contains("| Processing time |"));
    }

    #[test]
    fn stats_speaking_rate_computes_and_guards_zero() {
        let mut m = meta();
        m.duration_secs = 60; // 12 words over one minute → 12 wpm
        let t = render_transcript(&m, &[seg(0, 60_000, "a b c d e f g h i j k l")]);
        assert!(t.contains("| Speaking rate | 12 wpm |"));

        let mut z = meta();
        z.duration_secs = 0; // must not divide by zero
        assert!(render_transcript(&z, &[seg(0, 0, "hi there")]).contains("| Speaking rate | — |"));

        let mut a = meta();
        a.language = None; // None renders "auto" in the footer too
        assert!(render_transcript(&a, &[]).contains("| Language | auto |"));
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
    fn empty_segments_render_a_no_speech_body_not_a_blank_one() {
        // whisper legitimately returns no segments for silence/non-speech; a blank
        // `complete` sidecar looks broken. It must stay `complete` (not a failure)
        // but say so, and still carry the stats table.
        let t = render_transcript(&meta(), &[]);
        assert!(t.contains("vault-buddy-transcript: complete"));
        assert!(t.contains("_No speech detected._"));
        assert!(t.contains("## Statistics"));
        // All-empty-text segments take the same path.
        let t2 = render_transcript(&meta(), &[seg(0, 10, "   "), seg(10, 20, "")]);
        assert!(t2.contains("_No speech detected._"));
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
    fn cancelled_frontmatter_injection_is_escaped() {
        // Mirrors frontmatter_injection_is_escaped above, for render_cancelled:
        // a name needing YAML-quoting must produce a safely-quoted sidecar,
        // not one that breaks out of the frontmatter block.
        let c = render_cancelled("evil\"\ninjected: true.mp3");
        assert!(
            !c.contains("\ninjected:"),
            "newline must not inject a field"
        );
        assert!(c.contains("vault-buddy-transcript: cancelled"));
        assert!(c.contains(r#"transcript-of: "evil\" injected: true.mp3""#));
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

    #[test]
    fn transcript_status_classifies_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Missing);
        std::fs::write(transcript_path(&mp3), render_placeholder("x.mp3")).unwrap();
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Pending);
        std::fs::write(transcript_path(&mp3), render_error("x.mp3", "boom")).unwrap();
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Failed);
        // A finished sidecar (complete marker) — or any non-regenerable content.
        std::fs::write(
            transcript_path(&mp3),
            "---\nvault-buddy-transcript: complete\n---\nhi",
        )
        .unwrap();
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Complete);
        assert_eq!(TranscriptStatus::Missing.as_dto_str(), "none");
        assert_eq!(TranscriptStatus::Complete.as_dto_str(), "complete");
    }

    #[test]
    fn force_write_sidecar_overwrites_a_complete_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2026-07-04 1405 Meeting.transcript.md");
        std::fs::write(&path, "---\nvault-buddy-transcript: complete\n---\nold").unwrap();
        // replace_if_ours refuses (never-clobbers a finished transcript)...
        assert!(matches!(
            replace_if_ours(&path, "new").unwrap(),
            ReplaceOutcome::SkippedForeign
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "---\nvault-buddy-transcript: complete\n---\nold"
        );
        // ...but force does overwrite, and cleans its temp.
        force_write_sidecar(&path, "regenerated").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "regenerated");
        let temps: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(temps.is_empty(), "temp not cleaned: {temps:?}");
    }

    #[test]
    fn cancelled_marker_is_not_regenerable_and_classifies() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-06 0930 Meeting.mp3");
        let c = render_cancelled("2026-07-06 0930 Meeting.mp3");
        assert!(c.contains("vault-buddy-transcript: cancelled"));
        assert!(c.contains(r#"transcript-of: "2026-07-06 0930 Meeting.mp3""#));
        assert!(
            !is_regenerable(&c),
            "cancelled must never be auto-re-queued"
        );
        std::fs::write(transcript_path(&mp3), &c).unwrap();
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Cancelled);
        assert_eq!(TranscriptStatus::Cancelled.as_dto_str(), "cancelled");
        assert!(
            !needs_transcription(&mp3),
            "cancelled sidecar is not work to do"
        );
    }

    #[test]
    fn scan_skips_a_cancelled_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        let mp3 = month.join("2026-07-06 0930 Meeting.mp3");
        std::fs::write(&mp3, b"audio").unwrap();
        std::fs::write(
            transcript_path(&mp3),
            render_cancelled("2026-07-06 0930 Meeting.mp3"),
        )
        .unwrap();
        assert!(
            pending_transcriptions(dir.path()).is_empty(),
            "a cancelled recording must not backfill"
        );
    }
}
