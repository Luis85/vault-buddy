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
use std::time::Duration;

use serde::Serialize;
use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::editor::{
    self, is_valid_id, limits, validate_envelope, EditorError, EditorErrorCode, Map, Project,
    Record, WorkspaceEnvelope,
};

use super::project_store::{project_dir, store_dir, SourceRecord};
use super::redact::redact_path;

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

/// The suffix of a project's build directory, `.<projectId>.creating`
/// (review finding D-4) — `package_import`'s `.importing`, for a mint.
const CREATING_SUFFIX: &str = ".creating";

/// The project id a store entry named `.<id>.creating` was building, if it
/// is one — the startup sweep's ownership test (`store_sweep`). A leading
/// dot is never a valid project id, so `list_projects`, the re-pin sweep
/// and every other store walk skip such a directory already.
pub(crate) fn creating_project_id(name: &str) -> Option<&str> {
    name.strip_prefix('.')?
        .strip_suffix(CREATING_SUFFIX)
        .filter(|id| is_valid_id(id))
}

/// A project being built aside: removed (owned, no-follow) unless it was
/// installed. A CRASH skips the drop and leaves it for the startup sweep.
struct CreatingDir {
    dir: PathBuf,
    installed: bool,
}

impl CreatingDir {
    fn create(root: &Path, id: &str) -> io::Result<Self> {
        let dir = store_dir(root).join(format!(".{id}{CREATING_SUFFIX}"));
        std::fs::create_dir(&dir)?;
        Ok(Self {
            dir,
            installed: false,
        })
    }

    /// The one rename that makes the project exist.
    ///
    /// **Retried on `PermissionDenied`** (hardening Task 10 review, carried
    /// finding 3): a real-time AV scanner or indexer can hold `sources.json`
    /// or `project.json` open for a moment right after `write_json_with`
    /// returns — GAP-169's own "Access denied" history — and Windows refuses
    /// to rename a directory while a file inside it is open. This rides that
    /// out the way `delete_transcription_model` rides out a live mmap; any
    /// OTHER rename failure (a genuine id collision, an invalid target) is
    /// not retried and is returned on the first attempt.
    fn install(mut self, target: &Path) -> io::Result<()> {
        retry_permission_denied(|| std::fs::rename(&self.dir, target))?;
        self.installed = true;
        Ok(())
    }
}

/// Retry `op` while it fails with `PermissionDenied`, up to five attempts
/// 100 ms apart; any other error, or the fifth attempt's, is returned at
/// once. Pulled out of `CreatingDir::install` so the backoff itself is
/// unit-testable without a real file lock to race.
fn retry_permission_denied<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut last_err = None;
    for attempt in 0..5 {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && attempt < 4 => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last_err.expect("the loop above ran at least once"))
}

impl Drop for CreatingDir {
    fn drop(&mut self) {
        if self.installed {
            return;
        }
        if let Err(e) = remove_dir_no_follow(&self.dir) {
            log::warn!(
                "editor project store: could not remove an unfinished project build ({:?})",
                e.kind()
            );
        }
    }
}

/// Create a brand-new project directory and its two founding files.
///
/// **Built aside, installed last** (review finding D-4): both files are
/// written into `editor-projects\.<id>.creating\` and that directory is
/// renamed to the project's own name in ONE step, so the project folder
/// never exists without its `project.json` — a failure removes the build
/// directory, and a crash leaves it for the startup sweep
/// (`store_sweep`), which removes it once it is an hour old. It used to be
/// created in place, `sources.json` first, so a crash between the two
/// writes left a project folder no ownership proof could ever prove.
///
/// **A duplicate id is refused loudly** (`AlreadyExists`) rather than
/// silently adopted — two callers minting the same id would otherwise
/// interleave their writes into one directory. The build directory itself
/// is an exclusive `create_dir`, and every mint holds `EditorState::open`
/// across the existence check and the rename (the import's `choose_id`
/// discipline). The STORE's own root (`editor-projects`) is shared by
/// every project, so ensuring it exists is `create_dir_all`.
///
/// `project` is the bare graph a caller (Task 10's `editor_open_staged`)
/// has just built or migrated; this function is what turns it into a full
/// `WorkspaceEnvelope` at revision 1 — a fresh `Record` with no products
/// yet, an empty `workspace` preference blob, and
/// `saved_at`/`created_at`/`updated_at` all stamped now.
pub fn create_project(
    root: &Path,
    project: &Project,
    sources: &BTreeMap<String, SourceRecord>,
) -> io::Result<()> {
    create_project_with(&RealWriter, root, project, sources)
}

