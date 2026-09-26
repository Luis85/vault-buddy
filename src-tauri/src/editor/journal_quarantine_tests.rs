//! `journal_quarantine`'s tests (hardening Task 18, carried from Task 10's
//! re-review): the predecessor check runs once per session, and a journal
//! that cannot be set aside -- or cannot even be read -- is never
//! overwritten or deleted. Every one runs a real session over a tempdir
//! staged capture, through the same flush and close the app uses.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use vault_buddy_core::editor::commands::payloads::RenamePayload;
use vault_buddy_core::editor::{EditorCommand, EditorErrorCode, ExecuteRequest};
use vault_buddy_screen::staging::{self, StagedSidecar};

use crate::editor::project_store::project_dir;
use crate::editor::recovery::{flush_due, RecoveryJournal, JOURNAL_DEBOUNCE};
use crate::editor::save_commands::{open_project_session, save_project_in};
use crate::editor::session_close::close_in;
use crate::editor::session_commands::{
    execute_in, open_staged_session, snapshot_in, CloseDisposition,
};
use crate::editor::store_io::RECOVERY_FILE;
use crate::editor::EditorState;

const BASE: &str = "2026-09-26 0915 Journal demo";
const GARBAGE: &str = "{ \"schema\": \"vault-buddy-recovery/1\", not json";

struct Fixture {
    root: tempfile::TempDir,
    state: EditorState,
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
            width: 1600,
            height: 900,
            recorded_at: "2026-09-26T09:15:00Z".into(),
            ..Default::default()
        };
        staging::write_sidecar(&staging, BASE, &sidecar).unwrap();
        std::fs::write(staging.join(staging::mp4_file_name(BASE)), b"footage").unwrap();
        Self {
            root,
            state: EditorState::default(),
        }
    }

    fn root(&self) -> &Path {
        self.root.path()
    }

    fn staging(&self) -> PathBuf {
        staging::staging_dir(self.root())
    }

    fn dir(&self, pid: &str) -> PathBuf {
        project_dir(self.root(), pid).unwrap()
    }

    fn journal(&self, pid: &str) -> PathBuf {
        self.dir(pid).join(RECOVERY_FILE)
    }

    /// A session over `BASE`: `(sessionId, projectId)`.
    fn open(&self) -> (String, String) {
        let open = open_staged_session(&self.state, self.root(), &self.staging(), BASE).unwrap();
        (open.snapshot.session_id, open.project.id)
    }

    /// A project whose `recovery.json` is `GARBAGE` -- an earlier process's
    /// journal nobody can read -- reopened into a fresh session (which, by
    /// R7, left it exactly where it was).
    fn reopened_over_garbage(&self) -> (String, String) {
        let (sid, pid) = self.open();
        self.close(&sid, CloseDisposition::Keep).unwrap();
        std::fs::write(self.journal(&pid), GARBAGE).unwrap();
        let reopened = open_project_session(&self.state, self.root(), &pid, false).unwrap();
        (reopened.snapshot.session_id, pid)
    }

    fn rename(&self, sid: &str, cmd: &str, title: &str) {
        let revision = snapshot_in(&self.state, sid).unwrap().snapshot.revision;
        let request = ExecuteRequest {
            session_id: sid.to_string(),
            expected_revision: revision,
            command_id: cmd.to_string(),
            command: EditorCommand::Rename(RenamePayload {
                title: title.to_string(),
            }),
        };
        execute_in(&self.state, self.root(), &request).unwrap();
    }

    /// Run the journal worker's flush as if `rounds` debounce windows had
    /// passed -- a deadline, never a sleep.
    fn flush(&self, rounds: u32) {
        flush_due(
            &self.state,
            Instant::now() + JOURNAL_DEBOUNCE * rounds + Duration::from_millis(50),
        );
    }

    fn close(&self, sid: &str, disposition: CloseDisposition) -> Result<(), String> {
        close_in(&self.state, self.root(), &self.staging(), sid, disposition).map_err(|e| e.message)
    }

    fn set_aside(&self, pid: &str) -> Vec<String> {
        self.set_aside_as(pid, "recovery.unreadable-")
    }

    /// What sits beside `recovery.json` under `prefix`, blockers excluded.
    fn set_aside_as(&self, pid: &str, prefix: &str) -> Vec<String> {
        std::fs::read_dir(self.dir(pid))
            .unwrap()
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.starts_with(prefix) && !is_blocker(&e.path())
            })
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect()
    }

    /// R12: a project whose `recovery.json` is a READABLE journal an
    /// earlier session left behind ("Earlier", kept for later), reopened
    /// WITHOUT resuming it -- where a failed Resume's "Open saved project"
    /// lands. Returns the new session, the project and the journal's text.
    fn reopened_over_readable(&self) -> (String, String, String) {
        let (sid, pid) = self.open();
        self.rename(&sid, "cmd-0", "Earlier");
        self.close(&sid, CloseDisposition::Keep).unwrap();
        let earlier = std::fs::read_to_string(self.journal(&pid)).unwrap();
        let reopened = open_project_session(&self.state, self.root(), &pid, false).unwrap();
        (reopened.snapshot.session_id, pid, earlier)
    }

    fn journal_title(&self, pid: &str) -> String {
        let landed: RecoveryJournal =
            serde_json::from_slice(&std::fs::read(self.journal(pid)).unwrap()).unwrap();
        landed.project.title
    }
}

