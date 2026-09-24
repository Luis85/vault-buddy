//! Pure paths and ownership for the tutorial-editor project store —
//! tempdir-testable, no Tauri types.
//!
//! On-disk layout (`global-constraints.md`'s Contract reference):
//! `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\
//! {project.json, sources.json, products.json, recovery.json,
//! workspace.json, media\, takes\, products\, cache\, jobs\<jobId>\}`. This
//! module owns the LEAF path (`project_dir`) and a source's reference
//! (`SourceLocator`/`SourceRecord`/`resolve_source`); `store_io` owns the
//! files themselves.
//!
//! Also owns the pin (R6): the `extra["editorProjectId"]` key a project
//! writes into a staged capture's sidecar so it is ADOPTED by reference
//! rather than moved — staging and the project store stay two directories,
//! and the pin is the only thing that connects them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vault_buddy_core::editor::is_valid_id;
use vault_buddy_screen::staging::{self, StagedSidecar};
use vault_buddy_screen::staging_files;

/// The store's own directory name under the app's local data dir — a
/// sibling of `staging::STAGING_DIR_NAME`, never inside it: a project and
/// the staged capture it is pinned to are two directories on purpose (see
/// the module doc's "why not move").
pub const STORE_DIR: &str = "editor-projects";

/// `<root>/editor-projects`, the store's own root.
pub fn store_dir(root: &Path) -> PathBuf {
    root.join(STORE_DIR)
}

/// A project's own directory, `<root>/editor-projects/<id>`.
///
/// Refuses an invalid id (`vault_buddy_core::editor::is_valid_id`'s
/// `^[a-zA-Z0-9_-]{1,100}$`) rather than joining it onto a path: that
/// charset excludes `/`, `\` and `.`, so a valid id can never escape this
/// directory or address `..` — the property is structural, not a separate
/// containment check bolted on afterward.
pub fn project_dir(root: &Path, id: &str) -> Option<PathBuf> {
    if !is_valid_id(id) {
        return None;
    }
    Some(store_dir(root).join(id))
}

/// Where a source's bytes actually live within a project, as a raw string
/// tag (`{"store": "...", ...}`) so `sources.json` reads exactly like the
/// ADR's own example, `{"store":"staging","base":"<base>"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "store", rename_all = "lowercase")]
pub enum SourceLocator {
    /// A staged screen capture, adopted by reference (R6) — never copied
    /// into the project directory.
    Staging { base: String },
    /// A COMPANION file of a staged capture (F-22, F26) — its synchronized
    /// webcam track `<base>.webcam.mp4` today, a stem after Task 53 — also
    /// adopted by reference. `Staging { base }` can only ever name
    /// `<base>.mp4`; this names one more of the files the capture owns, and
    /// nothing else (see `resolve_source`).
    #[serde(rename = "stagingFile")]
    StagingFile { base: String, file: String },
    /// An imported original, copied into this project's own `media/`.
    Media { file: String },
    /// A recorded webcam take, in this project's own `takes/`.
    Takes { file: String },
    /// A procedurally-supplied asset with no file on disk at all (a
    /// synthesized card, a generated cue track).
    Builtin,
}

/// What kind of media a source is — the same refinement
/// `vault_buddy_core::editor::model::AssetKind`/`MediaType` draws for an
/// `Asset`, applied one layer down to the raw file a source record
/// describes before it becomes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceMediaKind {
    Video,
    Audio,
    Image,
}

