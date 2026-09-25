//! `editor_import_package`'s pipeline (Task 39; F-40; ADR R5; A22: "rejects
//! before installing state or granting filesystem access and current work
//! remains untouched").
//!
//! In order, all under the editor's `open` lock (the lock every project
//! mint already holds, so two imports — or an import and an
//! `editor_open_staged` — can never choose the same new id):
//!
//! 1. **Validate the whole file first.** A portable `.vbproject.zip` goes
//!    through `package::inspect_archive` (every entry name, size, ratio,
//!    kind, the manifest and the envelope, before any media is read); a
//!    lightweight `.vbproject.json` is read BOUNDED and passes
//!    `validate_envelope`. Asset ids that cannot become file names
//!    (`package_plan::asset_id_problem`) are refused here too.
//! 2. **Choose the id**: the manifest's `projectId`, or a fresh one when a
//!    project (or an import in progress) already holds it — the file then
//!    imports as a COPY and the envelope is re-keyed.
//! 3. **Build it aside**, in `editor-projects\.<id>.importing\`: each media
//!    (and product) file extracted through `PackageExtractor` (its local
//!    header checked against the central directory at extraction), bounded
//!    by the manifest's own size, and its byte count and SHA-256 compared
//!    with the manifest BEFORE the next entry is touched. Then
//!    `sources.json`, `project.json` and `workspace.json`.
//! 4. **Install last**: one directory rename into place. Any failure
//!    before it drops `ImportDir`, which removes the build directory with
//!    the store's owned, no-follow removal — nothing was ever installed.
//!    A crash leaves a `.importing` directory the startup sweep
//!    (`recovery::sweep_stale_imports`) removes after an hour.
//!
//! **Where the bytes live afterwards.** Every file-backed source becomes a
//! `Media { file }` record in the new project's own `media\`: a packaged
//! original is the extracted file (with its SHA-256), anything else is a
//! placeholder at `<assetId>.<ext>` that `missing_media` reports for
//! reconnection. A source that was a STAGING (or `StagingFile`) locator on
//! the exporting machine is no exception — that staged base does not exist
//! here, so it is a packaged media file (portable) or a missing placeholder
//! (lightweight). The import never reads or writes any staged capture's
//! sidecar, so it can never pin one.
//!
//! **Source facts.** The package format carries no `sources.json`, so the
//! export puts each source's facts (`hasAudio`, `hasVideo`, dimensions,
//! `mediaKind`, size, duration) in `record.extra` under
//! `package_plan::SOURCE_FACTS_KEY` (fix round 1). They are validated
//! strictly, taken OUT of the envelope before anything is stored, and used
//! for each record, so Task 27's detach-audio guard survives a round trip.
//! A file without them (an older build's, another editor's) gets its facts
//! from the asset, and a video's `hasAudio` is then FALSE: sound is never
//! invented (docs/Gaps.md GAP-182).

use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::device_names::is_reserved_device_name;
use vault_buddy_core::editor::package::{inspect_archive, validate_entry_name, PackageManifest};
use vault_buddy_core::editor::package_extract::PackageExtractor;
use vault_buddy_core::editor::package_plan::{
    asset_definitions, asset_id_problem, file_backed_asset_ids, placeholder_file_name,
    rekey_envelope, take_source_facts, FactsMediaKind, PackageFormat, SourceFacts,
};
use vault_buddy_core::editor::{
    has_canonical_file_name, is_valid_id, limits, new_project_id,
    product_file_name as product_canonical_name, sanitize, validate_envelope, Asset, AssetKind,
    EditorError, EditorErrorCode, EditorOpenResult, MediaType, WorkspaceEnvelope,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::package_commands::PathChooser;
use super::prefs_commands::WORKSPACE_FILE;
use super::project_store::{
    join_contained, project_dir, store_dir, SourceLocator, SourceMediaKind, SourceRecord,
};
use super::redact::redact_path;
use super::render_jobs::PRODUCTS_FILE;
use super::save_commands::map_write_error;
use super::session_commands::{missing_media, register_session};
use super::store_io::{read_bounded, remove_dir_no_follow, PROJECT_FILE, SOURCES_FILE};
use super::EditorState;

/// The suffix of an import's build directory, `.<projectId>.importing`.
const IMPORTING_SUFFIX: &str = ".importing";

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidProject, message)
}

/// The project id a store entry named `.<id>.importing` was building, if
/// it is one — the startup sweep's ownership test.
pub(crate) fn importing_project_id(name: &str) -> Option<&str> {
    name.strip_prefix('.')?
        .strip_suffix(IMPORTING_SUFFIX)
        .filter(|id| is_valid_id(id))
}

