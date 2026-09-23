//! Tests for `recovery` (Task 37 Part A) plus the save/close lock fixes
//! carried into this task from Task 12's review (they live here because
//! `save_commands_tests.rs` is at its LOC cap). Every one runs the
//! `AppHandle`-free halves against a tempdir laid out like production.

use std::io;
use std::time::{Duration, Instant};

use super::*;
use crate::editor::save_commands::{open_project_session, save_project_in, save_project_with};
use crate::editor::session_commands::{
    close_in, close_locked, execute_in, open_staged_session, snapshot_in, CloseDisposition,
};
use crate::editor::store_io::{create_project, ProjectWriter};
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::commands::payloads::RenamePayload;
use vault_buddy_core::editor::{EditorCommand, ExecuteRequest};
use vault_buddy_screen::staging::StagedSidecar;

const BASE: &str = "2026-09-20 1432 Demo";
const BASE2: &str = "2026-09-21 0915 Other";

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            root: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(f.staging()).unwrap();
        f
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
    fn staging(&self) -> PathBuf {
        staging::staging_dir(self.root.path())
    }
    fn stage(&self, base: &str) {
        staging::write_sidecar(&self.staging(), base, &sidecar(base)).unwrap();
        std::fs::write(self.mp4(base), b"not really an mp4").unwrap();
    }
    fn mp4(&self, base: &str) -> PathBuf {
        self.staging().join(staging::mp4_file_name(base))
    }
    fn sidecar_path(&self, base: &str) -> PathBuf {
        self.staging().join(staging::sidecar_file_name(base))
    }
    fn pin_of(&self, base: &str) -> Option<String> {
        pinned_project(&staging::read_sidecar(&self.sidecar_path(base)).unwrap())
    }
    fn journal(&self, pid: &str) -> PathBuf {
        journal_path(self.root(), pid).unwrap()
    }
    fn project_json(&self, pid: &str) -> PathBuf {
        project_dir(self.root(), pid).unwrap().join("project.json")
    }
}

// Asymmetric: non-16:9 dimensions and a non-round duration.
fn sidecar(base: &str) -> StagedSidecar {
    StagedSidecar {
        base: base.to_string(),
        vault_id: "vaultA".into(),
        source_title: "Demo window".into(),
        source_kind: "window".into(),
        inputs: vec!["mic-1".into()],
        duration_ms: 61_500,
        paused_ms: 0,
        width: 1600,
        height: 900,
        recorded_at: "2026-09-20T14:32:00Z".into(),
        timeline: None,
        extra: serde_json::Map::new(),
    }
}

fn rename(state: &EditorState, root: &Path, sid: &str, rev: u64, cmd: &str, title: &str) {
    let request = ExecuteRequest {
        session_id: sid.to_string(),
        expected_revision: rev,
        command_id: cmd.to_string(),
        command: EditorCommand::Rename(RenamePayload {
            title: title.to_string(),
        }),
    };
    execute_in(state, root, &request).unwrap();
}

/// A session over `BASE` with one acknowledged rename (revision 2), and
/// that rename's journal flushed to disk.
fn dirty_session_with_journal(f: &Fixture, state: &EditorState) -> (String, String) {
    f.stage(BASE);
    let open = open_staged_session(state, f.root(), &f.staging(), BASE).unwrap();
    let (sid, pid) = (open.snapshot.session_id.clone(), open.project.id.clone());
    rename(state, f.root(), &sid, 1, "cmd-1", "Tutorial");
    flush_due(
        state,
        Instant::now() + JOURNAL_DEBOUNCE + Duration::from_millis(50),
    );
    assert!(
        f.journal(&pid).is_file(),
        "precondition: the journal exists"
    );
    (sid, pid)
}

fn read_journal(f: &Fixture, pid: &str) -> RecoveryJournal {
    serde_json::from_slice(&std::fs::read(f.journal(pid)).unwrap()).unwrap()
}

