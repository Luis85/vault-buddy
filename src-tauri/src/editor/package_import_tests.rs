//! Tests for `package_import` — the IMPORT half (`editor_import_package`):
//! what is installed, what is refused, what a round trip preserves.
//! Fixtures: `package_test_support.rs`; the export half is
//! `package_commands_tests.rs`.

use serde_json::json;
use vault_buddy_core::editor::package_plan::PackageFormat;
use vault_buddy_core::editor::{new_product, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use crate::editor::package_commands::export_envelope;
use crate::editor::package_test_support::*;
use crate::editor::project_store::{project_dir, SourceLocator, SourceMediaKind};
use crate::editor::render_jobs::{products_in, read_ledger};
use crate::editor::session_commands::open_staged_session;
use crate::editor::store_io::{list_projects, load_project, load_sources};

#[test]
fn lightweight_import_lists_missing_media() {
    let (a, session_id, project_id) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let chooser = FakeChooser::saving_to(out.path().join("Demo.json"));
    let receipt = export(&a, &session_id, &chooser, PackageFormat::Lightweight)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.file_name, "Demo.vbproject.json");
    assert_eq!(receipt.format, PackageFormat::Lightweight);

    let b = Machine::new();
    let opened = import(&b, out.path().join("Demo.vbproject.json"))
        .unwrap()
        .unwrap();
    assert_eq!(opened.project, a.project(&session_id));
    assert_eq!(
        sorted_missing(&opened),
        ["img1", "src"],
        "the card needs no file"
    );
    let sources = load_sources(b.root(), &project_id).unwrap();
    assert_eq!(
        sources["src"].locator,
        SourceLocator::Media {
            file: "src.mp4".into()
        }
    );
    assert_eq!(
        sources["img1"].locator,
        SourceLocator::Media {
            file: "img1.png".into()
        }
    );
    assert_eq!(sources["img1"].size, IMAGE_BYTES.len() as u64);
    assert_eq!(sources["img1"].media_kind, SourceMediaKind::Image);
    let media = project_dir(b.root(), &project_id).unwrap().join("media");
    assert!(!media.join("src.mp4").exists() && !media.join("img1.png").exists());
}

#[test]
fn failed_import_installs_nothing() {
    let (a, session_id, _) = machine_a();
    let envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    // A well-formed archive whose SECOND media entry does not hash to what
    // the manifest says: the first extracts cleanly, then the check fails.
    let mut corrupt = packed("src", "media/src.mp4", SCREEN_BYTES);
    corrupt.sha = sha256_hex(b"not the packaged bytes");
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Corrupt.vbproject.zip");
    write_test_package(
        &path,
        &envelope,
        &[packed("img1", "media/img1.png", IMAGE_BYTES), corrupt],
        &[],
    );

    let b = Machine::new();
    let err = import(&b, path).expect_err("a corrupt entry refuses the import");
    assert_eq!(err.code, EditorErrorCode::InvalidProject, "{}", err.message);
    assert!(err.message.contains("media/src.mp4"), "{}", err.message);
    assert_eq!(
        b.store_entries(),
        Vec::<String>::new(),
        "nothing installed, nothing left"
    );
    assert!(lock_ignoring_poison(&b.state.sessions).is_empty());
    assert!(list_projects(b.root()).is_empty());
}

// A retained product travels with its record: its file lands at
// `products\<productId>.mp4` (Task 46's one product path, where
// `media_commands` resolves it) and the record lands in the LEDGER
// (`products.json`), so the imported project lists it before any save. A
// record naming any other file -- here a plausible `take-one.mp4` -- refuses
// the whole import rather than landing somewhere else.
#[test]
fn a_retained_product_is_installed_as_its_id_and_recorded_in_the_ledger() {
    let (a, session_id, project_id) = machine_a();
    let mut envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    let rendered: &[u8] = b"a rendered product, 30 long...";
    envelope.record.products.push(new_product(
        &envelope.project,
        5,
        "prod1",
        "Take one",
        "take-one.mp4",
        1_000,
        None,
        &envelope.saved_at,
    ));
    let media = [
        packed("img1", "media/img1.png", IMAGE_BYTES),
        packed("src", "media/src.mp4", SCREEN_BYTES),
    ];
    let product = [packed("prod1", "products/prod1.mp4", rendered)];
    let out = tempfile::tempdir().unwrap();
    let bad = out.path().join("Foreign.vbproject.zip");
    write_test_package(&bad, &envelope, &media, &product);
    envelope.record.products[0].filename = "prod1.mp4".to_string();
    let good = out.path().join("Product.vbproject.zip");
    write_test_package(&good, &envelope, &media, &product);

    let b = Machine::new();
    let err = import(&b, bad).expect_err("a foreign product file name is refused");
    assert!(err.message.contains("take-one.mp4"), "{}", err.message);
    assert_eq!(b.store_entries(), Vec::<String>::new());

    let opened = import(&b, good).unwrap().unwrap();
    assert_eq!(opened.snapshot.project_id, project_id);
    let products = project_dir(b.root(), &project_id).unwrap().join("products");
    assert_eq!(std::fs::read(products.join("prod1.mp4")).unwrap(), rendered);
    let ledger = read_ledger(b.root(), &project_id).unwrap();
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].filename, "prod1.mp4");
    let listed = products_in(&b.state, b.root(), &opened.snapshot.session_id).unwrap();
    assert!(listed[0].available, "the imported product plays");
}