fn importing_dir(root: &Path, id: &str) -> PathBuf {
    store_dir(root).join(format!(".{id}{IMPORTING_SUFFIX}"))
}

/// What a validated file brings: the envelope and, for a portable file,
/// its manifest.
struct Incoming {
    envelope: WorkspaceEnvelope,
    manifest: Option<PackageManifest>,
}

fn read_incoming(file: &File, path: &Path, format: PackageFormat) -> Result<Incoming, EditorError> {
    let incoming = match format {
        PackageFormat::Portable => {
            let index = inspect_archive(file)?;
            Incoming {
                envelope: index.envelope,
                manifest: Some(index.manifest),
            }
        }
        PackageFormat::Lightweight => {
            let bytes = read_bounded(path, limits::MAX_PROJECT_JSON_BYTES)?;
            let envelope: WorkspaceEnvelope = serde_json::from_slice(&bytes)
                .map_err(|e| invalid(format!("The project file is not valid: {e}")))?;
            validate_envelope(&envelope)?;
            Incoming {
                envelope,
                manifest: None,
            }
        }
    };
    if let Some(problem) = asset_id_problem(&incoming.envelope) {
        return Err(invalid(problem));
    }
    Ok(incoming)
}

/// The manifest's id when it is free here; otherwise a fresh one (a copy).
fn choose_id(root: &Path, wanted: &str) -> String {
    let taken = |id: &str| {
        project_dir(root, id).is_none_or(|dir| std::fs::symlink_metadata(dir).is_ok())
            || std::fs::symlink_metadata(importing_dir(root, id)).is_ok()
    };
    let mut id = wanted.to_string();
    while taken(&id) {
        id = new_project_id();
    }
    id
}

/// The build directory: removed (owned, no-follow) unless installed.
struct ImportDir {
    dir: PathBuf,
    installed: bool,
}

impl ImportDir {
    fn create(root: &Path, id: &str) -> Result<Self, EditorError> {
        std::fs::create_dir_all(store_dir(root)).map_err(map_write_error)?;
        let dir = importing_dir(root, id);
        std::fs::create_dir(&dir).map_err(map_write_error)?;
        Ok(Self {
            dir,
            installed: false,
        })
    }

    fn subdir(&self, name: &str) -> Result<PathBuf, EditorError> {
        let dir = self.dir.join(name);
        std::fs::create_dir_all(&dir).map_err(map_write_error)?;
        Ok(dir)
    }

    /// The one rename that makes the project exist. The target was checked
    /// free under the `open` lock (`choose_id`), which every project mint
    /// holds too.
    fn install(mut self, root: &Path, id: &str) -> Result<(), EditorError> {
        let target = project_dir(root, id).ok_or_else(|| invalid("invalid project id"))?;
        std::fs::rename(&self.dir, &target).map_err(map_write_error)?;
        self.installed = true;
        Ok(())
    }
}

impl Drop for ImportDir {
    fn drop(&mut self) {
        if self.installed {
            return;
        }
        if let Err(e) = remove_dir_no_follow(&self.dir) {
            log::warn!(
                "editor import: could not remove the unfinished import at {}: {e}",
                redact_path(&self.dir)
            );
        }
    }
}

/// One file taken out of the package and proven to be the manifest's.
struct Extracted {
    file: String,
    size: u64,
    sha256: String,
}

/// Extracts `entry` into `dir/name` (exclusive create — never onto a file
/// or a link already there) and PROVES it before anything else happens:
/// its local header at extraction, at most `size` bytes out, and exactly
/// `size` bytes whose SHA-256 is `sha256`.
fn extract_one(
    extractor: &mut PackageExtractor<&File>,
    entry: &str,
    dir: &Path,
    name: &str,
    size: u64,
    sha256: &str,
) -> Result<Extracted, EditorError> {
    let dest = join_contained(dir, name)
        .ok_or_else(|| invalid(format!("{entry:?} cannot be saved as a file here")))?;
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(map_write_error)?;
    let got = extractor.extract(entry, &mut out, size)?;
    // MUTATION CHECK (`failed_import_installs_nothing`): checking this only
    // after `ImportDir::install` would leave a project built from a corrupt
    // entry installed.
    if got.bytes != size || got.sha256 != sha256 {
        return Err(invalid(format!(
            "package entry {entry:?} does not match its manifest (size or SHA-256); the file is damaged"
        )));
    }
    out.sync_all().map_err(map_write_error)?;
    Ok(Extracted {
        file: name.to_string(),
        size,
        sha256: got.sha256,
    })
}

