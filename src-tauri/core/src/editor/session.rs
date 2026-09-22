//! The serial editor session: one project, bounded undo/redo, and the
//! command envelope (`ExecuteRequest`/`EditorSnapshot`) that installs a new
//! revision only after both `commands::apply` and `validate_project` accept
//! it (R14).
//!
//! `EditorSession::execute` is the ENTIRE life of a command, in order
//! (Contract reference + this task's brief): wrong `session_id` ->
//! `sessionGone`; an invalid `command_id` -> `invalidRequest`; a REPLAYED
//! `command_id` -> the current snapshot, unchanged (an at-least-once IPC
//! caller may retry a request whose reply it never saw, and that retry
//! must be a no-op, not a second edit -- checked BEFORE the revision
//! comparison, since the retry's `expectedRevision` is stale by
//! construction once the first attempt already landed); a stale
//! `expectedRevision` -> `revisionConflict`; `undo`/`redo` go through
//! `history.rs` directly (never through `commands::apply`, which has no
//! `History` to consult); everything else goes through `commands::apply`
//! then `validate_project` -- and ONLY on success does the candidate ever
//! replace `self.project` or `self.revision` advance. That ordering is the
//! atomicity guarantee: a command that fails validation must leave the
//! session byte-for-byte as it was.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::commands::{self, EditorCommand, InternalCommand};
use super::error::{EditorError, EditorErrorCode};
use super::history::History;
use super::ids::is_valid_id;
use super::limits::MAX_RECENT_COMMANDS;
use super::model::Project;
use super::time::project_duration;
use super::validate::validate_project;

/// The `editor_execute` request envelope (Contract reference `Envelopes`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteRequest {
    pub session_id: String,
    pub expected_revision: u64,
    pub command_id: String,
    pub command: EditorCommand,
}

/// What every successful (or replayed) `execute` call returns (Contract
/// reference `Envelopes`). `persisted_revision`/`undo_label`/`redo_label`
/// serialize as explicit `null`, never an omitted key -- the frontend reads
/// their absence as "never saved" / "nothing to undo/redo", not as "field
/// not sent this time".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorSnapshot {
    pub session_id: String,
    pub project_id: String,
    pub revision: u64,
    pub persisted_revision: Option<u64>,
    pub title: String,
    pub duration_ms: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
}

/// A live editing session over one project (R14): every accepted command
/// installs a NEW revision atomically. Undo/redo travel the same revision
/// counter (`DATA-MODEL.md` § Entities: "undo still advances the current
/// revision even though it changes the graph back").
pub struct EditorSession {
    session_id: String,
    project: Project,
    revision: u64,
    persisted_revision: Option<u64>,
    history: History,
    /// `(commandId, revision)` pairs, most-recently-applied last, capped at
    /// `MAX_RECENT_COMMANDS` -- an at-least-once IPC caller may retry a
    /// request whose reply it never saw; replaying the SAME `command_id`
    /// must be a no-op, not a second edit.
    recent: VecDeque<(String, u64)>,
}

impl EditorSession {
    /// A freshly opened project starts at revision 1 -- there is no "zero"
    /// state a caller could ever observe or race against, and a persisted
    /// `Record.revision` (`model_cues::Record`) is itself required to be
    /// `>= 1` (`validate_envelope`), so a brand-new in-memory session
    /// matches what a freshly-saved one would read back as.
    pub fn new(session_id: impl Into<String>, project: Project) -> Self {
        Self {
            session_id: session_id.into(),
            project,
            revision: 1,
            persisted_revision: None,
            history: History::new(),
            recent: VecDeque::new(),
        }
    }