/// One entry of a project's `sources.json` — everything the project needs
/// to know about a source WITHOUT re-probing the file on every open.
///
/// The wire shape is the ADR §4 native source registry entry exactly:
/// `{"store": ..., "base"?/"file"?: ..., "sha256"?, "size", "durationMs",
/// "width"?, "height"?, "hasAudio", "hasVideo", "mediaKind"}` — `locator`
/// is flattened so `store`/`base`/`file` land at the TOP level beside the
/// rest rather than nested under a `"locator"` key, and `width`/`height`
/// are optional (an audio-only source has neither).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    #[serde(flatten)]
    pub locator: SourceLocator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub size: u64,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    pub has_audio: bool,
    pub has_video: bool,
    pub media_kind: SourceMediaKind,
    /// Set when the user CONFIRMED a different file as this source's
    /// replacement (Task 40, `editor_relink_media` with `confirmReplace`):
    /// the identity the record had before, so the project remembers that
    /// its bytes are a deliberate substitute and not the original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_from: Option<ReplacedFrom>,
}

/// The identity a replaced source had (`SourceRecord::replaced_from`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedFrom {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub size: u64,
    pub duration_ms: u64,
    pub media_kind: SourceMediaKind,
}

/// A joined path, but only when `name` is a single ordinary path component
/// with nothing in it that could mean something OUTSIDE this platform's own
/// parser.
///
/// This is deliberately NOT `dir.join(name)` plus a `.parent() == Some(dir)`
/// check (what `staging::write_sidecar` uses): that comparison is only as
/// strict as the CURRENT platform's `Path` parses `name`, and `sources.json`
/// travels in portable packages (a later task) — a name written on one
/// platform is read back on whichever platform opens the package. `"C:x"` is
/// a Windows drive-RELATIVE escape but an utterly ordinary single-component
/// file name on Unix, so a Linux CI run of this exact check would accept it
/// and pass a test that must fail. `:` and `\` are therefore refused as
/// characters, unconditionally, on every platform — never left to whichever
/// platform's `Path` happens to be compiled in — on top of requiring exactly
/// one `Component::Normal` (which already rules out `/`, `..`, `.` and an
/// absolute root on every platform `Path` supports).
pub(crate) fn join_contained(dir: &Path, name: &str) -> Option<PathBuf> {
    if name.contains(':') || name.contains('\\') {
        return None;
    }
    let mut components = Path::new(name).components();
    let Some(std::path::Component::Normal(_)) = components.next() else {
        return None;
    };
    if components.next().is_some() {
        return None;
    }
    Some(dir.join(name))
}

/// Resolve a source record to the file it actually names on disk, or
/// `None` when it has no file (`Builtin`) or the recorded name would
/// escape the directory it is supposed to live in.
///
/// `root_local_app_data` is the SAME root every other store/staging path is
/// rooted at (`app_local_data_dir()`), never a project-relative path — a
/// source record on its own does not know where the app's data lives.
pub fn resolve_source(
    root_local_app_data: &Path,
    project_id: &str,
    record: &SourceRecord,
) -> Option<PathBuf> {
    match &record.locator {
        SourceLocator::Staging { base } => {
            let dir = staging::staging_dir(root_local_app_data);
            join_contained(&dir, &staging::mp4_file_name(base))
        }
        // Membership first (F26): `file` must be literally one of the names
        // `capture_file_names` says `base` owns. Stems are `&[]` until Task
        // 53 threads the sidecar's list through; the webcam name needs none.
        SourceLocator::StagingFile { base, file } => {
            if !staging_files::capture_file_names(base, &[]).contains(file) {
                return None;
            }
            join_contained(&staging::staging_dir(root_local_app_data), file)
        }
        SourceLocator::Media { file } => {
            let dir = project_dir(root_local_app_data, project_id)?.join("media");
            join_contained(&dir, file)
        }
        SourceLocator::Takes { file } => {
            let dir = project_dir(root_local_app_data, project_id)?.join("takes");
            join_contained(&dir, file)
        }
        SourceLocator::Builtin => None,
    }
}

/// The sidecar key a tutorial project writes to ADOPT a staged capture by
/// reference (R6) — a project's `sources.json` still names the capture by
/// its `base`, but the capture's own sidecar carries this key back so
/// `staged_commands`/`staging_commands` can tell a pinned capture apart
/// from an ordinary one without opening the project store at all.
const PIN_KEY: &str = "editorProjectId";

