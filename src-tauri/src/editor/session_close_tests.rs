//! `session_close`'s tests: a session's `discardProject` against the other
//! writers of a staged capture's pin (review finding I-1, GAP-214 item 7).
//! Every one runs on a tempdir staged capture opened into a live session.

use std::path::PathBuf;
use std::sync::atomic::Ordering::SeqCst;
use std::time::{Duration, Instant};

use vault_buddy_core::editor::EditorErrorCode;
use vault_buddy_screen::staging::{self, StagedSidecar};

use super::*;
use crate::editor::discard::{is_closing, mark_closing};
use crate::editor::media_jobs::JobKind;
use crate::editor::project_store::pinned_project;
use crate::editor::save_commands::open_project_session;
use crate::editor::session_commands::open_staged_session;

const BASE: &str = "2026-09-25 1010 Close demo";

struct Fixture {
    root: tempfile::TempDir,
    state: EditorState,
    session: String,
    project: String,
}

impl Fixture {
    /// One staged capture, opened into a project and a live session (which
    /// pins the capture to the project).
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let staging = staging::staging_dir(root.path());
        std::fs::create_dir_all(&staging).unwrap();
        let sidecar = StagedSidecar {
            base: BASE.to_string(),
            vault_id: "vaultA".into(),
            source_title: "Demo window".into(),
            source_kind: "window".into(),
            inputs: vec!["mic-1".into()],
            duration_ms: 52_300,
            width: 1280,
            height: 720,
            recorded_at: "2026-09-25T10:10:00Z".into(),
            ..Default::default()
        };
        staging::write_sidecar(&staging, BASE, &sidecar).unwrap();
        std::fs::write(staging.join(staging::mp4_file_name(BASE)), b"footage").unwrap();
        let state = EditorState::default();
        let open = open_staged_session(&state, root.path(), &staging, BASE).unwrap();
        Self {
            root,
            state,
            session: open.snapshot.session_id,
            project: open.snapshot.project_id,
        }
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    fn staging(&self) -> PathBuf {
        staging::staging_dir(self.root.path())
    }

    fn project_dir(&self) -> PathBuf {
        self.root().join("editor-projects").join(&self.project)
    }

    fn discard(&self) -> Result<(), EditorError> {
        close_in(
            &self.state,
            self.root(),
            &self.staging(),
            &self.session,
            CloseDisposition::DiscardProject,
        )
    }

    fn pinned(&self) -> Option<String> {
        let sidecar =
            staging::read_sidecar(&self.staging().join(staging::sidecar_file_name(BASE))).unwrap();
        pinned_project(&sidecar)
    }
}

/// Poll `done` until it holds; a test failure (never a silent pass) after
/// five seconds.
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// I-1: every other writer of a pin and remover of a project holds
// `EditorState::open`; a session's discard did not. An open in progress
// (holding `open`) could land a pin the discard's unpin never saw, leaving
// the capture pinned to a project that no longer exists — which refuses
// the capture's own Discard. The discard now takes `open` before its
// closing mark, so while an open holds it the discard has done NOTHING:
// the pin is still there and the mark is not taken.
#[test]
fn a_session_discard_waits_for_an_open_in_progress() {
    let f = Fixture::new();
    let open_guard = lock_ignoring_poison(&f.state.open);
    std::thread::scope(|s| {
        let discard = std::thread::Builder::new()
            .name("t-discard".into())
            .spawn_scoped(s, || f.discard())
            .unwrap();
        // A handshake, not a sleep: the discard sets this immediately
        // before it locks `open`, which this thread holds.
        wait_until("the discard to reach the open lock", || {
            f.state.test_hooks.discard_waiting_for_open.load(SeqCst)
        });
        assert_eq!(
            f.pinned().as_deref(),
            Some(f.project.as_str()),
            "the discard unpinned while an open held the open lock"
        );
        assert!(
            !is_closing(&f.state, &f.session),
            "the closing mark was taken outside the open lock"
        );
        drop(open_guard);
        discard.join().unwrap().unwrap();
    });
    assert_eq!(f.pinned(), None, "the pin is cleared once the open is done");
    assert!(
        !f.project_dir().exists(),
        "the project directory is removed"
    );
}

// I-1's residual: an open must never hand back (and reuse) the live
// session of a project a discard is removing — that editor would be dead
// the moment the discard lands.
#[test]
fn an_open_of_a_project_being_discarded_is_refused() {
    let f = Fixture::new();
    let _mark = mark_closing(&f.state, &f.session).unwrap();
    let err = open_staged_session(&f.state, f.root(), &f.staging(), BASE).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert_eq!(err.message, "This project is being discarded.");
}

#[test]
fn an_open_by_id_of_a_project_being_discarded_is_refused() {
    let f = Fixture::new();
    let _mark = mark_closing(&f.state, &f.session).unwrap();
    for use_recovery in [false, true] {
        let err = open_project_session(&f.state, f.root(), &f.project, use_recovery).unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert_eq!(err.message, "This project is being discarded.");
    }
}