    pub fn execute(&mut self, req: &ExecuteRequest) -> Result<EditorSnapshot, EditorError> {
        if req.session_id != self.session_id {
            return Err(EditorError::new(
                EditorErrorCode::SessionGone,
                "sessionId does not match this session",
            ));
        }
        if !is_valid_id(&req.command_id) {
            return Err(EditorError::new(
                EditorErrorCode::InvalidRequest,
                "commandId is not a valid entity id",
            ));
        }
        if self.recent.iter().any(|(id, _)| id == &req.command_id) {
            return Ok(self.snapshot());
        }
        if req.expected_revision != self.revision {
            return Err(EditorError::new(
                EditorErrorCode::RevisionConflict,
                format!(
                    "expected revision {} but the session is at {}",
                    req.expected_revision, self.revision
                ),
            ));
        }

        match &req.command {
            EditorCommand::Undo => {
                let (restored, _label) =
                    self.history.undo(self.project.clone()).ok_or_else(|| {
                        EditorError::new(EditorErrorCode::InvalidRequest, "nothing to undo")
                    })?;
                self.project = restored;
            }
            EditorCommand::Redo => {
                let (restored, _label) =
                    self.history.redo(self.project.clone()).ok_or_else(|| {
                        EditorError::new(EditorErrorCode::InvalidRequest, "nothing to redo")
                    })?;
                self.project = restored;
            }
            other => {
                let (candidate, label) = commands::apply(&self.project, other)?;
                validate_project(&candidate)?;
                let previous = std::mem::replace(&mut self.project, candidate);
                self.history.push(previous, label);
            }
        }

        self.revision += 1;
        self.record_command(req.command_id.clone());
        Ok(self.snapshot())
    }

    /// The `InternalCommand` counterpart of `execute`: same
    /// validate-then-install atomicity, no wire bookkeeping (no
    /// `sessionId`/`commandId`/`expectedRevision` -- an internal caller
    /// already holds `&mut self`, which IS the ownership/currency proof an
    /// external IPC request needs those fields to establish).
    pub fn execute_internal(
        &mut self,
        cmd: &InternalCommand,
    ) -> Result<EditorSnapshot, EditorError> {
        let (candidate, label) = commands::apply_internal(&self.project, cmd)?;
        validate_project(&candidate)?;
        let previous = std::mem::replace(&mut self.project, candidate);
        self.history.push(previous, label);
        self.revision += 1;
        Ok(self.snapshot())
    }

    fn record_command(&mut self, command_id: String) {
        self.recent.push_back((command_id, self.revision));
        while self.recent.len() > MAX_RECENT_COMMANDS {
            self.recent.pop_front();
        }
    }

