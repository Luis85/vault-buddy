//! The project store's own files — `project.json` and `sources.json` today
//! (`products.json`/`recovery.json`/`workspace.json` and the
//! `media\`/`takes\`/`products\`/`cache\`/`jobs\` sub-directories arrive
//! with the tasks that actually populate them). `project_store` owns WHERE
//! a project lives; this module owns what is written there and how it is
//! read back.
//!
//! Every write here rides the rails `global-constraints.md`'s
//! Project-store-writes bullet names: `capture_note::write_atomic_replacing`
//! for the store's own JSON files (temp + fsync + replacing rename — never
//! `std::fs::write`, which truncates before the first new byte lands and
//! would lose the whole document to a crash mid-write). Deletion is
//! owned-file-only and no-follow, never `remove_dir_all` on anything not
//! proven to be a project directory this app created.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::{
    self, is_valid_id, limits, validate_envelope, EditorError, EditorErrorCode, Map, Project,
    Record, WorkspaceEnvelope,
};

use super::project_store::{project_dir, store_dir, SourceRecord};

pub(crate) const PROJECT_FILE: &str = "project.json";
pub(crate) const SOURCES_FILE: &str = "sources.json";
pub(crate) const RECOVERY_FILE: &str = "recovery.json";

fn invalid_id(id: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("{id:?} is not a valid project id"),
    )
}

fn invalid_id_err(id: &str) -> EditorError {
    EditorError::new(
        EditorErrorCode::InvalidRequest,
        format!("{id:?} is not a valid project id"),
    )
}

/// The write seam every store file goes through (`editor_save_project`'s own
/// tests, `save_commands.rs`): a real `io::Error` from a full disk or a
/// denied permission is impractical to reproduce portably from a test, so
/// this trait lets a test inject one instead of touching the filesystem's
/// own failure modes. `RealWriter` — `capture_note::write_atomic_replacing`,
/// exactly what every store write used before this seam existed — is what
/// every production caller gets; nothing here changes the on-disk write
/// itself (temp + fsync + replacing rename), only where it is dispatched
/// from.
pub trait ProjectWriter {
    fn write(&self, path: &Path, content: &str) -> io::Result<()>;
}

/// The production `ProjectWriter`: `write_atomic_replacing`, unchanged.
pub struct RealWriter;

impl ProjectWriter for RealWriter {
    fn write(&self, path: &Path, content: &str) -> io::Result<()> {
        write_atomic_replacing(path, content)
    }
}

fn write_json_with<T: Serialize>(
    writer: &dyn ProjectWriter,
    path: &Path,
    value: &T,
) -> io::Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writer.write(path, &json)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    write_json_with(&RealWriter, path, value)
}

/// Create a brand-new project directory and its two founding files.
///
/// **Exclusive create for the LEAF** (`std::fs::create_dir`, never
/// `create_dir_all`): a duplicate project id must be refused loudly rather
/// than silently adopted — two callers minting the same id would otherwise
/// interleave their writes into one directory. The STORE's own root
/// (`editor-projects`) is the opposite case — it is shared by every
/// project, so ensuring it exists is `create_dir_all`, same as the
/// audio/screen capture roots elsewhere in this app.
///
/// `project` is the bare graph a caller (Task 10's `editor_open_staged`, a
/// portable-package import) has just built or migrated; this function is
/// what turns it into a full `WorkspaceEnvelope` at revision 1 — a fresh
/// `Record` with no products yet, an empty `workspace` preference blob, and
/// `saved_at`/`created_at`/`updated_at` all stamped now. `sources.json` is
/// written FIRST: a project whose graph references a source id that
/// `sources.json` does not carry yet is a worse failure mode than a
/// `sources.json` with no `project.json` to go with it (the load side
/// refuses the latter outright; nothing reads sources.json without a valid
/// project.json beside it).
pub fn create_project(
    root: &Path,
    project: &Project,
    sources: &BTreeMap<String, SourceRecord>,
) -> io::Result<()> {
    let dir = project_dir(root, &project.id).ok_or_else(|| invalid_id(&project.id))?;
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir(&dir)?;
    let now = chrono::Local::now().to_rfc3339();
    let envelope = WorkspaceEnvelope {
        schema: editor::WORKSPACE_SCHEMA.to_string(),
        project: project.clone(),
        workspace: serde_json::json!({}),
        record: Record {
            id: project.id.clone(),
            revision: 1,
            created_at: now.clone(),
            updated_at: now.clone(),
            products: Vec::new(),
            extra: Map::new(),
        },
        saved_at: now,
        extra: Map::new(),
    };
    write_json(&dir.join(SOURCES_FILE), sources)?;
    write_json(&dir.join(PROJECT_FILE), &envelope)?;
    Ok(())
}