#[test]
fn existing_project_id_imports_as_a_copy() {
    let (a, session_id, project_id) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Copy.vbproject.zip");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();
    let original = std::fs::read(
        project_dir(a.root(), &project_id)
            .unwrap()
            .join("project.json"),
    )
    .unwrap();

    let opened = import(&a, path).unwrap().unwrap();
    let copy_id = opened.snapshot.project_id.clone();
    assert_ne!(copy_id, project_id, "the id already exists here");
    assert_ne!(
        opened.snapshot.session_id, session_id,
        "a new session, not the live one"
    );
    assert_eq!(opened.project.id, copy_id);
    let mut expected = a.project(&session_id);
    expected.id = copy_id.clone();
    assert_eq!(opened.project, expected);
    let (envelope, _) = load_project(a.root(), &copy_id).unwrap();
    assert_eq!(envelope.record.id, copy_id);
    assert_eq!(media_file(&a, &copy_id, "src.mp4"), SCREEN_BYTES);
    assert_eq!(
        std::fs::read(
            project_dir(a.root(), &project_id)
                .unwrap()
                .join("project.json")
        )
        .unwrap(),
        original,
        "the original project is untouched"
    );
    let mut listed: Vec<String> = list_projects(a.root())
        .into_iter()
        .map(|p| p.project_file_id)
        .collect();
    listed.sort();
    let mut both = vec![project_id, copy_id];
    both.sort();
    assert_eq!(listed, both);
}

// Review Important 1: the import used to set `hasAudio: true` for every
// non-image source, so a screen capture recorded with NO audio device came
// back able to detach an empty audio clip, the edit Task 27 exists to
// refuse. The exporting side's facts now travel with the file.
#[test]
fn a_silent_capture_still_refuses_detach_audio_after_a_portable_round_trip() {
    let a = Machine::new();
    a.stage_with(&[]);
    let opened = open_staged_session(&a.state, a.root(), &a.staging(), BASE).unwrap();
    let session_id = opened.snapshot.session_id.clone();
    detach_src(&a, &session_id, &opened.project).expect_err("refused on the recording machine");
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Silent.vbproject.zip");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();

    let b = Machine::new();
    let imported = import(&b, path).unwrap().unwrap();
    let pid = imported.snapshot.project_id.clone();
    let record = load_sources(b.root(), &pid).unwrap()["src"].clone();
    assert!(!record.has_audio, "the silent capture stays silent");
    assert_eq!((record.width, record.height), (Some(1600), Some(900)));
    let err = detach_src(&b, &imported.snapshot.session_id, &imported.project)
        .expect_err("detaching nothing is still refused after the round trip");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{}", err.message);
}

// A file without facts (an older build's, another editor's) must never
// INVENT a sound track: a video's `hasAudio` defaults to false.
#[test]
fn a_project_file_without_source_facts_never_invents_audio() {
    let (a, session_id, project_id) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Facts.vbproject.json");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Lightweight,
    )
    .unwrap()
    .unwrap();
    let with_facts = Machine::new();
    import(&with_facts, path.clone()).unwrap().unwrap();
    let sources = load_sources(with_facts.root(), &project_id).unwrap();
    assert!(
        sources["src"].has_audio,
        "a capture with a microphone keeps its sound"
    );
    assert_eq!(sources["img1"].media_kind, SourceMediaKind::Image);

    let mut raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    raw["record"]
        .as_object_mut()
        .unwrap()
        .remove("vaultBuddySourceFacts")
        .expect("the export carried the facts");
    let bare = out.path().join("Bare.vbproject.json");
    std::fs::write(&bare, serde_json::to_vec(&raw).unwrap()).unwrap();
    let without = Machine::new();
    import(&without, bare).unwrap().unwrap();
    let sources = load_sources(without.root(), &project_id).unwrap();
    assert!(!sources["src"].has_audio, "no fact, no invented audio");
    assert!(!sources["img1"].has_audio);
}

// Review Minor: untrusted package content must never land verbatim in the
// store: `project.json` carries the SANITIZED workspace, like
// `workspace.json` and like `editor_save_project`, and never the facts'
// transport key.
#[test]
fn the_stored_envelope_carries_only_the_sanitized_workspace() {
    let (a, session_id, project_id) = machine_a();
    let mut envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    envelope.workspace = json!({"theme": "light", "smuggled": {"script": "x"}});
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Dirty.vbproject.json");
    std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
    let b = Machine::new();
    import(&b, path).unwrap().unwrap();
    let (stored, _) = load_project(b.root(), &project_id).unwrap();
    assert!(
        stored.workspace.get("smuggled").is_none(),
        "{}",
        stored.workspace
    );
    assert_eq!(stored.workspace["theme"], "light");
    assert!(!stored.record.extra.contains_key("vaultBuddySourceFacts"));
}