// The debounce is part of the contract: nothing before the window closes,
// the acknowledged state once it has.
#[test]
fn journal_is_written_after_an_acknowledged_command() {
    let f = Fixture::new();
    f.stage(BASE);
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let (sid, pid) = (open.snapshot.session_id.clone(), open.project.id.clone());

    rename(&state, f.root(), &sid, 1, "cmd-1", "Tutorial");
    assert!(
        state.journal.is_pending(&sid),
        "the edit must schedule a journal"
    );
    flush_due(&state, Instant::now());
    assert!(
        !f.journal(&pid).exists(),
        "nothing may be written before the debounce window closes"
    );

    flush_due(
        &state,
        Instant::now() + JOURNAL_DEBOUNCE + Duration::from_millis(50),
    );
    let journal = read_journal(&f, &pid);
    assert_eq!(journal.schema, RECOVERY_SCHEMA);
    assert_eq!(journal.session_revision, 2);
    assert_eq!(journal.saved_revision, Some(1));
    assert_eq!(journal.project.title, "Tutorial");
    assert!(!state.journal.is_pending(&sid));
}

// The real `editor-journal` loop, not just `flush_due`: it must write on
// its own once the window closes.
#[test]
fn the_journal_worker_writes_without_an_explicit_flush() {
    let f = Fixture::new();
    f.stage(BASE);
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let (sid, pid) = (open.snapshot.session_id.clone(), open.project.id.clone());
    let stop = AtomicBool::new(false);
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .name("editor-journal-test".into())
            .spawn_scoped(s, || journal_worker_loop(&state, &stop))
            .unwrap();
        rename(&state, f.root(), &sid, 1, "cmd-1", "Tutorial");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f.journal(&pid).is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        stop.store(true, Ordering::SeqCst);
        state.journal.wake.notify_all();
    });
    assert!(
        f.journal(&pid).is_file(),
        "the worker never wrote the journal"
    );
}

#[test]
fn save_at_current_revision_removes_the_journal() {
    let f = Fixture::new();
    let state = EditorState::default();
    let (sid, pid) = dirty_session_with_journal(&f, &state);
    save_project_in(&state, f.root(), &sid, 2).unwrap();
    assert!(
        !f.journal(&pid).exists(),
        "a save of the current revision leaves nothing to recover"
    );
}

// Fix round 1 (review, Important): an edit acknowledged AFTER the save
// read the current revision (execute takes no save lock) must keep its
// pending journal write — the save must never forget a schedule it did not
// cause, or that edit is dirty in memory with no recovery.json behind it.
#[test]
fn an_edit_landing_after_the_saves_revision_read_keeps_its_journal() {
    let f = Fixture::new();
    let state = std::sync::Arc::new(EditorState::default());
    let (sid, pid) = dirty_session_with_journal(&f, &state);
    let (hook_state, hook_root, hook_sid) = (
        std::sync::Arc::clone(&state),
        f.root().to_path_buf(),
        sid.clone(),
    );
    crate::editor::save_commands::after_revision_read::set(move || {
        rename(
            &hook_state,
            &hook_root,
            &hook_sid,
            2,
            "cmd-racing",
            "Racing",
        );
    });

    save_project_in(&state, f.root(), &sid, 2).unwrap();

    assert_eq!(snapshot_in(&state, &sid).unwrap().snapshot.revision, 3);
    assert!(
        state.journal.is_pending(&sid),
        "the racing edit's journal write was dropped by the save"
    );
    flush_due(&state, Instant::now() + Duration::from_secs(5));
    assert_eq!(read_journal(&f, &pid).session_revision, 3);
}

/// A writer that lands one more acknowledged edit while the save's own
/// write is in flight — so the save commits an OLDER revision than the
/// session ends up at.
struct EditDuringWrite<'a> {
    state: &'a EditorState,
    root: &'a Path,
    sid: String,
}

impl ProjectWriter for EditDuringWrite<'_> {
    fn write(&self, path: &Path, content: &str) -> io::Result<()> {
        rename(self.state, self.root, &self.sid, 2, "cmd-late", "Later");
        write_atomic_replacing(path, content)
    }
}

// MUTATION CHECK target: deleting the journal on ANY save turns this red —
// revision 3 exists only in memory and in the journal.
#[test]
fn save_of_an_older_revision_keeps_the_journal() {
    let f = Fixture::new();
    let state = EditorState::default();
    let (sid, pid) = dirty_session_with_journal(&f, &state);
    let writer = EditDuringWrite {
        state: &state,
        root: f.root(),
        sid: sid.clone(),
    };
    let receipt = save_project_with(&writer, &state, f.root(), &sid, 2).unwrap();
    assert_eq!(receipt.saved_revision, 2);
    assert_eq!(snapshot_in(&state, &sid).unwrap().snapshot.revision, 3);
    assert!(
        f.journal(&pid).is_file(),
        "a save of an older revision must keep the journal"
    );
}

