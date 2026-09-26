//! `webcam_recover.rs`' tests (GAP-197, hardening Task 9): a real tempdir
//! project opened through the two open paths, real part files aged by
//! their modification time, and a FAKE `TakeIo` (no ffmpeg) whose "remux"
//! copies the part and whose "probe" answers fixed facts.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use vault_buddy_core::editor::probe::ProbeFacts;
use vault_buddy_core::editor::{EditorError, EditorErrorCode, EditorOpenResult};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging::{self, StagedSidecar};

use crate::editor::project_store::{minimal_project, SourceLocator, SourceMediaKind, SourceRecord};
use crate::editor::recovery::{flush_due, RecoveryJournal, RECOVERY_SCHEMA};
use crate::editor::save_commands::open_project_session_with;
use crate::editor::session_close::close_in;
use crate::editor::session_commands::{open_staged_session_with, CloseDisposition};
use crate::editor::store_io::{create_project, load_sources, write_sources};
use crate::editor::webcam_finish::TakeIo;
use crate::editor::EditorState;

use super::*;

const PROJECT: &str = "proj1";
const TAKE_A: &str = "take-a1a1a1a1a1";
const TAKE_B: &str = "take-b2b2b2b2b2";

#[derive(Default)]
struct FakeIo<'a> {
    no_ffmpeg: bool,
    /// When set, the remux records whether this (the editor's `open` lock)
    /// was free while it ran.
    open: Option<&'a std::sync::Mutex<()>>,
    open_was_free: std::cell::Cell<Option<bool>>,
}

impl FakeIo<'_> {
    fn missing(&self) -> Result<(), EditorError> {
        if self.no_ffmpeg {
            return Err(EditorError::new(
                EditorErrorCode::EncoderUnavailable,
                "ffmpeg is not installed",
            ));
        }
        Ok(())
    }
}

impl TakeIo for FakeIo<'_> {
    fn remux(&self, part: &Path, out: &Path) -> Result<(), EditorError> {
        if let Some(open) = self.open {
            self.open_was_free.set(Some(open.try_lock().is_ok()));
        }
        self.missing()?;
        std::fs::copy(part, out)
            .map(|_| ())
            .map_err(|e| EditorError::new(EditorErrorCode::Internal, e.to_string()))
    }

    fn ready(&self) -> Result<(), EditorError> {
        self.missing()
    }

    fn probe(&self, _path: &Path) -> Result<ProbeFacts, EditorError> {
        Ok(ProbeFacts {
            duration_ms: 4_200,
            width: Some(640),
            height: Some(360),
            has_video: true,
            has_audio: true,
        })
    }
}

struct Fixture {
    root: tempfile::TempDir,
    state: EditorState,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        create_project(root.path(), &minimal_project(PROJECT), &BTreeMap::new()).unwrap();
        Self {
            root,
            state: EditorState::default(),
        }
    }

    fn takes(&self) -> PathBuf {
        let dir = self
            .root
            .path()
            .join("editor-projects")
            .join(PROJECT)
            .join("takes");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open(&self, io: &FakeIo<'_>) -> EditorOpenResult {
        open_project_session_with(&self.state, self.root.path(), PROJECT, false, io).unwrap()
    }

    fn journal(&self) -> PathBuf {
        self.root
            .path()
            .join("editor-projects")
            .join(PROJECT)
            .join("recovery.json")
    }

    /// A pre-crash journal: the unsaved edit is a renamed title.
    fn write_journal(&self, title: &str) -> Vec<u8> {
        let mut project = minimal_project(PROJECT);
        project.title = title.to_string();
        let journal = RecoveryJournal {
            schema: RECOVERY_SCHEMA.into(),
            session_revision: 3,
            saved_revision: Some(1),
            project,
        };
        let bytes = serde_json::to_vec_pretty(&journal).unwrap();
        std::fs::write(self.journal(), &bytes).unwrap();
        bytes
    }
}

fn part_name(take: &str) -> String {
    format!(".{take}.webm.part")
}

