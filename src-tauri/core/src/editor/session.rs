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

use super::commands::{self, CommandContext, EditorCommand, InternalCommand};
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

    /// A session resumed from an existing on-disk envelope, at exactly the
    /// revision that envelope carries (Task 12 fix round 1, controller
    /// ruling). `record.revision` (`model_cues::Record`) is monotonic
    /// across a project's WHOLE life, not just one session's: a project
    /// saved at revision 3, closed, and reopened must resume at 3, never
    /// reset to 1 -- `new`'s "starts at 1" is only right for a project that
    /// has genuinely never been saved (a fresh mint, whose own envelope on
    /// disk also carries revision 1, so calling this with `revision: 1` for
    /// that case is equivalent). `persisted_revision` starts at
    /// `Some(revision)`: what is in memory on resume IS exactly what was
    /// just read off disk.
    pub fn resume(session_id: impl Into<String>, project: Project, revision: u64) -> Self {
        Self {
            session_id: session_id.into(),
            project,
            revision,
            persisted_revision: Some(revision),
            history: History::new(),
            recent: VecDeque::new(),
        }
    }

    /// A session resumed from a recovery journal (Task 37): `revision` is
    /// the journal's working revision, `persisted_revision` the one the
    /// project store last committed. The two differ, so the session opens
    /// dirty: the recovered edits are exactly what is NOT on disk yet. The
    /// caller keeps `revision > persisted_revision` (monotonic across the
    /// project's life, the `resume` rule).
    pub fn resume_recovered(
        session_id: impl Into<String>,
        project: Project,
        revision: u64,
        persisted_revision: u64,
    ) -> Self {
        Self {
            persisted_revision: Some(persisted_revision),
            ..Self::resume(session_id, project, revision)
        }
    }

    /// `ctx` carries the facts a command needs from outside the graph
    /// (`commands::CommandContext`, Task 27 F15) -- the shell builds it
    /// from the project's `sources.json` before calling in.
    pub fn execute(
        &mut self,
        req: &ExecuteRequest,
        ctx: &CommandContext<'_>,
    ) -> Result<EditorSnapshot, EditorError> {
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
                let (candidate, label) = commands::apply(&self.project, other, ctx)?;
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
        // A reconnect (Task 40) changes `sources.json`, never the graph:
        // Undo could not put the old file back, so it is no undo step.
        if !matches!(cmd, InternalCommand::RelinkAssets(_)) {
            self.history.push(previous, label);
        }
        self.revision += 1;
        Ok(self.snapshot())
    }

    /// `execute_internal` for a native command that arrives through an IPC
    /// REQUEST (Task 46's `editor_restore_product`): the same `commandId`
    /// and `expectedRevision` bookkeeping `execute` applies -- an invalid id
    /// is `invalidRequest`, a replayed one returns the current snapshot
    /// unchanged (checked first, as in `execute`), a stale revision is
    /// `revisionConflict` -- then the native apply.
    pub fn execute_internal_as(
        &mut self,
        expected_revision: u64,
        command_id: &str,
        cmd: &InternalCommand,
    ) -> Result<EditorSnapshot, EditorError> {
        if !is_valid_id(command_id) {
            return Err(EditorError::new(
                EditorErrorCode::InvalidRequest,
                "commandId is not a valid entity id",
            ));
        }
        if self.recent.iter().any(|(id, _)| id == command_id) {
            return Ok(self.snapshot());
        }
        if expected_revision != self.revision {
            return Err(EditorError::new(
                EditorErrorCode::RevisionConflict,
                format!(
                    "expected revision {expected_revision} but the session is at {}",
                    self.revision
                ),
            ));
        }
        let snapshot = self.execute_internal(cmd)?;
        self.record_command(command_id.to_string());
        Ok(snapshot)
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

    /// The project graph at the session's current revision — read-only; the
    /// only way to change it is `execute`/`execute_internal`.
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Marks `r` as persisted, but only if it is not AHEAD of the session's
    /// own revision (a save receipt racing a later edit must not claim a
    /// revision the session has not actually reached yet) AND not BEHIND
    /// the persisted revision already recorded (Task 12 fix round 1: two
    /// concurrent saves can complete out of order -- an older save's
    /// `mark_saved` landing after a newer one's must never regress the
    /// mark). Both guards together make `persisted_revision` monotonically
    /// non-decreasing for the life of the session; the per-session save
    /// lock (`save_commands.rs`) additionally prevents two saves from being
    /// in flight at once in the first place, so this is belt-and-suspenders
    /// against any future caller that calls `mark_saved` outside that lock.
    pub fn mark_saved(&mut self, r: u64) {
        if r > self.revision {
            return;
        }
        if self
            .persisted_revision
            .is_none_or(|persisted| r > persisted)
        {
            self.persisted_revision = Some(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::commands::payloads::{RenamePayload, SplitClipPayload};
    use crate::editor::test_support::{minimal_project, no_context};

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
            .execute(
                &rename_request("s1", 1, "cmd-1", "New Title"),
                &no_context(),
            )
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
            .execute(
                &rename_request("s1", 99, "cmd-1", "New Title"),
                &no_context(),
            )
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

        let first = session.execute(&req, &no_context()).unwrap();
        assert_eq!(first.revision, 2);

        // Same commandId, same (now-stale) expectedRevision -- an
        // at-least-once retry, not a second edit.
        let second = session.execute(&req, &no_context()).unwrap();
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
            .execute(
                &rename_request("wrong-session", 1, "cmd-1", "New Title"),
                &no_context(),
            )
            .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::SessionGone);
    }

    #[test]
    fn undo_advances_the_revision() {
        let mut session = new_session();
        session
            .execute(
                &rename_request("s1", 1, "cmd-1", "New Title"),
                &no_context(),
            )
            .unwrap();
        assert_eq!(session.revision, 2);

        let undo_req = ExecuteRequest {
            session_id: "s1".to_string(),
            expected_revision: 2,
            command_id: "cmd-2".to_string(),
            command: EditorCommand::Undo,
        };
        let snap = session.execute(&undo_req, &no_context()).unwrap();

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
            .execute(&rename_request("s1", 1, "cmd-1", "First"), &no_context())
            .unwrap();
        session
            .execute(
                &ExecuteRequest {
                    session_id: "s1".to_string(),
                    expected_revision: 2,
                    command_id: "cmd-undo".to_string(),
                    command: EditorCommand::Undo,
                },
                &no_context(),
            )
            .unwrap();

        // A new edit, not a redo, must clear the redo stack.
        let snap = session
            .execute(&rename_request("s1", 3, "cmd-2", "Second"), &no_context())
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
            session.execute(&req, &no_context()).unwrap();
        }

        let mut undone = 0;
        loop {
            let req = ExecuteRequest {
                session_id: "s1".to_string(),
                expected_revision: session.revision,
                command_id: format!("undo-{undone}"),
                command: EditorCommand::Undo,
            };
            match session.execute(&req, &no_context()) {
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
            .execute(&rename_request("s1", 1, "cmd-1", &too_long), &no_context())
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
        let snap = session.execute(&paste_req, &no_context()).unwrap();
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
        session.execute(&undo_req, &no_context()).unwrap();
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

    // Task 12 fix round 1 (controller ruling): a session resumed from an
    // existing on-disk envelope must start AT that envelope's revision, not
    // reset to 1 -- record.revision is monotonic across a project's whole
    // life.
    #[test]
    fn resume_starts_at_the_given_revision_and_marks_it_saved() {
        let session = EditorSession::resume("s1", minimal_project(), 7);
        let snap = session.snapshot();
        assert_eq!(snap.revision, 7);
        assert_eq!(snap.persisted_revision, Some(7));
        assert!(
            !snap.can_undo,
            "a resumed session starts with no local history"
        );
    }

    // Task 37: a session resumed from a recovery journal carries the
    // journal's revision but only the STORE's committed revision as
    // persisted -- it must open dirty, or the recovered edits would read as
    // already saved and a close would never offer to keep them.
    #[test]
    fn resume_recovered_starts_dirty_at_the_journal_revision() {
        let session = EditorSession::resume_recovered("s1", minimal_project(), 9, 4);
        let snap = session.snapshot();
        assert_eq!(snap.revision, 9);
        assert_eq!(snap.persisted_revision, Some(4));
        assert_ne!(snap.revision, snap.persisted_revision.unwrap());
    }

    // Task 12 fix round 1: two concurrent saves racing on one session must
    // never let the LATER-COMPLETING call's mark_saved regress
    // persistedRevision below what an EARLIER-COMPLETING call already
    // recorded (belt-and-suspenders alongside the per-session save lock).
    #[test]
    fn mark_saved_never_moves_persisted_revision_backwards() {
        let mut session = new_session();
        session
            .execute(&rename_request("s1", 1, "cmd-1", "First"), &no_context())
            .unwrap();
        session
            .execute(&rename_request("s1", 2, "cmd-2", "Second"), &no_context())
            .unwrap();
        assert_eq!(session.revision, 3);

        session.mark_saved(3);
        assert_eq!(session.persisted_revision, Some(3));

        // MUTATION CHECK: dropping the backwards-guard in `mark_saved` makes
        // this assertion fail -- a stale mark_saved(2) racing in after the
        // newer mark_saved(3) would otherwise silently regress the mark.
        session.mark_saved(2);
        assert_eq!(
            session.persisted_revision,
            Some(3),
            "an older mark_saved must never regress a newer one"
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
    // Task 46: a native command that arrives as a REQUEST (a product
    // restore) is held to `execute`'s own bookkeeping -- a stale revision
    // applies nothing, a replayed commandId is a no-op, and the restore is
    // ONE undo step back to the pre-restore graph.
    #[test]
    fn an_internal_request_checks_revision_and_replay_and_undoes_in_one_step() {
        use crate::editor::commands::payloads::RestoreSnapshotPayload;

        let mut session = new_session();
        session
            .execute(&rename_request("s1", 1, "cmd-1", "Edited"), &no_context())
            .unwrap();
        let edited = session.project.clone();
        let mut frozen = minimal_project();
        frozen.title = "Frozen".to_string();
        let restore = InternalCommand::RestoreSnapshot(Box::new(RestoreSnapshotPayload {
            product_id: "prod-1".to_string(),
            project: frozen.clone(),
        }));

        let e = session
            .execute_internal_as(1, "cmd-2", &restore)
            .unwrap_err();
        assert_eq!(e.code, EditorErrorCode::RevisionConflict);
        assert_eq!(session.project, edited, "a stale restore applies nothing");
        assert_eq!(
            session
                .execute_internal_as(2, "bad id!", &restore)
                .unwrap_err()
                .code,
            EditorErrorCode::InvalidRequest
        );

        let snap = session.execute_internal_as(2, "cmd-2", &restore).unwrap();
        assert_eq!(snap.revision, 3);
        assert_eq!(session.project, frozen);
        let replay = session.execute_internal_as(2, "cmd-2", &restore).unwrap();
        assert_eq!(replay.revision, 3, "a replayed restore is a no-op");

        session
            .execute(
                &ExecuteRequest {
                    session_id: "s1".into(),
                    expected_revision: 3,
                    command_id: "cmd-3".into(),
                    command: EditorCommand::Undo,
                },
                &no_context(),
            )
            .unwrap();
        assert_eq!(
            session.project, edited,
            "undo returns to the pre-restore graph"
        );
    }
}