// A27: a malformed journal is reported, never "repaired" or replaced.
#[test]
fn malformed_journal_is_reported_and_kept() {
    let f = Fixture::new();
    f.stage(BASE);
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let pid = open.project.id.clone();
    close_in(
        &state,
        f.root(),
        &f.staging(),
        &open.snapshot.session_id,
        CloseDisposition::Keep,
    )
    .unwrap();
    let before = b"{ \"schema\": \"vault-buddy-recovery/1\", not json".to_vec();
    std::fs::write(f.journal(&pid), &before).unwrap();

    let e = open_project_session(&state, f.root(), &pid, true).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidProject);
    assert_eq!(std::fs::read(f.journal(&pid)).unwrap(), before);
    assert!(
        lock_ignoring_poison(&state.sessions).is_empty(),
        "a refused recovery must not open a session"
    );
}

// A journal naming another project is not this project's working copy.
#[test]
fn a_journal_for_another_project_is_refused() {
    let f = Fixture::new();
    let state = EditorState::default();
    let (sid, pid) = dirty_session_with_journal(&f, &state);
    close_in(&state, f.root(), &f.staging(), &sid, CloseDisposition::Keep).unwrap();
    let mut journal = read_journal(&f, &pid);
    journal.project.id = "someone-else".into();
    write_atomic_replacing(&f.journal(&pid), &serde_json::to_string(&journal).unwrap()).unwrap();
    let e = open_project_session(&state, f.root(), &pid, true).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidProject);
}

#[test]
fn open_with_recovery_starts_dirty_with_the_journals_project() {
    let f = Fixture::new();
    let state = EditorState::default();
    let (sid, pid) = dirty_session_with_journal(&f, &state);
    close_in(&state, f.root(), &f.staging(), &sid, CloseDisposition::Keep).unwrap();

    let reopened = open_project_session(&state, f.root(), &pid, true).unwrap();
    assert!(reopened.recovered);
    assert_eq!(reopened.project.title, "Tutorial");
    assert_eq!(reopened.snapshot.revision, 2);
    assert_eq!(reopened.snapshot.persisted_revision, Some(1));
    assert_eq!(reopened.source_base.as_deref(), Some(BASE));

    let plain = open_project_session(&EditorState::default(), f.root(), &pid, false).unwrap();
    assert!(!plain.recovered);
    assert_ne!(plain.project.title, "Tutorial", "false opens project.json");
}

#[test]
fn close_keep_flushes_the_pending_journal_synchronously() {
    let f = Fixture::new();
    f.stage(BASE);
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let (sid, pid) = (open.snapshot.session_id.clone(), open.project.id.clone());
    rename(&state, f.root(), &sid, 1, "cmd-1", "Tutorial");
    close_in(&state, f.root(), &f.staging(), &sid, CloseDisposition::Keep).unwrap();
    assert_eq!(read_journal(&f, &pid).session_revision, 2);
    assert!(!state.journal.is_pending(&sid));
}

#[test]
fn discard_recovery_deletes_only_the_journal_and_ends_the_session() {
    let f = Fixture::new();
    let state = EditorState::default();
    let (sid, pid) = dirty_session_with_journal(&f, &state);
    rename(&state, f.root(), &sid, 2, "cmd-2", "Pending");
    let project_before = std::fs::read(f.project_json(&pid)).unwrap();

    close_in(
        &state,
        f.root(),
        &f.staging(),
        &sid,
        CloseDisposition::DiscardRecovery,
    )
    .unwrap();
    flush_due(&state, Instant::now() + Duration::from_secs(5));

    assert!(!f.journal(&pid).exists(), "the journal must be gone");
    assert!(
        !state.journal.is_pending(&sid),
        "and must never be rewritten"
    );
    assert_eq!(std::fs::read(f.project_json(&pid)).unwrap(), project_before);
    assert_eq!(f.pin_of(BASE).as_deref(), Some(pid.as_str()));
    assert_eq!(
        snapshot_in(&state, &sid).unwrap_err().code,
        EditorErrorCode::SessionGone
    );
}