/// Write `bytes` at `path` last modified `age` ago.
fn write_aged(path: &Path, bytes: &[u8], age: Duration) {
    std::fs::write(path, bytes).unwrap();
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

const STALE: Duration = Duration::from_secs(120);

// GAP-197: a crash mid-take left the user's recording in a hidden `.part`
// that nothing ever offered back. On the project's next open it lands as
// "Webcam take N (recovered)" through the finish's own land + register —
// remuxed, recorded in `sources.json`, one asset — with the part and a
// leftover remux temp gone, and the open answers the session as it stands
// AFTER the recovery (or the webview's next edit meets a stale revision).
#[test]
fn a_stale_part_is_recovered_on_a_new_sessions_open() {
    let f = Fixture::new();
    let takes = f.takes();
    write_aged(&takes.join(part_name(TAKE_B)), b"EBML-older", STALE * 2);
    write_aged(&takes.join(part_name(TAKE_A)), b"EBML-newer", STALE);
    write_aged(&takes.join(format!(".{TAKE_A}.remux.webm")), b"half", STALE);

    let opened = f.open(&FakeIo::default());

    let names: Vec<(&str, &str)> = opened
        .project
        .assets
        .iter()
        .map(|a| (a.id.as_str(), a.name.as_str()))
        .collect();
    assert_eq!(
        names,
        vec![
            (TAKE_B, "Webcam take 1 (recovered)"),
            (TAKE_A, "Webcam take 2 (recovered)"),
        ],
        "numbered by the take rule, oldest first"
    );
    assert_eq!(opened.project.assets[0].duration_ms, 4_200);
    assert_eq!(
        std::fs::read(takes.join(format!("{TAKE_A}.webm"))).unwrap(),
        b"EBML-newer"
    );
    assert_eq!(
        std::fs::read(takes.join(format!("{TAKE_B}.webm"))).unwrap(),
        b"EBML-older"
    );
    for gone in [
        part_name(TAKE_A),
        part_name(TAKE_B),
        format!(".{TAKE_A}.remux.webm"),
    ] {
        assert!(!takes.join(&gone).exists(), "{gone} is gone");
    }
    let sources = load_sources(f.root.path(), PROJECT).unwrap();
    assert_eq!(
        sources[TAKE_A].locator,
        SourceLocator::Takes {
            file: format!("{TAKE_A}.webm")
        }
    );
    let live = lock_ignoring_poison(&f.state.sessions)[&opened.snapshot.session_id].snapshot();
    assert_eq!(
        opened.snapshot.revision, live.revision,
        "the open answers the recovered session"
    );
    assert_eq!(opened.snapshot.persisted_revision, Some(1));
    assert!(opened.snapshot.revision > 1, "unsaved until the user saves");
    assert!(
        lock_ignoring_poison(&f.state.takes.0).is_empty(),
        "a recovery leaves no pending take behind"
    );
}

// A part written in the last minute may belong to a take still being
// recorded (or one this process is finishing), and a REUSED session is not
// a new open — neither is touched.
#[test]
fn a_fresh_part_and_a_reused_session_recover_nothing() {
    let f = Fixture::new();
    let takes = f.takes();
    write_aged(&takes.join(part_name(TAKE_A)), b"live", Duration::ZERO);
    let first = f.open(&FakeIo::default());
    assert!(
        first.project.assets.is_empty(),
        "a fresh part is left alone"
    );
    assert_eq!(
        std::fs::read(takes.join(part_name(TAKE_A))).unwrap(),
        b"live"
    );

    write_aged(&takes.join(part_name(TAKE_B)), b"stale", STALE);
    let again = f.open(&FakeIo::default());
    assert_eq!(again.snapshot.session_id, first.snapshot.session_id);
    assert!(again.project.assets.is_empty(), "reuse recovers nothing");
    assert!(takes.join(part_name(TAKE_B)).is_file());
}

// No-follow: a symlink wearing a part's name is never read, moved or
// removed, and neither is what it points at.
#[test]
fn a_symlink_wearing_a_part_name_is_left_alone() {
    let f = Fixture::new();
    let takes = f.takes();
    let outside = f.root.path().join("outside.webm");
    write_aged(&outside, b"not ours", STALE);
    let link = takes.join(part_name(TAKE_A));
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&outside, &link);
    #[cfg(not(windows))]
    let made = std::os::unix::fs::symlink(&outside, &link);
    if let Err(e) = made {
        eprintln!("SKIP a_symlink_wearing_a_part_name_is_left_alone: cannot create a symlink: {e}");
        return;
    }
    let opened = f.open(&FakeIo::default());
    assert!(opened.project.assets.is_empty());
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&outside).unwrap(), b"not ours");
    assert!(!takes.join(format!("{TAKE_A}.webm")).exists());
}

