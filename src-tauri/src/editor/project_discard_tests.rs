//! `project_discard.rs`' tests (final whole-branch review I3): a real
//! tempdir store and staging directory, damaged in the ways a project can
//! stop opening.

use std::path::{Path, PathBuf};

use vault_buddy_screen::staging::{self, StagedSidecar};

use super::*;
use crate::editor::project_store::{pin_staged, pinned_project};
use crate::editor::session_commands::open_staged_session;

const BASE: &str = "2026-09-22 1105 Broken demo";
const OTHER: &str = "2026-09-22 1110 Another capture";

struct Fixture {
    root: tempfile::TempDir,
    state: EditorState,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(staging::staging_dir(root.path())).unwrap();
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

    fn stage(&self, base: &str) {
        let sidecar = StagedSidecar {
            base: base.to_string(),
            vault_id: "vaultA".into(),
            source_title: "Demo window".into(),
            source_kind: "window".into(),
            duration_ms: 52_900,
            width: 1680,
            height: 1050,
            recorded_at: "2026-09-22T11:05:00Z".into(),
            ..Default::default()
        };
        staging::write_sidecar(&self.staging(), base, &sidecar).unwrap();
        std::fs::write(
            self.staging().join(staging::mp4_file_name(base)),
            b"footage",
        )
        .unwrap();
    }

    /// A project opened on `base`, its session then closed again (by
    /// dropping the state), so only the files remain.
    fn project_for(&self, base: &str) -> String {
        self.stage(base);
        let state = EditorState::default();
        let open = open_staged_session(&state, self.root(), &self.staging(), base).unwrap();
        open.snapshot.project_id
    }

    fn dir(&self, project: &str) -> PathBuf {
        self.root().join("editor-projects").join(project)
    }

    fn pin_of(&self, base: &str) -> Option<String> {
        let path = self.staging().join(staging::sidecar_file_name(base));
        pinned_project(&staging::read_sidecar(&path).unwrap())
    }

    fn discard(&self, project: &str) -> Result<(), EditorError> {
        discard_project_in(&self.state, self.root(), &self.staging(), project)
    }
}

// An unreadable `sources.json` is the case that stranded a capture: the
// project would not open, the only discard needed a session, and the pin
// refused the capture's own Discard. The staging scan unpins what
// `sources.json` can no longer name.
#[test]
fn a_project_whose_sources_are_damaged_is_discarded_and_its_capture_unpinned() {
    let f = Fixture::new();
    let project = f.project_for(BASE);
    std::fs::write(f.dir(&project).join("sources.json"), b"{ damaged").unwrap();

    f.discard(&project).expect("the damaged project goes");

    assert!(!f.dir(&project).exists());
    assert_eq!(f.pin_of(BASE), None, "the capture is free to discard");
    assert!(
        f.staging().join(staging::mp4_file_name(BASE)).is_file(),
        "the recording is never touched (R6)"
    );
}

// A hand-edited pin on ANOTHER capture naming this project is cleared too
// (that capture's Discard was refused by it); a pin naming a different
// project is left alone.
#[test]
fn every_capture_pinned_to_the_project_is_unpinned_and_no_other() {
    let f = Fixture::new();
    let project = f.project_for(BASE);
    let other = f.project_for(OTHER);
    const THIRD: &str = "2026-09-22 1115 Third";
    f.stage(THIRD);
    pin_staged(&f.staging(), THIRD, &project).unwrap();

    f.discard(&project).unwrap();

    assert_eq!(f.pin_of(BASE), None);
    assert_eq!(f.pin_of(THIRD), None);
    assert_eq!(f.pin_of(OTHER), Some(other.clone()));
    assert!(f.dir(&other).join("project.json").is_file());
}

// A `project.json` that is JSON but no longer a valid project still says
// whose it is: its own `project.id` is the ownership proof `remove_project`
// needs, so it can be discarded.
#[test]
fn a_project_whose_project_json_no_longer_validates_is_discarded() {
    let f = Fixture::new();
    let project = f.project_for(BASE);
    let path = f.dir(&project).join("project.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["project"]["tracks"] = serde_json::json!("not a list");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

    f.discard(&project).expect("owned by its own id");
    assert!(!f.dir(&project).exists());
}

// Bytes that are not JSON at all prove nothing about whose folder this is,
// so nothing is removed and the refusal says so without a path.
#[test]
fn a_project_json_that_is_not_json_is_refused_and_left_in_place() {
    let f = Fixture::new();
    let project = f.project_for(BASE);
    std::fs::write(f.dir(&project).join("project.json"), b"\x00\x01garbage").unwrap();

    let e = f.discard(&project).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidProject);
    assert!(f.dir(&project).join("project.json").is_file());
    assert!(f.dir(&project).join("sources.json").is_file());
}