// Owned-file check: something that is not a plain file wearing the
// journal's name is refused, never recursed into.
#[test]
fn remove_journal_refuses_a_directory_wearing_the_name() {
    let f = Fixture::new();
    std::fs::create_dir_all(project_dir(f.root(), "proj1").unwrap()).unwrap();
    let path = journal_path(f.root(), "proj1").unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("keep.txt"), b"x").unwrap();
    assert!(remove_journal(f.root(), "proj1").is_err());
    assert!(path.join("keep.txt").is_file());
    remove_journal(f.root(), "proj2").expect("an absent journal is already gone");
}

// A clean session never journals: after a save at the current revision a
// late flush must not bring the journal back.
#[test]
fn a_clean_session_is_never_journaled() {
    let f = Fixture::new();
    let state = EditorState::default();
    let (sid, pid) = dirty_session_with_journal(&f, &state);
    save_project_in(&state, f.root(), &sid, 2).unwrap();
    note_acknowledged(&state, f.root(), &sid);
    flush_due(&state, Instant::now() + Duration::from_secs(5));
    assert!(!f.journal(&pid).exists());
}

#[test]
fn recovery_journal_wire_literal() {
    let journal = RecoveryJournal {
        schema: RECOVERY_SCHEMA.into(),
        session_revision: 5,
        saved_revision: Some(3),
        project: super::super::project_store::minimal_project("proj1"),
    };
    let v = serde_json::to_value(&journal).unwrap();
    assert_eq!(v["schema"], serde_json::json!("vault-buddy-recovery/1"));
    assert_eq!(v["sessionRevision"], serde_json::json!(5));
    assert_eq!(v["savedRevision"], serde_json::json!(3));
    assert_eq!(v["project"]["id"], serde_json::json!("proj1"));
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 4, "exactly the four journal keys: {keys:?}");
}

// F34.
#[test]
fn startup_sweep_repins_an_unpinned_project_whose_staged_base_still_exists() {
    let f = Fixture::new();
    f.stage(BASE);
    f.stage(BASE2);
    let a = open_staged_session(&EditorState::default(), f.root(), &f.staging(), BASE).unwrap();
    let b = open_staged_session(&EditorState::default(), f.root(), &f.staging(), BASE2).unwrap();
    let (pa, pb) = (a.project.id.clone(), b.project.id.clone());
    // A: the crash between create_project and pin_staged.
    unpin_for_test(&f, BASE);
    // B: a pin naming a project that no longer exists (a hand edit, or a
    // sidecar rewritten from an older copy).
    pin_staged(&f.staging(), BASE2, "ghost-project").unwrap();

    let report = run_startup_repin(f.root(), &f.staging());

    let mut want = vec![
        (pa.clone(), BASE.to_string()),
        (pb.clone(), BASE2.to_string()),
    ];
    want.sort();
    let mut got = report.repinned.clone();
    got.sort();
    assert_eq!(got, want);
    assert!(report.orphaned.is_empty(), "{:?}", report.orphaned);
    assert_eq!(f.pin_of(BASE).as_deref(), Some(pa.as_str()));
    assert_eq!(f.pin_of(BASE2).as_deref(), Some(pb.as_str()));
    // A second sweep finds nothing left to do.
    assert_eq!(
        run_startup_repin(f.root(), &f.staging()),
        RepinReport::default()
    );
}

fn unpin_for_test(f: &Fixture, base: &str) {
    let path = f.sidecar_path(base);
    let mut s = staging::read_sidecar(&path).unwrap();
    s.extra.remove("editorProjectId");
    staging::write_sidecar(&f.staging(), base, &s).unwrap();
}

// F34. MUTATION CHECK target: re-pinning without checking the staged base
// still exists turns this red through B, whose sidecar survived its video.
#[test]
fn startup_sweep_reports_without_repinning_when_the_staged_base_is_gone() {
    let f = Fixture::new();
    f.stage(BASE);
    f.stage(BASE2);
    let a = open_staged_session(&EditorState::default(), f.root(), &f.staging(), BASE).unwrap();
    let b = open_staged_session(&EditorState::default(), f.root(), &f.staging(), BASE2).unwrap();
    let (pa, pb) = (a.project.id.clone(), b.project.id.clone());
    // A: the whole staged capture is gone.
    std::fs::remove_file(f.mp4(BASE)).unwrap();
    std::fs::remove_file(f.sidecar_path(BASE)).unwrap();
    // B: its video is gone, a stale unpinned sidecar is left behind.
    unpin_for_test(&f, BASE2);
    std::fs::remove_file(f.mp4(BASE2)).unwrap();
    let b_sidecar_before = std::fs::read(f.sidecar_path(BASE2)).unwrap();

    let report = run_startup_repin(f.root(), &f.staging());

    assert!(report.repinned.is_empty(), "{:?}", report.repinned);
    let mut want = vec![pa.clone(), pb.clone()];
    want.sort();
    assert_eq!(report.orphaned, want);
    assert!(!f.sidecar_path(BASE).exists(), "no pin may be invented");
    assert_eq!(
        std::fs::read(f.sidecar_path(BASE2)).unwrap(),
        b_sidecar_before
    );
    assert!(
        project_dir(f.root(), &pa).unwrap().is_dir(),
        "never deleted"
    );
    assert!(
        project_dir(f.root(), &pb).unwrap().is_dir(),
        "never deleted"
    );
}