/// A directory link — a junction on Windows, which needs no privilege.
fn dir_link(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|o| o.status.success());
    made
}

// No-follow at the folder too: a `takes` that is a link to a folder this
// app never made is not walked, and a directory link wearing a part's name
// inside a real `takes` is not a part.
#[test]
fn a_link_standing_in_for_the_takes_folder_or_a_part_is_left_alone() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    let precious = outside.path().join(part_name(TAKE_A));
    write_aged(&precious, b"not ours", STALE);
    let takes = f
        .root
        .path()
        .join("editor-projects")
        .join(PROJECT)
        .join("takes");
    if !dir_link(outside.path(), &takes) {
        eprintln!("SKIP a_link_standing_in_for_the_takes_folder_or_a_part_is_left_alone: no link");
        return;
    }
    let opened = f.open(&FakeIo::default());
    assert!(
        opened.project.assets.is_empty(),
        "a linked takes is not walked"
    );
    assert_eq!(std::fs::read(&precious).unwrap(), b"not ours");
    assert!(!outside.path().join(format!("{TAKE_A}.webm")).exists());

    let g = Fixture::new();
    let real = g.takes();
    assert!(dir_link(outside.path(), &real.join(part_name(TAKE_B))));
    let opened = g.open(&FakeIo::default());
    assert!(
        opened.project.assets.is_empty(),
        "a directory is not a part"
    );
    assert!(real.join(part_name(TAKE_B)).exists());
    assert_eq!(std::fs::read(&precious).unwrap(), b"not ours");
}

// A09 without ffmpeg: the recorded bytes BECOME the take's `.webm` and are
// registered with their length unknown — recorded as 0, never guessed.
#[test]
fn without_ffmpeg_the_interrupted_take_lands_raw_with_length_zero() {
    let f = Fixture::new();
    let takes = f.takes();
    write_aged(&takes.join(part_name(TAKE_A)), b"EBML-raw", STALE);
    let opened = f.open(&FakeIo {
        no_ffmpeg: true,
        ..FakeIo::default()
    });
    assert_eq!(opened.project.assets.len(), 1);
    assert_eq!(opened.project.assets[0].name, "Webcam take 1 (recovered)");
    assert_eq!(opened.project.assets[0].duration_ms, 0);
    assert_eq!(
        std::fs::read(takes.join(format!("{TAKE_A}.webm"))).unwrap(),
        b"EBML-raw"
    );
    assert!(!takes.join(part_name(TAKE_A)).exists());
    let record = &load_sources(f.root.path(), PROJECT).unwrap()[TAKE_A];
    assert_eq!(record.duration_ms, 0);
    assert_eq!(record.size, 8);
}

