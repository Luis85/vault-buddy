//! Tests for `session_commands` — every one runs the `AppHandle`-free half
//! against a tempdir (`root` = the app's local data dir, staging beneath it
//! exactly as in production).

use super::*;
use vault_buddy_core::editor::commands::payloads::RenamePayload;
use vault_buddy_core::editor::EditorCommand;
use vault_buddy_screen::staging::StagedSidecar;

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
    fn staging(&self) -> std::path::PathBuf {
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
    fn sidecar(&self, base: &str) -> StagedSidecar {
        staging::read_sidecar(&self.staging().join(staging::sidecar_file_name(base))).unwrap()
    }
    fn project_dirs(&self) -> Vec<String> {
        match std::fs::read_dir(self.root().join("editor-projects")) {
            Ok(entries) => entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

// Asymmetric on purpose: a non-16:9 width/height and a non-round duration,
// so a swapped axis or a dropped duration shows up.
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

// A01: the destination vault comes from the capture's OWN sidecar, never
// from any store/UI state — there is none in this fixture to come from.
#[test]
fn open_staged_resolves_the_vault_from_the_sidecar() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let opened = open_staged_in(f.root(), &f.staging(), BASE).expect("opens");
    assert_eq!(opened.envelope.project.destination.vault, "vaultA");
    assert_eq!(
        opened.sources[STAGED_ASSET_ID].locator,
        SourceLocator::Staging {
            base: BASE.to_string()
        }
    );
    assert!(opened.sources[STAGED_ASSET_ID].has_audio);
    assert_eq!(
        pinned_project(&f.sidecar(BASE)).as_deref(),
        Some(opened.envelope.project.id.as_str()),
        "the sidecar must be pinned to the project it opened"
    );
}

// A duplicated `editor:open` (or a second Edit click) must never mint a
// second project for the same capture.
#[test]
fn open_staged_twice_returns_the_same_project() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let first = open_staged_in(f.root(), &f.staging(), BASE).unwrap();
    let second = open_staged_in(f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(first.envelope.project.id, second.envelope.project.id);
    assert_eq!(
        f.project_dirs().len(),
        1,
        "one directory under editor-projects"
    );
}

// A crash after `create_project` but before `pin_staged` leaves a project
// and an unpinned capture; the reopen must ADOPT that project rather than
// mint a second one beside it.
#[test]
fn open_staged_crash_between_create_and_pin_is_adopted_on_reopen() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let first = open_staged_in(f.root(), &f.staging(), BASE).unwrap();
    let pid = first.envelope.project.id.clone();
    // Simulate the crash: the pin never landed.
    unpin_staged(&f.staging(), BASE, &pid).unwrap();
    assert_eq!(pinned_project(&f.sidecar(BASE)), None);

    let reopened = open_staged_in(f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(reopened.envelope.project.id, pid);
    assert_eq!(f.project_dirs(), vec![pid.clone()]);
    assert_eq!(pinned_project(&f.sidecar(BASE)), Some(pid), "re-pinned");
}

#[test]
fn open_staged_refuses_an_unsafe_base() {
    let f = Fixture::new();
    let e = open_staged_in(f.root(), &f.staging(), "../x")
        .err()
        .expect("refused");
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(f.project_dirs().is_empty());
}

#[test]
fn open_staged_reports_a_missing_sidecar_as_source_missing() {
    let f = Fixture::new();
    let e = open_staged_in(f.root(), &f.staging(), BASE)
        .err()
        .expect("refused");
    assert_eq!(e.code, EditorErrorCode::SourceMissing);
}

// F7: a recovered capture (and, separately, one with no known length)
// migrated as-is would become an empty, permanently-pinned project whose
// capture could never be discarded. Each fixture trips exactly ONE of the
// two conditions.
#[test]
fn open_staged_refuses_a_recovered_or_zero_duration_sidecar() {
    let recovered = {
        let mut s = sidecar(BASE, "vaultA");
        s.extra
            .insert("recovered".into(), serde_json::Value::Bool(true));
        s
    };
    let zero = StagedSidecar {
        duration_ms: 0,
        ..sidecar(BASE, "vaultA")
    };
    for (what, s) in [("recovered", recovered), ("zero-duration", zero)] {
        let f = Fixture::new();
        f.stage(&s);
        let e = open_staged_in(f.root(), &f.staging(), BASE)
            .err()
            .unwrap_or_else(|| panic!("a {what} sidecar must be refused"));
        assert_eq!(e.code, EditorErrorCode::InvalidRequest, "{what}");
        assert_eq!(e.message, UNKNOWN_LENGTH, "{what}");
        assert!(f.project_dirs().is_empty(), "{what}: no project created");
        assert_eq!(
            pinned_project(&f.sidecar(BASE)),
            None,
            "{what}: nothing pinned"
        );
    }
}

#[test]
fn open_staged_session_reuses_a_live_session_and_reports_missing_media() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let a = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(a.source_base.as_deref(), Some(BASE));
    assert!(a.missing.is_empty(), "the staged mp4 is on disk");
    assert_eq!(a.snapshot.persisted_revision, Some(a.snapshot.revision));
    let b = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(
        a.snapshot.session_id, b.snapshot.session_id,
        "live session reused"
    );

    std::fs::remove_file(f.staging().join(staging::mp4_file_name(BASE))).unwrap();
    let missing = missing_media(
        f.root(),
        &a.project,
        &open_staged_in(f.root(), &f.staging(), BASE)
            .unwrap()
            .sources,
    );
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].asset_id, STAGED_ASSET_ID);
    assert_eq!(missing[0].name, "Demo window");
    assert_eq!(missing[0].expected_duration_ms, 61_500);
    assert_eq!(missing[0].expected_size, b"not really an mp4".len() as u64);
}

