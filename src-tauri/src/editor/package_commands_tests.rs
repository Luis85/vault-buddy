//! Tests for `package_commands` and `package_import` — the `AppHandle`-free
//! halves against tempdirs, the native dialogs behind the `PathChooser`
//! seam. "Machine A" exports; "machine B" is a second, empty app-data root
//! (the file moved to another computer), so a staged capture that exists
//! only on A cannot be found on B by accident.
//!
//! The fixture project is asymmetric on purpose: the staged screen capture
//! (a `screen` builtin, a Staging source), an imported image (a Media
//! source) and a title card (a `card` builtin, no file) — three assets,
//! three different packaging rules, each with its own byte length.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::json;
use vault_buddy_core::editor::package::{
    write_package, PackageManifest, PackageMedia, PackageProduct, WORKSPACE_NAME,
};
use vault_buddy_core::editor::{
    new_product, AssetKind, EditorCommand, EditorErrorCode, ExecuteRequest, InternalCommand,
    MediaType, PACKAGE_SCHEMA,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging::{self, StagedSidecar};

use super::*;
use crate::editor::prefs_commands::{read_workspace, save_workspace_in};
use crate::editor::project_store::{
    project_dir, store_dir, SourceLocator, SourceMediaKind, SourceRecord,
};
use crate::editor::session_commands::{execute_in, open_staged_session};
use crate::editor::store_io::{list_projects, load_project, load_sources};

const BASE: &str = "2026-09-22 0915 Demo";
const SCREEN_BYTES: &[u8] = b"a staged screen capture, 47 bytes long........";
const IMAGE_BYTES: &[u8] = b"PNG-ish image bytes (29 long)";

/// The dialogs, answered by the test. `asked` counts every dialog shown.
struct FakeChooser {
    save: Option<PathBuf>,
    open: Option<PathBuf>,
    asked: Cell<u32>,
}

impl FakeChooser {
    fn saving_to(path: PathBuf) -> Self {
        Self {
            save: Some(path),
            open: None,
            asked: Cell::new(0),
        }
    }
    fn opening(path: PathBuf) -> Self {
        Self {
            save: None,
            open: Some(path),
            asked: Cell::new(0),
        }
    }
}

impl PathChooser for FakeChooser {
    fn save_target(&self, _format: PackageFormat, _suggested: &str) -> Option<PathBuf> {
        self.asked.set(self.asked.get() + 1);
        self.save.clone()
    }
    fn package_to_open(&self) -> Option<PathBuf> {
        self.asked.set(self.asked.get() + 1);
        self.open.clone()
    }
}

struct Machine {
    root: tempfile::TempDir,
    state: EditorState,
}

impl Machine {
    fn new() -> Self {
        let m = Self {
            root: tempfile::tempdir().unwrap(),
            state: EditorState::default(),
        };
        std::fs::create_dir_all(m.staging()).unwrap();
        m
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
    fn staging(&self) -> PathBuf {
        staging::staging_dir(self.root.path())
    }
    fn stage(&self) {
        let sidecar = StagedSidecar {
            base: BASE.to_string(),
            vault_id: "vaultA".into(),
            source_title: "Demo window".into(),
            source_kind: "window".into(),
            inputs: vec!["mic-1".into()],
            duration_ms: 61_500,
            paused_ms: 0,
            width: 1600,
            height: 900,
            recorded_at: "2026-09-22T09:15:00Z".into(),
            timeline: None,
            extra: serde_json::Map::new(),
        };
        staging::write_sidecar(&self.staging(), BASE, &sidecar).unwrap();
        std::fs::write(
            self.staging().join(staging::mp4_file_name(BASE)),
            SCREEN_BYTES,
        )
        .unwrap();
    }
    fn sidecar_bytes(&self) -> Vec<u8> {
        std::fs::read(self.staging().join(staging::sidecar_file_name(BASE))).unwrap()
    }
    fn execute(&self, session_id: &str, command: serde_json::Value) {
        let revision = self.revision(session_id);
        let request = ExecuteRequest {
            session_id: session_id.to_string(),
            expected_revision: revision,
            command_id: format!("cmd-{revision}"),
            command: serde_json::from_value::<EditorCommand>(command).unwrap(),
        };
        execute_in(&self.state, self.root(), &request).unwrap();
    }
    fn revision(&self, session_id: &str) -> u64 {
        lock_ignoring_poison(&self.state.sessions)[session_id]
            .snapshot()
            .revision
    }
    fn project(&self, session_id: &str) -> vault_buddy_core::editor::Project {
        lock_ignoring_poison(&self.state.sessions)[session_id]
            .project()
            .clone()
    }
    fn store_entries(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(store_dir(self.root()))
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

/// Machine A's project: the staged capture on `v1`, an imported image
/// after it, and a title card; renamed once, so it is at revision 5 and
/// was never saved (the export freezes the SESSION, not the store).
fn machine_a() -> (Machine, String, String) {
    let a = Machine::new();
    a.stage();
    let opened = open_staged_session(&a.state, a.root(), &a.staging(), BASE).unwrap();
    let session_id = opened.snapshot.session_id.clone();
    let project_id = opened.snapshot.project_id.clone();
    let media = project_dir(a.root(), &project_id).unwrap().join("media");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::write(media.join("img1.png"), IMAGE_BYTES).unwrap();
    let mut sources = load_sources(a.root(), &project_id).unwrap();
    sources.insert(
        "img1".to_string(),
        SourceRecord {
            locator: SourceLocator::Media {
                file: "img1.png".to_string(),
            },
            sha256: None,
            size: IMAGE_BYTES.len() as u64,
            duration_ms: 5_000,
            width: Some(640),
            height: Some(360),
            has_audio: false,
            has_video: true,
            media_kind: SourceMediaKind::Image,
        },
    );
    crate::editor::store_io::write_sources(a.root(), &project_id, &sources).unwrap();
    let mut image = serde_json::from_value::<vault_buddy_core::editor::Asset>(json!({
        "id": "img1", "kind": "video", "name": "diagram.png", "duration_ms": 5000,
        "width": 640, "height": 360, "size": IMAGE_BYTES.len(), "media_type": "image"
    }))
    .unwrap();
    image.media_type = Some(MediaType::Image);
    assert_eq!(image.kind, AssetKind::Video);
    lock_ignoring_poison(&a.state.sessions)
        .get_mut(&session_id)
        .unwrap()
        .execute_internal(&InternalCommand::AddAssets(
            vault_buddy_core::editor::commands::payloads::AddAssetsPayload {
                assets: vec![image],
            },
        ))
        .unwrap();
    a.execute(
        &session_id,
        json!({"kind": "insertClip", "assetId": "img1", "trackId": "v1",
               "startMs": 70_000, "inMs": 0, "outMs": 5_000}),
    );
    a.execute(
        &session_id,
        json!({"kind": "addCard", "preset": "intro", "trackId": null, "startMs": 80_000,
               "durationMs": 3_000, "title": "Welcome", "subtitle": "Part one"}),
    );
    a.execute(
        &session_id,
        json!({"kind": "rename", "title": "Portable demo"}),
    );
    (a, session_id, project_id)
}

fn export(
    m: &Machine,
    session_id: &str,
    chooser: &FakeChooser,
    format: PackageFormat,
) -> Result<Option<PackageReceipt>, EditorError> {
    let revision = m.revision(session_id);
    export_package_in(&m.state, m.root(), chooser, session_id, revision, format)
}

fn import(m: &Machine, path: PathBuf) -> Result<Option<EditorOpenResult>, EditorError> {
    import_package_in(&m.state, m.root(), &FakeChooser::opening(path))
}

fn sorted_missing(result: &EditorOpenResult) -> Vec<String> {
    let mut ids: Vec<String> = result.missing.iter().map(|m| m.asset_id.clone()).collect();
    ids.sort();
    ids
}

fn media_file(m: &Machine, project_id: &str, name: &str) -> Vec<u8> {
    std::fs::read(
        project_dir(m.root(), project_id)
            .unwrap()
            .join("media")
            .join(name),
    )
    .unwrap()
}

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

/// One packaged file: its manifest path, its bytes, and the SHA-256 the
/// manifest claims (the true one unless a test says otherwise).
struct Packed<'a> {
    id: &'a str,
    path: &'a str,
    bytes: &'a [u8],
    sha: String,
}

fn packed<'a>(id: &'a str, path: &'a str, bytes: &'a [u8]) -> Packed<'a> {
    Packed {
        id,
        path,
        bytes,
        sha: sha256_hex(bytes),
    }
}

/// A package built straight through `write_package`, for what an export of
/// ours never writes (a lying digest, a retained product).
fn write_test_package(
    path: &Path,
    envelope: &WorkspaceEnvelope,
    media: &[Packed],
    products: &[Packed],
) {
    let manifest = PackageManifest {
        schema: PACKAGE_SCHEMA.to_string(),
        project_id: envelope.project.id.clone(),
        saved_at: envelope.saved_at.clone(),
        workspace: WORKSPACE_NAME.to_string(),
        media: media
            .iter()
            .map(|m| PackageMedia {
                asset_id: m.id.to_string(),
                path: m.path.to_string(),
                size: m.bytes.len() as u64,
                sha256: m.sha.clone(),
            })
            .collect(),
        products: products
            .iter()
            .map(|p| PackageProduct {
                product_id: p.id.to_string(),
                path: p.path.to_string(),
                size: p.bytes.len() as u64,
                sha256: p.sha.clone(),
            })
            .collect(),
    };
    let files: Vec<(String, std::io::Cursor<Vec<u8>>)> = media
        .iter()
        .chain(products)
        .map(|f| (f.path.to_string(), std::io::Cursor::new(f.bytes.to_vec())))
        .collect();
    let workspace = serde_json::to_vec(envelope).unwrap();
    write_package(
        std::fs::File::create(path).unwrap(),
        &manifest,
        &workspace,
        files,
    )
    .unwrap();
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

// A retained product travels with its record: its file lands in
// `products\` under the record's own file name (where `media_commands`
// resolves it), and a file name that cannot be a plain file refuses the
// whole import rather than landing somewhere else.
#[test]
fn a_retained_product_is_installed_under_its_record_file_name() {
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
    let out = tempfile::tempdir().unwrap();
    let good = out.path().join("Product.vbproject.zip");
    write_test_package(
        &good,
        &envelope,
        &media,
        &[packed("prod1", "products/prod1.mp4", rendered)],
    );
    envelope.record.products[0].filename = "CON.mp4".to_string();
    let bad = out.path().join("Device.vbproject.zip");
    write_test_package(
        &bad,
        &envelope,
        &media,
        &[packed("prod1", "products/prod1.mp4", rendered)],
    );

    let b = Machine::new();
    let err = import(&b, bad).expect_err("a device file name is refused");
    assert!(err.message.contains("CON.mp4"), "{}", err.message);
    assert_eq!(b.store_entries(), Vec::<String>::new());

    let opened = import(&b, good).unwrap().unwrap();
    assert_eq!(opened.snapshot.project_id, project_id);
    let products = project_dir(b.root(), &project_id).unwrap().join("products");
    assert_eq!(
        std::fs::read(products.join("take-one.mp4")).unwrap(),
        rendered
    );
    let (saved, _) = load_project(b.root(), &project_id).unwrap();
    assert_eq!(saved.record.products.len(), 1);
    assert_eq!(saved.record.products[0].filename, "take-one.mp4");
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

fn sha256_hex(bytes: &[u8]) -> String {
    let mut sink = std::io::sink();
    vault_buddy_core::editor::import_io::copy_hashing(&mut &bytes[..], &mut sink)
        .unwrap()
        .1
}