/// Is this staged capture pinned to a tutorial project, and if so which
/// one? Reads the sidecar's flattened `extra` map — the same forward-
/// compatible catch-all every other sidecar field beyond this build's own
/// uses — so a non-string value (the sidecar is hand-editable) is not a
/// claim in either direction, `staged_commands::summary_is_recovered`'s
/// posture applied to a string key instead of a bool one.
pub fn pinned_project(sidecar: &StagedSidecar) -> Option<String> {
    sidecar.extra.get(PIN_KEY)?.as_str().map(str::to_string)
}

/// Write the pin into a staged capture's sidecar (R6: adopted by
/// reference, not moved). A read-modify-write through
/// `staging::write_sidecar`, so it inherits that writer's atomic
/// temp+fsync+rename and its containment refusal — this function never
/// touches the file system directly.
///
/// Errs when the capture has no sidecar to pin at all (`NotFound`-shaped):
/// pinning is meaningless without a capture on the other end.
pub fn pin_staged(staging_dir: &Path, base: &str, project_id: &str) -> std::io::Result<()> {
    let path = staging_dir.join(staging::sidecar_file_name(base));
    let mut sidecar = staging::read_sidecar(&path).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("no staged capture named {base:?} to pin"),
        )
    })?;
    sidecar.extra.insert(
        PIN_KEY.to_string(),
        serde_json::Value::String(project_id.to_string()),
    );
    staging::write_sidecar(staging_dir, base, &sidecar).map(|_| ())
}

/// Clear the pin, but only when it is OURS — a project id this call was not
/// asked to clear must never erase a different project's pin. A capture
/// with no sidecar, or no pin at all, is already unpinned: both degrade to
/// `Ok(())` rather than an error, the same "the path is clear" posture
/// `discard_staged_files` uses for an already-gone file.
pub fn unpin_staged(staging_dir: &Path, base: &str, project_id: &str) -> std::io::Result<()> {
    let path = staging_dir.join(staging::sidecar_file_name(base));
    let Some(mut sidecar) = staging::read_sidecar(&path) else {
        return Ok(());
    };
    if pinned_project(&sidecar).as_deref() != Some(project_id) {
        return Ok(());
    }
    sidecar.extra.remove(PIN_KEY);
    staging::write_sidecar(staging_dir, base, &sidecar).map(|_| ())
}

