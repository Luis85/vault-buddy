//! Tests for `save_commands` — every one runs the `AppHandle`-free halves
//! against a tempdir (`root` = the app's local data dir, staging beneath it
//! exactly as in production), the `session_commands_tests.rs` precedent.

use std::path::PathBuf;

use super::*;
use crate::editor::session_commands::{
    close_in, execute_in, open_staged_session, snapshot_in, CloseDisposition,
};
use vault_buddy_core::editor::commands::payloads::{
    RenamePayload, SplitClipPayload, TrimClipPayload,
};
use vault_buddy_core::editor::{EditorCommand, ExecuteRequest};
use vault_buddy_screen::staging::{self, StagedSidecar};

const BASE: &str = "2026-09-20 1432 Demo";

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
    fn stage(&self, sidecar: &StagedSidecar) {
        staging::write_sidecar(&self.staging(), &sidecar.base, sidecar).unwrap();
        std::fs::write(
            self.staging().join(staging::mp4_file_name(&sidecar.base)),
            b"not really an mp4",
        )
        .unwrap();
    }
    fn project_json_path(&self, id: &str) -> PathBuf {
        self.root()
            .join("editor-projects")
            .join(id)
            .join("project.json")
    }
}

// Asymmetric on purpose (`global-constraints.md`'s fixture-flaw rule): a
// non-16:9 width/height and a non-round duration, so a swapped axis or a
// dropped duration would show up.
fn sidecar(base: &str, vault_id: &str) -> StagedSidecar {
    StagedSidecar {
        base: base.to_string(),
        vault_id: vault_id.to_string(),
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

struct FailingWriter {
    error: fn() -> io::Error,
}

impl ProjectWriter for FailingWriter {
    fn write(&self, _path: &Path, _content: &str) -> io::Result<()> {
        Err((self.error)())
    }
}

fn boom() -> io::Error {
    io::Error::other("boom")
}

// `global-constraints.md`'s Known-host-facts test list: inject raw OS 112 on
// Windows, `StorageFull` elsewhere -- `map_write_error`'s own doc explains
// why both are checked rather than trusting one platform's mapping.
#[cfg(windows)]
fn disk_full_error() -> io::Error {
    io::Error::from_raw_os_error(112)
}
#[cfg(not(windows))]
fn disk_full_error() -> io::Error {
    io::Error::from(io::ErrorKind::StorageFull)
}

fn permission_denied_error() -> io::Error {
    io::Error::from(io::ErrorKind::PermissionDenied)
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
fn save_commits_the_acknowledged_revision() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();

    let renamed = execute_in(&state, &rename_request(&sid, 1, "cmd-1", "Tutorial")).unwrap();
    assert_eq!(renamed.snapshot.revision, 2);

    let receipt = save_project_in(&state, f.root(), &sid, 2).unwrap();
    assert_eq!(receipt.session_id, sid);
    assert_eq!(receipt.saved_revision, 2);
    assert_eq!(receipt.project_file_id, pid);

    let (on_disk, _sources) = load_project(f.root(), &pid).unwrap();
    assert_eq!(on_disk.record.revision, 2);
    assert_eq!(on_disk.project.title, "Tutorial");
    assert_eq!(on_disk.record.id, pid);
    assert!(on_disk.record.products.is_empty());

    let after = snapshot_in(&state, &sid).unwrap();
    assert_eq!(after.snapshot.persisted_revision, Some(2));
}

#[test]
fn save_with_a_stale_revision_is_a_conflict_and_writes_nothing() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();
    execute_in(&state, &rename_request(&sid, 1, "cmd-1", "Renamed")).unwrap();

    let path = f.project_json_path(&pid);
    let before = std::fs::read(&path).unwrap();

    // MUTATION CHECK: dropping the `snap.revision != expected_revision`
    // comparison in `save_project_with` makes this assertion fail -- the
    // stale save would succeed and silently overwrite project.json instead
    // of being refused.
    let err = save_project_in(&state, f.root(), &sid, 1).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::RevisionConflict);

    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        before, after,
        "a rejected save must never touch project.json"
    );

    let snap = snapshot_in(&state, &sid).unwrap();
    assert_eq!(
        snap.snapshot.persisted_revision,
        Some(1),
        "still only the open-time mark, revision 2 was never acknowledged"
    );
}

