//! Tests for `package_commands` — the EXPORT half (`editor_export_package`)
//! and one full round trip, against tempdirs with the native dialogs behind
//! the `PathChooser` seam. Fixtures: `package_test_support.rs`; the import
//! half is `package_import_tests.rs`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;
use vault_buddy_core::editor::{EditorErrorCode, InternalCommand};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

use super::*;
use crate::editor::package_test_support::*;
use crate::editor::prefs_commands::{read_workspace, save_workspace_in};
use crate::editor::project_store::{project_dir, SourceLocator};
use crate::editor::store_io::{load_project, load_sources};

#[test]
fn portable_export_then_import_reopens_identically() {
    let (a, session_id, project_id) = machine_a();
    save_workspace_in(
        &a.state,
        a.root(),
        &session_id,
        json!({"theme": "light", "playheadMs": 1234}),
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();
    let chooser = FakeChooser::saving_to(out.path().join("Demo"));
    let receipt = export(&a, &session_id, &chooser, PackageFormat::Portable)
        .unwrap()
        .expect("a receipt, the dialog was not cancelled");
    assert_eq!(
        receipt,
        PackageReceipt {
            session_id: session_id.clone(),
            saved_revision: 5,
            file_name: "Demo.vbproject.zip".to_string(),
            format: PackageFormat::Portable,
        }
    );
    let exported = a.project(&session_id);

    // Machine B holds a staged capture with the SAME base: the import must
    // still carry the packaged copy and never pin B's capture.
    let b = Machine::new();
    b.stage();
    let sidecar_before = b.sidecar_bytes();
    let opened = import(&b, out.path().join("Demo.vbproject.zip"))
        .unwrap()
        .expect("the dialog was not cancelled");
    assert_eq!(
        opened.snapshot.project_id, project_id,
        "a fresh store keeps the id"
    );
    assert_eq!(opened.snapshot.revision, 5);
    assert_eq!(opened.snapshot.persisted_revision, Some(5));
    assert_eq!(opened.project, exported);
    assert!(opened.missing.is_empty(), "{:?}", opened.missing);
    assert_eq!(opened.source_base, None);
    assert!(!opened.recovered);
    assert_eq!(b.sidecar_bytes(), sidecar_before, "import never pins");

    assert_eq!(media_file(&b, &project_id, "src.mp4"), SCREEN_BYTES);
    assert_eq!(media_file(&b, &project_id, "img1.png"), IMAGE_BYTES);
    let sources = load_sources(b.root(), &project_id).unwrap();
    let locators: BTreeMap<&str, &SourceLocator> = sources
        .iter()
        .map(|(k, v)| (k.as_str(), &v.locator))
        .collect();
    assert_eq!(
        locators,
        BTreeMap::from([
            (
                "img1",
                &SourceLocator::Media {
                    file: "img1.png".into()
                }
            ),
            (
                "src",
                &SourceLocator::Media {
                    file: "src.mp4".into()
                }
            ),
        ]),
        "the staging base became a packaged Media file; the card has no source"
    );
    assert!(sources.values().all(|r| r.sha256.is_some()));
    let (envelope, _) = load_project(b.root(), &project_id).unwrap();
    assert_eq!(envelope.project, exported);
    assert_eq!(envelope.record.revision, 5);
    assert_eq!(
        read_workspace(b.root(), &project_id).unwrap(),
        read_workspace(a.root(), &project_id).unwrap()
    );
    assert_eq!(
        opened.workspace.theme,
        Some(vault_buddy_core::editor::Theme::Light)
    );
    assert_eq!(b.store_entries(), vec![project_id], "no .importing is left");
}

#[test]
fn export_refuses_to_replace_an_unrelated_file() {
    let (a, session_id, _) = machine_a();
    let out = tempfile::tempdir().unwrap();
    // Positive control: our own earlier export IS replaced.
    let ours = out.path().join("Ours.vbproject.zip");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(ours.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();
    let first = std::fs::read(&ours).unwrap();
    a.execute(
        &session_id,
        json!({"kind": "rename", "title": "Renamed again"}),
    );
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(ours.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .expect("replacing this project's own file is allowed");
    assert_ne!(
        std::fs::read(&ours).unwrap(),
        first,
        "the file was replaced"
    );

    let unrelated = out.path().join("notes.vbproject.zip");
    std::fs::write(&unrelated, b"somebody else's notes").unwrap();
    let err = export(
        &a,
        &session_id,
        &FakeChooser::saving_to(unrelated.clone()),
        PackageFormat::Portable,
    )
    .expect_err("an unrelated file is never replaced");
    assert_eq!(err.code, EditorErrorCode::WriteDenied);
    assert_eq!(
        err.message,
        "Choose a new name — that file is not this project"
    );
    assert_eq!(std::fs::read(&unrelated).unwrap(), b"somebody else's notes");

    // A valid project file of ANOTHER project is just as unrelated.
    let (other, other_session, _) = machine_a();
    let theirs = out.path().join("theirs.vbproject.json");
    export(
        &other,
        &other_session,
        &FakeChooser::saving_to(theirs.clone()),
        PackageFormat::Lightweight,
    )
    .unwrap()
    .unwrap();
    let theirs_before = std::fs::read(&theirs).unwrap();
    let err = export(
        &a,
        &session_id,
        &FakeChooser::saving_to(theirs.clone()),
        PackageFormat::Lightweight,
    )
    .expect_err("another project's file is never replaced");
    assert_eq!(err.code, EditorErrorCode::WriteDenied);
    assert_eq!(std::fs::read(&theirs).unwrap(), theirs_before);

    let mut left: Vec<String> = std::fs::read_dir(out.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "Ours.vbproject.zip",
            "notes.vbproject.zip",
            "theirs.vbproject.json"
        ],
        "no temp file is left behind"
    );
}

#[test]
fn oversized_portable_export_suggests_lightweight() {
    let (a, session_id, project_id) = machine_a();
    let image = project_dir(a.root(), &project_id)
        .unwrap()
        .join("media")
        .join("img1.png");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&image)
        .unwrap()
        .set_len(limits::MAX_PACKAGE_MEDIA_BYTES)
        .unwrap();
    let out = tempfile::tempdir().unwrap();
    let chooser = FakeChooser::saving_to(out.path().join("Big"));
    let err = export(&a, &session_id, &chooser, PackageFormat::Portable)
        .expect_err("past the 200 MiB media limit");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert_eq!(
        err.message,
        "This project is too large for a portable file (limit 200 MiB). Save a lightweight copy instead."
    );
    assert_eq!(
        chooser.asked.get(),
        0,
        "refused before asking where to save"
    );
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    export(&a, &session_id, &chooser, PackageFormat::Lightweight)
        .unwrap()
        .expect("the lightweight copy still saves");
}

#[test]
fn a_cancelled_dialog_writes_nothing_and_answers_null() {
    let (a, session_id, _) = machine_a();
    let chooser = FakeChooser {
        save: None,
        open: None,
        asked: Cell::new(0),
    };
    assert_eq!(
        export(&a, &session_id, &chooser, PackageFormat::Portable).unwrap(),
        None
    );
    assert!(import_package_in(&a.state, a.root(), &chooser)
        .unwrap()
        .is_none());
    assert_eq!(chooser.asked.get(), 2);
}

#[test]
fn a_stale_revision_is_a_conflict_before_any_dialog() {
    let (a, session_id, _) = machine_a();
    let chooser = FakeChooser::saving_to(PathBuf::from("unused"));
    let err = export_package_in(
        &a.state,
        a.root(),
        &chooser,
        &session_id,
        1,
        PackageFormat::Portable,
    )
    .expect_err("revision 1 is stale");
    assert_eq!(err.code, EditorErrorCode::RevisionConflict);
    assert_eq!(chooser.asked.get(), 0);
}

#[test]
fn case_colliding_asset_ids_are_refused_up_front() {
    let (a, session_id, _) = machine_a();
    let clash = serde_json::from_value::<vault_buddy_core::editor::Asset>(json!({
        "id": "IMG1", "kind": "audio", "name": "clash.wav", "duration_ms": 1000
    }))
    .unwrap();
    lock_ignoring_poison(&a.state.sessions)
        .get_mut(&session_id)
        .unwrap()
        .execute_internal(&InternalCommand::AddAssets(
            vault_buddy_core::editor::commands::payloads::AddAssetsPayload {
                assets: vec![clash],
            },
        ))
        .unwrap();
    let chooser = FakeChooser::saving_to(PathBuf::from("unused"));
    let err = export(&a, &session_id, &chooser, PackageFormat::Lightweight)
        .expect_err("ids differing only by case cannot share a package");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("IMG1"), "{}", err.message);
    assert_eq!(chooser.asked.get(), 0);
}

