//! Tests for `subtitle_commands` (Task 48; F-35): the `AppHandle`-free
//! `export_subtitles_in` against a live session, with the save dialog
//! answered by a fixed path in a tempdir.

use std::path::PathBuf;

use serde_json::json;
use vault_buddy_core::editor::{EditorErrorCode, EditorSession, Project};

use super::*;

const SESSION: &str = "ses-sub";

/// A clip at OUTPUT 700 playing source `[1000, 9000)` at speed 2, and a
/// second clip later on the timeline -- so source time, clip-relative time
/// and timeline time are three different numbers for every cue.
fn project() -> Project {
    serde_json::from_value(json!({
        "schema": "vault-buddy-video-project/3",
        "id": "proj-sub",
        "title": "Deploy: a walkthrough",
        "canvas": { "width": 1280, "height": 720, "fps": 30 },
        "master_gain": 1,
        "assets": [{ "id": "av", "kind": "video", "name": "demo.mp4", "duration_ms": 60000 }],
        "tracks": [{
            "id": "v1", "kind": "video", "name": "Screen", "visible": true,
            "locked": false, "muted": false, "solo": false, "volume": 1
        }],
        "clips": [{
            "id": "c1", "asset_id": "av", "track_id": "v1", "name": "c1",
            "start_ms": 700, "in_ms": 1000, "out_ms": 9000,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1, "speed": 2
        }, {
            "id": "c2", "asset_id": "av", "track_id": "v1", "name": "c2",
            "start_ms": 10000, "in_ms": 20000, "out_ms": 30000,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }],
        "effects": [], "markers": [], "transitions": [],
        "captions": {
            "enabled": false, "burn_in": false, "font_size": 29,
            "position": "bottom", "background": true,
            "cues": [
                { "id": "q2", "clip_id": "c2", "start_ms": 21000, "end_ms": 22500, "text": "Second <b>" },
                { "id": "q1", "clip_id": "c1", "start_ms": 3000, "end_ms": 5000, "text": "First" }
            ]
        },
        "destination": { "vault": "", "folder": "", "dated": false }
    }))
    .expect("the fixture is a valid project document")
}

fn state_with(project: Project) -> EditorState {
    let state = EditorState::default();
    state
        .sessions
        .lock()
        .unwrap()
        .insert(SESSION.into(), EditorSession::resume(SESSION, project, 3));
    state
}

/// Answers the save dialog with `dir/<name>`, remembering the suggestion.
struct Chooser {
    answer: Option<PathBuf>,
    suggested: std::sync::Mutex<Vec<String>>,
}

impl SubtitleTarget for Chooser {
    fn save_target(&self, _format: SubtitleFormat, suggested: &str) -> Option<PathBuf> {
        self.suggested.lock().unwrap().push(suggested.to_string());
        self.answer.clone()
    }
}

fn chooser(answer: Option<PathBuf>) -> Chooser {
    Chooser {
        answer,
        suggested: std::sync::Mutex::new(Vec::new()),
    }
}

// F-35: a subtitle file is the TIMELINE's -- every cue in OUTPUT time over
// the whole project, in playing order, whether or not captions are shown or
// burned in. q1 is stored at source 3000-5000 on a 2x clip starting at 700
// (output 1700-2700); q2 at source 21000-22500 on a 1x clip at 10000
// (output 11000-12500).
#[test]
fn subtitle_export_uses_output_time() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_with(project());
    let pick = chooser(Some(dir.path().join("walkthrough")));

    let name = export_subtitles_in(&state, &pick, SESSION, SubtitleFormat::Srt).unwrap();

    assert_eq!(name.as_deref(), Some("walkthrough.srt"));
    let text = std::fs::read_to_string(dir.path().join("walkthrough.srt")).unwrap();
    assert_eq!(
        text,
        "1\n00:00:01,700 --> 00:00:02,700\nFirst\n\n\
         2\n00:00:11,000 --> 00:00:12,500\nSecond <b>\n"
    );
    assert_eq!(
        pick.suggested.lock().unwrap().as_slice(),
        ["Deploy - a walkthrough.srt"]
    );
    let entries: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(entries.len(), 1, "no temp left behind");
}

#[test]
fn a_vtt_export_escapes_markup_and_keeps_output_time() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_with(project());
    // An extension already there (any case) is kept, not doubled.
    let pick = chooser(Some(dir.path().join("captions.VTT")));
    let name = export_subtitles_in(&state, &pick, SESSION, SubtitleFormat::Vtt).unwrap();
    assert_eq!(name.as_deref(), Some("captions.VTT"));
    let text = std::fs::read_to_string(dir.path().join("captions.VTT")).unwrap();
    assert!(
        text.starts_with("WEBVTT\n\n1\n00:00:01.700 --> 00:00:02.700\nFirst\n"),
        "{text}"
    );
    assert!(
        text.contains("00:00:11.000 --> 00:00:12.500\nSecond &lt;b&gt;\n"),
        "{text}"
    );
}

// A first write never replaces: an existing file at the chosen name is
// refused and left byte-identical (the package export's rule).
#[test]
fn an_existing_file_is_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("taken.srt");
    std::fs::write(&target, b"theirs").unwrap();
    let state = state_with(project());
    let e = export_subtitles_in(
        &state,
        &chooser(Some(target.clone())),
        SESSION,
        SubtitleFormat::Srt,
    )
    .unwrap_err();
    assert_eq!(e.code, EditorErrorCode::WriteDenied);
    assert_eq!(std::fs::read(&target).unwrap(), b"theirs");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

// Nothing to export is said BEFORE the dialog opens; a dismissed dialog is
// `null`, not an error.
#[test]
fn no_captions_refuses_and_a_dismissed_dialog_is_null() {
    let mut bare = project();
    bare.captions = None;
    let pick = chooser(None);
    let e =
        export_subtitles_in(&state_with(bare), &pick, SESSION, SubtitleFormat::Srt).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(
        pick.suggested.lock().unwrap().is_empty(),
        "no dialog for nothing"
    );
    let none = export_subtitles_in(&state_with(project()), &pick, SESSION, SubtitleFormat::Vtt);
    assert_eq!(none.unwrap(), None);
}

#[test]
fn the_format_wire_spelling_is_pinned() {
    let formats: Vec<SubtitleFormat> = serde_json::from_value(json!(["srt", "vtt"])).unwrap();
    assert_eq!(formats, [SubtitleFormat::Srt, SubtitleFormat::Vtt]);
}
