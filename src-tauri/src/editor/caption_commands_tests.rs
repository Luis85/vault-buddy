//! Tests for `caption_commands` -- the `AppHandle`-free halves: the bounded
//! read of the file the dialog granted, and the parse -> plan -> ONE
//! `ImportCaptions` edit against a live session.

use serde_json::json;
use vault_buddy_core::editor::{EditorSession, Project};

use super::*;

const SESSION: &str = "ses-cap";

/// One video clip at output 700 playing source `[1000, 9000)` at speed 2:
/// the file's clip-relative OUTPUT times, the stored SOURCE times and the
/// timeline positions are three different numbers.
fn project() -> Project {
    serde_json::from_value(json!({
        "schema": "vault-buddy-video-project/3",
        "id": "proj-cap",
        "title": "Captions",
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
        }],
        "effects": [], "markers": [], "transitions": [],
        "destination": { "vault": "", "folder": "", "dated": false }
    }))
    .expect("the fixture is a valid project document")
}

fn state_with_session() -> EditorState {
    let state = EditorState::default();
    state.sessions.lock().unwrap().insert(
        SESSION.to_string(),
        EditorSession::resume(SESSION, project(), 4),
    );
    state
}

const SRT: &str = "1\r\n00:00:00,500 --> 00:00:01,500\r\nInside\r\n\r\n\
                   2\r\n00:00:04,000 --> 00:00:05,000\r\nAfter the clip\r\n\r\n\
                   3\r\n00:00:03,500 --> 00:00:04,500\r\nRuns past the end\r\n";

#[test]
fn import_converts_to_source_time_and_reports_the_skipped_cue() {
    let state = state_with_session();
    let result = import_captions_in(&state, SESSION, "c1", false, SRT).unwrap();
    assert_eq!((result.imported, result.skipped), (2, 1));
    assert_eq!(
        result.projection.snapshot.revision, 5,
        "the whole import is ONE edit"
    );
    assert_eq!(
        result.projection.snapshot.undo_label.as_deref(),
        Some("Import captions")
    );
    let cues = &result.projection.project.captions.as_ref().unwrap().cues;
    let spans: Vec<(u64, u64, &str)> = cues
        .iter()
        .map(|c| (c.start_ms, c.end_ms, c.text.as_str()))
        .collect();
    assert_eq!(
        spans,
        vec![
            (2_000, 4_000, "Inside"),
            (8_000, 9_000, "Runs past the end")
        ]
    );
}

#[test]
fn a_parse_error_names_the_line_and_changes_nothing() {
    let state = state_with_session();
    let err = import_captions_in(
        &state,
        SESSION,
        "c1",
        false,
        "1\n00:00:01 --> 00:00:02,000\nx\n",
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.starts_with("Line 2:"), "{}", err.message);
    let sessions = state.sessions.lock().unwrap();
    assert_eq!(sessions[SESSION].snapshot().revision, 4);
}

#[test]
fn nothing_inside_the_clip_is_refused_and_changes_nothing() {
    let state = state_with_session();
    let late = "1\n00:00:04,000 --> 00:00:05,000\nLate\n";
    let err = import_captions_in(&state, SESSION, "c1", true, late).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert_eq!(
        state.sessions.lock().unwrap()[SESSION].snapshot().revision,
        4
    );
}

#[test]
fn an_unknown_session_is_session_gone() {
    let state = EditorState::default();
    let err = import_captions_in(&state, "nope", "c1", false, SRT).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::SessionGone);
}

#[test]
fn read_caption_file_reads_a_small_utf8_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("talk.srt");
    std::fs::write(&path, "\u{feff}1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
    let text = read_caption_file(&path).unwrap();
    assert!(text.contains("Hi"));
}

#[test]
fn read_caption_file_refuses_an_oversized_file_without_naming_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret-folder-name.srt");
    std::fs::write(&path, vec![b'a'; 2 * 1024 * 1024 + 1]).unwrap();
    let err = read_caption_file(&path).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("2 MiB"), "{}", err.message);
    assert!(
        !err.message.contains("secret-folder-name"),
        "an error must never carry the picked file's path: {}",
        err.message
    );
}

#[test]
fn read_caption_file_refuses_text_that_is_not_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("latin1.srt");
    std::fs::write(&path, [0x31, 0x0a, 0xe9, 0xff, 0xfe]).unwrap();
    let err = read_caption_file(&path).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("UTF-8"), "{}", err.message);
}

#[test]
fn read_caption_file_reports_a_vanished_file_without_its_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gone-away.srt");
    let err = read_caption_file(&path).unwrap_err();
    assert!(!err.message.contains("gone-away"), "{}", err.message);
}

// Fix round 1: like `editor_import_media`, Rust -- not the UI's disabled
// button -- refuses a second import while one is running in the same
// session, so two quick invocations never open two dialogs over one clip.
#[test]
fn a_second_concurrent_caption_import_is_refused() {
    let state = state_with_session();
    claim_caption_import(&state, SESSION).unwrap();
    let err = claim_caption_import(&state, SESSION).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("already"), "{}", err.message);
    // Another session is independent.
    claim_caption_import(&state, "ses-other").unwrap();
    // Released: the next import may start.
    release_caption_import(&state, SESSION);
    claim_caption_import(&state, SESSION).unwrap();
}