#[test]
fn package_receipt_wire_literal() {
    let receipt = PackageReceipt {
        session_id: "ses-1".into(),
        saved_revision: 7,
        file_name: "Demo.vbproject.zip".into(),
        format: PackageFormat::Portable,
    };
    assert_eq!(
        serde_json::to_value(&receipt).unwrap(),
        json!({"sessionId": "ses-1", "savedRevision": 7,
               "fileName": "Demo.vbproject.zip", "format": "portable"})
    );
}

// Review Minor: the manifest's digests come from a first read, the archive
// from a second. A same-size change in between must fail the export, not
// yield a receipt for a file the import will refuse.
#[test]
fn a_source_changed_while_the_archive_is_written_fails_the_export() {
    let (a, session_id, project_id) = machine_a();
    let image = project_dir(a.root(), &project_id)
        .unwrap()
        .join("media")
        .join("img1.png");
    let changed = image.clone();
    before_archive_write::set(move || {
        let mut bytes = std::fs::read(&changed).unwrap();
        bytes[0] ^= 0xff;
        std::fs::write(&changed, bytes).unwrap();
    });
    let out = tempfile::tempdir().unwrap();
    let chooser = FakeChooser::saving_to(out.path().join("Racy.vbproject.zip"));
    let err = export(&a, &session_id, &chooser, PackageFormat::Portable)
        .expect_err("the written bytes are not the manifest's");
    assert!(err.message.contains("changed"), "{}", err.message);
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        0,
        "no file, no temp"
    );
    assert_eq!(std::fs::read(&image).unwrap().len(), IMAGE_BYTES.len());
}