// A20: an injected write failure must leave the previous project.json
// byte-identical and must never advance persistedRevision.
#[test]
fn injected_write_failure_keeps_the_last_good_file() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();
    execute_in(&state, &rename_request(&sid, 1, "cmd-1", "Renamed")).unwrap();

    let path = f.project_json_path(&pid);
    let before = std::fs::read(&path).unwrap();

    let writer = FailingWriter { error: boom };
    let err = save_project_with(&writer, &state, f.root(), &sid, 2).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::Internal);

    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        before, after,
        "a failed write must leave project.json byte-identical (A20)"
    );

    let snap = snapshot_in(&state, &sid).unwrap();
    assert_eq!(
        snap.snapshot.persisted_revision,
        Some(1),
        "mark_saved must not run when the write failed"
    );
}

#[test]
fn disk_full_maps_to_disk_full() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();

    let writer = FailingWriter {
        error: disk_full_error,
    };
    let err = save_project_with(&writer, &state, f.root(), &sid, 1).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::DiskFull);
    assert!(err.retryable, "diskFull must be retryable");
}

// Not separately named in the brief's test list, but `map_write_error`'s
// other documented mapping -- cheap to pin alongside diskFull.
#[test]
fn permission_denied_maps_to_write_denied() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();

    let writer = FailingWriter {
        error: permission_denied_error,
    };
    let err = save_project_with(&writer, &state, f.root(), &sid, 1).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::WriteDenied);
}

// A03: split + trim + save + close + open must restore the exact edited
// graph, and none of it may touch the staged source file's own bytes.
#[test]
fn reopen_after_save_restores_clips_and_cues() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let mp4_path = f.staging().join(staging::mp4_file_name(BASE));
    let bytes_before = std::fs::read(&mp4_path).unwrap();

    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();
    assert_eq!(open.project.clips.len(), 1, "one clip from migration");
    let original_clip_id = open.project.clips[0].id.clone();

    let split = execute_in(
        &state,
        &ExecuteRequest {
            session_id: sid.clone(),
            expected_revision: 1,
            command_id: "cmd-split".into(),
            command: EditorCommand::SplitClip(SplitClipPayload {
                clip_id: original_clip_id.clone(),
                at_ms: 15_000,
            }),
        },
    )
    .unwrap();
    assert_eq!(
        split.project.clips.len(),
        2,
        "split must add exactly one clip"
    );
    let right_id = split
        .project
        .clips
        .iter()
        .find(|c| c.id != original_clip_id)
        .expect("the split's right half")
        .id
        .clone();

    let trimmed = execute_in(
        &state,
        &ExecuteRequest {
            session_id: sid.clone(),
            expected_revision: split.snapshot.revision,
            command_id: "cmd-trim".into(),
            command: EditorCommand::TrimClip(TrimClipPayload {
                clip_id: right_id,
                start_ms: 15_000,
                in_ms: 20_000,
                out_ms: 40_000,
            }),
        },
    )
    .unwrap();

    let receipt = save_project_in(&state, f.root(), &sid, trimmed.snapshot.revision).unwrap();
    assert_eq!(receipt.saved_revision, trimmed.snapshot.revision);

    close_in(&state, f.root(), &f.staging(), &sid, CloseDisposition::Keep).unwrap();

    let reopened = open_project_session(&state, f.root(), &pid).unwrap();
    assert_eq!(
        reopened.project, trimmed.project,
        "reopening a closed, saved project must restore the exact edited graph"
    );
    assert_eq!(reopened.source_base.as_deref(), Some(BASE));
    assert_eq!(
        reopened.snapshot.revision, trimmed.snapshot.revision,
        "reopen must resume at the saved revision, not reset to 1"
    );
    assert_eq!(
        reopened.snapshot.persisted_revision,
        Some(trimmed.snapshot.revision)
    );

    // Read the raw bytes, not a hash of them -- a byte-for-byte comparison
    // is at least as strong evidence as a SHA-256 match and needs no new
    // crate dependency for a test-only need.
    let bytes_after = std::fs::read(&mp4_path).unwrap();
    assert_eq!(
        bytes_before, bytes_after,
        "saving and reopening a project must never touch the staged source file"
    );
}