// A project a live session holds is discarded through that session (its
// jobs, takes and journal are stopped first), never behind its back.
#[test]
fn a_project_open_in_a_session_is_refused() {
    let f = Fixture::new();
    f.stage(BASE);
    let open = open_staged_session(&f.state, f.root(), &f.staging(), BASE).unwrap();
    let project = open.snapshot.project_id;

    let e = f.discard(&project).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert_eq!(
        e.message,
        "This project is open in the editor. Discard it from its project menu."
    );
    assert!(f.dir(&project).join("project.json").is_file());
    assert_eq!(f.pin_of(BASE), Some(project));
}

#[test]
fn an_invalid_or_unknown_id_is_refused() {
    let f = Fixture::new();
    assert_eq!(
        f.discard("../escape").unwrap_err().code,
        EditorErrorCode::InvalidRequest
    );
    assert_eq!(
        f.discard("proj-nothing-here").unwrap_err().code,
        EditorErrorCode::SourceMissing
    );
}

// GAP-214 item 8 (review N4): a folder whose `project.json` names ANOTHER
// project proves it is not this project's, so the discard is refused BEFORE
// any pin is released — the capture pinned to the folder's id keeps its pin
// — and the refusal is fixed role copy: no redaction handle, no parser text.
#[test]
fn a_folder_owned_by_another_id_is_refused_before_any_pin_is_released() {
    let f = Fixture::new();
    let project = f.project_for(BASE);
    let path = f.dir(&project).join("project.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["project"]["id"] = serde_json::json!("proj-someone-else");
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

    let e = f.discard(&project).unwrap_err();

    assert_eq!(e.code, EditorErrorCode::InvalidProject);
    assert_eq!(
        e.message,
        "This project could not be discarded because its files do not belong to it."
    );
    assert!(!e.message.contains("<path:#") && !e.message.contains("expected"));
    assert_eq!(
        f.pin_of(BASE),
        Some(project.clone()),
        "the pin is untouched"
    );
    assert!(path.is_file(), "nothing is removed");
}

// The same proof for bytes that are not JSON at all: the refusal is the
// fixed copy, never serde's "expected value at line 1" text, and the pin
// stays.
#[test]
fn a_project_json_that_is_not_json_keeps_the_pin_and_says_so_in_role_words() {
    let f = Fixture::new();
    let project = f.project_for(BASE);
    std::fs::write(f.dir(&project).join("project.json"), b"\x00\x01garbage").unwrap();

    let e = f.discard(&project).unwrap_err();

    assert_eq!(
        e.message,
        "This project could not be discarded because its files do not belong to it."
    );
    assert_eq!(f.pin_of(BASE), Some(project), "the pin is untouched");
}

// Carried from the Task 2/3 reviews: the pin scan read ANY failure to list
// staging as "nothing pinned", so the project was removed and a pin to it
// left behind. Only a staging folder that does not exist holds no pins; one
// that cannot be listed (here: a FILE where the folder should be) refuses
// the discard, project and pins untouched.
#[test]
fn a_staging_folder_that_cannot_be_listed_refuses_the_discard() {
    let f = Fixture::new();
    let project = f.project_for(BASE);
    let not_a_folder = f.root().join("staging-is-a-file");
    std::fs::write(&not_a_folder, b"not a folder").unwrap();

    let e = discard_project_in(&f.state, f.root(), &not_a_folder, &project).unwrap_err();

    assert_eq!(e.code, EditorErrorCode::Internal);
    assert_eq!(
        e.message,
        "The captures linked to this project could not be checked, so the project was kept. \
         Try again in a moment."
    );
    assert!(f.dir(&project).join("project.json").is_file());
    assert_eq!(f.pin_of(BASE), Some(project));
}

// ...while a staging folder that does not exist at all pins nothing, and
// the discard goes ahead.
#[test]
fn a_staging_folder_that_does_not_exist_pins_nothing() {
    let f = Fixture::new();
    let project = f.project_for(BASE);
    let missing = f.root().join("no-such-staging");

    discard_project_in(&f.state, f.root(), &missing, &project).expect("nothing is pinned");
    assert!(!f.dir(&project).exists());
}
