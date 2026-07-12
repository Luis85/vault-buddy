//! Read-only enumeration of a vault's past recordings for the in-app
//! Recordings list. Scans the shared `transcript::capture_mp3s` walk — the
//! flat root AND the dated `<root>/YYYY/MM` layout (capture-named `.mp3`s
//! only, no symlink follow — identical discipline to
//! `transcript::pending_transcriptions`) — and pairs each recording with the
//! `type`/`duration` read back from its companion note. Never writes.

use crate::capture_note::note_field;
use crate::transcript::{capture_mp3s, transcript_status, TranscriptStatus};
use std::path::{Path, PathBuf};

/// One recording surfaced in the list. `title`/`recorded_at` come from the
/// capture base name (always parseable — it passed `is_capture_base`);
/// `duration`/`recording_type` are best-effort from the companion note and
/// are None when there's no note or the field is absent (→ "Ungrouped").
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingEntry {
    pub mp3_path: PathBuf,
    pub title: String,
    pub recorded_at: String,
    pub duration: Option<String>,
    pub recording_type: Option<String>,
    /// State of the `<base>.transcript.md` sidecar (drives the row indicator
    /// and the re-transcribe confirm).
    pub transcript_status: TranscriptStatus,
}

/// Chars of the `YYYY-MM-DD HHmm ` prefix (10 date + space + 4 time + space).
const PREFIX_CHARS: usize = 16;

/// Every capture recording under the given roots (flat or dated `YYYY/MM`,
/// see `capture_mp3s`), newest first. Read-only: reads each companion
/// `<base>.md` for type/duration but never writes. Missing/unreadable roots
/// and notes degrade silently.
pub fn list_recordings(roots: &[PathBuf]) -> Vec<RecordingEntry> {
    let mut out = Vec::new();
    for root in roots {
        for (path, base) in capture_mp3s(root) {
            out.push(entry_for(&path, &base));
        }
    }
    // Newest first. Base names begin with a lexically-sortable
    // `YYYY-MM-DD HHmm`, so a reverse compare on recorded_at is chronological;
    // same-minute ties fall back to the path for a stable order.
    out.sort_by(|a, b| {
        b.recorded_at
            .cmp(&a.recorded_at)
            .then_with(|| b.mp3_path.cmp(&a.mp3_path))
    });
    out
}

fn entry_for(mp3_path: &Path, base: &str) -> RecordingEntry {
    let (title, recorded_at) = split_base(base);
    // Companion note is best-effort: read type + duration when it's there.
    let (duration, recording_type) = match std::fs::read_to_string(mp3_path.with_extension("md")) {
        Ok(content) => (
            note_field(&content, "duration"),
            note_field(&content, "type"),
        ),
        Err(_) => (None, None),
    };
    RecordingEntry {
        mp3_path: mp3_path.to_path_buf(),
        title,
        recorded_at,
        duration,
        recording_type,
        transcript_status: transcript_status(mp3_path),
    }
}