// Two projects claiming one capture: the one the sidecar already names
// keeps it; the other is reported, never allowed to steal the pin.
#[test]
fn startup_sweep_never_steals_a_pin_another_project_holds() {
    let f = Fixture::new();
    f.stage(BASE);
    let a = open_staged_session(&EditorState::default(), f.root(), &f.staging(), BASE).unwrap();
    let pa = a.project.id.clone();
    let (_, sources) = super::super::store_io::load_project(f.root(), &pa).unwrap();
    create_project(
        f.root(),
        &super::super::project_store::minimal_project("zz-second"),
        &sources,
    )
    .unwrap();

    let report = run_startup_repin(f.root(), &f.staging());

    assert!(report.repinned.is_empty());
    assert_eq!(report.orphaned, vec!["zz-second".to_string()]);
    assert_eq!(f.pin_of(BASE).as_deref(), Some(pa.as_str()));
}

// Fix round 1 (review, pin integrity): the pin names an EXISTING project
// whose sources.json cannot be read right now. It may well claim this
// capture, so the pin is left alone and the unpinned claimant is reported --
// an unreadable file is never proof the pin is free to take.
#[test]
fn startup_sweep_never_steals_a_pin_from_a_project_it_cannot_read() {
    let f = Fixture::new();
    f.stage(BASE);
    let a = open_staged_session(&EditorState::default(), f.root(), &f.staging(), BASE).unwrap();
    let pa = a.project.id.clone();
    let (_, sources) = super::super::store_io::load_project(f.root(), &pa).unwrap();
    create_project(
        f.root(),
        &super::super::project_store::minimal_project("zz-second"),
        &sources,
    )
    .unwrap();
    // The pin names A, whose sources.json is now unreadable.
    std::fs::write(
        project_dir(f.root(), &pa).unwrap().join("sources.json"),
        b"{ not json",
    )
    .unwrap();

    let report = run_startup_repin(f.root(), &f.staging());

    assert!(report.repinned.is_empty(), "{:?}", report.repinned);
    assert_eq!(report.orphaned, vec!["zz-second".to_string()]);
    assert_eq!(f.pin_of(BASE).as_deref(), Some(pa.as_str()));
}

// Carried from Task 12 (a): a project.json whose presence cannot even be
// CHECKED is not "absent" — the save must refuse rather than degrade and
// stamp a fresh envelope over whatever is there.
#[test]
fn a_save_refuses_when_the_project_file_cannot_be_checked() {
    let f = Fixture::new();
    f.stage(BASE);
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    // A root whose metadata lookup fails with something other than
    // NotFound: an invalid name on Windows, a file used as a directory on
    // Unix.
    #[cfg(windows)]
    let bad_root = f.root().join("bad<name");
    #[cfg(not(windows))]
    let bad_root = {
        let file = f.root().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        file.join("below")
    };
    let e = save_project_in(&state, &bad_root, &sid, 1).unwrap_err();
    assert!(
        e.message.contains("could not check"),
        "expected the existence check to refuse, got: {}",
        e.message
    );
}