/// `create_project` through an injectable writer (its failure test).
pub(crate) fn create_project_with(
    writer: &dyn ProjectWriter,
    root: &Path,
    project: &Project,
    sources: &BTreeMap<String, SourceRecord>,
) -> io::Result<()> {
    let dir = project_dir(root, &project.id).ok_or_else(|| invalid_id(&project.id))?;
    std::fs::create_dir_all(store_dir(root))?;
    match std::fs::symlink_metadata(&dir) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("project {:?} already exists", project.id),
            ))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let build = CreatingDir::create(root, &project.id)?;
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
    write_json_with(writer, &build.dir.join(SOURCES_FILE), sources)?;
    write_json_with(writer, &build.dir.join(PROJECT_FILE), &envelope)?;
    build.install(&dir)
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
    let json = serde_json::to_string_pretty(envelope)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    // Never write a `project.json` that `load_project` would refuse to read
    // back (fix round 1): the last good file stays where it is instead.
    if json.len() as u64 > limits::MAX_PROJECT_JSON_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!(
                "it would be {} bytes, past the {} byte limit a project can be reopened at",
                json.len(),
                limits::MAX_PROJECT_JSON_BYTES
            ),
        ));
    }
    writer.write(&dir.join(PROJECT_FILE), &json)
}

pub(crate) fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, EditorError> {
    use std::io::Read;
    let cannot = |e: io::Error| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Cannot read the file {}: {e}", redact_path(path)),
        )
    };
    let too_big = |len: u64| {
        EditorError::new(
            EditorErrorCode::InvalidProject,
            format!(
                "The file {} is {len} bytes, exceeding the {max_bytes} byte maximum",
                redact_path(path)
            ),
        )
    };
    // No-follow (final review M4): a store file replaced by a link is not
    // read through, and the size checked is the size of what is read.
    let meta = std::fs::symlink_metadata(path).map_err(cannot)?;
    if !meta.is_file() {
        return Err(EditorError::new(
            EditorErrorCode::Internal,
            format!(
                "The file {} is not a plain file; it was not read",
                redact_path(path)
            ),
        ));
    }
    if meta.len() > max_bytes {
        return Err(too_big(meta.len()));
    }
    // Bounded again while reading: the file may grow after the check.
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(max_bytes + 1).read_to_end(&mut bytes))
        .map_err(cannot)?;
    if bytes.len() as u64 > max_bytes {
        return Err(too_big(bytes.len() as u64));
    }
    Ok(bytes)
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
                "The project file {} is not valid: {e}",
                redact_path(&project_path)
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
    let bytes = read_bounded(&path, limits::MAX_PROJECT_JSON_BYTES)?;
    // `invalidProject`, not `internal` (final review I3): a sources file
    // that does not parse is a damaged project — no retry opens it — and
    // `editor_open_staged` re-migrates a capture pinned to one.
    serde_json::from_slice(&bytes).map_err(|e| {
        EditorError::new(
            EditorErrorCode::InvalidProject,
            format!("The sources file {} is not valid: {e}", redact_path(&path)),
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
        let Ok(bytes) = read_bounded(
            &entry.path().join(PROJECT_FILE),
            limits::MAX_PROJECT_JSON_BYTES,
        ) else {
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

/// The most of `project.json` the ownership proof reads: 64 MiB, eight times
/// the LOAD bound, because a `project.json` past the load bound (hand-edited,
/// or installed by a build before the import measured what it writes,
/// M-V2) is exactly the project that cannot open and must stay discardable
/// (GAP-214 item 1). ONE named bound for every remover (review ruling R2).
/// The proof deserializes only `/project/id` (`OwnerDocument`), so this is
/// the read's size, never a 64 MiB JSON tree.
const OWNERSHIP_PROOF_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// All of `project.json` the ownership proof looks at: `/project/id`.
/// Every other field is skipped by serde without being built.
#[derive(serde::Deserialize)]
struct OwnerDocument {
    project: OwnerProject,
}

#[derive(serde::Deserialize)]
struct OwnerProject {
    id: String,
}

/// Whether a staged capture's pin names a project that still exists —
/// THREE states, because the answer decides whether a Discard or Clear
/// deletes the recording (review finding D-2; Task 4 fix round 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinLiveness {
    /// `project.json` is there (anything wearing the name counts).
    Live,
    /// Provably gone: the id names no project, or `project.json` is
    /// `NotFound`. Only this pins nothing.
    Gone,
    /// Any other error reading it (access denied, a sharing violation, an
    /// antivirus hold): the project may well exist, so every caller treats
    /// it as `Live` — never as a licence to delete.
    Unknown,
}

/// The pure half of `pin_liveness`: what a `symlink_metadata` answer on
/// `project.json` says. Only `NotFound` is "gone" — `Path::is_file`, which
/// this replaces, read EVERY error as gone and so let a Discard delete the
/// capture of a project it merely could not see.
pub(crate) fn liveness_of(probe: &io::Result<std::fs::Metadata>) -> PinLiveness {
    match probe {
        Ok(_) => PinLiveness::Live,
        Err(e) if e.kind() == io::ErrorKind::NotFound => PinLiveness::Gone,
        Err(_) => PinLiveness::Unknown,
    }
}

/// Does the pin `project_id` name a project that still exists? A pin whose
/// project folder, or that folder's `project.json`, is gone pins NOTHING —
/// the editor's own open already re-adopts such a capture — so it neither
/// refuses a staged capture's Discard nor keeps it out of Clear. An id that
/// is not a valid id cannot name a project. No-follow: a link wearing the
/// name is `Live`, which only ever keeps a capture.
pub(crate) fn pin_liveness(root: &Path, project_id: &str) -> PinLiveness {
    let Some(dir) = project_dir(root, project_id) else {
        return PinLiveness::Gone;
    };
    let probe = std::fs::symlink_metadata(dir.join(PROJECT_FILE));
    let liveness = liveness_of(&probe);
    if let (PinLiveness::Unknown, Err(e)) = (liveness, &probe) {
        log::warn!(
            "a staged capture's pin to project {project_id} could not be checked ({:?}); \
             treating it as live",
            e.kind()
        );
    }
    liveness
}

/// Prove that the folder `project_dir(root, id)` is `id`'s own, before
/// anything acts on that belief — the ONE ownership check every remover
/// calls (`remove_project`, and both discards before they release a pin,
/// GAP-214 item 8).
///
/// `project.json` must be JSON AND its own `project.id` must equal `id`
/// (the document need not be a valid project any more — final review I3:
/// a damaged project is exactly the one that must stay discardable, and its
/// own `project.id` is still the proof of whose folder this is). Without
/// that second check, a directory whose name and content disagree
/// (hand-edited, or a caller that passed the wrong id) could have the WRONG
/// project's `project.json` believed and the RIGHT directory deleted anyway.
///
/// **The project directory itself is checked first, before its
/// `project.json` is ever read.** If `dir` were itself a symlink or (on
/// Windows) a junction, `read_dir`/`read` on it transparently follow it to
/// whatever it points at, so the proof would be about a directory this app
/// never created. Rust reports an NTFS junction as a symlink too, so one
/// `is_symlink()` check here covers both.
///
/// A refusal that means "these files are not this project's" is
/// `InvalidProject`; a folder that cannot be inspected at all is `Internal`.
pub(crate) fn prove_ownership(root: &Path, id: &str) -> Result<(), EditorError> {
    let dir = project_dir(root, id).ok_or_else(|| invalid_id_err(id))?;
    let dir_meta = std::fs::symlink_metadata(&dir).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!(
                "Cannot resolve the project folder {}: {e}",
                redact_path(&dir)
            ),
        )
    })?;
    if dir_meta.file_type().is_symlink() {
        return Err(EditorError::new(
            EditorErrorCode::Internal,
            format!(
                "The project folder {} is a symlink or junction; refusing to remove through it",
                redact_path(&dir)
            ),
        ));
    }
    let project_path = dir.join(PROJECT_FILE);
    let bytes = read_bounded(&project_path, OWNERSHIP_PROOF_MAX_BYTES)?;
    let document: OwnerDocument = serde_json::from_slice(&bytes).map_err(|e| {
        EditorError::new(
            EditorErrorCode::InvalidProject,
            format!(
                "The project file {} does not name its project: {e}",
                redact_path(&project_path)
            ),
        )
    })?;
    if document.project.id != id {
        return Err(EditorError::new(
            EditorErrorCode::InvalidProject,
            format!(
                "refusing to remove the project folder {}: its own project id does not match {id:?}",
                redact_path(&dir)
            ),
        ));
    }
    Ok(())
}