// Never a second copy of what is already there, never a record clobbered:
// a part whose take already landed (its `.webm` exists) or is already
// registered (a `sources.json` record — its file since lost) is left in
// place; an EMPTY stale part recorded nothing and is removed; a name that
// is not a take's own part is not ours.
#[test]
fn only_an_unregistered_nonempty_part_of_ours_is_recovered() {
    let f = Fixture::new();
    let takes = f.takes();
    const TAKE_C: &str = "take-c3c3c3c3c3";
    let record = SourceRecord {
        locator: SourceLocator::Takes {
            file: format!("{TAKE_C}.webm"),
        },
        sha256: Some("ab".repeat(32)),
        size: 77,
        duration_ms: 9_000,
        width: Some(320),
        height: Some(240),
        has_audio: true,
        has_video: true,
        media_kind: SourceMediaKind::Video,
        replaced_from: None,
    };
    let registered = BTreeMap::from([(TAKE_C.to_string(), record)]);
    write_sources(f.root.path(), PROJECT, &registered).unwrap();
    write_aged(&takes.join(part_name(TAKE_C)), b"lost-file", STALE);
    write_aged(&takes.join(part_name(TAKE_A)), b"dup", STALE);
    write_aged(&takes.join(format!("{TAKE_A}.webm")), b"landed", STALE);
    write_aged(&takes.join(part_name(TAKE_B)), b"", STALE);
    write_aged(&takes.join(".take-x.y.webm.part"), b"foreign", STALE);
    let opened = f.open(&FakeIo::default());
    assert!(opened.project.assets.is_empty());
    assert_eq!(
        std::fs::read(takes.join(part_name(TAKE_A))).unwrap(),
        b"dup"
    );
    assert_eq!(
        std::fs::read(takes.join(format!("{TAKE_A}.webm"))).unwrap(),
        b"landed"
    );
    assert!(!takes.join(part_name(TAKE_B)).exists(), "empty: removed");
    assert!(takes.join(".take-x.y.webm.part").is_file());
    assert_eq!(
        load_sources(f.root.path(), PROJECT).unwrap(),
        registered,
        "a registered take's record is never clobbered"
    );
    assert_eq!(
        std::fs::read(takes.join(part_name(TAKE_C))).unwrap(),
        b"lost-file"
    );
}

// Bounded: one open recovers at most `MAX_RECOVERED_PER_OPEN` takes (each
// a remux of its own); the rest wait, untouched, for the next new session.
#[test]
fn one_open_recovers_a_bounded_number_of_takes() {
    let f = Fixture::new();
    let takes = f.takes();
    let ids: Vec<String> = (0..=MAX_RECOVERED_PER_OPEN)
        .map(|i| format!("take-c{i:09}"))
        .collect();
    for (i, id) in ids.iter().enumerate() {
        write_aged(
            &takes.join(part_name(id)),
            b"take",
            STALE + Duration::from_secs(60 * (10 - i as u64)),
        );
    }
    let opened = f.open(&FakeIo::default());
    assert_eq!(opened.project.assets.len(), MAX_RECOVERED_PER_OPEN);
    let last = ids.last().unwrap();
    assert!(takes.join(part_name(last)).is_file(), "the newest waits");

    let sid = opened.snapshot.session_id.clone();
    let staging = staging::staging_dir(f.root.path());
    close_in(
        &f.state,
        f.root.path(),
        &staging,
        &sid,
        CloseDisposition::Keep,
    )
    .unwrap();
    // The recovered takes left the project unsaved, so Keep wrote the
    // journal: the next new session is the Resume one (review C1).
    let reopened =
        open_project_session_with(&f.state, f.root.path(), PROJECT, true, &FakeIo::default())
            .unwrap();
    assert_eq!(reopened.project.assets.len(), MAX_RECOVERED_PER_OPEN + 1);
    assert!(
        reopened.project.assets.iter().any(|a| &a.id == last),
        "the next new session recovers it"
    );
}

/// A staged capture (sidecar + `.mp4`) under `staging`; its base.
fn stage_capture(staging: &Path) -> &'static str {
    std::fs::create_dir_all(staging).unwrap();
    let base = "2026-09-20 1432 Demo";
    let sidecar = StagedSidecar {
        base: base.to_string(),
        vault_id: "vaultA".into(),
        source_title: "Demo window".into(),
        source_kind: "window".into(),
        duration_ms: 61_500,
        width: 1600,
        height: 900,
        recorded_at: "2026-09-20T14:32:00Z".into(),
        ..Default::default()
    };
    staging::write_sidecar(staging, base, &sidecar).unwrap();
    std::fs::write(staging.join(staging::mp4_file_name(base)), b"mp4").unwrap();
    base
}