/// Persist an already-built envelope — the ongoing-save path
/// (`editor_save_project`), as opposed to `create_project`'s one-time mint.
/// Only `project.json` is rewritten: `sources.json` changes far less often
/// (only when a source is added/relinked) and is written by whatever
/// operation actually changes it, not on every ordinary save.
///
/// Takes the `ProjectWriter` explicitly rather than defaulting to
/// `RealWriter` internally: `editor_save_project`'s own tests inject a
/// `ProjectWriter` that fails (disk-full, permission-denied) to prove a
/// failed save leaves the previous file untouched, without touching the
/// real filesystem's own failure modes. Every production caller passes
/// `&RealWriter`.
pub fn commit_project(
    writer: &dyn ProjectWriter,
    root: &Path,
    id: &str,
    envelope: &WorkspaceEnvelope,
) -> io::Result<()> {
    let dir = project_dir(root, id).ok_or_else(|| invalid_id(id))?;
    write_json_with(writer, &dir.join(PROJECT_FILE), envelope)
}

pub(crate) fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, EditorError> {
    let meta = std::fs::metadata(path).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Cannot read {}: {e}", path.display()),
        )
    })?;
    if meta.len() > max_bytes {
        return Err(EditorError::new(
            EditorErrorCode::InvalidProject,
            format!(
                "{} is {} bytes, exceeding the {max_bytes} byte maximum",
                path.display(),
                meta.len()
            ),
        ));
    }
    std::fs::read(path).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Cannot read {}: {e}", path.display()),
        )
    })
}

/// Whether SOMETHING already occupies `id`'s `project.json` path — the ONE
/// condition `editor_save_project` (`save_commands.rs`) may degrade its
/// read of the last-saved envelope for (Task 12 fix round 2, finding 1).
/// Checked BEFORE `load_project` is ever called: every other read failure
/// — a corrupt or oversized `project.json`, `PermissionDenied`, a Windows
/// sharing violation, a missing/unreadable/corrupt `sources.json` beside a
/// perfectly valid `project.json` — means something IS there and IS
/// meaningful, so the save must refuse rather than silently overwrite it
/// with a fresh `{}` workspace.
///
/// `Path::try_exists`, never `Path::exists` or `is_file` (Task 12 review,
/// carried to Task 37): `exists` answers `false` for ANY metadata failure —
/// a permission problem, an invalid name, a sharing violation — which the
/// save would then misread as "never saved" and degrade on. Only a
/// genuine not-found is `Ok(false)`; every other failure is an `Err` the
/// save refuses on. A DIRECTORY swapped in for `project.json` (this
/// module's tests' portable stand-in for an unreadable file) still reports
/// present, so the save refuses rather than overwriting it.
pub fn project_file_exists(root: &Path, id: &str) -> io::Result<bool> {
    match project_dir(root, id) {
        Some(dir) => dir.join(PROJECT_FILE).try_exists(),
        None => Ok(false),
    }
}