/// Permanently remove a project directory — an irreversible unlink, not a
/// trash, mirroring `delete_task`'s posture on the vault side.
///
/// **Ownership is proven before anything is deleted** (`prove_ownership`,
/// the one check every remover shares).
///
/// **No-follow, structurally**: every entry the walk visits is inspected
/// with `symlink_metadata` before it is trusted to be a plain file or
/// directory, and a symlink anywhere in the tree refuses the WHOLE removal
/// before a single file is unlinked — `discard_staged_files`'s two-pass
/// discipline, widened from a flat file set to a directory tree. Files are
/// removed, then directories deepest-first via `remove_dir` (never
/// `remove_dir_all`, which would recurse without this module's own
/// symlink check at every level). `walk_no_follow` only inspects what is
/// INSIDE the folder; `prove_ownership` has already refused a folder that
/// is itself a symlink or junction, and `remove_tree` refuses one again.
pub fn remove_project(root: &Path, id: &str) -> Result<(), EditorError> {
    prove_ownership(root, id)?;
    let dir = project_dir(root, id).ok_or_else(|| invalid_id_err(id))?;
    remove_tree(&dir, Some(PROJECT_FILE)).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!(
                "Could not remove the project folder {}: {e}",
                redact_path(&dir)
            ),
        )
    })
}

