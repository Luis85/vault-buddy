//! `discard.rs`' tests (final whole-branch review I1, C2, M3): a real
//! tempdir staged capture opened into a project, and discards racing a
//! take finish, an import and new work.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Mutex;

use vault_buddy_core::editor::probe::ProbeFacts;
use vault_buddy_screen::staging::{self, StagedSidecar};

use super::*;
use crate::editor::caption_commands::claim_caption_import;
use crate::editor::media_jobs::{start_job_in, JobPhase, JobReporter, JobTerminal, NoSubscriber};
use crate::editor::project_store::pinned_project;
use crate::editor::relink_commands::claim_relink;
use crate::editor::session_commands::{
    close_in, open_staged_session, snapshot_in, CloseDisposition,
};
use crate::editor::webcam_commands::{append_in, begin_in, finish_in, ChunkHeaders, TakeIo};

const BASE: &str = "2026-09-21 0915 Discard demo";

struct Fixture {
    root: tempfile::TempDir,
    state: EditorState,
    session: String,
    project: String,
}

impl Fixture {
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
            duration_ms: 43_700,
            width: 1440,
            height: 960,
            recorded_at: "2026-09-21T09:15:00Z".into(),
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

    /// A take with one accepted chunk, ready to finish at sequence 0.
    fn take_with_a_chunk(&self) -> String {
        let take = begin_in(
            &self.state,
            self.root(),
            &self.session,
            "video/webm",
            None,
            &GatedIo::open(),
        )
        .unwrap()
        .take_id;
        append_in(&self.state, &self.chunk(&take, 0), Ok(b"webm bytes")).unwrap();
        take
    }

    fn chunk(&self, take: &str, seq: u64) -> ChunkHeaders {
        ChunkHeaders {
            session_id: self.session.clone(),
            take_id: take.to_string(),
            seq,
        }
    }

    /// Everything the refusal must leave exactly as it was.
    fn assert_untouched(&self) {
        assert!(self.project_dir().join("project.json").is_file());
        assert!(self.project_dir().join("sources.json").is_file());
        assert_eq!(self.pinned().as_deref(), Some(self.project.as_str()));
        assert!(
            snapshot_in(&self.state, &self.session).is_ok(),
            "the session stays open for a retry"
        );
    }
}

/// A `TakeIo` whose remux waits for the test to open its gate — a finish
/// frozen mid-remux, the minutes a real `-c copy` of a long take takes.
struct GatedIo {
    gate: Mutex<Option<mpsc::Receiver<()>>>,
}

impl GatedIo {
    fn open() -> Self {
        Self {
            gate: Mutex::new(None),
        }
    }

    fn closed() -> (Self, mpsc::Sender<()>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                gate: Mutex::new(Some(rx)),
            },
            tx,
        )
    }
}

impl TakeIo for GatedIo {
    fn remux(&self, part: &Path, out: &Path) -> Result<(), EditorError> {
        if let Some(rx) = self.gate.lock().unwrap().take() {
            rx.recv().unwrap();
        }
        std::fs::copy(part, out)
            .map(|_| ())
            .map_err(|e| EditorError::new(EditorErrorCode::Internal, e.to_string()))
    }

    fn probe(&self, _path: &Path) -> Result<ProbeFacts, EditorError> {
        Ok(ProbeFacts {
            duration_ms: 3_100,
            width: Some(848),
            height: Some(480),
            has_video: true,
            has_audio: true,
        })
    }

    fn ready(&self) -> Result<(), EditorError> {
        Ok(())
    }
}