/// Load a project's saved envelope and its sources, refusing anything that
/// does not validate.
///
/// **Bounded, and read-only on every path.** The size check runs before a
/// single byte is read into memory (`limits::MAX_PROJECT_JSON_BYTES`), so
/// an absurdly large `project.json` cannot be used to exhaust memory just
/// by being opened. A malformed or oversized file is reported as
/// `invalidProject` and the file is NEVER rewritten here — there is no
/// write in this function at all, so a caller can retry, inspect, or hand
/// the project to Discard without this function ever having touched disk
/// a second time.
pub fn load_project(
    root: &Path,
    id: &str,
) -> Result<(WorkspaceEnvelope, BTreeMap<String, SourceRecord>), EditorError> {
    let dir = project_dir(root, id).ok_or_else(|| invalid_id_err(id))?;
    let project_path = dir.join(PROJECT_FILE);
    let bytes = read_bounded(&project_path, limits::MAX_PROJECT_JSON_BYTES)?;
    let envelope: WorkspaceEnvelope = serde_json::from_slice(&bytes).map_err(|e| {
        EditorError::new(
            EditorErrorCode::InvalidProject,
            format!(
                "{} is not a valid project file: {e}",
                project_path.display()
            ),
        )
    })?;
    validate_envelope(&envelope)?;
    let sources = read_sources(&dir)?;
    Ok((envelope, sources))
}

/// A project's `sources.json` alone — what discarding a project needs to
/// know (which staged capture it pinned) without trusting or validating the
/// graph it is about to delete.
pub fn load_sources(root: &Path, id: &str) -> Result<BTreeMap<String, SourceRecord>, EditorError> {
    let dir = project_dir(root, id).ok_or_else(|| invalid_id_err(id))?;
    read_sources(&dir)
}

/// Rewrite a project's `sources.json` — the media import's per-file
/// registration (Task 25). Same rails as every other store file
/// (`write_atomic_replacing`); the caller holds the session's save lock
/// across its load-modify-write so two writers never lose each other's
/// entry and a discard never removes the directory mid-write.
pub fn write_sources(
    root: &Path,
    id: &str,
    sources: &BTreeMap<String, SourceRecord>,
) -> io::Result<()> {
    let dir = project_dir(root, id).ok_or_else(|| invalid_id(id))?;
    write_json(&dir.join(SOURCES_FILE), sources)
}

fn read_sources(dir: &Path) -> Result<BTreeMap<String, SourceRecord>, EditorError> {
    let path = dir.join(SOURCES_FILE);
    let bytes = std::fs::read(&path).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Cannot read {}: {e}", path.display()),
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("{} is not valid: {e}", path.display()),
        )
    })
}

/// One row of `editor_list_projects` (Contract reference `ProjectSummaryDto`
/// — not yet defined anywhere else, so this is its home). Every field is
/// the exact camelCase spelling the contract names; `sourceBase` is the
/// staging base a project was adopted from, when it has one, so the picker
/// can say "from the capture you just made" rather than only a title.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummaryDto {
    pub project_file_id: String,
    pub title: String,
    pub updated_at: String,
    pub persisted_revision: u64,
    pub has_recovery: bool,
    pub source_base: Option<String>,
}

/// `pub(crate)`: also `editor_open_project`'s (`save_commands.rs`) source
/// for `EditorOpenResult.sourceBase` when reopening a project by id rather
/// than by the staged capture that minted it.
pub(crate) fn source_base_of(sources: &BTreeMap<String, SourceRecord>) -> Option<String> {
    sources.values().find_map(|r| match &r.locator {
        super::project_store::SourceLocator::Staging { base } => Some(base.clone()),
        _ => None,
    })
}

/// Every project the store holds, newest-saved first.
///
/// A VIEW, so it degrades like every other listing in this app
/// (AGENTS.md's view-may-degrade / guard-must-refuse split): an unreadable
/// store directory is an empty list, and any entry this cannot trust — a
/// directory name that fails `is_valid_id`, a `project.json` that will not
/// parse or validate, or one whose OWN `project.id` disagrees with the
/// directory it was found in — is skipped rather than aborting the whole
/// listing. `sources.json` is read best-effort too, purely for
/// `sourceBase`; a project missing or malformed sources still lists, just
/// without that one extra fact.
pub fn list_projects(root: &Path) -> Vec<ProjectSummaryDto> {
    let Ok(entries) = std::fs::read_dir(store_dir(root)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if !is_valid_id(&id) {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path().join(PROJECT_FILE)) else {
            continue;
        };
        let Ok(envelope) = serde_json::from_slice::<WorkspaceEnvelope>(&bytes) else {
            continue;
        };
        if envelope.project.id != id {
            continue;
        }
        if let Err(e) = validate_envelope(&envelope) {
            log::warn!(
                "editor project store: {} parses but does not validate, skipping: {}",
                id,
                e.message
            );
            continue;
        }
        let sources = read_sources(&entry.path()).unwrap_or_default();
        out.push(ProjectSummaryDto {
            project_file_id: id,
            title: envelope.project.title.clone(),
            updated_at: envelope.record.updated_at.clone(),
            persisted_revision: envelope.record.revision,
            has_recovery: entry.path().join(RECOVERY_FILE).is_file(),
            source_base: source_base_of(&sources),
        });
    }
    out.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.project_file_id.cmp(&b.project_file_id))
    });
    out
}