// GAP-214 item 7: the discard found the captures to unpin through the
// project's `sources.json` — so a project whose `sources.json` is gone or
// damaged could not be discarded, and its capture stayed pinned for good.
// The pin lives in the capture's sidecar; the discard now finds it there.
#[test]
fn a_session_discard_unpins_by_sidecar_when_sources_json_is_gone() {
    let f = Fixture::new();
    std::fs::remove_file(f.project_dir().join("sources.json")).unwrap();
    f.discard().expect("the discard does not need sources.json");
    assert_eq!(f.pinned(), None, "the capture is unpinned");
    assert!(
        !f.project_dir().exists(),
        "the project directory is removed"
    );
    assert!(
        f.staging().join(staging::mp4_file_name(BASE)).is_file(),
        "the recording survives"
    );
}

// GAP-214 item 8, the session half: ownership is proven before any pin is
// released, so a folder whose `project.json` names another project refuses
// the discard with the capture still pinned, in fixed words.
#[test]
fn a_session_discard_proves_ownership_before_it_unpins() {
    let f = Fixture::new();
    let path = f.project_dir().join("project.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["project"]["id"] = serde_json::json!("proj-someone-else");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

    let e = f.discard().unwrap_err();

    assert_eq!(e.code, EditorErrorCode::InvalidProject);
    assert_eq!(
        e.message,
        "This project could not be discarded because its files do not belong to it."
    );
    assert_eq!(f.pinned(), Some(f.project.clone()), "the pin is untouched");
    assert!(path.is_file());
}

// Fix round 1 (Task 4 review, Important 2): the ownership proof and the
// staging listing ran in `close_locked`, AFTER the quiesce had cancelled
// the session's renders and publishes — so a discard refused for a folder
// that is not the project's had already killed the user's render (GAP-214
// item 5: a refused discard cancels nothing). Both now run under `open`,
// before the closing mark.
#[test]
fn a_discard_refused_for_ownership_leaves_a_running_render_running() {
    let f = Fixture::new();
    let (_job, cancel) =
        crate::editor::media_jobs::start_job_in(&f.state, &f.session, JobKind::Render).unwrap();
    let path = f.project_dir().join("project.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["project"]["id"] = serde_json::json!("proj-someone-else");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

    let e = f.discard().unwrap_err();

    assert_eq!(
        e.message,
        "This project could not be discarded because its files do not belong to it."
    );
    assert!(
        !cancel.load(SeqCst),
        "a refused discard cancelled the render"
    );
    assert!(lock_ignoring_poison(&f.state.jobs).is_running(&f.session, JobKind::Render));
    assert!(!is_closing(&f.state, &f.session), "no closing mark is left");
    assert_eq!(f.pinned(), Some(f.project.clone()));
}

// ...and the same for a staging folder that cannot be listed (here: a file
// where the folder should be): refused before anything is cancelled.
#[test]
fn a_discard_refused_for_unlistable_staging_leaves_a_running_render_running() {
    let f = Fixture::new();
    let (_job, cancel) =
        crate::editor::media_jobs::start_job_in(&f.state, &f.session, JobKind::Render).unwrap();
    let not_a_folder = f.root().join("staging-is-a-file");
    std::fs::write(&not_a_folder, b"not a folder").unwrap();

    let e = close_in(
        &f.state,
        f.root(),
        &not_a_folder,
        &f.session,
        CloseDisposition::DiscardProject,
    )
    .unwrap_err();

    assert_eq!(
        e.message,
        "The captures linked to this project could not be checked, so the project was kept. \
         Try again in a moment."
    );
    assert!(
        !cancel.load(SeqCst),
        "a refused discard cancelled the render"
    );
    assert!(f.project_dir().join("project.json").is_file());
}

// C-2 (hardening Task 18): `execute_in` applies an edit under `sessions`,
// acknowledges it, and only THEN schedules its journal. A Keep close that
// runs in between used to find nothing pending, write nothing and drop the
// session -- an acknowledged edit gone. The close now moves the session out
// and journals whatever it holds. The edit is applied exactly the way
// `execute_in` applies it, and the close runs before its schedule would.
#[test]
fn keep_journals_an_edit_acknowledged_before_its_journal_was_scheduled() {
    use std::collections::BTreeSet;

    use vault_buddy_core::editor::commands::payloads::RenamePayload;
    use vault_buddy_core::editor::commands::CommandContext;
    use vault_buddy_core::editor::{EditorCommand, ExecuteRequest};

    use crate::editor::recovery::RecoveryJournal;

    let f = Fixture::new();
    {
        let mut sessions = lock_ignoring_poison(&f.state.sessions);
        let session = sessions.get_mut(&f.session).unwrap();
        let request = ExecuteRequest {
            session_id: f.session.clone(),
            expected_revision: session.snapshot().revision,
            command_id: "cmd-1".into(),
            command: EditorCommand::Rename(RenamePayload {
                title: "Acknowledged".into(),
            }),
        };
        let no_audio = BTreeSet::new();
        let ctx = CommandContext {
            assets_with_audio: &no_audio,
        };
        session.execute(&request, &ctx).unwrap();
    }
    assert!(!f.state.journal.is_pending(&f.session), "precondition");

    close_in(
        &f.state,
        f.root(),
        &f.staging(),
        &f.session,
        CloseDisposition::Keep,
    )
    .unwrap();

    let journal = std::fs::read(f.project_dir().join("recovery.json"))
        .expect("the acknowledged edit was journaled");
    let journal: RecoveryJournal = serde_json::from_slice(&journal).unwrap();
    assert_eq!(journal.project.title, "Acknowledged");
    assert_eq!(journal.session_revision, 2);
}