const BLOCKER: &[u8] = b"blocker";

const DISCARD_FAILED: &str = "The unsaved changes could not be discarded right now. Try again.";

fn is_blocker(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|b| b == BLOCKER)
}

/// Take every name a set-aside could use for the next few seconds, so it
/// exhausts its collision retries and FAILS -- deterministically, on every
/// platform (a sharing violation would also fail the replacing write).
fn block_set_aside(dir: &Path) -> Vec<PathBuf> {
    block_set_aside_as(dir, "recovery.unreadable")
}

/// `block_set_aside` for the names under `prefix`.
fn block_set_aside_as(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut taken = Vec::new();
    // A full minute ahead: a slow run cannot slip past it (and one that
    // did would fail these tests, never pass them silently).
    for secs in now..now + 60 {
        for attempt in 0..20u32 {
            let name = if attempt == 0 {
                format!("{prefix}-{secs}.json")
            } else {
                format!("{prefix}-{secs}-{attempt}.json")
            };
            let path = dir.join(name);
            std::fs::write(&path, BLOCKER).unwrap();
            taken.push(path);
        }
    }
    taken
}

// Carried item 1: the predecessor check parses the whole previous journal,
// under the save lock. It runs for a session's FIRST write only -- every
// later write replaces the session's own journal.
#[test]
fn the_predecessor_is_checked_once_per_session() {
    let f = Fixture::new();
    let (sid, pid) = f.open();
    for (n, title) in ["One", "Two", "Three"].iter().enumerate() {
        f.rename(&sid, &format!("cmd-{n}"), title);
        f.flush(1);
    }
    let landed: RecoveryJournal =
        serde_json::from_slice(&std::fs::read(f.journal(&pid)).unwrap()).unwrap();
    assert_eq!(landed.project.title, "Three");
    assert_eq!(
        f.state.test_hooks.predecessor_checks.load(Ordering::SeqCst),
        1,
        "three writes, one check"
    );
}