// I1: a finish holds its take's entry across the remux, writing into
// `takes\` with an ffmpeg handle that refuses deletion. A discard that did
// not wait removed the project under it — and, `project.json` going first,
// left a project nothing could open or discard.
#[test]
fn a_discard_refuses_while_a_take_is_finishing_and_succeeds_after() {
    let f = Fixture::new();
    let take = f.take_with_a_chunk();
    let (io, release) = GatedIo::closed();
    std::thread::scope(|scope| {
        let finishing = scope.spawn(|| finish_in(&f.state, f.root(), &io, &f.session, &take, 0));
        while f.state.takes.wait_idle(&f.session, Duration::ZERO) {
            std::thread::yield_now();
        }
        let discarded = f.discard();
        // Released BEFORE asserting, so a failing assertion never leaves
        // the finish frozen and the scope waiting on it forever.
        let untouched = f.project_dir().join("project.json").is_file();
        let pinned = f.pinned();
        release.send(()).unwrap();
        let landed = finishing.join().unwrap();
        let refused = discarded.expect_err("a discard must wait for the finish");
        assert_eq!(refused.code, EditorErrorCode::InvalidRequest);
        assert_eq!(
            refused.message,
            "A webcam take is still being saved. Wait for it to finish, then discard the project."
        );
        assert!(untouched && pinned.as_deref() == Some(f.project.as_str()));
        f.assert_untouched();
        landed.expect("the take lands");
    });
    f.discard()
        .expect("once the take landed, the discard goes through");
    assert!(!f.project_dir().exists());
    assert_eq!(f.pinned(), None);
}

// I1: an import copying a large file stops only between files. A discard
// cancels it, waits, and refuses when it has not stopped — never removing
// the project under the copy.
#[test]
fn a_discard_refuses_while_an_import_it_cannot_stop_is_running() {
    let f = Fixture::new();
    let (_job, cancel) = start_job_in(&f.state, &f.session, JobKind::Import).unwrap();
    let refused = f.discard().expect_err("the import has not stopped");
    assert_eq!(
        refused.message,
        "Media is still being copied into this project. Wait for the import to finish, then \
         discard the project."
    );
    assert!(
        cancel.load(std::sync::atomic::Ordering::SeqCst),
        "asked to stop"
    );
    f.assert_untouched();
}

#[test]
fn a_discard_waits_for_an_import_that_stops_then_removes_the_project() {
    let f = Fixture::new();
    let (job, cancel) = start_job_in(&f.state, &f.session, JobKind::Import).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !cancel.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::yield_now();
            }
            JobReporter::new(
                &f.state.jobs,
                &NoSubscriber,
                &f.session,
                &job,
                JobKind::Import,
            )
            .finish(JobPhase::Cancelled, JobTerminal::default());
        });
        f.discard().expect("the import stopped in time");
    });
    assert!(!f.project_dir().exists());
}

// C2: between the mark and the save lock, nothing new may start writing
// into the project the discard is about to remove.
#[test]
fn start_paths_refuse_a_session_that_is_being_discarded() {
    let f = Fixture::new();
    let take = f.take_with_a_chunk();
    let mark = mark_closing(&f.state, &f.session).unwrap();
    let discarding = "This project is being discarded, so nothing new can start in it.";
    for kind in [
        JobKind::Import,
        JobKind::Render,
        JobKind::Publish,
        JobKind::Peaks,
    ] {
        let e = start_job_in(&f.state, &f.session, kind).unwrap_err();
        assert_eq!((kind, e.message.as_str()), (kind, discarding));
    }
    let begin = begin_in(
        &f.state,
        f.root(),
        &f.session,
        "video/webm",
        None,
        &GatedIo::open(),
    );
    assert_eq!(begin.unwrap_err().message, discarding);
    assert_eq!(
        lock_ignoring_poison(&f.state.takes.0).len(),
        1,
        "the refused begin left no slot"
    );
    let append = append_in(&f.state, &f.chunk(&take, 1), Ok(b"more"));
    assert_eq!(append.unwrap_err().message, discarding);
    let finish = finish_in(&f.state, f.root(), &GatedIo::open(), &f.session, &take, 0);
    assert_eq!(finish.unwrap_err().message, discarding);
    assert_eq!(
        claim_relink(&f.state, &f.session).unwrap_err().message,
        discarding
    );
    assert!(lock_ignoring_poison(&f.state.relinks).is_empty());
    assert_eq!(
        claim_caption_import(&f.state, &f.session)
            .unwrap_err()
            .message,
        discarding
    );
    assert!(lock_ignoring_poison(&f.state.caption_imports).is_empty());
    let twice = mark_closing(&f.state, &f.session).err().unwrap();
    assert_eq!(twice.message, "This project is already being discarded.");
    drop(mark);
    assert!(start_job_in(&f.state, &f.session, JobKind::Peaks).is_ok());
}