#[test]
fn edit_after_save_stays_dirty() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();

    let renamed = execute_in(&state, &rename_request(&sid, 1, "cmd-1", "First")).unwrap();
    assert_eq!(renamed.snapshot.revision, 2);

    save_project_in(&state, f.root(), &sid, 2).unwrap();

    let renamed_again = execute_in(&state, &rename_request(&sid, 2, "cmd-2", "Second")).unwrap();
    assert_eq!(renamed_again.snapshot.revision, 3);
    assert_eq!(
        renamed_again.snapshot.persisted_revision,
        Some(2),
        "saved at 2, edited again to 3 -- must read dirty"
    );

    let snap = snapshot_in(&state, &sid).unwrap();
    assert_eq!(snap.snapshot.persisted_revision, Some(2));
    assert_eq!(snap.snapshot.revision, 3);
}

#[test]
fn save_receipt_wire_literal() {
    let receipt = SaveReceipt {
        session_id: "ses-1".into(),
        saved_revision: 4,
        project_file_id: "proj-1".into(),
    };
    assert_eq!(
        serde_json::to_value(&receipt).unwrap(),
        serde_json::json!({
            "sessionId": "ses-1",
            "savedRevision": 4,
            "projectFileId": "proj-1",
        }),
    );
}

#[test]
fn use_recovery_true_is_refused() {
    let e = refuse_recovery_flag(true).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(refuse_recovery_flag(false).is_ok());
}

#[test]
fn list_projects_reflects_a_save_and_stays_sorted_newest_first() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();

    execute_in(&state, &rename_request(&sid, 1, "cmd-1", "Renamed")).unwrap();
    save_project_in(&state, f.root(), &sid, 2).unwrap();

    let rows = store_io::list_projects(f.root());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].project_file_id, pid);
    assert_eq!(rows[0].title, "Renamed");
    assert_eq!(rows[0].persisted_revision, 2);
}

// Fix round 1, finding 1: a `load_project` failure used to be silently
// swallowed (`Err(_) => {}`) regardless of WHY it failed, and the save
// proceeded to overwrite the file anyway. A corrupt/oversized project.json
// is `invalidProject` and must refuse the save outright -- the fixture is
// the exact oversized-padding shape `store_io`'s own
// `load_refuses_an_oversized_file` test uses, so this pins the SAVE path's
// reaction to the same failure store_io's read path already reports.
#[test]
fn a_corrupt_project_json_refuses_the_save_rather_than_overwriting_it() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();
    execute_in(&state, &rename_request(&sid, 1, "cmd-1", "Renamed")).unwrap();

    let path = f.project_json_path(&pid);
    let json = std::fs::read_to_string(&path).unwrap();
    let pad = (vault_buddy_core::editor::limits::MAX_PROJECT_JSON_BYTES as usize + 1)
        .saturating_sub(json.len());
    std::fs::write(&path, format!("{json}{}", " ".repeat(pad))).unwrap();
    let before = std::fs::read(&path).unwrap();

    // MUTATION CHECK: reverting to `Err(_) => (json!({}), now)` for every
    // error (rather than propagating `invalidProject`) makes this
    // assertion fail -- the save would succeed and silently clobber the
    // corrupt-but-still-on-disk file instead of refusing to touch it.
    let err = save_project_in(&state, f.root(), &sid, 2).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidProject);

    let after = std::fs::read(&path).unwrap();
    assert_eq!(
        before, after,
        "a refused save must never overwrite the corrupt file"
    );
}