// The staged-capture open path recovers too: a capture's project, closed,
// reopened through its capture's Edit after a crash left a take behind.
#[test]
fn the_staged_open_recovers_an_interrupted_take() {
    let root = tempfile::tempdir().unwrap();
    let staging = staging::staging_dir(root.path());
    let base = stage_capture(&staging);
    let state = EditorState::default();
    let io = FakeIo::default();
    let first = open_staged_session_with(&state, root.path(), &staging, base, &io).unwrap();
    let project_id = first.project.id.clone();
    close_in(
        &state,
        root.path(),
        &staging,
        &first.snapshot.session_id,
        CloseDisposition::Keep,
    )
    .unwrap();
    let takes = root
        .path()
        .join("editor-projects")
        .join(&project_id)
        .join("takes");
    std::fs::create_dir(&takes).unwrap();
    write_aged(&takes.join(part_name(TAKE_A)), b"EBML", STALE);

    // Review C1: a staged open never uses the journal, so while one exists
    // it recovers nothing (the offer comes first) and leaves it as it was.
    let journal = takes.with_file_name("recovery.json");
    std::fs::write(&journal, b"unsaved edits").unwrap();
    let held = open_staged_session_with(&state, root.path(), &staging, base, &io).unwrap();
    assert!(held.project.assets.iter().all(|a| a.id != TAKE_A));
    assert_eq!(std::fs::read(&journal).unwrap(), b"unsaved edits");
    assert!(takes.join(part_name(TAKE_A)).is_file());
    std::fs::remove_file(&journal).unwrap();
    close_in(
        &state,
        root.path(),
        &staging,
        &held.snapshot.session_id,
        CloseDisposition::Keep,
    )
    .unwrap();

    let reopened = open_staged_session_with(&state, root.path(), &staging, base, &io).unwrap();
    let take = reopened
        .project
        .assets
        .iter()
        .find(|a| a.id == TAKE_A)
        .expect("recovered");
    assert_eq!(take.name, "Webcam take 1 (recovered)");
    assert!(!takes.join(part_name(TAKE_A)).exists());
}

// Fix round 1 (review C1): the common crash is a take started over UNSAVED
// edits, so `recovery.json` is there too. A clean open (the webview has not
// yet offered Resume or Discard) must not recover into its session: the
// recovered asset would dirty it, the journal thread would overwrite the
// pre-crash journal within 500 ms, and a dirty open never shows the offer —
// the unsaved edits lost without a word. The take waits for the offer.
#[test]
fn a_clean_open_over_unsaved_changes_recovers_nothing() {
    let f = Fixture::new();
    let takes = f.takes();
    write_aged(&takes.join(part_name(TAKE_A)), b"EBML", STALE);
    let journal = f.write_journal("Before the crash");

    let opened = f.open(&FakeIo::default());
    assert!(opened.project.assets.is_empty(), "the take waits");
    assert_eq!(
        opened.snapshot.persisted_revision,
        Some(opened.snapshot.revision)
    );
    flush_due(&f.state, std::time::Instant::now() + Duration::from_secs(5));
    assert_eq!(
        std::fs::read(f.journal()).unwrap(),
        journal,
        "byte-identical"
    );
    assert_eq!(
        std::fs::read(takes.join(part_name(TAKE_A))).unwrap(),
        b"EBML"
    );
}

// Hardening Task 10 fix round 1 (review minor 9): `journal_present` checks
// only whether something wears `recovery.json`'s name, never whether it can
// be PARSED, so an UNREADABLE journal holds the take back exactly like a
// readable one — and stays held back after an ordinary open, since R7 means
// open time never touches it. It is only released once `discardRecovery`
// has genuinely moved it out of the way (not a test deleting it by hand),
// on the NEXT minted session.
#[test]
fn an_unreadable_journal_holds_the_take_back_until_discard_moves_it_aside() {
    let f = Fixture::new();
    let takes = f.takes();
    write_aged(&takes.join(part_name(TAKE_A)), b"EBML", STALE);
    let garbage = b"{ \"schema\": \"vault-buddy-recovery/1\", not json".to_vec();
    std::fs::write(f.journal(), &garbage).unwrap();

    let held = f.open(&FakeIo::default());
    assert!(held.project.assets.is_empty(), "the take waits");
    assert_eq!(
        std::fs::read(f.journal()).unwrap(),
        garbage,
        "R7: an ordinary open never touches the journal, readable or not"
    );
    assert!(takes.join(part_name(TAKE_A)).is_file());

    let staging = staging::staging_dir(f.root.path());
    close_in(
        &f.state,
        f.root.path(),
        &staging,
        &held.snapshot.session_id,
        CloseDisposition::DiscardRecovery,
    )
    .unwrap();
    assert!(
        !f.journal().exists(),
        "R7b: discard moves an unreadable journal aside, off its exact name"
    );
    let quarantined: Vec<PathBuf> = std::fs::read_dir(f.journal().parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("recovery.unreadable-"))
        })
        .collect();
    assert_eq!(quarantined.len(), 1, "{quarantined:?}");
    assert_eq!(std::fs::read(&quarantined[0]).unwrap(), garbage);

    let reopened = f.open(&FakeIo::default());
    let take = reopened
        .project
        .assets
        .iter()
        .find(|a| a.id == TAKE_A)
        .expect("recovered now that the journal is genuinely gone from that name");
    assert_eq!(take.name, "Webcam take 1 (recovered)");
    assert!(!takes.join(part_name(TAKE_A)).exists());
}