/// Permanently remove a project directory — an irreversible unlink, not a
/// trash, mirroring `delete_task`'s posture on the vault side.
///
/// **Ownership is proven before anything is deleted**: `project.json` must
/// parse AND its own `project.id` must equal `id`. Without that second
/// check, a directory whose name and content disagree (hand-edited, or a
/// caller that passed the wrong id) could have the WRONG project's
/// `project.json` believed and the RIGHT directory deleted anyway — the
/// check is what makes "removed only what it verified" a property of this
/// function rather than of whichever id the caller happened to pass.
///
/// **No-follow, structurally**: every entry the walk visits is inspected
/// with `symlink_metadata` before it is trusted to be a plain file or
/// directory, and a symlink anywhere in the tree refuses the WHOLE removal
/// before a single file is unlinked — `discard_staged_files`'s two-pass
/// discipline, widened from a flat file set to a directory tree. Files are
/// removed, then directories deepest-first via `remove_dir` (never
/// `remove_dir_all`, which would recurse without this module's own
/// symlink check at every level).
///
/// **The project directory itself is checked first, before its
/// `project.json` is ever read.** `walk_no_follow` only inspects what is
/// INSIDE `dir` — if `dir` were itself a symlink or (on Windows) a
/// junction, `read_dir`/`read` on it transparently follow it to whatever it
/// points at, so both the ownership check and the walk would operate on a
/// directory this function never created. Rust reports an NTFS junction as
/// a symlink too, so one `is_symlink()` check here covers both.
pub fn remove_project(root: &Path, id: &str) -> Result<(), EditorError> {
    let dir = project_dir(root, id).ok_or_else(|| invalid_id_err(id))?;
    let dir_meta = std::fs::symlink_metadata(&dir).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Cannot resolve {}: {e}", dir.display()),
        )
    })?;
    if dir_meta.file_type().is_symlink() {
        return Err(EditorError::new(
            EditorErrorCode::Internal,
            format!(
                "{} is a symlink or junction; refusing to remove through it",
                dir.display()
            ),
        ));
    }
    let project_path = dir.join(PROJECT_FILE);
    let bytes = std::fs::read(&project_path).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Cannot read {}: {e}", project_path.display()),
        )
    })?;
    let envelope: WorkspaceEnvelope = serde_json::from_slice(&bytes).map_err(|e| {
        EditorError::new(
            EditorErrorCode::InvalidProject,
            format!(
                "{} is not a valid project file: {e}",
                project_path.display()
            ),
        )
    })?;
    if envelope.project.id != id {
        return Err(EditorError::new(
            EditorErrorCode::InvalidProject,
            format!(
                "refusing to remove {}: its own project id does not match {id:?}",
                dir.display()
            ),
        ));
    }
    remove_dir_no_follow(&dir).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Could not remove {}: {e}", dir.display()),
        )
    })
}

/// Collect every file and directory under `dir` (never following a
/// symlink), then remove the files and the directories deepest-first.
/// Two passes on purpose: a symlink discovered partway through the walk
/// must refuse the WHOLE removal, which is only possible when nothing has
/// been unlinked yet.
pub(crate) fn remove_dir_no_follow(dir: &Path) -> io::Result<()> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    walk_no_follow(dir, &mut files, &mut dirs)?;
    for file in &files {
        std::fs::remove_file(file)?;
    }
    // The walk pushes a directory before it descends into it, so reversing
    // the collected order removes children before their parents.
    dirs.reverse();
    for d in &dirs {
        std::fs::remove_dir(d)?;
    }
    Ok(())
}