    pub fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            session_id: self.session_id.clone(),
            project_id: self.project.id.clone(),
            revision: self.revision,
            persisted_revision: self.persisted_revision,
            title: self.project.title.clone(),
            duration_ms: project_duration(&self.project),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            undo_label: self.history.undo_label().map(str::to_string),
            redo_label: self.history.redo_label().map(str::to_string),
        }
    }

    /// Marks `r` as persisted, but only if it is not AHEAD of the session's
    /// own revision -- a save receipt racing a later edit must not claim a
    /// revision the session has not actually reached yet.
    pub fn mark_saved(&mut self, r: u64) {
        if r <= self.revision {
            self.persisted_revision = Some(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::commands::payloads::{RenamePayload, SplitClipPayload};
    use crate::editor::test_support::minimal_project;

    fn new_session() -> EditorSession {
        EditorSession::new("s1", minimal_project())
    }

    fn rename_request(
        session_id: &str,
        expected_revision: u64,
        command_id: &str,
        title: &str,
    ) -> ExecuteRequest {
        ExecuteRequest {
            session_id: session_id.to_string(),
            expected_revision,
            command_id: command_id.to_string(),
            command: EditorCommand::Rename(RenamePayload {
                title: title.to_string(),
            }),
        }
    }

    #[test]
    fn rename_advances_the_revision_and_is_undoable() {
        let mut session = new_session();
        assert_eq!(session.revision, 1);

        let snap = session
            .execute(&rename_request("s1", 1, "cmd-1", "New Title"))
            .unwrap();

        assert_eq!(snap.revision, 2);
        assert_eq!(snap.title, "New Title");
        assert!(snap.can_undo);
        assert_eq!(snap.undo_label.as_deref(), Some("Rename"));
        assert_eq!(session.project.title, "New Title");
    }

    #[test]
    fn stale_expected_revision_is_a_conflict_and_changes_nothing() {
        let mut session = new_session();
        let before = session.project.clone();
        let before_revision = session.revision;

        // MUTATION CHECK: skipping the `expected_revision != self.revision`
        // comparison in `execute` makes this assertion fail (the command
        // would apply instead of erroring).
        let err = session
            .execute(&rename_request("s1", 99, "cmd-1", "New Title"))
            .unwrap_err();

        assert_eq!(err.code, EditorErrorCode::RevisionConflict);
        assert_eq!(
            session.project, before,
            "project must be untouched on conflict"
        );
        assert_eq!(session.revision, before_revision);
    }

    #[test]
    fn replayed_command_id_is_not_applied_twice() {
        let mut session = new_session();
        let req = rename_request("s1", 1, "cmd-1", "New Title");

        let first = session.execute(&req).unwrap();
        assert_eq!(first.revision, 2);

        // Same commandId, same (now-stale) expectedRevision -- an
        // at-least-once retry, not a second edit.
        let second = session.execute(&req).unwrap();
        assert_eq!(
            second.revision, 2,
            "a replayed commandId must not advance the revision again"
        );
        assert_eq!(session.revision, 2);
    }

    #[test]
    fn wrong_session_is_session_gone() {
        let mut session = new_session();
        let err = session
            .execute(&rename_request("wrong-session", 1, "cmd-1", "New Title"))
            .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::SessionGone);
    }

    #[test]
    fn undo_advances_the_revision() {
        let mut session = new_session();
        session
            .execute(&rename_request("s1", 1, "cmd-1", "New Title"))
            .unwrap();
        assert_eq!(session.revision, 2);

        let undo_req = ExecuteRequest {
            session_id: "s1".to_string(),
            expected_revision: 2,
            command_id: "cmd-2".to_string(),
            command: EditorCommand::Undo,
        };
        let snap = session.execute(&undo_req).unwrap();

        assert_eq!(snap.revision, 3, "undo still advances the revision");
        assert_eq!(
            session.project.title, "Project",
            "title must be restored to before the rename"
        );
        assert!(!snap.can_undo);
        assert!(snap.can_redo);
        assert_eq!(snap.redo_label.as_deref(), Some("Rename"));
    }

    #[test]
    fn redo_after_new_edit_is_cleared() {
        let mut session = new_session();
        session
            .execute(&rename_request("s1", 1, "cmd-1", "First"))
            .unwrap();
        session
            .execute(&ExecuteRequest {
                session_id: "s1".to_string(),
                expected_revision: 2,
                command_id: "cmd-undo".to_string(),
                command: EditorCommand::Undo,
            })
            .unwrap();

        // A new edit, not a redo, must clear the redo stack.
        let snap = session
            .execute(&rename_request("s1", 3, "cmd-2", "Second"))
            .unwrap();
        assert!(!snap.can_redo, "a fresh edit must clear redo");
        assert_eq!(snap.redo_label, None);
    }

    #[test]
    fn history_is_bounded() {
        let mut session = new_session();
        for i in 0..101 {
            let req = rename_request(
                "s1",
                session.revision,
                &format!("cmd-{i}"),
                &format!("Title {i}"),
            );
            session.execute(&req).unwrap();
        }

        let mut undone = 0;
        loop {
            let req = ExecuteRequest {
                session_id: "s1".to_string(),
                expected_revision: session.revision,
                command_id: format!("undo-{undone}"),
                command: EditorCommand::Undo,
            };
            match session.execute(&req) {
                Ok(_) => undone += 1,
                Err(_) => break,
            }
        }
        assert_eq!(undone, 100, "MAX_HISTORY caps undo depth at 100");
    }

    #[test]
    fn invalid_candidate_is_rejected_atomically() {
        let mut session = new_session();
        let before = session.project.clone();
        let before_revision = session.revision;
        let too_long = "x".repeat(161);

        // MUTATION CHECK: installing the candidate BEFORE calling
        // `validate_project` (instead of after) makes this assertion fail
        // -- `session.project` would already carry the too-long title by
        // the time the error is returned.
        let err = session
            .execute(&rename_request("s1", 1, "cmd-1", &too_long))
            .unwrap_err();

        assert_eq!(err.code, EditorErrorCode::InvalidProject);
        assert_eq!(
            session.project, before,
            "an invalid candidate must never be installed"
        );
        assert_eq!(session.revision, before_revision);
    }

    #[test]
    fn undo_of_paste_restores_the_whole_operation() {
        // Proves the session-level integration, not just `paste_fragment`'s
        // own pure function: a pasted clip (plus everything it added) must
        // vanish on undo exactly like any other command's candidate, via
        // the SAME `history.push`/`history.undo` machinery every other
        // command already goes through -- nothing about wiring a new
        // command family into `apply`'s dispatch bypasses that.
        use crate::editor::commands::payloads::{ClipboardFragment, PasteFragmentPayload};
        use crate::editor::model::{AssetKind, TrackKind};
        use crate::editor::test_support::{asset, clip, track};

        let mut project = minimal_project();
        project.tracks.push(track("v1", TrackKind::Video, false));
        project.assets.push(asset("a1", AssetKind::Video, 5_000));
        project.clips.push(clip("c1", "v1", "a1", 0, 0, 200));
        let before = project.clone();

        let mut session = EditorSession::new("s1", project);
        assert_eq!(session.project.clips.len(), 1);

        let fragment = ClipboardFragment {
            clips: vec![clip("c1", "v1", "a1", 0, 0, 200)],
            effects: Vec::new(),
            captions: Vec::new(),
            markers: Vec::new(),
            origin_ms: 0,
        };
        let paste_req = ExecuteRequest {
            session_id: "s1".to_string(),
            expected_revision: 1,
            command_id: "cmd-paste".to_string(),
            command: EditorCommand::PasteFragment(PasteFragmentPayload {
                fragment,
                track_id: "v1".to_string(),
                at_ms: 1_000,
            }),
        };
        let snap = session.execute(&paste_req).unwrap();
        assert_eq!(
            session.project.clips.len(),
            2,
            "paste must add exactly one clip"
        );
        assert_eq!(snap.undo_label.as_deref(), Some("Paste"));

        let undo_req = ExecuteRequest {
            session_id: "s1".to_string(),
            expected_revision: snap.revision,
            command_id: "cmd-undo".to_string(),
            command: EditorCommand::Undo,
        };
        session.execute(&undo_req).unwrap();
        assert_eq!(
            session.project, before,
            "undo must restore the WHOLE operation -- the project must be \
             byte-for-byte what it was before the paste, not merely down \
             to the same clip count"
        );
    }

    #[test]
    fn execute_request_wire_literal() {
        let json = serde_json::json!({
            "sessionId": "s",
            "expectedRevision": 1,
            "commandId": "c1",
            "command": {"kind": "splitClip", "clipId": "c", "atMs": 1200},
        });
        let req: ExecuteRequest = serde_json::from_value(json).expect("literal must deserialize");
        assert_eq!(req.session_id, "s");
        assert_eq!(req.expected_revision, 1);
        assert_eq!(req.command_id, "c1");
        assert_eq!(
            req.command,
            EditorCommand::SplitClip(SplitClipPayload {
                clip_id: "c".to_string(),
                at_ms: 1200,
            })
        );

        let with_extra_key = serde_json::json!({
            "sessionId": "s",
            "expectedRevision": 1,
            "commandId": "c1",
            "command": {"kind": "splitClip", "clipId": "c", "atMs": 1200},
            "bogus": true,
        });
        assert!(
            serde_json::from_value::<ExecuteRequest>(with_extra_key).is_err(),
            "an extra top-level key must be rejected"
        );
    }

    #[test]
    fn snapshot_wire_literal() {
        let snapshot = EditorSnapshot {
            session_id: "s".to_string(),
            project_id: "p".to_string(),
            revision: 3,
            persisted_revision: None,
            title: "Demo".to_string(),
            duration_ms: 5_000,
            can_undo: true,
            can_redo: false,
            undo_label: Some("Rename".to_string()),
            redo_label: None,
        };
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            serde_json::json!({
                "sessionId": "s",
                "projectId": "p",
                "revision": 3,
                "persistedRevision": null,
                "title": "Demo",
                "durationMs": 5000,
                "canUndo": true,
                "canRedo": false,
                "undoLabel": "Rename",
                "redoLabel": null,
            })
        );
    }
}