// Fix round 1, finding 1's other branch: a project.json that has
// genuinely vanished out from under a live session (not corrupt, just
// gone) is the one case that may still degrade -- to a fresh `{}`
// workspace and a fresh `createdAt` -- rather than failing the save
// outright, since there is nothing left to refuse ON.
#[test]
fn a_missing_project_json_degrades_to_a_fresh_workspace_rather_than_failing() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();
    execute_in(&state, &rename_request(&sid, 1, "cmd-1", "Renamed")).unwrap();

    // Directory stays (so the write below can still land); only the file
    // this save would otherwise read the last-saved envelope from is gone.
    std::fs::remove_file(f.project_json_path(&pid)).unwrap();

    let receipt = save_project_in(&state, f.root(), &sid, 2).unwrap();
    assert_eq!(receipt.saved_revision, 2);

    let (on_disk, _sources) = load_project(f.root(), &pid).unwrap();
    assert_eq!(on_disk.workspace, serde_json::json!({}));
    assert!(
        chrono::DateTime::parse_from_rfc3339(&on_disk.record.created_at).is_ok(),
        "createdAt must degrade to a fresh, parseable timestamp: {:?}",
        on_disk.record.created_at
    );
}

// A writer that signals when it has been ENTERED (so a test can prove a
// concurrent caller is genuinely blocked behind it, not merely racing) and
// then blocks until told to proceed -- lets a test force the exact
// interleaving a real disk write's timing cannot be relied on to produce.
struct GatedWriter {
    entered: std::sync::mpsc::Sender<()>,
    proceed: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

impl ProjectWriter for GatedWriter {
    fn write(&self, path: &Path, content: &str) -> io::Result<()> {
        let _ = self.entered.send(());
        let _ = self.proceed.lock().unwrap().recv();
        RealWriter.write(path, content)
    }
}

// Fix round 1, finding 2: two concurrent saves on ONE session must not
// interleave their read-revision/commit/mark_saved sequence. This forces
// the exact race the fix's commit message describes -- A reads revision 2
// and is held (by `GatedWriter`) mid-write, an edit then advances the
// session to revision 3, and B (targeting that new revision) is spawned
// while A still holds the per-session save lock. Without the lock, B's
// read-through-write would race ahead of A's stalled one instead of
// blocking behind it.
#[test]
fn concurrent_saves_on_one_session_serialize_and_leave_persisted_revision_matching_disk() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();
    execute_in(&state, &rename_request(&sid, 1, "cmd-1", "First")).unwrap();

    let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
    let (proceed_tx, proceed_rx) = std::sync::mpsc::channel::<()>();
    let gated = GatedWriter {
        entered: entered_tx,
        proceed: std::sync::Mutex::new(proceed_rx),
    };

    std::thread::scope(|scope| {
        let a = std::thread::Builder::new()
            .name("editor-save-a".into())
            .spawn_scoped(scope, || {
                save_project_with(&gated, &state, f.root(), &sid, 2)
            })
            .unwrap();

        // Wait until A is inside its write -- i.e. holding the per-session
        // save lock -- before doing anything else.
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("A must enter its write");

        // While A still holds the lock: a concurrent edit (revision 2 ->
        // 3), then a second save racing for the SAME session, targeting
        // the NEW revision.
        execute_in(&state, &rename_request(&sid, 2, "cmd-2", "Second")).unwrap();
        let b = std::thread::Builder::new()
            .name("editor-save-b".into())
            .spawn_scoped(scope, || save_project_in(&state, f.root(), &sid, 3))
            .unwrap();

        // B must NOT be able to complete while A still holds the lock. Read
        // the flag BEFORE unblocking A, but assert on it only AFTER both
        // threads are joined -- if this ever panics before A is released
        // and joined, `thread::scope` would otherwise deadlock waiting to
        // join a thread parked forever on `proceed.recv()`.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let b_was_still_blocked = !b.is_finished();

        // Let A finish; only then can B proceed.
        proceed_tx.send(()).unwrap();
        let a_result = a.join().unwrap();
        let b_result = b.join().unwrap();
        let a_receipt = a_result.expect("A's save");
        let b_receipt = b_result.expect("B's save");
        assert!(
            b_was_still_blocked,
            "B must block behind A's per-session save lock, not race it"
        );
        assert_eq!(a_receipt.saved_revision, 2);
        assert_eq!(b_receipt.saved_revision, 3);
    });