// M3 (the doc was corrected, not the code): closing a SESSION — any
// disposition — ends its renders and publishes, because a render lands
// only in a live session (`render_jobs::publish` answers `cancelled` once
// the session is gone). What never cancels one is the window's X: the
// close guard's "Keep it running" only hides the window.
#[test]
fn closing_a_session_with_keep_cancels_its_render() {
    let f = Fixture::new();
    let (_job, cancel) = start_job_in(&f.state, &f.session, JobKind::Render).unwrap();
    close_in(
        &f.state,
        f.root(),
        &f.staging(),
        &f.session,
        CloseDisposition::Keep,
    )
    .unwrap();
    assert!(cancel.load(std::sync::atomic::Ordering::SeqCst));
    assert!(f.project_dir().join("project.json").is_file());
}

// M1: `drop_session` held `by_project` and `sessions` — the maps every
// command takes — across the take cleanup's unlinks (and the other leaf
// cleanups), against `EditorState`'s rule that the maps are never held
// across disk I/O. The maps now live in `unregister_session` alone, which
// touches nothing else.
#[test]
fn drop_session_releases_the_maps_before_any_cleanup() {
    use crate::structural_scan::{fn_body, production_code};
    let code = production_code(include_str!("session_commands.rs"));
    let drop_body = fn_body(&code, "fn drop_session(");
    for map in ["state.by_project", "state.sessions"] {
        assert!(
            !drop_body.contains(map),
            "drop_session must not lock {map} itself"
        );
    }
    assert!(drop_body.contains("unregister_session(state, session_id)"));
    let maps_body = fn_body(&code, "fn unregister_session(");
    for cleanup in [
        "takes",
        "journal",
        "jobs",
        "thumbnails",
        "save_locks",
        "std::fs",
    ] {
        assert!(
            !maps_body.contains(cleanup),
            "unregister_session must do nothing but the maps ({cleanup})"
        );
    }
}

// I1: `workspace.json` is written on every debounced view change. Written
// outside the save lock, it could land a temp file in the project folder
// while a discard (which holds that lock) was removing it — or recreate
// `workspace.json` in a folder the discard had just emptied.
#[test]
fn a_workspace_save_waits_for_a_discard_in_progress_and_then_finds_the_session_gone() {
    use crate::editor::prefs_commands::save_workspace_in;
    use crate::editor::save_commands::session_save_lock;
    let f = Fixture::new();
    let workspace = f.project_dir().join("workspace.json");
    let _ = std::fs::remove_file(&workspace);
    let lock = session_save_lock(&f.state, &f.session).unwrap();
    let discarding = lock_ignoring_poison(&lock);
    let (saved, early) = std::thread::scope(|scope| {
        let saving = scope
            .spawn(|| save_workspace_in(&f.state, f.root(), &f.session, serde_json::json!({})));
        std::thread::sleep(Duration::from_millis(200));
        let early = workspace.exists();
        // What the discard does under that lock: the folder goes, then the
        // session.
        crate::editor::store_io::remove_project(f.root(), &f.project).unwrap();
        lock_ignoring_poison(&f.state.sessions).remove(&f.session);
        drop(discarding);
        (saving.join().unwrap(), early)
    });
    assert!(
        !early,
        "the workspace was written under a discard in progress"
    );
    assert_eq!(saved.unwrap_err().code, EditorErrorCode::SessionGone);
    assert!(!f.project_dir().exists(), "nothing recreated the folder");
}
