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