// Carried from Task 12 (b): a save queued behind a discard must find the
// session gone — never degrade into a generic write failure against a
// directory the discard just removed.
#[test]
fn a_save_queued_behind_a_discard_finds_the_session_gone() {
    let f = Fixture::new();
    f.stage(BASE);
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let (sid, pid) = (open.snapshot.session_id.clone(), open.project.id.clone());
    let lock = session_save_lock(&state, &sid).unwrap();
    let guard = lock_ignoring_poison(&lock);
    let result = std::thread::scope(|s| {
        let save = std::thread::Builder::new()
            .name("editor-save-queued".into())
            .spawn_scoped(s, || save_project_in(&state, f.root(), &sid, 1))
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        close_locked(
            &state,
            f.root(),
            &f.staging(),
            &sid,
            &pid,
            CloseDisposition::DiscardProject,
        )
        .unwrap();
        drop(guard);
        save.join().unwrap()
    });
    assert_eq!(result.unwrap_err().code, EditorErrorCode::SessionGone);
}

// Carried from Task 12 (c): a session dropped between
// `session_save_lock`'s existence check and its insert must not leave an
// entry nothing will ever prune.
#[test]
fn session_save_lock_never_leaks_an_entry_for_a_session_dropped_mid_call() {
    let f = Fixture::new();
    f.stage(BASE);
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let map_guard = lock_ignoring_poison(&state.save_locks);
    let result = std::thread::scope(|s| {
        let t = std::thread::Builder::new()
            .name("editor-save-lock-race".into())
            .spawn_scoped(s, || session_save_lock(&state, &sid).map(|_| ()))
            .unwrap();
        std::thread::sleep(Duration::from_millis(200));
        lock_ignoring_poison(&state.sessions).remove(&sid);
        drop(map_guard);
        t.join().unwrap()
    });
    assert_eq!(result.unwrap_err().code, EditorErrorCode::SessionGone);
    assert!(
        !lock_ignoring_poison(&state.save_locks).contains_key(&sid),
        "an entry was minted for a session that no longer exists"
    );
}

// Carried from Task 12 (d): Keep and DiscardRecovery take the save lock,
// like DiscardProject — a close must never slip in beside an in-flight save.
#[test]
fn keep_and_discard_recovery_wait_for_the_save_lock() {
    for disposition in [CloseDisposition::Keep, CloseDisposition::DiscardRecovery] {
        let f = Fixture::new();
        let state = EditorState::default();
        let (sid, _pid) = dirty_session_with_journal(&f, &state);
        let lock = session_save_lock(&state, &sid).unwrap();
        let guard = lock_ignoring_poison(&lock);
        let (was_blocked, result) = std::thread::scope(|s| {
            let close = std::thread::Builder::new()
                .name("editor-close-waits".into())
                .spawn_scoped(s, || {
                    close_in(&state, f.root(), &f.staging(), &sid, disposition)
                })
                .unwrap();
            std::thread::sleep(Duration::from_millis(200));
            let was_blocked = !close.is_finished() && snapshot_in(&state, &sid).is_ok();
            drop(guard);
            (was_blocked, close.join().unwrap())
        });
        assert!(was_blocked, "{disposition:?} must wait for the save lock");
        result.unwrap_or_else(|e| panic!("{disposition:?}: {}", e.message));
        assert!(snapshot_in(&state, &sid).is_err());
    }
}

// Task 39: an import builds its project in `.<id>.importing` and renames it
// into place last, so a crash mid-import leaves that directory behind. The
// sweep removes only such directories, only once they are an hour old
// (an import running right now is younger), and nothing else in the store.
#[test]
fn stale_import_directories_are_swept_and_nothing_else() {
    let f = Fixture::new();
    let store = store_dir(f.root());
    let stale = store.join(".abc123.importing");
    std::fs::create_dir_all(stale.join("media")).unwrap();
    std::fs::write(stale.join("media").join("src.mp4"), b"half an import").unwrap();
    std::fs::write(stale.join("project.json"), b"{}").unwrap();
    let keep_dirs = [".abc123.importing.bak", "abc123", ".bad!id.importing"];
    for name in keep_dirs {
        std::fs::create_dir_all(store.join(name)).unwrap();
    }
    std::fs::write(store.join(".def456.importing"), b"a file, not ours").unwrap();

    let now = std::time::SystemTime::now();
    assert!(
        sweep_stale_imports(f.root(), now).is_empty(),
        "a fresh import is left alone"
    );
    assert!(stale.is_dir());

    let later = now + Duration::from_secs(2 * 60 * 60);
    assert_eq!(sweep_stale_imports(f.root(), later), [".abc123.importing"]);
    assert!(!stale.exists());
    for name in keep_dirs {
        assert!(store.join(name).is_dir(), "{name} is not an import's");
    }
    assert!(store.join(".def456.importing").is_file());
}