/// Collect every file and directory under `dir` (never following a
/// symlink) and remove them in `removal_order`. Two passes on purpose: a
/// symlink discovered partway through the walk must refuse the WHOLE
/// removal, which is only possible when nothing has been unlinked yet.
pub(crate) fn remove_dir_no_follow(dir: &Path) -> io::Result<()> {
    remove_tree(dir, None)
}

/// `dir` ITSELF is checked first (S-2): `read_dir` on a symlink or an NTFS
/// junction (which Rust reports as a symlink) walks the folder it points
/// at, and every file there would be removed. `walk_no_follow` only sees
/// what is inside.
fn remove_tree(dir: &Path, last: Option<&str>) -> io::Result<()> {
    if std::fs::symlink_metadata(dir)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is a symlink or junction; refusing to remove through it",
                redact_path(dir)
            ),
        ));
    }
    for (path, is_dir) in removal_plan(dir, last)? {
        if is_dir {
            std::fs::remove_dir(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// The order a removal unlinks in (final review I1): everything inside a
/// sub-folder first, then the sub-folders deepest-first, then `dir`'s own
/// files with `last` (`project.json`, for a project) after all of them,
/// then `dir`. A removal that fails part way — a file some other process
/// holds open without delete sharing — therefore never leaves a project
/// without the one file its ownership check needs, and a retry can finish.
fn removal_plan(dir: &Path, last: Option<&str>) -> io::Result<Vec<(PathBuf, bool)>> {
    let mut files = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    walk_no_follow(dir, &mut files, &mut dirs)?;
    let top_level = |p: &&PathBuf| p.parent() == Some(dir);
    let is_last = |p: &&PathBuf| last.is_some_and(|name| p.file_name() == Some(name.as_ref()));
    let file = |p: &PathBuf| (p.clone(), false);
    let mut plan: Vec<(PathBuf, bool)> = files.iter().filter(|p| !top_level(p)).map(file).collect();
    // The walk pushes a directory before it descends into it, so reversing
    // the collected order removes children before their parents.
    plan.extend(dirs.iter().skip(1).rev().map(|d| (d.clone(), true)));
    plan.extend(
        files
            .iter()
            .filter(|p| top_level(p) && !is_last(p))
            .map(file),
    );
    plan.extend(
        files
            .iter()
            .filter(|p| top_level(p) && is_last(p))
            .map(file),
    );
    plan.push((dir.to_path_buf(), true));
    Ok(plan)
}

#[cfg(test)]
pub(crate) fn removal_order(dir: &Path, last: Option<&str>) -> io::Result<Vec<PathBuf>> {
    Ok(removal_plan(dir, last)?
        .into_iter()
        .map(|(p, _)| p)
        .collect())
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
                    redact_path(&path)
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
#[path = "store_io_tests.rs"]
mod tests;