// …and Resume (the journal's working copy, a MINTED session) is where it
// comes back: the recovered take lands beside the pre-crash edits, and the
// next journal write carries both.
#[test]
fn resuming_unsaved_changes_recovers_the_take_beside_them() {
    let f = Fixture::new();
    let takes = f.takes();
    write_aged(&takes.join(part_name(TAKE_A)), b"EBML", STALE);
    f.write_journal("Before the crash");

    let resumed =
        open_project_session_with(&f.state, f.root.path(), PROJECT, true, &FakeIo::default())
            .unwrap();
    assert!(resumed.recovered);
    assert_eq!(resumed.project.title, "Before the crash");
    assert_eq!(resumed.project.assets.len(), 1);
    assert_eq!(resumed.project.assets[0].name, "Webcam take 1 (recovered)");
    assert!(!takes.join(part_name(TAKE_A)).exists());

    flush_due(&f.state, std::time::Instant::now() + Duration::from_secs(5));
    let written: RecoveryJournal =
        serde_json::from_slice(&std::fs::read(f.journal()).unwrap()).unwrap();
    assert_eq!(
        written.project.title, "Before the crash",
        "the edits survive"
    );
    assert!(written.project.assets.iter().any(|a| a.id == TAKE_A));
}

// Fix round 1 (review I1): `open` is the OUTERMOST lock, held for bounded
// I/O only (`EditorState`'s doc). A recovery remuxes through an external
// ffmpeg bounded at 15 minutes a take — held under `open` it would stall
// every other open, discard, import and sweep that long. The open answers
// the recovered session, but the remux runs with `open` released.
#[test]
fn a_recovery_runs_with_the_open_lock_released() {
    for use_recovery in [false, true] {
        let f = Fixture::new();
        write_aged(&f.takes().join(part_name(TAKE_A)), b"EBML", STALE);
        if use_recovery {
            f.write_journal("Before the crash");
        }
        let io = FakeIo {
            open: Some(&f.state.open),
            ..FakeIo::default()
        };
        let opened =
            open_project_session_with(&f.state, f.root.path(), PROJECT, use_recovery, &io).unwrap();
        assert_eq!(opened.project.assets.len(), 1, "recovered");
        assert_eq!(
            io.open_was_free.get(),
            Some(true),
            "the remux ran with `open` released (useRecovery: {use_recovery})"
        );
    }
    let root = tempfile::tempdir().unwrap();
    let staging = staging::staging_dir(root.path());
    let state = EditorState::default();
    let base = stage_capture(&staging);
    let first =
        open_staged_session_with(&state, root.path(), &staging, base, &FakeIo::default()).unwrap();
    let takes = root
        .path()
        .join("editor-projects")
        .join(&first.project.id)
        .join("takes");
    let sid = first.snapshot.session_id.clone();
    close_in(&state, root.path(), &staging, &sid, CloseDisposition::Keep).unwrap();
    std::fs::create_dir(&takes).unwrap();
    write_aged(&takes.join(part_name(TAKE_A)), b"EBML", STALE);
    let io = FakeIo {
        open: Some(&state.open),
        ..FakeIo::default()
    };
    open_staged_session_with(&state, root.path(), &staging, base, &io).unwrap();
    assert_eq!(io.open_was_free.get(), Some(true), "the staged open too");
}