// Carried item 2: an unreadable journal that cannot be set aside is NEVER
// replaced. The write is skipped and left pending; once the set-aside can
// succeed, the next write moves it aside and lands.
#[test]
fn a_journal_that_cannot_be_set_aside_is_never_overwritten() {
    let f = Fixture::new();
    let (sid, pid) = f.reopened_over_garbage();
    let blockers = block_set_aside(&f.dir(&pid));

    f.rename(&sid, "cmd-1", "Tutorial");
    f.flush(1);

    assert_eq!(std::fs::read_to_string(f.journal(&pid)).unwrap(), GARBAGE);
    assert!(f.set_aside(&pid).is_empty());
    assert!(
        f.state.journal.is_pending(&sid),
        "the write waits for the next round"
    );

    for path in blockers {
        std::fs::remove_file(path).unwrap();
    }
    f.flush(3);
    assert_eq!(f.set_aside(&pid), [GARBAGE.to_string()]);
    let landed: RecoveryJournal =
        serde_json::from_slice(&std::fs::read(f.journal(&pid)).unwrap()).unwrap();
    assert_eq!(landed.project.title, "Tutorial");
}

// Carried items 2 and 4 on Discard: a set-aside that fails keeps the
// journal in place and says so -- Discard fails and the session stays open
// for a retry, instead of reporting a journal "kept aside" that was not.
#[test]
fn discard_recovery_that_cannot_set_an_unreadable_journal_aside_fails_and_keeps_it() {
    let f = Fixture::new();
    let (sid, pid) = f.reopened_over_garbage();
    block_set_aside(&f.dir(&pid));
    f.rename(&sid, "cmd-1", "Pending");

    let refused = f.close(&sid, CloseDisposition::DiscardRecovery);

    // Fix round 1: role wording, never a file handle.
    assert_eq!(refused, Err(DISCARD_FAILED.to_string()));
    assert_eq!(std::fs::read_to_string(f.journal(&pid)).unwrap(), GARBAGE);
    assert!(f.set_aside(&pid).is_empty());
    assert!(snapshot_in(&f.state, &sid).is_ok(), "still open to retry");
    // Fix round 1: the session stays open, so its pending journal write
    // must survive the refusal too.
    assert!(f.state.journal.is_pending(&sid));
}

// Carried item 3: a journal that could not even be READ is not deleted --
// nobody has looked at those bytes. Discard fails and can be retried.
// Here the file is held open by a handle that lets it be deleted but not
// read (`FILE_SHARE_DELETE` alone), the shape of a transient sharing
// violation that a delete would still get past.
#[cfg(windows)]
#[test]
fn discard_recovery_never_deletes_a_journal_it_could_not_read() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_DELETE: u32 = 0x4;

    let f = Fixture::new();
    let (sid, pid) = f.open();
    f.rename(&sid, "cmd-1", "Tutorial");
    f.flush(1);
    let before = std::fs::read(f.journal(&pid)).unwrap();
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_DELETE)
        .open(f.journal(&pid))
        .unwrap();

    let refused = f.close(&sid, CloseDisposition::DiscardRecovery);

    drop(held);
    assert_eq!(refused, Err(DISCARD_FAILED.to_string()));
    assert_eq!(std::fs::read(f.journal(&pid)).unwrap(), before);
    assert_eq!(
        snapshot_in(&f.state, &sid)
            .map(|p| p.snapshot.revision)
            .ok(),
        Some(2),
        "still open to retry"
    );
    // And the retry, once the file can be read, discards it.
    f.close(&sid, CloseDisposition::DiscardRecovery).unwrap();
    assert!(!f.journal(&pid).exists());
    assert_eq!(
        snapshot_in(&f.state, &sid).unwrap_err().code,
        EditorErrorCode::SessionGone
    );
}

// Fix round 1 (Important): a save of the current revision removes the
// journal -- but R7 holds there too. After "Open saved project" over an
// unreadable journal the session is clean and Save is enabled; the save
// used to DELETE those bytes. It now sets them aside, like the writer.
#[test]
fn a_save_sets_an_unreadable_journal_aside_instead_of_deleting_it() {
    let f = Fixture::new();
    let (sid, pid) = f.reopened_over_garbage();
    let revision = snapshot_in(&f.state, &sid).unwrap().snapshot.revision;

    save_project_in(&f.state, f.root(), &sid, revision).unwrap();

    assert!(!f.journal(&pid).exists(), "the offer is answered");
    assert_eq!(f.set_aside(&pid), [GARBAGE.to_string()]);
}

