//! Shared fixtures for the project-file suites (`package_commands_tests.rs`,
//! the export half, and `package_import_tests.rs`, the import half), split
//! when one suite passed the 800-line cap in Task 39's fix round 1.
//!
//! "Machine A" exports; "machine B" is a second, empty app-data root (the
//! file moved to another computer), so a staged capture that exists only on
//! A cannot be found on B by accident. The fixture project is asymmetric on
//! purpose: the staged screen capture (a `screen` builtin, a Staging
//! source), an imported image (a Media source) and a title card (a `card`
//! builtin, no file) — three assets, three different packaging rules, each
//! with its own byte length.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use serde_json::json;
use vault_buddy_core::editor::package::{
    write_package, PackageManifest, PackageMedia, PackageProduct, WORKSPACE_NAME,
};
use vault_buddy_core::editor::package_plan::PackageFormat;
use vault_buddy_core::editor::{
    AssetKind, EditorCommand, EditorError, EditorOpenResult, ExecuteRequest, InternalCommand,
    MediaType, WorkspaceEnvelope, PACKAGE_SCHEMA,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging::{self, StagedSidecar};

use super::package_commands::{export_package_in, PackageReceipt, PathChooser};
use super::package_import::import_package_in;
use super::project_store::{project_dir, store_dir, SourceLocator, SourceMediaKind, SourceRecord};
use super::session_commands::{execute_in, open_staged_session};
use super::store_io::load_sources;
use super::EditorState;

pub(crate) const BASE: &str = "2026-09-22 0915 Demo";
pub(crate) const SCREEN_BYTES: &[u8] = b"a staged screen capture, 47 bytes long........";
pub(crate) const IMAGE_BYTES: &[u8] = b"PNG-ish image bytes (29 long)";

/// The dialogs, answered by the test. `asked` counts every dialog shown.
pub(crate) struct FakeChooser {
    pub(crate) save: Option<PathBuf>,
    pub(crate) open: Option<PathBuf>,
    pub(crate) asked: Cell<u32>,
}

impl FakeChooser {
    pub(crate) fn saving_to(path: PathBuf) -> Self {
        Self {
            save: Some(path),
            open: None,
            asked: Cell::new(0),
        }
    }
    pub(crate) fn opening(path: PathBuf) -> Self {
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

pub(crate) struct Machine {
    pub(crate) root: tempfile::TempDir,
    pub(crate) state: EditorState,
}

impl Machine {
    pub(crate) fn new() -> Self {
        let m = Self {
            root: tempfile::tempdir().unwrap(),
            state: EditorState::default(),
        };
        std::fs::create_dir_all(m.staging()).unwrap();
        m
    }
    pub(crate) fn root(&self) -> &Path {
        self.root.path()
    }
    pub(crate) fn staging(&self) -> PathBuf {
        staging::staging_dir(self.root.path())
    }
    pub(crate) fn stage(&self) {
        self.stage_with(&["mic-1"]);
    }
    /// `inputs` empty = a capture recorded with no audio device, whose
    /// source record says `hasAudio: false`.
    pub(crate) fn stage_with(&self, inputs: &[&str]) {
        let sidecar = StagedSidecar {
            base: BASE.to_string(),
            vault_id: "vaultA".into(),
            source_title: "Demo window".into(),
            source_kind: "window".into(),
            inputs: inputs.iter().map(|i| i.to_string()).collect(),
            duration_ms: 61_500,
            paused_ms: 0,
            width: 1600,
            height: 900,
            recorded_at: "2026-09-22T09:15:00Z".into(),
            timeline: None,
            webcam: None,
            extra: serde_json::Map::new(),
        };
        staging::write_sidecar(&self.staging(), BASE, &sidecar).unwrap();
        std::fs::write(
            self.staging().join(staging::mp4_file_name(BASE)),
            SCREEN_BYTES,
        )
        .unwrap();
    }
    pub(crate) fn sidecar_bytes(&self) -> Vec<u8> {
        std::fs::read(self.staging().join(staging::sidecar_file_name(BASE))).unwrap()
    }
    pub(crate) fn execute(&self, session_id: &str, command: serde_json::Value) {
        self.try_execute(session_id, command).unwrap();
    }
    pub(crate) fn try_execute(
        &self,
        session_id: &str,
        command: serde_json::Value,
    ) -> Result<vault_buddy_core::editor::EditorProjection, EditorError> {
        let revision = self.revision(session_id);
        let request = ExecuteRequest {
            session_id: session_id.to_string(),
            expected_revision: revision,
            command_id: format!("cmd-{revision}"),
            command: serde_json::from_value::<EditorCommand>(command).unwrap(),
        };
        execute_in(&self.state, self.root(), &request)
    }
    pub(crate) fn revision(&self, session_id: &str) -> u64 {
        lock_ignoring_poison(&self.state.sessions)[session_id]
            .snapshot()
            .revision
    }
    pub(crate) fn project(&self, session_id: &str) -> vault_buddy_core::editor::Project {
        lock_ignoring_poison(&self.state.sessions)[session_id]
            .project()
            .clone()
    }
    pub(crate) fn store_entries(&self) -> Vec<String> {
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
pub(crate) fn machine_a() -> (Machine, String, String) {
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
            replaced_from: None,
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

pub(crate) fn export(
    m: &Machine,
    session_id: &str,
    chooser: &FakeChooser,
    format: PackageFormat,
) -> Result<Option<PackageReceipt>, EditorError> {
    let revision = m.revision(session_id);
    export_package_in(&m.state, m.root(), chooser, session_id, revision, format)
}

pub(crate) fn import(m: &Machine, path: PathBuf) -> Result<Option<EditorOpenResult>, EditorError> {
    import_package_in(&m.state, m.root(), &FakeChooser::opening(path))
}

pub(crate) fn sorted_missing(result: &EditorOpenResult) -> Vec<String> {
    let mut ids: Vec<String> = result.missing.iter().map(|m| m.asset_id.clone()).collect();
    ids.sort();
    ids
}

pub(crate) fn media_file(m: &Machine, project_id: &str, name: &str) -> Vec<u8> {
    std::fs::read(
        project_dir(m.root(), project_id)
            .unwrap()
            .join("media")
            .join(name),
    )
    .unwrap()
}

/// One packaged file: its manifest path, its bytes, and the SHA-256 the
/// manifest claims (the true one unless a test says otherwise).
pub(crate) struct Packed<'a> {
    pub(crate) id: &'a str,
    pub(crate) path: &'a str,
    pub(crate) bytes: &'a [u8],
    pub(crate) sha: String,
}

pub(crate) fn packed<'a>(id: &'a str, path: &'a str, bytes: &'a [u8]) -> Packed<'a> {
    Packed {
        id,
        path,
        bytes,
        sha: sha256_hex(bytes),
    }
}

/// A package built straight through `write_package`, for what an export of
/// ours never writes (a lying digest, a retained product).
pub(crate) fn write_test_package(
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

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut sink = std::io::sink();
    vault_buddy_core::editor::import_io::copy_hashing(&mut &bytes[..], &mut sink)
        .unwrap()
        .1
}

/// `detachAudio` on the clip of asset `src`: Task 27's guard refuses it when
/// `src`'s source record says it has no sound.
pub(crate) fn detach_src(
    m: &Machine,
    session_id: &str,
    project: &vault_buddy_core::editor::Project,
) -> Result<(), EditorError> {
    let clip = project.clips.iter().find(|c| c.asset_id == "src").unwrap();
    m.try_execute(
        session_id,
        json!({"kind": "detachAudio", "clipId": clip.id, "audioTrackId": null}),
    )
    .map(|_| ())
}
