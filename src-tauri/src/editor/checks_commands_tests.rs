//! `checks_commands`' tests: the three shell facts `run_checks` is handed
//! come from the session's OWN store and registry -- a missing original
//! from `sources.json` and the disk (the reconnect dialog's
//! `missing_media`), sound from each record's `hasAudio`, and the webcam
//! takes still open in `EditorState::takes`.

use std::collections::BTreeMap;
use std::path::Path;

use vault_buddy_core::editor::checks::{CheckAction, CheckCode, Severity, TargetKind};
use vault_buddy_core::editor::probe::ProbeFacts;
use vault_buddy_core::editor::{EditorErrorCode, EditorSession, Project};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::*;
use crate::editor::project_store::{project_dir, SourceLocator, SourceMediaKind, SourceRecord};
use crate::editor::store_io::create_project;
use crate::editor::webcam_commands::{begin_in, TakeIo};

const SESSION: &str = "ses-checks";

/// ffmpeg "present", so a take can begin; nothing here finishes one.
struct ReadyIo;

impl TakeIo for ReadyIo {
    fn remux(&self, _part: &Path, _out: &Path) -> Result<(), EditorError> {
        unreachable!("no take is finished here")
    }
    fn probe(&self, _path: &Path) -> Result<ProbeFacts, EditorError> {
        unreachable!("no take is finished here")
    }
    fn ready(&self) -> Result<(), EditorError> {
        Ok(())
    }
}

/// A missing capture under c1, and a present voice-over under a MUTED s1:
/// the only sound in the edit is silenced.
fn project() -> Project {
    serde_json::from_value(serde_json::json!({
        "schema": "vault-buddy-video-project/3",
        "id": "proj-checks",
        "title": "Checks",
        "canvas": { "width": 1280, "height": 720, "fps": 30 },
        "master_gain": 1,
        "assets": [
            { "id": "cap", "kind": "video", "name": "capture.mp4", "duration_ms": 20_000 },
            { "id": "voice", "kind": "audio", "name": "voice.m4a", "duration_ms": 20_000 }
        ],
        "tracks": [
            { "id": "v1", "kind": "video", "name": "Screen", "visible": true,
              "locked": false, "muted": false, "solo": false, "volume": 1 },
            { "id": "a1", "kind": "audio", "name": "Voice", "visible": true,
              "locked": false, "muted": false, "solo": false, "volume": 1 }
        ],
        "clips": [
            { "id": "c1", "asset_id": "cap", "track_id": "v1", "name": "c1",
              "start_ms": 0, "in_ms": 0, "out_ms": 9_000, "fade_in_ms": 0, "fade_out_ms": 0,
              "fade_curve": "linear", "opacity": 1, "volume": 1, "muted": false,
              "x": 0, "y": 0, "w": 1, "h": 1 },
            { "id": "s1", "asset_id": "voice", "track_id": "a1", "name": "s1",
              "start_ms": 0, "in_ms": 0, "out_ms": 9_000, "fade_in_ms": 0, "fade_out_ms": 0,
              "fade_curve": "linear", "opacity": 1, "volume": 1, "muted": true,
              "x": 0, "y": 0, "w": 1, "h": 1 }
        ],
        "effects": [], "markers": [], "transitions": [],
        "destination": { "vault": "vault-1", "folder": "", "dated": false }
    }))
    .unwrap()
}

fn record(file: &str, has_audio: bool, kind: SourceMediaKind) -> SourceRecord {
    SourceRecord {
        locator: SourceLocator::Media {
            file: file.to_string(),
        },
        sha256: None,
        size: 10,
        duration_ms: 20_000,
        width: None,
        height: None,
        has_audio,
        has_video: kind == SourceMediaKind::Video,
        media_kind: kind,
        replaced_from: None,
    }
}

struct Fixture {
    root: tempfile::TempDir,
    state: EditorState,
}

impl Fixture {
    /// The capture's record says it has NO sound: only the voice-over does.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut sources = BTreeMap::new();
        sources.insert(
            "cap".to_string(),
            record("cap.mp4", false, SourceMediaKind::Video),
        );
        sources.insert(
            "voice".to_string(),
            record("voice.m4a", true, SourceMediaKind::Audio),
        );
        create_project(root.path(), &project(), &sources).unwrap();
        let media = project_dir(root.path(), "proj-checks")
            .unwrap()
            .join("media");
        std::fs::create_dir(&media).unwrap();
        std::fs::write(media.join("voice.m4a"), b"0123456789").unwrap();
        let state = EditorState::default();
        lock_ignoring_poison(&state.sessions).insert(
            SESSION.to_string(),
            EditorSession::resume(SESSION, project(), 3),
        );
        Self { root, state }
    }

    fn checks(&self) -> Vec<vault_buddy_core::editor::checks::CheckFinding> {
        checks_in(&self.state, self.root.path(), SESSION).unwrap()
    }
}

#[test]
fn checks_read_missing_media_sound_and_open_takes_from_the_session() {
    let f = Fixture::new();
    let findings = f.checks();
    let codes: Vec<CheckCode> = findings.iter().map(|x| x.code).collect();
    assert_eq!(codes, vec![CheckCode::MissingMedia, CheckCode::AllMuted]);
    let missing = &findings[0];
    assert_eq!(missing.severity, Severity::Blocking);
    assert_eq!(missing.target.kind, TargetKind::Asset);
    assert_eq!(missing.target.id.as_deref(), Some("cap"));
    assert_eq!(missing.action, Some(CheckAction::Reconnect));

    begin_in(
        &f.state,
        f.root.path(),
        SESSION,
        "video/webm",
        None,
        &ReadyIo,
    )
    .unwrap();
    let codes: Vec<CheckCode> = f.checks().iter().map(|x| x.code).collect();
    assert_eq!(
        codes,
        vec![
            CheckCode::MissingMedia,
            CheckCode::AllMuted,
            CheckCode::PendingTake
        ]
    );
}

#[test]
fn a_file_put_back_is_no_longer_missing() {
    let f = Fixture::new();
    let media = project_dir(f.root.path(), "proj-checks")
        .unwrap()
        .join("media");
    std::fs::write(media.join("cap.mp4"), b"0123456789").unwrap();
    let codes: Vec<CheckCode> = f.checks().iter().map(|x| x.code).collect();
    assert_eq!(codes, vec![CheckCode::AllMuted]);
}

#[test]
fn an_ended_session_is_refused() {
    let f = Fixture::new();
    let e = checks_in(&f.state, f.root.path(), "ses-other").unwrap_err();
    assert_eq!(e.code, EditorErrorCode::SessionGone);
}