#[test]
fn execute_in_applies_to_the_named_session_and_refuses_a_stale_one() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let request = ExecuteRequest {
        session_id: open.snapshot.session_id.clone(),
        expected_revision: open.snapshot.revision,
        command_id: "cmd-1".into(),
        command: EditorCommand::Rename(RenamePayload {
            title: "Renamed".into(),
        }),
    };
    let p = execute_in(&state, &request).unwrap();
    assert_eq!(p.project.title, "Renamed");
    assert_eq!(p.snapshot.revision, open.snapshot.revision + 1);
    assert_eq!(snapshot_in(&state, &open.snapshot.session_id).unwrap(), p);

    let gone = ExecuteRequest {
        session_id: "ses-unknown".into(),
        ..request
    };
    assert_eq!(
        execute_in(&state, &gone).unwrap_err().code,
        EditorErrorCode::SessionGone
    );
}

#[test]
fn close_keep_drops_the_session_but_not_the_project() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    close_in(&state, f.root(), &f.staging(), &sid, CloseDisposition::Keep).unwrap();
    assert_eq!(
        snapshot_in(&state, &sid).unwrap_err().code,
        EditorErrorCode::SessionGone
    );
    assert!(lock_ignoring_poison(&state.by_project).is_empty());
    assert_eq!(f.project_dirs().len(), 1);
    assert!(pinned_project(&f.sidecar(BASE)).is_some());
}

// Discarding the EDIT must never delete what the user recorded (R6): the
// project goes, the pin is cleared, the staged capture stays.
#[test]
fn close_discard_project_removes_the_project_and_unpins_the_capture() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    close_in(
        &state,
        f.root(),
        &f.staging(),
        &sid,
        CloseDisposition::DiscardProject,
    )
    .unwrap();
    assert!(f.project_dirs().is_empty(), "project directory removed");
    assert_eq!(pinned_project(&f.sidecar(BASE)), None, "pin cleared");
    assert!(
        f.staging().join(staging::mp4_file_name(BASE)).is_file(),
        "the recording survives"
    );
    assert_eq!(
        snapshot_in(&state, &sid).unwrap_err().code,
        EditorErrorCode::SessionGone
    );
}

#[test]
fn close_discard_recovery_is_refused_until_it_exists_and_keeps_the_session() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    let e = close_in(
        &state,
        f.root(),
        &f.staging(),
        &sid,
        CloseDisposition::DiscardRecovery,
    )
    .unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(snapshot_in(&state, &sid).is_ok());
}

#[test]
fn close_disposition_decodes_the_wire_spelling() {
    for (wire, want) in [
        ("\"keep\"", CloseDisposition::Keep),
        ("\"discardRecovery\"", CloseDisposition::DiscardRecovery),
        ("\"discardProject\"", CloseDisposition::DiscardProject),
    ] {
        assert_eq!(
            serde_json::from_str::<CloseDisposition>(wire).unwrap(),
            want
        );
    }
    assert!(serde_json::from_str::<CloseDisposition>("\"discard_project\"").is_err());
}