// ...and when it cannot be set aside, the save still succeeds and the
// bytes stay exactly where they are.
#[test]
fn a_save_keeps_an_unreadable_journal_it_cannot_set_aside() {
    let f = Fixture::new();
    let (sid, pid) = f.reopened_over_garbage();
    block_set_aside(&f.dir(&pid));
    let revision = snapshot_in(&f.state, &sid).unwrap().snapshot.revision;

    save_project_in(&f.state, f.root(), &sid, revision).unwrap();

    assert_eq!(std::fs::read_to_string(f.journal(&pid)).unwrap(), GARBAGE);
    assert!(f.set_aside(&pid).is_empty());
}

// ---- R12 (amends R7; final review I-2): a readable journal is replaced or
// deleted only by the session that RESUMED it or wrote it itself. A session
// minted over a journal it did not resume -- a failed Resume's "Open saved
// project", the one way to decline a readable journal -- sets it aside
// first, readable or not, so the Resume dialog's "Their file is kept in the
// project folder." holds for every failed Resume. ----

#[test]
fn a_first_write_sets_an_unresumed_readable_journal_aside() {
    let f = Fixture::new();
    let (sid, pid, earlier) = f.reopened_over_readable();

    f.rename(&sid, "cmd-1", "Mine");
    f.flush(1);

    assert_eq!(f.set_aside_as(&pid, "recovery.unresumed-"), [earlier]);
    assert!(f.set_aside(&pid).is_empty(), "it was never unreadable");
    assert_eq!(f.journal_title(&pid), "Mine");
}

#[test]
fn a_save_sets_an_unresumed_readable_journal_aside_instead_of_deleting_it() {
    let f = Fixture::new();
    let (sid, pid, earlier) = f.reopened_over_readable();
    let revision = snapshot_in(&f.state, &sid).unwrap().snapshot.revision;

    save_project_in(&f.state, f.root(), &sid, revision).unwrap();

    assert!(!f.journal(&pid).exists(), "the offer is answered");
    assert_eq!(f.set_aside_as(&pid, "recovery.unresumed-"), [earlier]);
}

// The paired negative: a session that RESUMED the journal owns it -- its
// writes replace it and a save removes it, nothing is set aside.
#[test]
fn a_resumed_session_replaces_and_removes_its_own_journal() {
    let f = Fixture::new();
    let (sid, pid) = f.open();
    f.rename(&sid, "cmd-0", "Earlier");
    f.close(&sid, CloseDisposition::Keep).unwrap();
    let resumed = open_project_session(&f.state, f.root(), &pid, true).unwrap();
    let sid = resumed.snapshot.session_id;

    f.rename(&sid, "cmd-1", "Mine");
    f.flush(1);
    assert_eq!(f.journal_title(&pid), "Mine");
    let revision = snapshot_in(&f.state, &sid).unwrap().snapshot.revision;
    save_project_in(&f.state, f.root(), &sid, revision).unwrap();

    assert!(!f.journal(&pid).exists());
    assert!(f.set_aside_as(&pid, "recovery.").is_empty());
}

// A staged capture's Edit that reopens the project mints a session over the
// same journal without resuming it: the same rule.
#[test]
fn a_staged_open_over_a_readable_journal_sets_it_aside_too() {
    let f = Fixture::new();
    let (sid, pid) = f.open();
    f.rename(&sid, "cmd-0", "Earlier");
    f.close(&sid, CloseDisposition::Keep).unwrap();
    let earlier = std::fs::read_to_string(f.journal(&pid)).unwrap();
    let (sid, _) = f.open();

    f.rename(&sid, "cmd-1", "Mine");
    f.flush(1);

    assert_eq!(f.set_aside_as(&pid, "recovery.unresumed-"), [earlier]);
}