/// Shared test fixture: `test_support::minimal_project`'s counterpart for
/// the shell's own tests, since `core::editor::test_support` is
/// `#[cfg(test)] pub(crate)` to `core` alone and cannot cross the crate
/// boundary. `pub(crate)` so `store_io`'s sibling test module can reuse it
/// too, the same reason `core`'s own fixture module exists.
#[cfg(test)]
pub(crate) fn minimal_project(id: &str) -> vault_buddy_core::editor::Project {
    vault_buddy_core::editor::Project {
        schema: vault_buddy_core::editor::PROJECT_SCHEMA.to_string(),
        id: id.to_string(),
        title: "Untitled".to_string(),
        canvas: vault_buddy_core::editor::Canvas {
            width: 1280,
            height: 720,
            fps: 30,
            extra: vault_buddy_core::editor::Map::new(),
        },
        master_gain: 1.0,
        assets: Vec::new(),
        tracks: Vec::new(),
        clips: Vec::new(),
        effects: Vec::new(),
        markers: Vec::new(),
        transitions: Vec::new(),
        captions: None,
        destination: vault_buddy_core::editor::Destination {
            vault: String::new(),
            folder: String::new(),
            dated: false,
            extra: vault_buddy_core::editor::Map::new(),
        },
        extra: vault_buddy_core::editor::Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sidecar_fixture(base: &str) -> StagedSidecar {
        StagedSidecar {
            base: base.to_string(),
            vault_id: "v1".into(),
            source_title: "Demo".into(),
            source_kind: "screen".into(),
            inputs: Vec::new(),
            duration_ms: 5_000,
            paused_ms: 0,
            width: 1920,
            height: 1080,
            recorded_at: "2026-09-20T14:32:00Z".into(),
            timeline: None,
            webcam: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn project_dir_refuses_an_invalid_id() {
        let root = Path::new("/lad");
        assert_eq!(
            project_dir(root, "abc-123_XY"),
            Some(PathBuf::from("/lad/editor-projects/abc-123_XY"))
        );
        assert_eq!(project_dir(root, ""), None);
        assert_eq!(
            project_dir(root, "a/b"),
            None,
            "a separator must be refused"
        );
        assert_eq!(
            project_dir(root, "../evil"),
            None,
            "a traversal must be refused"
        );
    }

    // A malicious file NAME (never the project's own id, which is already
    // charset-restricted) is the escape vector `resolve_source` guards:
    // media/takes file names come from a project's own `sources.json`,
    // which — unlike a project id — carries no charset restriction at all.
    #[test]
    fn resolve_source_refuses_escaping_file_names() {
        let root = tempfile::tempdir().unwrap();
        let record = |locator: SourceLocator| SourceRecord {
            locator,
            sha256: None,
            size: 0,
            duration_ms: 0,
            width: None,
            height: None,
            has_audio: false,
            has_video: false,
            media_kind: SourceMediaKind::Video,
            replaced_from: None,
        };
        assert_eq!(
            resolve_source(
                root.path(),
                "proj1",
                &record(SourceLocator::Media {
                    file: "../x".to_string()
                })
            ),
            None,
            "a parent-directory escape must be refused"
        );
        assert_eq!(
            resolve_source(
                root.path(),
                "proj1",
                &record(SourceLocator::Takes {
                    file: "C:x".to_string()
                })
            ),
            None,
            "a drive-relative escape must be refused"
        );
    }

    #[test]
    fn resolve_source_joins_an_ordinary_file_name_under_the_right_subdirectory() {
        let root = tempfile::tempdir().unwrap();
        let record = |locator: SourceLocator| SourceRecord {
            locator,
            sha256: None,
            size: 0,
            duration_ms: 0,
            width: None,
            height: None,
            has_audio: false,
            has_video: false,
            media_kind: SourceMediaKind::Video,
            replaced_from: None,
        };
        assert_eq!(
            resolve_source(
                root.path(),
                "proj1",
                &record(SourceLocator::Media {
                    file: "clip.mp4".to_string()
                })
            ),
            Some(
                root.path()
                    .join("editor-projects")
                    .join("proj1")
                    .join("media")
                    .join("clip.mp4")
            )
        );
        assert_eq!(
            resolve_source(
                root.path(),
                "proj1",
                &record(SourceLocator::Staging {
                    base: "2026-09-20 1432 Demo".to_string()
                })
            ),
            Some(
                root.path()
                    .join("screen-captures")
                    .join("2026-09-20 1432 Demo.mp4")
            )
        );
    }

    #[test]
    fn resolve_source_answers_nothing_for_a_builtin_source() {
        let root = tempfile::tempdir().unwrap();
        let record = SourceRecord {
            locator: SourceLocator::Builtin,
            sha256: None,
            size: 0,
            duration_ms: 0,
            width: None,
            height: None,
            has_audio: false,
            has_video: false,
            media_kind: SourceMediaKind::Video,
            replaced_from: None,
        };
        assert_eq!(resolve_source(root.path(), "proj1", &record), None);
    }

    // F26: a `StagingFile` names a file under STAGING, where every capture's
    // recordings live side by side -- so membership in what its OWN capture
    // owns is the whole guard. A bare containment check would let a
    // hand-edited `sources.json` serve (and a package export copy) any
    // other capture's video, or anything else that happens to sit there.
    #[test]
    fn staging_file_locator_refuses_a_name_capture_file_names_does_not_own() {
        let root = tempfile::tempdir().unwrap();
        let base = "2026-09-20 1432 Demo";
        let record = |file: &str| SourceRecord {
            locator: SourceLocator::StagingFile {
                base: base.to_string(),
                file: file.to_string(),
            },
            sha256: None,
            size: 0,
            duration_ms: 0,
            width: None,
            height: None,
            has_audio: false,
            has_video: true,
            media_kind: SourceMediaKind::Video,
            replaced_from: None,
        };
        assert_eq!(
            resolve_source(
                root.path(),
                "proj1",
                &record(&staging::webcam_file_name(base))
            ),
            Some(
                root.path()
                    .join("screen-captures")
                    .join("2026-09-20 1432 Demo.webcam.mp4")
            ),
            "the capture's own webcam file resolves"
        );
        for not_owned in [
            "../x",
            "unrelated.mp4",
            "2026-09-20 1500 Other.webcam.mp4",
            "2026-09-20 1500 Other.mp4",
            "",
        ] {
            assert_eq!(
                resolve_source(root.path(), "proj1", &record(not_owned)),
                None,
                "{not_owned:?} is not a file {base:?} owns"
            );
        }
        // An escaping BASE is refused even when the file is "its own".
        let escaping = SourceRecord {
            locator: SourceLocator::StagingFile {
                base: "../x".into(),
                file: staging::webcam_file_name("../x"),
            },
            ..record("")
        };
        assert_eq!(resolve_source(root.path(), "proj1", &escaping), None);
    }

    // `sources.json` is read back by every later build: pin the new tag's
    // spelling literally, never against the struct re-serialized.
    #[test]
    fn a_staging_file_locator_serializes_its_literal_wire_shape() {
        let v = serde_json::to_value(SourceLocator::StagingFile {
            base: "2026-09-20 1432 Demo".to_string(),
            file: "2026-09-20 1432 Demo.webcam.mp4".to_string(),
        })
        .unwrap();
        let literal = serde_json::json!({
            "store": "stagingFile",
            "base": "2026-09-20 1432 Demo",
            "file": "2026-09-20 1432 Demo.webcam.mp4"
        });
        assert_eq!(v, literal);
        assert_eq!(
            serde_json::from_value::<SourceLocator>(literal).unwrap(),
            SourceLocator::StagingFile {
                base: "2026-09-20 1432 Demo".to_string(),
                file: "2026-09-20 1432 Demo.webcam.mp4".to_string(),
            }
        );
    }

    #[test]
    fn a_staging_locator_round_trips_the_adr_example_shape() {
        let v = serde_json::to_value(SourceLocator::Staging {
            base: "2026-09-20 1432 Demo".to_string(),
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({"store": "staging", "base": "2026-09-20 1432 Demo"})
        );
    }

    // ADR §4's native source registry entry, literally: `{ "<assetId>": {
    // "store": ..., "base"?/"file"?: ..., "sha256"?, "size", "durationMs",
    // "width"?, "height"?, "hasAudio", "hasVideo", "mediaKind" } }`.
    // `#[serde(flatten)]` on `locator` is what puts `store`/`base` at the
    // TOP level rather than nested under a `"locator"` key — a
    // struct-re-serialized-against-itself test cannot catch a missing
    // flatten, since both sides would nest identically; only a literal
    // pins the wire shape.
    #[test]
    fn source_record_serializes_the_adr_native_registry_entry_shape() {
        let record = SourceRecord {
            locator: SourceLocator::Staging {
                base: "2026-09-20 1432 Demo".to_string(),
            },
            sha256: None,
            size: 1,
            duration_ms: 1,
            width: None,
            height: None,
            has_audio: true,
            has_video: true,
            media_kind: SourceMediaKind::Video,
            replaced_from: None,
        };
        assert_eq!(
            serde_json::to_value(&record).unwrap(),
            serde_json::json!({
                "store": "staging",
                "base": "2026-09-20 1432 Demo",
                "size": 1,
                "durationMs": 1,
                "hasAudio": true,
                "hasVideo": true,
                "mediaKind": "video",
            }),
        );
    }

    // The `media` locator's own shape: `file`, not `base`, at the top level.
    #[test]
    fn source_record_media_locator_serializes_with_file_not_base() {
        let record = SourceRecord {
            locator: SourceLocator::Media {
                file: "clip.mp4".to_string(),
            },
            sha256: None,
            size: 1,
            duration_ms: 1,
            width: None,
            height: None,
            has_audio: true,
            has_video: true,
            media_kind: SourceMediaKind::Video,
            replaced_from: None,
        };
        assert_eq!(
            serde_json::to_value(&record).unwrap(),
            serde_json::json!({
                "store": "media",
                "file": "clip.mp4",
                "size": 1,
                "durationMs": 1,
                "hasAudio": true,
                "hasVideo": true,
                "mediaKind": "video",
            }),
        );
    }

    // Task 40: a CONFIRMED replacement remembers the identity it replaced,
    // spelled literally; a record written before it (no key) still reads.
    #[test]
    fn a_replaced_source_record_carries_its_former_identity() {
        let json = serde_json::json!({
            "store": "media",
            "file": "a1.mov",
            "sha256": "bb",
            "size": 12,
            "durationMs": 40_000,
            "hasAudio": false,
            "hasVideo": true,
            "mediaKind": "video",
            "replacedFrom": { "size": 30, "durationMs": 31_000, "mediaKind": "video" },
        });
        let record: SourceRecord = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(
            record.replaced_from,
            Some(ReplacedFrom {
                sha256: None,
                size: 30,
                duration_ms: 31_000,
                media_kind: SourceMediaKind::Video,
            })
        );
        assert_eq!(serde_json::to_value(&record).unwrap(), json);
    }

    #[test]
    fn pin_round_trips_through_the_sidecar_extra() {
        let dir = tempfile::tempdir().unwrap();
        let base = "2026-09-20 1432 Demo";
        staging::write_sidecar(dir.path(), base, &sidecar_fixture(base)).unwrap();
        assert_eq!(
            pinned_project(
                &staging::read_sidecar(&dir.path().join(staging::sidecar_file_name(base))).unwrap()
            ),
            None,
            "an unpinned capture must not already report a project"
        );

        pin_staged(dir.path(), base, "proj1").unwrap();
        let pinned =
            staging::read_sidecar(&dir.path().join(staging::sidecar_file_name(base))).unwrap();
        assert_eq!(pinned_project(&pinned).as_deref(), Some("proj1"));

        unpin_staged(dir.path(), base, "proj1").unwrap();
        let unpinned =
            staging::read_sidecar(&dir.path().join(staging::sidecar_file_name(base))).unwrap();
        assert_eq!(pinned_project(&unpinned), None);
    }

    #[test]
    fn unpin_never_clears_a_different_projects_pin() {
        let dir = tempfile::tempdir().unwrap();
        let base = "2026-09-20 1432 Demo";
        staging::write_sidecar(dir.path(), base, &sidecar_fixture(base)).unwrap();
        pin_staged(dir.path(), base, "proj1").unwrap();

        unpin_staged(dir.path(), base, "some-other-project").unwrap();

        let s = staging::read_sidecar(&dir.path().join(staging::sidecar_file_name(base))).unwrap();
        assert_eq!(pinned_project(&s).as_deref(), Some("proj1"));
    }

    #[test]
    fn pin_staged_refuses_a_capture_with_no_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        assert!(pin_staged(dir.path(), "nowhere", "proj1").is_err());
    }

    #[test]
    fn unpin_staged_of_an_already_gone_capture_is_success() {
        let dir = tempfile::tempdir().unwrap();
        unpin_staged(dir.path(), "nowhere", "proj1").expect("nothing to do is success");
    }
}