    let (on_disk, _sources) = load_project(f.root(), &pid).unwrap();
    let snap = snapshot_in(&state, &sid).unwrap();
    assert_eq!(
        on_disk.record.revision, 3,
        "B's write (the later, unblocked save) must be what actually lands"
    );
    assert_eq!(
        snap.snapshot.persisted_revision,
        Some(on_disk.record.revision),
        "persistedRevision must always equal what is actually on disk, never a stale mark \
         left behind by an earlier save completing after a later one"
    );
}

// Controller ruling (Task 12 fix round 1): record.revision is monotonic
// across a project's WHOLE life, not just one session's -- reopening a
// project must resume its session at the SAVED revision, never reset to 1.
#[test]
fn record_revision_is_monotonic_across_a_close_and_reopen() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let pid = open.project.id.clone();

    let r2 = execute_in(&state, &rename_request(&sid, 1, "cmd-1", "First")).unwrap();
    assert_eq!(r2.snapshot.revision, 2);
    let r3 = execute_in(&state, &rename_request(&sid, 2, "cmd-2", "Second")).unwrap();
    assert_eq!(r3.snapshot.revision, 3);
    save_project_in(&state, f.root(), &sid, 3).unwrap();

    close_in(&state, f.root(), &f.staging(), &sid, CloseDisposition::Keep).unwrap();

    // MUTATION CHECK: minting the resumed session via `EditorSession::new`
    // (always revision 1) instead of `EditorSession::resume` makes this
    // assertion fail.
    let reopened = open_project_session(&state, f.root(), &pid).unwrap();
    assert_eq!(
        reopened.snapshot.revision, 3,
        "reopen must resume at the saved revision, not reset to 1"
    );
    assert_eq!(reopened.snapshot.persisted_revision, Some(3));

    let new_sid = reopened.snapshot.session_id.clone();
    let r4 = execute_in(&state, &rename_request(&new_sid, 3, "cmd-3", "Third")).unwrap();
    assert_eq!(r4.snapshot.revision, 4);
    save_project_in(&state, f.root(), &new_sid, 4).unwrap();

    let (on_disk, _sources) = load_project(f.root(), &pid).unwrap();
    assert_eq!(on_disk.record.revision, 4);

    let rows = store_io::list_projects(f.root());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].persisted_revision, 4);
}

// Controller ruling, the OTHER named path: `editor_open_staged` reopening
// an already-pinned, already-saved project (not just `editor_open_project`
// by id) must resume at the on-disk revision too -- `open_staged_in`'s
// "a pin naming a project that still exists reopens it" branch feeds the
// exact same `register_session` this task's fix changed.
#[test]
fn open_staged_resumes_a_pinned_project_at_its_saved_revision_not_1() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let first = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = first.snapshot.session_id.clone();

    execute_in(&state, &rename_request(&sid, 1, "cmd-1", "First")).unwrap();
    let r3 = execute_in(&state, &rename_request(&sid, 2, "cmd-2", "Second")).unwrap();
    assert_eq!(r3.snapshot.revision, 3);
    save_project_in(&state, f.root(), &sid, 3).unwrap();
    close_in(&state, f.root(), &f.staging(), &sid, CloseDisposition::Keep).unwrap();

    let reopened = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(
        reopened.snapshot.revision, 3,
        "reopening the same staged capture must resume the pinned project at its saved \
         revision, not reset to 1"
    );
    assert_eq!(reopened.snapshot.persisted_revision, Some(3));
}
