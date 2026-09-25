//! Tests for `diagnostics` (Task 58; F-50): the document is counts,
//! capabilities and codes, whatever the project, its media and its jobs
//! are called — and it lands as a NEW file the user named.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;
use vault_buddy_core::editor::fingerprint::new_product;
use vault_buddy_core::editor::{EditorErrorCode, EditorSession, Project};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::*;
use crate::editor::media_jobs::{JobReporter, JobTerminal, NoSubscriber, PerFileError};
use crate::editor::project_store::{SourceLocator, SourceMediaKind, SourceRecord};
use crate::editor::render_jobs::write_ledger;
use crate::editor::store_io::create_project;

const SESSION: &str = "ses-diag";
const PROJECT: &str = "proj-diag";
const SECRET_TITLE: &str = "Secret plan";
const SECRET_PATH: &str = "C:\\Users\\x\\a.mp4";

fn project() -> Project {
    serde_json::from_value(json!({
        "schema": "vault-buddy-video-project/3",
        "id": PROJECT,
        "title": SECRET_TITLE,
        "canvas": { "width": 1280, "height": 720, "fps": 30 },
        "master_gain": 1,
        "assets": [{ "id": "cap", "kind": "video", "name": SECRET_PATH, "duration_ms": 9000 }],
        "tracks": [{
            "id": "v1", "kind": "video", "name": "Secret track", "visible": true,
            "locked": false, "muted": false, "solo": false, "volume": 1
        }],
        "clips": [{
            "id": "c1", "asset_id": "cap", "track_id": "v1", "name": "Secret clip",
            "start_ms": 0, "in_ms": 0, "out_ms": 9000, "fade_in_ms": 0, "fade_out_ms": 0,
            "fade_curve": "linear", "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }],
        "effects": [], "markers": [], "transitions": [],
        "destination": { "vault": "vault-secret", "folder": "Secret folder", "dated": false }
    }))
    .expect("a valid project")
}

fn env() -> Environment {
    Environment {
        app_version: "0.7.2".into(),
        os: "windows x86_64".into(),
        ffmpeg: FfmpegDiagnostics {
            found: true,
            version: Some("9.0".into()),
            filters: vec!["ass".into(), "xfade".into()],
        },
        webview2_version: Some("131.0.2903.70".into()),
    }
}

/// A project on disk named `SECRET_TITLE` whose one source is
/// `SECRET_PATH`, open in a session, with one product and a job whose
/// error message names the file.
fn seeded() -> (tempfile::TempDir, EditorState) {
    let root = tempfile::tempdir().unwrap();
    let mut sources = BTreeMap::new();
    sources.insert(
        "cap".to_string(),
        SourceRecord {
            locator: SourceLocator::Media {
                file: SECRET_PATH.into(),
            },
            sha256: None,
            size: 10,
            duration_ms: 9000,
            width: Some(1920),
            height: Some(1080),
            has_audio: true,
            has_video: true,
            media_kind: SourceMediaKind::Video,
            replaced_from: None,
        },
    );
    create_project(root.path(), &project(), &sources).unwrap();
    let product = new_product(
        &project(),
        1,
        "prod-1",
        "Secret plan v1",
        "prod-1.mp4",
        9000,
        None,
        "2026-09-24T10:00:00+02:00",
    );
    write_ledger(root.path(), PROJECT, &[product]).unwrap();

    let state = EditorState::default();
    lock_ignoring_poison(&state.sessions).insert(
        SESSION.to_string(),
        EditorSession::resume(SESSION, project(), 3),
    );
    let (job, _) = lock_ignoring_poison(&state.jobs).register(SESSION, JobKind::Import);
    JobReporter::new(&state.jobs, &NoSubscriber, SESSION, &job, JobKind::Import).finish(
        JobPhase::Failed,
        JobTerminal {
            per_file: Some(vec![PerFileError {
                name: SECRET_PATH.into(),
                error: format!("{SECRET_PATH} could not be read"),
            }]),
            error: Some(EditorError::new(
                EditorErrorCode::UnsupportedMedia,
                format!("{SECRET_TITLE}: {SECRET_PATH} is not a video"),
            )),
            ..JobTerminal::default()
        },
    );
    (root, state)
}

#[test]
fn diagnostics_contain_no_paths_or_names() {
    let (root, state) = seeded();
    let diagnostics = collect_in(&state, root.path(), env());
    let text = serde_json::to_string(&diagnostics).unwrap();
    for leak in [
        SECRET_TITLE,
        "Secret",
        SECRET_PATH,
        "a.mp4",
        "Users",
        "vault-secret",
        PROJECT,
        SESSION,
        "prod-1",
    ] {
        assert!(
            !text.contains(leak),
            "the diagnostics carry {leak:?}: {text}"
        );
    }
    // What it does carry: the counts, the environment and the job's CODE.
    assert_eq!(diagnostics.sessions, 1);
    assert_eq!(diagnostics.projects, 1);
    assert_eq!(diagnostics.products, 1);
    assert_eq!(
        diagnostics.jobs,
        vec![JobDiagnostics {
            kind: JobKind::Import,
            phase: JobPhase::Failed,
            error_code: Some(EditorErrorCode::UnsupportedMedia),
        }]
    );
}

#[test]
fn the_document_is_the_wire_shape() {
    let diagnostics = Diagnostics {
        app_version: "0.7.2".into(),
        os: "windows x86_64".into(),
        ffmpeg: FfmpegDiagnostics {
            found: true,
            version: Some("9.0".into()),
            filters: vec!["ass".into()],
        },
        sessions: 2,
        projects: 3,
        products: 4,
        jobs: vec![
            JobDiagnostics {
                kind: JobKind::Render,
                phase: JobPhase::Rendering,
                error_code: None,
            },
            JobDiagnostics {
                kind: JobKind::Publish,
                phase: JobPhase::Failed,
                error_code: Some(EditorErrorCode::DiskFull),
            },
        ],
        webview2_version: None,
    };
    assert_eq!(
        serde_json::to_value(&diagnostics).unwrap(),
        json!({
            "appVersion": "0.7.2",
            "os": "windows x86_64",
            "ffmpeg": { "found": true, "version": "9.0", "filters": ["ass"] },
            "sessions": 2,
            "projects": 3,
            "products": 4,
            "jobs": [
                { "kind": "render", "phase": "rendering", "errorCode": null },
                { "kind": "publish", "phase": "failed", "errorCode": "diskFull" }
            ],
            "webview2Version": null
        })
    );
}

#[test]
fn no_store_and_no_sessions_is_all_zero() {
    let root = tempfile::tempdir().unwrap();
    let diagnostics = collect_in(&EditorState::default(), root.path(), env());
    assert_eq!(
        (
            diagnostics.sessions,
            diagnostics.projects,
            diagnostics.products
        ),
        (0, 0, 0)
    );
    assert!(diagnostics.jobs.is_empty());
}

struct Fixed(Option<PathBuf>);

impl DiagnosticsTarget for Fixed {
    fn save_target(&self, suggested: &str) -> Option<PathBuf> {
        assert_eq!(suggested, "vault-buddy-diagnostics.json");
        self.0.clone()
    }
}

#[test]
fn the_export_writes_a_new_json_file_and_never_replaces_one() {
    let (root, state) = seeded();
    let diagnostics = collect_in(&state, root.path(), env());
    let out = tempfile::tempdir().unwrap();

    let name = export_in(&Fixed(Some(out.path().join("support"))), &diagnostics).unwrap();
    assert_eq!(name.as_deref(), Some("support.json"));
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.path().join("support.json")).unwrap()).unwrap();
    assert_eq!(written, serde_json::to_value(&diagnostics).unwrap());

    // The same name again: refused, and the file is left as it was.
    std::fs::write(out.path().join("support.json"), b"mine").unwrap();
    let refused = export_in(&Fixed(Some(out.path().join("support.json"))), &diagnostics)
        .expect_err("an existing file is never replaced");
    assert_eq!(refused.code, EditorErrorCode::WriteDenied);
    assert_eq!(
        std::fs::read(out.path().join("support.json")).unwrap(),
        b"mine"
    );
    let left: Vec<_> = std::fs::read_dir(out.path()).unwrap().flatten().collect();
    assert_eq!(left.len(), 1, "no temporary file outlives the refusal");
}

#[test]
fn a_dismissed_dialog_writes_nothing() {
    let diagnostics = collect_in(
        &EditorState::default(),
        tempfile::tempdir().unwrap().path(),
        env(),
    );
    assert_eq!(export_in(&Fixed(None), &diagnostics).unwrap(), None);
}