/// `"2026-07-04 1405 Standup"` → (title "Standup", recorded_at
/// "2026-07-04 14:05"). The base already passed `is_capture_base`, so the
/// fixed-width prefix is safe to slice; a base with nothing after the prefix
/// (shouldn't happen for our names) falls back to the whole base as the title.
fn split_base(base: &str) -> (String, String) {
    let chars: Vec<char> = base.chars().collect();
    let date: String = chars[0..10].iter().collect();
    let hh: String = chars[11..13].iter().collect();
    let mm: String = chars[13..15].iter().collect();
    let recorded_at = format!("{date} {hh}:{mm}");
    let title: String = chars.iter().skip(PREFIX_CHARS).collect();
    let title = if title.trim().is_empty() {
        base.to_string()
    } else {
        title
    };
    (title, recorded_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_note::{render_note, NoteMeta};

    /// Write `<base>.mp3` under `root/YYYY/MM`, plus a companion note with the
    /// given `type` (and a 65s duration → "1:05") when `note_type` is Some.
    fn write_recording(root: &Path, year: &str, month: &str, base: &str, note_type: Option<&str>) {
        let dir = root.join(year).join(month);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{base}.mp3")), b"id3").unwrap();
        if let Some(t) = note_type {
            let meta = NoteMeta {
                recorded_at: "2026-07-04T14:05:00+02:00".into(),
                duration_secs: 65,
                vault_name: "Work".into(),
                recording_type: t.into(),
                paused: None,
                input_devices: vec![],
                event: None,
                transcribe: false,
                follow_up: false,
            };
            std::fs::write(
                dir.join(format!("{base}.md")),
                render_note(&meta, &format!("{base}.mp3")),
            )
            .unwrap();
        }
    }

    #[test]
    fn lists_a_recording_with_type_and_duration_from_the_note() {
        let root = tempfile::tempdir().unwrap();
        write_recording(
            root.path(),
            "2026",
            "07",
            "2026-07-04 1405 Standup",
            Some("Meeting"),
        );
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Standup");
        assert_eq!(list[0].recorded_at, "2026-07-04 14:05");
        assert_eq!(list[0].duration.as_deref(), Some("1:05"));
        assert_eq!(list[0].recording_type.as_deref(), Some("Meeting"));
    }

    #[test]
    fn type_comes_from_the_note_not_the_folder() {
        // A recording physically under one root whose note says "Voice Note".
        let root = tempfile::tempdir().unwrap();
        write_recording(
            root.path(),
            "2026",
            "07",
            "2026-07-04 1405 Chat",
            Some("Voice Note"),
        );
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list[0].recording_type.as_deref(), Some("Voice Note"));
    }

    #[test]
    fn recording_without_a_note_has_no_type_or_duration() {
        let root = tempfile::tempdir().unwrap();
        write_recording(root.path(), "2026", "07", "2026-07-04 1405 Orphan", None);
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Orphan");
        assert_eq!(list[0].duration, None);
        assert_eq!(list[0].recording_type, None);
    }

    #[test]
    fn sorts_newest_first_and_merges_multiple_roots() {
        let meetings = tempfile::tempdir().unwrap();
        let voice = tempfile::tempdir().unwrap();
        write_recording(
            meetings.path(),
            "2026",
            "07",
            "2026-07-04 0900 Early",
            Some("Meeting"),
        );
        write_recording(
            voice.path(),
            "2026",
            "07",
            "2026-07-04 1700 Late",
            Some("Voice Note"),
        );
        let list = list_recordings(&[meetings.path().to_path_buf(), voice.path().to_path_buf()]);
        let titles: Vec<_> = list.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Late", "Early"]); // 17:00 before 09:00
    }

    #[test]
    fn ignores_non_capture_files_and_in_progress_parts() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("2026").join("07");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("holiday.mp3"), b"x").unwrap(); // not a capture base
        std::fs::write(dir.join(".2026-07-04 1405 Live.mp3.part"), b"x").unwrap(); // in-progress
        write_recording(
            root.path(),
            "2026",
            "07",
            "2026-07-04 1405 Real",
            Some("Meeting"),
        );
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Real");
    }

    #[test]
    fn a_missing_root_yields_no_entries() {
        let list = list_recordings(&[PathBuf::from("/no/such/place")]);
        assert!(list.is_empty());
    }

    #[test]
    fn lists_recordings_from_both_flat_and_dated_layouts() {
        let root = tempfile::tempdir().unwrap();
        // dated
        write_recording(
            root.path(),
            "2026",
            "07",
            "2026-07-04 0900 Dated",
            Some("Meeting"),
        );
        // flat: mp3 directly under the root
        std::fs::write(root.path().join("2026-07-04 1000 Flat.mp3"), b"id3").unwrap();
        let list = list_recordings(&[root.path().to_path_buf()]);
        let titles: Vec<_> = list.iter().map(|e| e.title.as_str()).collect();
        assert!(titles.contains(&"Flat"));
        assert!(titles.contains(&"Dated"));
    }

    #[test]
    fn ignores_foreign_and_part_files_at_the_flat_root() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("holiday.mp3"), b"x").unwrap(); // not a capture base
        std::fs::write(root.path().join(".2026-07-04 1405 Live.mp3.part"), b"x").unwrap(); // in-progress
        std::fs::write(root.path().join("2026-07-04 1405 Real.mp3"), b"id3").unwrap();
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Real");
    }

    #[test]
    fn reports_transcript_status_per_recording() {
        use crate::transcript::{transcript_path, TranscriptStatus};
        let root = tempfile::tempdir().unwrap();
        write_recording(
            root.path(),
            "2026",
            "07",
            "2026-07-04 1405 Done",
            Some("Meeting"),
        );
        write_recording(
            root.path(),
            "2026",
            "07",
            "2026-07-04 1400 Raw",
            Some("Meeting"),
        );
        // Give the newer one a finished sidecar.
        let done_mp3 = root
            .path()
            .join("2026")
            .join("07")
            .join("2026-07-04 1405 Done.mp3");
        std::fs::write(
            transcript_path(&done_mp3),
            "---\nvault-buddy-transcript: complete\n---\nhi",
        )
        .unwrap();
        let list = list_recordings(&[root.path().to_path_buf()]);
        // Newest-first: "1405 Done" then "1400 Raw".
        assert_eq!(list[0].transcript_status, TranscriptStatus::Complete);
        assert_eq!(list[1].transcript_status, TranscriptStatus::Missing);
    }
}
