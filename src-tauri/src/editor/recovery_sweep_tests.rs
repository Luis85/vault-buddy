//! Tests for `recovery`'s two startup sweeps — the re-pin pass
//! (`run_startup_repin`, F34) and the abandoned-import / interrupted-publish
//! reports (`sweep_stale_imports`, `interrupted_publishes`, Tasks 39 and 48)
//! — split out of `recovery_tests.rs` at its LOC cap (hardening Task 10 fix
//! round 1). Deliberately independent of that file's own `Fixture`: these
//! tests exercise a different SEAM (the startup sweeps never touch a live
//! session's journal), so a small duplicated fixture here is simpler than
//! wiring cross-module visibility for one struct.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::editor::redact::redact_name;
use crate::editor::session_commands::open_staged_session;
use crate::editor::store_io::create_project;
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
        ..Default::default()
    }
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

// Task 48 (F36; ADR R13): a publish journal a crash left behind, not at
// `complete`, is REPORTED -- naming where the video landed and that its
// note did not -- and left exactly where it was: never deleted, never
// retried, so the next start reports it again. A finished publish's
// leftover journal directory is removed quietly: nothing was lost.
#[test]
fn interrupted_publish_is_reported_not_deleted() {
    let f = Fixture::new();
    let jobs = project_dir(f.root(), "proj-a").unwrap().join("jobs");
    let write = |job: &str, json: &str| {
        let dir = jobs.join(job);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("publish.json"), json).unwrap();
        dir.join("publish.json")
    };
    let video = write(
        "job-video",
        r#"{"step":"video","video":"Tutorials/Demo (2).mp4","note":"Tutorials/Demo (2).md"}"#,
    );
    let reserved = write(
        "job-reserved",
        r#"{"step":"reserved","video":"Tutorials/Other.mp4","note":null}"#,
    );
    let complete = write(
        "job-done",
        r#"{"step":"complete","video":"Tutorials/Done.mp4","note":null}"#,
    );
    let before = std::fs::read(&video).unwrap();

    let reports = interrupted_publishes(f.root());

    // M-V3 (hardening Task 11): the vault-relative names are handles.
    assert_eq!(
        reports,
        vec![
            format!(
                "A publish was interrupted before its video was saved as {}. A hidden \
                 partial copy may be left in that folder; publish it again.",
                redact_name("Tutorials/Other.mp4")
            ),
            format!(
                "A publish was interrupted: the video was saved as {} but its note was not.",
                redact_name("Tutorials/Demo (2).mp4")
            ),
        ]
    );
    assert_eq!(std::fs::read(&video).unwrap(), before, "never rewritten");
    assert!(reserved.is_file(), "never deleted");
    assert!(
        !complete.parent().unwrap().exists(),
        "a finished one is swept"
    );
    assert_eq!(interrupted_publishes(f.root()).len(), 2, "reported again");
}

// M-V3 (hardening Task 11): the report is logged on every start, and the
// journal's names are vault-relative -- a folder and the product's title.
// Every arm of the report names them only as `<name:#...>` handles.
#[test]
fn a_publish_report_names_no_vault_file() {
    let journal = |step, note: Option<&str>| PublishJournal {
        step,
        video: "Secret/Plan (2).mp4".to_string(),
        note: note.map(str::to_string),
    };
    let video = redact_name("Secret/Plan (2).mp4");
    let note = redact_name("Secret/Plan (2).md");
    let cases = [
        (
            journal(PublishStep::Reserved, None),
            format!(
                "A publish was interrupted before its video was saved as {video}. A hidden \
                 partial copy may be left in that folder; publish it again."
            ),
        ),
        (
            journal(PublishStep::Video, Some("Secret/Plan (2).md")),
            format!(
                "A publish was interrupted: the video was saved as {video} but its note was not."
            ),
        ),
        (
            journal(PublishStep::Note, Some("Secret/Plan (2).md")),
            format!(
                "A publish was interrupted after it saved the video as {video} and its note \
                 as {note}."
            ),
        ),
        (
            journal(PublishStep::Note, None),
            format!("A publish was interrupted after it saved the video as {video}."),
        ),
    ];
    for (journal, want) in cases {
        let shown = publish_report(&journal);
        assert_eq!(shown, want);
        assert!(!shown.contains("Secret") && !shown.contains('/'), "{shown}");
    }
}