fn walk_no_follow(dir: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{} is a symlink; refusing to remove through it",
                    path.display()
                ),
            ));
        }
        if meta.is_dir() {
            dirs.push(path.clone());
            walk_no_follow(&path, files, dirs)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::project_store::minimal_project;
    use super::*;

    #[test]
    fn create_then_load_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let project = minimal_project("proj1");
        let mut sources = BTreeMap::new();
        sources.insert(
            "src1".to_string(),
            SourceRecord {
                locator: super::super::project_store::SourceLocator::Staging {
                    base: "2026-09-20 1432 Demo".to_string(),
                },
                sha256: None,
                size: 1_234,
                duration_ms: 60_000,
                width: Some(1920),
                height: Some(1080),
                has_audio: true,
                has_video: true,
                media_kind: super::super::project_store::SourceMediaKind::Video,
                replaced_from: None,
            },
        );

        create_project(root.path(), &project, &sources).expect("create succeeds");
        let (envelope, back_sources) = load_project(root.path(), "proj1").expect("load succeeds");

        assert_eq!(envelope.project, project);
        assert_eq!(envelope.record.revision, 1);
        assert_eq!(envelope.record.products.len(), 0);
        assert_eq!(back_sources, sources);
    }

    #[test]
    fn create_project_refuses_a_duplicate_id() {
        let root = tempfile::tempdir().unwrap();
        let project = minimal_project("proj1");
        create_project(root.path(), &project, &BTreeMap::new()).unwrap();
        let err = create_project(root.path(), &project, &BTreeMap::new())
            .expect_err("a second create under the same id must be refused");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn load_refuses_an_oversized_file() {
        // A VALID, well-formed envelope — not `b'x'` garbage, which the
        // earlier version of this test used and which is malformed JSON on
        // its own. That fixture passed for the wrong reason: dropping the
        // size check entirely left the parse failure to refuse it anyway,
        // so the test stayed green under a mutation that deleted the very
        // check it claims to cover. JSON tolerates trailing whitespace, so
        // padding a real envelope past the byte cap keeps it perfectly
        // loadable — except for the size check this test exists to pin.
        let root = tempfile::tempdir().unwrap();
        create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
        let dir = project_dir(root.path(), "proj1").unwrap();
        let path = dir.join(PROJECT_FILE);
        let json = std::fs::read_to_string(&path).unwrap();
        let pad = (limits::MAX_PROJECT_JSON_BYTES as usize + 1).saturating_sub(json.len());
        let padded = format!("{json}{}", " ".repeat(pad));
        assert!(padded.len() as u64 > limits::MAX_PROJECT_JSON_BYTES);
        std::fs::write(&path, padded.as_bytes()).unwrap();

        let err =
            load_project(root.path(), "proj1").expect_err("an oversized file must be refused");
        assert_eq!(err.code, EditorErrorCode::InvalidProject);
        assert!(
            err.message.contains("byte"),
            "expected a size-specific message, got: {}",
            err.message
        );
    }

    // A27: a malformed `project.json` must be reported, not "repaired" —
    // `load_project` contains no write at all, so the file the user's disk
    // holds is exactly what it held before the failed load.
    #[test]
    fn malformed_project_is_reported_and_left_byte_identical() {
        let root = tempfile::tempdir().unwrap();
        let dir = project_dir(root.path(), "proj1").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let before = b"{ not actually json".to_vec();
        std::fs::write(dir.join(PROJECT_FILE), &before).unwrap();

        let err = load_project(root.path(), "proj1").expect_err("malformed JSON must be refused");
        assert_eq!(err.code, EditorErrorCode::InvalidProject);
        let after = std::fs::read(dir.join(PROJECT_FILE)).unwrap();
        assert_eq!(after, before, "a failed load must never rewrite the file");
    }

    #[test]
    fn commit_project_persists_a_new_revision() {
        let root = tempfile::tempdir().unwrap();
        let project = minimal_project("proj1");
        create_project(root.path(), &project, &BTreeMap::new()).unwrap();
        let (mut envelope, _) = load_project(root.path(), "proj1").unwrap();
        envelope.record.revision = 2;
        envelope.record.updated_at = "2026-09-22T00:00:00Z".to_string();

        commit_project(&RealWriter, root.path(), "proj1", &envelope).unwrap();

        let (reloaded, _) = load_project(root.path(), "proj1").unwrap();
        assert_eq!(reloaded.record.revision, 2);
    }

    #[test]
    fn list_projects_reports_every_valid_project_and_skips_what_it_cannot_trust() {
        let root = tempfile::tempdir().unwrap();
        create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
        create_project(root.path(), &minimal_project("proj2"), &BTreeMap::new()).unwrap();
        // A directory whose own project.id disagrees with its name.
        let mismatched = project_dir(root.path(), "proj3").unwrap();
        std::fs::create_dir_all(&mismatched).unwrap();
        let (mut envelope, _) = load_project(root.path(), "proj1").unwrap();
        envelope.project.id = "somewhere-else".to_string();
        write_json(&mismatched.join(PROJECT_FILE), &envelope).unwrap();

        let rows = list_projects(root.path());
        let ids: Vec<&str> = rows.iter().map(|r| r.project_file_id.as_str()).collect();
        assert!(ids.contains(&"proj1"));
        assert!(ids.contains(&"proj2"));
        assert!(!ids.contains(&"somewhere-else"));
        assert_eq!(rows.len(), 2, "the mismatched directory must be skipped");
    }

    // The doc says "will not parse OR validate" — this pins the second
    // half, which nothing else here exercised: a file that is perfectly
    // well-formed JSON matching the envelope shape, but semantically
    // invalid (`validate_project`'s own rules), must not be listed either.
    #[test]
    fn list_projects_skips_a_project_that_parses_but_fails_semantic_validation() {
        let root = tempfile::tempdir().unwrap();
        create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
        let (mut envelope, _) = load_project(root.path(), "proj1").unwrap();
        envelope.project.master_gain = 5.0; // out of validate_project's [0,1] range
        let dir = project_dir(root.path(), "proj1").unwrap();
        write_json(&dir.join(PROJECT_FILE), &envelope).unwrap();

        let rows = list_projects(root.path());
        assert!(
            rows.is_empty(),
            "an invalid-but-parseable project must not be listed"
        );
    }

    #[test]
    fn remove_project_refuses_a_directory_whose_project_id_differs() {
        let root = tempfile::tempdir().unwrap();
        create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
        // Overwrite project.json under a DIFFERENT id than the directory name.
        let (mut envelope, _) = load_project(root.path(), "proj1").unwrap();
        envelope.project.id = "not-proj1".to_string();
        let dir = project_dir(root.path(), "proj1").unwrap();
        write_json(&dir.join(PROJECT_FILE), &envelope).unwrap();

        let err =
            remove_project(root.path(), "proj1").expect_err("an id mismatch must refuse removal");
        assert_eq!(err.code, EditorErrorCode::InvalidProject);
        assert!(dir.is_dir(), "the directory must survive a refused removal");
    }

    #[test]
    fn remove_project_removes_everything_it_created() {
        let root = tempfile::tempdir().unwrap();
        create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
        let dir = project_dir(root.path(), "proj1").unwrap();
        std::fs::create_dir_all(dir.join("media")).unwrap();
        std::fs::write(dir.join("media").join("clip.mp4"), b"x").unwrap();

        remove_project(root.path(), "proj1").expect("removal succeeds");

        assert!(!dir.exists());
    }

    // Unix-only: a Windows symlink needs `SeCreateSymbolicLinkPrivilege`
    // (Developer Mode or an elevated process). Skip VISIBLY rather than
    // silently passing when this account lacks it, the `981bf67` posture
    // `export_worker::vault_dir`'s own symlink-escape test already uses for
    // the exact same reason.
    #[test]
    fn remove_project_never_follows_a_symlink() {
        let root = tempfile::tempdir().unwrap();
        create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
        let dir = project_dir(root.path(), "proj1").unwrap();
        std::fs::create_dir_all(dir.join("media")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let precious = outside.path().join("precious.bin");
        std::fs::write(&precious, b"not ours").unwrap();
        let link = dir.join("media").join("linked.bin");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&precious, &link).unwrap();
        #[cfg(not(unix))]
        {
            if let Err(e) = std::os::windows::fs::symlink_file(&precious, &link) {
                if e.raw_os_error() == Some(1314) {
                    eprintln!(
                        "SKIP: remove_project_never_follows_a_symlink — symlink_file needs \
                         SeCreateSymbolicLinkPrivilege (Developer Mode or an elevated process); \
                         this account lacks it (OS error 1314)"
                    );
                    return;
                }
                panic!("symlink_file failed unexpectedly: {e}");
            }
        }

        let err = remove_project(root.path(), "proj1").expect_err("a symlink must refuse removal");
        assert_eq!(err.code, EditorErrorCode::Internal);
        assert!(precious.is_file(), "the removal followed the symlink");
        assert!(
            dir.is_dir(),
            "a refused removal must leave the directory in place"
        );
    }

    // Finding: `walk_no_follow` only inspects what is INSIDE `dir` — it
    // never checked `dir` itself. A REAL project sits outside the store, at
    // a path a symlink (or, on Windows, an NTFS junction — reported as a
    // symlink by Rust too) stands in for at the expected project location:
    // both the ownership read AND `read_dir` would silently follow it, so
    // the walk would delete files that were never this app's to remove.
    #[test]
    fn remove_project_refuses_a_symlinked_project_directory_itself() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        // A real project OUTSIDE the store — with a valid, matching
        // project.json, so if the link were followed the ownership check
        // would actually pass and only the walk's own no-follow discipline
        // would be left to save it.
        create_project(outside.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
        let real_dir = project_dir(outside.path(), "proj1").unwrap();
        let link = project_dir(root.path(), "proj1").unwrap();
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_dir, &link).unwrap();
        #[cfg(not(unix))]
        {
            if let Err(e) = std::os::windows::fs::symlink_dir(&real_dir, &link) {
                if e.raw_os_error() == Some(1314) {
                    eprintln!(
                        "SKIP: remove_project_refuses_a_symlinked_project_directory_itself — \
                         symlink_dir needs SeCreateSymbolicLinkPrivilege (Developer Mode or an \
                         elevated process); this account lacks it (OS error 1314)"
                    );
                    return;
                }
                panic!("symlink_dir failed unexpectedly: {e}");
            }
        }

        let err = remove_project(root.path(), "proj1")
            .expect_err("a symlinked project directory must refuse removal");
        assert_eq!(err.code, EditorErrorCode::Internal);
        assert!(
            real_dir.join(PROJECT_FILE).is_file(),
            "the real project was removed through the link"
        );
    }

    #[test]
    fn project_summary_dto_serializes_camel_case_literal() {
        let dto = ProjectSummaryDto {
            project_file_id: "abc123".into(),
            title: "My Tutorial".into(),
            updated_at: "2026-09-21T10:00:00+02:00".into(),
            persisted_revision: 3,
            has_recovery: true,
            source_base: Some("2026-09-20 1432 Demo".into()),
        };
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({
                "projectFileId": "abc123",
                "title": "My Tutorial",
                "updatedAt": "2026-09-21T10:00:00+02:00",
                "persistedRevision": 3,
                "hasRecovery": true,
                "sourceBase": "2026-09-20 1432 Demo",
            }),
        );
    }

    #[test]
    fn project_summary_dto_carries_a_null_source_base_when_none() {
        let dto = ProjectSummaryDto {
            project_file_id: "abc123".into(),
            title: "Untitled".into(),
            updated_at: "2026-09-21T10:00:00+02:00".into(),
            persisted_revision: 1,
            has_recovery: false,
            source_base: None,
        };
        assert_eq!(
            serde_json::to_value(&dto).unwrap()["sourceBase"],
            serde_json::Value::Null
        );
    }
}