// A set-aside that fails defers the write exactly like the unreadable case.
#[test]
fn an_unresumed_journal_that_cannot_be_set_aside_is_never_overwritten() {
    let f = Fixture::new();
    let (sid, pid, earlier) = f.reopened_over_readable();
    block_set_aside_as(&f.dir(&pid), "recovery.unresumed");

    f.rename(&sid, "cmd-1", "Mine");
    f.flush(1);

    assert_eq!(std::fs::read_to_string(f.journal(&pid)).unwrap(), earlier);
    assert!(f.state.journal.is_pending(&sid));
}

// Discard changes on a session that has not written its own journal yet
// would delete the earlier one it declined: kept aside instead...
#[test]
fn discard_changes_before_the_first_write_keeps_an_unresumed_journal() {
    let f = Fixture::new();
    let (sid, pid, earlier) = f.reopened_over_readable();
    f.rename(&sid, "cmd-1", "Mine");

    f.close(&sid, CloseDisposition::DiscardRecovery).unwrap();

    assert!(!f.journal(&pid).exists());
    assert_eq!(f.set_aside_as(&pid, "recovery.unresumed-"), [earlier]);
}

// ...while the recovery OFFER's own Discard (a clean session: the user
// chose to discard exactly that journal, A27) still deletes it.
#[test]
fn the_recovery_offers_discard_still_deletes_a_readable_journal() {
    let f = Fixture::new();
    let (sid, pid, _) = f.reopened_over_readable();

    f.close(&sid, CloseDisposition::DiscardRecovery).unwrap();

    assert!(!f.journal(&pid).exists());
    assert!(f.set_aside_as(&pid, "recovery.").is_empty());
}

// Final review M-4: Discard deletes on exactly the cases it knows -- a
// readable journal (or, `invalidRequest`, one that is absent) -- and treats
// every other error, today's or a future code, as a refusal.
#[test]
fn a_discard_never_deletes_on_an_unexpected_error() {
    use super::{discard_action, DiscardAction};
    use vault_buddy_core::editor::EditorError;
    let err = |code| EditorError::new(code, "x");
    for code in [
        EditorErrorCode::Internal,
        EditorErrorCode::DiskFull,
        EditorErrorCode::PermissionDenied,
        EditorErrorCode::WriteDenied,
    ] {
        assert_eq!(
            discard_action(Err(&err(code)), false),
            DiscardAction::Refuse
        );
    }
    let absent = err(EditorErrorCode::InvalidRequest);
    assert_eq!(discard_action(Err(&absent), false), DiscardAction::Remove);
    let bad = err(EditorErrorCode::InvalidProject);
    assert_eq!(
        discard_action(Err(&bad), false),
        DiscardAction::SetAside(super::UNREADABLE)
    );
    assert_eq!(discard_action(Ok(()), false), DiscardAction::Remove);
    assert_eq!(
        discard_action(Ok(()), true),
        DiscardAction::SetAside(super::UNRESUMED)
    );
}

// Final review M-3: the Resume dialog renders this message; serde's text
// (which names a hand-edited key) and a hand-edited schema string stay in
// the log, never in the message.
#[test]
fn an_unreadable_journal_is_reported_in_fixed_words() {
    let f = Fixture::new();
    let (sid, pid) = f.open();
    f.rename(&sid, "cmd-0", "Earlier");
    f.close(&sid, CloseDisposition::Keep).unwrap();
    let real = std::fs::read_to_string(f.journal(&pid)).unwrap();
    let cases = [
        (
            r#"{"schema":"vault-buddy-recovery/1","secretKey":1}"#.to_string(),
            "The unsaved changes could not be read.",
        ),
        (
            real.replace("vault-buddy-recovery/1", "secret-format"),
            "The unsaved changes use an unknown format.",
        ),
    ];
    for (text, message) in cases {
        std::fs::write(f.journal(&pid), text).unwrap();
        let refused = open_project_session(&f.state, f.root(), &pid, true).unwrap_err();
        assert_eq!(refused.code, EditorErrorCode::InvalidProject);
        assert_eq!(refused.message, message);
    }
}