/// A retained product's file name, as `media_commands` resolves it under
/// `products\`: `<productId>.mp4` (checked up front, `import_file`), one
/// plain, non-device component, unique ignoring case.
fn product_file_name<'a>(
    env: &'a WorkspaceEnvelope,
    product_id: &str,
    seen: &mut HashSet<String>,
) -> Result<&'a str, EditorError> {
    let filename = env
        .record
        .products
        .iter()
        .find(|p| p.id == product_id)
        .map(|p| p.filename.as_str())
        .ok_or_else(|| invalid(format!("product {product_id} is not in the project")))?;
    let usable = validate_entry_name(&format!("products/{filename}")).is_ok()
        && !filename.contains('/')
        && !is_reserved_device_name(filename)
        && seen.insert(filename.to_lowercase());
    if usable {
        Ok(filename)
    } else {
        Err(invalid(format!(
            "product {product_id}'s file name {filename:?} cannot be saved as a file"
        )))
    }
}

/// Every packaged media and product file, extracted and verified; returns
/// the media by asset id.
fn extract_all(
    file: &File,
    manifest: &PackageManifest,
    env: &WorkspaceEnvelope,
    build: &ImportDir,
) -> Result<BTreeMap<String, Extracted>, EditorError> {
    let mut extractor = PackageExtractor::open(file)?;
    let mut media = BTreeMap::new();
    if !manifest.media.is_empty() {
        let dir = build.subdir("media")?;
        for m in &manifest.media {
            let ext = m.path.rsplit_once('.').map_or("bin", |(_, ext)| ext);
            let name = format!("{}.{ext}", m.asset_id);
            let got = extract_one(&mut extractor, &m.path, &dir, &name, m.size, &m.sha256)?;
            media.insert(m.asset_id.clone(), got);
        }
    }
    let mut seen = HashSet::new();
    if !manifest.products.is_empty() {
        let dir = build.subdir("products")?;
        for p in &manifest.products {
            let name = product_file_name(env, &p.product_id, &mut seen)?;
            extract_one(&mut extractor, &p.path, &dir, name, p.size, &p.sha256)?;
        }
    }
    Ok(media)
}

fn dimension(value: Option<&serde_json::Number>) -> Option<u32> {
    value
        .and_then(serde_json::Number::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

/// A `sources.json` record for a file-backed asset: the extracted file when
/// the package carried it, a placeholder otherwise (module doc).
/// The facts a source record needs when the file carried none for it (a
/// build before fix round 1, or another editor): read off the asset, and
/// `hasAudio` only for an AUDIO asset. A video's sound is never invented,
/// or Task 27's detach-audio guard would accept an edit that makes an
/// empty audio clip; an honest "no" is what a reconnect can lift.
fn facts_from_asset(asset: &Asset) -> SourceFacts {
    let image = asset.media_type == Some(MediaType::Image);
    SourceFacts {
        has_audio: asset.kind == AssetKind::Audio && !image,
        has_video: asset.kind == AssetKind::Video,
        width: dimension(asset.width.as_ref()),
        height: dimension(asset.height.as_ref()),
        media_kind: match (image, asset.kind) {
            (true, _) => FactsMediaKind::Image,
            (false, AssetKind::Audio) => FactsMediaKind::Audio,
            (false, AssetKind::Video) => FactsMediaKind::Video,
        },
        size: asset.size.unwrap_or(0),
        duration_ms: asset.duration_ms,
    }
}

/// A `sources.json` record for a file-backed asset: the extracted file when
/// the package carried it, a placeholder otherwise; its facts are the
/// exporting machine's when the file carried them (`SOURCE_FACTS_KEY`),
/// else `facts_from_asset`.
fn source_record(
    asset: &Asset,
    extracted: Option<&Extracted>,
    carried: Option<&SourceFacts>,
) -> SourceRecord {
    let facts = carried.cloned().unwrap_or_else(|| facts_from_asset(asset));
    let (file, size, sha256) = match extracted {
        Some(x) => (x.file.clone(), x.size, Some(x.sha256.clone())),
        None => (placeholder_file_name(asset), facts.size, None),
    };
    SourceRecord {
        locator: SourceLocator::Media { file },
        sha256,
        size,
        duration_ms: facts.duration_ms,
        width: facts.width,
        height: facts.height,
        has_audio: facts.has_audio,
        has_video: facts.has_video,
        media_kind: match facts.media_kind {
            FactsMediaKind::Image => SourceMediaKind::Image,
            FactsMediaKind::Audio => SourceMediaKind::Audio,
            FactsMediaKind::Video => SourceMediaKind::Video,
        },
        replaced_from: None,
    }
}

fn build_sources(
    env: &WorkspaceEnvelope,
    extracted: &BTreeMap<String, Extracted>,
    facts: &BTreeMap<String, SourceFacts>,
) -> BTreeMap<String, SourceRecord> {
    let defs = asset_definitions(env);
    file_backed_asset_ids(env)
        .into_iter()
        .filter_map(|id| {
            let asset = *defs.get(id.as_str())?.first()?;
            let record = source_record(asset, extracted.get(&id), facts.get(&id));
            Some((id, record))
        })
        .collect()
}

fn write_json(dir: &Path, name: &str, value: &impl serde::Serialize) -> Result<(), EditorError> {
    let json = serde_json::to_string_pretty(value).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Could not encode {name}: {e}"),
        )
    })?;
    write_atomic_replacing(&dir.join(name), &json).map_err(map_write_error)
}