// Review Minor: normalizing `Ours` to `Ours.vbproject.zip` must not replace
// that existing file; the dialog only confirmed an overwrite of what the
// user actually picked.
#[test]
fn a_normalized_name_never_replaces_a_file_the_dialog_did_not_confirm() {
    let (a, session_id, _) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let typed = out.path().join("Ours");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(typed.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();
    let saved = out.path().join("Ours.vbproject.zip");
    let before = std::fs::read(&saved).unwrap();
    a.execute(&session_id, json!({"kind": "rename", "title": "Changed"}));
    let err = export(
        &a,
        &session_id,
        &FakeChooser::saving_to(typed),
        PackageFormat::Portable,
    )
    .expect_err("the normalized name exists and was not the one chosen");
    assert_eq!(err.code, EditorErrorCode::WriteDenied);
    assert!(
        err.message.contains("Ours.vbproject.zip"),
        "{}",
        err.message
    );
    assert_eq!(std::fs::read(&saved).unwrap(), before);
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(saved.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .expect("choosing that exact file replaces it");
    assert_ne!(std::fs::read(&saved).unwrap(), before);
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 1);
}

// Task 51 (F26): a capture's synchronized webcam track is a `StagingFile`
// source -- a locator the package collector had never seen. A portable file
// must CARRY it (else the presenter silently vanishes from the copy), and a
// lightweight one must report it missing on the other machine, exactly as
// it does the capture itself.
#[test]
fn a_staged_webcam_track_is_packaged_or_reported_missing() {
    const WEBCAM_BYTES: &[u8] = b"a synchronized webcam track (38 long).";
    let a = Machine::new();
    a.stage();
    let mut sidecar =
        staging::read_sidecar(&a.staging().join(staging::sidecar_file_name(BASE))).unwrap();
    sidecar.webcam = Some(staging::WebcamSidecar {
        file: staging::webcam_file_name(BASE),
        width: 640,
        height: 480,
        device_label: "Integrated Camera".into(),
        offset_ms: 250,
        extra: serde_json::Map::new(),
    });
    staging::write_sidecar(&a.staging(), BASE, &sidecar).unwrap();
    std::fs::write(
        a.staging().join(staging::webcam_file_name(BASE)),
        WEBCAM_BYTES,
    )
    .unwrap();
    let opened = crate::editor::session_commands::open_staged_session(
        &a.state,
        a.root(),
        &a.staging(),
        BASE,
    )
    .unwrap();
    let session_id = opened.snapshot.session_id.clone();
    let project_id = opened.snapshot.project_id.clone();
    let out = tempfile::tempdir().unwrap();

    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(out.path().join("Cam")),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();
    let b = Machine::new();
    let imported = import(&b, out.path().join("Cam.vbproject.zip"))
        .unwrap()
        .unwrap();
    assert!(imported.missing.is_empty(), "{:?}", imported.missing);
    assert_eq!(media_file(&b, &project_id, "webcam.mp4"), WEBCAM_BYTES);
    assert_eq!(
        load_sources(b.root(), &project_id).unwrap()["webcam"].locator,
        SourceLocator::Media {
            file: "webcam.mp4".into()
        }
    );

    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(out.path().join("Cam")),
        PackageFormat::Lightweight,
    )
    .unwrap()
    .unwrap();
    let c = Machine::new();
    let imported = import(&c, out.path().join("Cam.vbproject.json"))
        .unwrap()
        .unwrap();
    assert_eq!(sorted_missing(&imported), ["src", "webcam"]);
}
