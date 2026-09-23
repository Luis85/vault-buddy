//! The read-side envelopes the editor's IPC commands return (Contract
//! reference `Envelopes`): `EditorProjection` (every committed edit and
//! every snapshot fetch) and `EditorOpenResult` (opening a project), plus
//! `MissingMedia`, one row of the open result's "these sources are gone"
//! list.
//!
//! Pure and Tauri-free, so they live here beside `EditorSnapshot` rather
//! than in the shell: everything they carry is already a `core::editor`
//! type. The envelope keys are camelCase like every other DTO; the
//! `project` inside travels in DOCUMENT spelling (R3) — it is the project
//! graph itself, serialized exactly as `project.json` stores it.

use serde::Serialize;

use super::model::Project;
use super::session::{EditorSession, EditorSnapshot};
use super::workspace::Workspace;

/// What `editor_execute` and `editor_get_snapshot` return: the session's
/// snapshot plus the full project graph at that revision. The frontend
/// installs the graph wholesale (R14 — Rust is authoritative for committed
/// edits), so it is never a diff.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorProjection {
    pub snapshot: EditorSnapshot,
    pub project: Project,
}

impl EditorProjection {
    /// The projection of `session` as it stands right now.
    pub fn of(session: &EditorSession) -> Self {
        Self {
            snapshot: session.snapshot(),
            project: session.project().clone(),
        }
    }
}

/// One source a freshly opened project references but whose file is not on
/// disk (a staged capture discarded from outside the app, a media file a
/// sync client removed). Carries only what the reconnect dialog shows —
/// never a path.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingMedia {
    pub asset_id: String,
    pub name: String,
    pub expected_size: u64,
    pub expected_duration_ms: u64,
}

/// What `editor_open_staged` (and, later, `editor_open_project` /
/// `editor_import_package`) return. `source_base` and the three lists are
/// explicit even when empty/`null` — the frontend reads an absent key as a
/// decoding error, not as "none".
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorOpenResult {
    pub snapshot: EditorSnapshot,
    pub project: Project,
    pub workspace: Workspace,
    pub missing: Vec<MissingMedia>,
    pub source_base: Option<String>,
    pub recovered: bool,
}

/// What `editor_import_captions` returns once the user picked a file (ADR
/// §3.3; `null` on the wire when the dialog was cancelled): the projection
/// after the ONE import edit, how many cues landed on the clip, and how
/// many fell outside it -- reported, never silently dropped (Task 36).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionImportResult {
    pub projection: EditorProjection,
    pub imported: usize,
    pub skipped: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::test_support::minimal_project;
    use serde_json::json;

    fn snapshot() -> EditorSnapshot {
        EditorSnapshot {
            session_id: "ses-1".to_string(),
            project_id: "project".to_string(),
            revision: 3,
            persisted_revision: None,
            title: "Project".to_string(),
            duration_ms: 0,
            can_undo: true,
            can_redo: false,
            undo_label: Some("Rename".to_string()),
            redo_label: None,
        }
    }

    // The project graph inside, spelled literally in DOCUMENT spelling
    // (`master_gain`, not `masterGain`) — R3: a camelCase rename leaking
    // from the envelope onto the graph would make the frontend's decoder and
    // `project.json` disagree about the same object.
    fn minimal_project_json() -> serde_json::Value {
        json!({
            "schema": "vault-buddy-video-project/3",
            "id": "project",
            "title": "Project",
            "canvas": { "width": 1280, "height": 720, "fps": 30 },
            "master_gain": 1.0,
            "assets": [],
            "tracks": [],
            "clips": [],
            "effects": [],
            "markers": [],
            "transitions": [],
            "destination": { "vault": "", "folder": "", "dated": false }
        })
    }

    fn snapshot_json() -> serde_json::Value {
        json!({
            "sessionId": "ses-1",
            "projectId": "project",
            "revision": 3,
            "persistedRevision": null,
            "title": "Project",
            "durationMs": 0,
            "canUndo": true,
            "canRedo": false,
            "undoLabel": "Rename",
            "redoLabel": null
        })
    }

    #[test]
    fn editor_projection_serializes_the_contract_shape() {
        let p = EditorProjection {
            snapshot: snapshot(),
            project: minimal_project(),
        };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            json!({ "snapshot": snapshot_json(), "project": minimal_project_json() })
        );
    }

    #[test]
    fn missing_media_serializes_camel_case() {
        let m = MissingMedia {
            asset_id: "src".to_string(),
            name: "Demo".to_string(),
            expected_size: 4_096,
            expected_duration_ms: 61_500,
        };
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({
                "assetId": "src",
                "name": "Demo",
                "expectedSize": 4096,
                "expectedDurationMs": 61500
            })
        );
    }

    // `sourceBase: null` and `missing: []` must be PRESENT keys, and the
    // sanitized-empty workspace must be `{}` (not omitted) — the decoder
    // treats a missing key as a malformed reply.
    #[test]
    fn editor_open_result_serializes_the_contract_shape() {
        let r = EditorOpenResult {
            snapshot: snapshot(),
            project: minimal_project(),
            workspace: Workspace::default(),
            missing: vec![MissingMedia {
                asset_id: "src".to_string(),
                name: "Demo".to_string(),
                expected_size: 7,
                expected_duration_ms: 9,
            }],
            source_base: Some("2026-09-20 1432 Demo".to_string()),
            recovered: false,
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({
                "snapshot": snapshot_json(),
                "project": minimal_project_json(),
                "workspace": {},
                "missing": [
                    { "assetId": "src", "name": "Demo", "expectedSize": 7, "expectedDurationMs": 9 }
                ],
                "sourceBase": "2026-09-20 1432 Demo",
                "recovered": false
            })
        );
        let none = EditorOpenResult {
            missing: Vec::new(),
            source_base: None,
            ..r
        };
        let v = serde_json::to_value(&none).unwrap();
        assert_eq!(v["sourceBase"], serde_json::Value::Null);
        assert!(v.as_object().unwrap().contains_key("sourceBase"));
        assert_eq!(v["missing"], json!([]));
    }

    #[test]
    fn projection_of_a_session_carries_its_current_project() {
        let session = EditorSession::new("ses-9", minimal_project());
        let p = EditorProjection::of(&session);
        assert_eq!(p.snapshot.session_id, "ses-9");
        assert_eq!(p.project, minimal_project());
    }

    #[test]
    fn caption_import_result_serializes_the_contract_shape() {
        let r = CaptionImportResult {
            projection: EditorProjection {
                snapshot: snapshot(),
                project: minimal_project(),
            },
            imported: 12,
            skipped: 3,
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({
                "projection": { "snapshot": snapshot_json(), "project": minimal_project_json() },
                "imported": 12,
                "skipped": 3
            })
        );
    }
}