fn import_file(
    state: &EditorState,
    root: &Path,
    path: &Path,
) -> Result<EditorOpenResult, EditorError> {
    let format = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(PackageFormat::of_file_name)
        .ok_or_else(|| {
            EditorError::new(
                EditorErrorCode::InvalidRequest,
                "Choose a .vbproject.zip or .vbproject.json project file.",
            )
        })?;
    let is_file = std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file());
    if !is_file {
        return Err(invalid("The chosen project file is not a plain file."));
    }
    let file = File::open(path).map_err(map_write_error)?;
    let incoming = read_incoming(&file, path, format)?;

    let _open = lock_ignoring_poison(&state.open);
    let mut envelope = incoming.envelope;
    // Transport only: taken OUT of the envelope before anything is stored.
    let facts = take_source_facts(&mut envelope).map_err(invalid)?;
    // Task 46: a product's file is `products\<productId>.mp4` and nothing
    // else -- refused here, before anything is built, like every other
    // name this file carries.
    if let Some(p) = envelope
        .record
        .products
        .iter()
        .find(|p| !has_canonical_file_name(p))
    {
        return Err(invalid(format!(
            "product {}'s file name {:?} is not {}",
            p.id,
            p.filename,
            product_canonical_name(&p.id)
        )));
    }
    let id = choose_id(root, &envelope.project.id);
    if id != envelope.project.id {
        rekey_envelope(&mut envelope, &id);
    }
    let build = ImportDir::create(root, &id)?;
    let extracted = match &incoming.manifest {
        Some(manifest) => extract_all(&file, manifest, &envelope, &build)?,
        None => BTreeMap::new(),
    };
    let sources = build_sources(&envelope, &extracted, &facts);
    // `project.json` carries the SANITIZED blob too (fix round 1), exactly
    // as `editor_save_project` embeds it: untrusted package content never
    // lands in the store verbatim.
    let workspace = sanitize(&envelope.workspace);
    envelope.workspace = serde_json::to_value(&workspace).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("Could not encode the workspace: {e}"),
        )
    })?;
    write_json(&build.dir, SOURCES_FILE, &sources)?;
    // The ledger is the products' authority (Task 46, R5): an imported
    // project lists its retained products before any save. `project.json`
    // keeps them WITHOUT their snapshots, as a save does (fix round 1).
    if !envelope.record.products.is_empty() {
        write_json(&build.dir, PRODUCTS_FILE, &envelope.record.products)?;
        for product in &mut envelope.record.products {
            product.snapshot = None;
        }
    }
    write_json(&build.dir, PROJECT_FILE, &envelope)?;
    write_json(&build.dir, WORKSPACE_FILE, &workspace)?;
    build.install(root, &id)?;

    let projection = register_session(state, envelope.project, envelope.record.revision);
    let missing = missing_media(root, &projection.project, &sources);
    Ok(EditorOpenResult {
        snapshot: projection.snapshot,
        project: projection.project,
        workspace,
        missing,
        source_base: None,
        recovered: false,
    })
}

/// The `AppHandle`-free half of `editor_import_package`: `None` when the
/// open dialog was dismissed.
pub(crate) fn import_package_in(
    state: &EditorState,
    root: &Path,
    chooser: &dyn PathChooser,
) -> Result<Option<EditorOpenResult>, EditorError> {
    let Some(path) = chooser.package_to_open() else {
        return Ok(None);
    };
    import_file(state, root, &path).map(Some)
}

#[cfg(test)]
#[path = "package_import_tests.rs"]
mod tests;
