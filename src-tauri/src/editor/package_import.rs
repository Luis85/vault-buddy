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
//! 3. **Measure what the store will hold** (M-V2): `project.json` and the
//!    product ledger are serialised PRETTY, exactly as they are written, and
//!    refused past the bounds their readers enforce, BEFORE any directory
//!    exists. A compact file under the read bound can be far over it once
//!    pretty-printed, and an installed project nothing can read could be
//!    neither saved, listed nor discarded.
//! 4. **Build it aside**, in `editor-projects\.<id>.importing\`: each media
//!    (and product) file extracted through `PackageExtractor` (its local
//!    header checked against the central directory at extraction), bounded
//!    by the manifest's own size, and its byte count and SHA-256 compared
//!    with the manifest BEFORE the next entry is touched. Then
//!    `sources.json`, `project.json` and `workspace.json`.
//! 5. **Install last**: one directory rename into place. Any failure
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
//! invented (docs/Gaps.md GAP-182). Either way they are only a CLAIM: a
//! file the package carries is re-probed once extracted (`Prober`: ffprobe,
//! or the native sniff for an image) and its probe wins (GAP-215); only a
//! placeholder, or an import with no ffmpeg, keeps the claim.

use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use vault_buddy_core::capture_note::write_atomic_replacing;
use vault_buddy_core::device_names::is_reserved_device_name;
use vault_buddy_core::editor::import_io::settle_av_import;
use vault_buddy_core::editor::package::{
    inspect_archive, refuse_unparsable, validate_entry_name, PackageManifest,
};
use vault_buddy_core::editor::package_extract::{PackageExtractor, ValidatedArchive};
use vault_buddy_core::editor::package_plan::{
    asset_definitions, asset_id_problem, file_backed_asset_ids, placeholder_file_name,
    rekey_envelope, take_source_facts, FactsMediaKind, PackageFormat, SourceFacts,
};
use vault_buddy_core::editor::probe::{ImportKind, ProbeFacts};
use vault_buddy_core::editor::{
    has_canonical_file_name, is_valid_id, limits, new_project_id, sanitize, validate_envelope,
    Asset, AssetKind, EditorError, EditorErrorCode, EditorOpenResult, MediaType, WorkspaceEnvelope,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::errors::internal;
use super::media_import::{sniff_copy, FfprobeImportIo, ImportIo};
use super::package_commands::PathChooser;
use super::prefs_commands::WORKSPACE_FILE;
use super::project_store::{
    join_contained, project_dir, store_dir, SourceLocator, SourceMediaKind, SourceRecord,
};
use super::redact::redact_path;
use super::render_jobs::{LEDGER_MAX_BYTES, PRODUCTS_FILE};
use super::save_commands::map_write_error;
use super::session_commands::{missing_media, register_session};
use super::store_io::{read_bounded, remove_dir_no_follow, PROJECT_FILE, SOURCES_FILE};
use super::EditorState;

/// The re-probe of a file the package CARRIED (GAP-215): what the extracted
/// bytes are, asked of ffprobe (or, for an image, of the native sniff) as
/// the kind the project's graph gives the asset. `None` when it cannot say
/// -- no ffmpeg, or a file it cannot read -- and the carried facts then
/// stand. A seam, like `ImportIo`/`TakeIo`, so a test answers it.
pub(crate) trait Prober {
    fn probe(&self, path: &Path, kind: ImportKind) -> Option<ProbeFacts>;
}

/// The production prober: the media import's own `FfprobeImportIo` (the
/// user's ffmpeg, resolved once on first need, and `probe_media`) and its
/// image sniff.
#[derive(Default)]
pub(crate) struct FfprobeProber {
    io: FfprobeImportIo,
}

impl Prober for FfprobeProber {
    fn probe(&self, path: &Path, kind: ImportKind) -> Option<ProbeFacts> {
        let probed = if kind == ImportKind::Image {
            sniff_copy(path).map(|(w, h)| ProbeFacts {
                duration_ms: 0,
                width: Some(w),
                height: Some(h),
                has_video: true,
                has_audio: false,
            })
        } else {
            // No ffmpeg: nothing to ask, and the carried facts stand
            // (today's behaviour, GAP-215).
            self.io.av_ready().ok()?;
            self.io.probe_av(path, kind)
        };
        probed
            .map_err(|e| {
                log::warn!(
                    "editor import: packaged media {} could not be probed ({:?}); its carried facts are kept",
                    redact_path(path),
                    e.code
                );
            })
            .ok()
    }
}

/// The suffix of an import's build directory, `.<projectId>.importing`.
const IMPORTING_SUFFIX: &str = ".importing";

/// The refusal for a file whose stored form would be past what the store
/// can read back (M-V2).
const TOO_LARGE_TO_INSTALL: &str = "This project file is too large to install.";

/// `EditorErrorCode::InvalidProject` -- distinct from the shared
/// `errors::invalid` (`InvalidRequest`): a whole package FILE failed
/// validation here, not one request's shape.
fn invalid_project(message: impl Into<String>) -> EditorError {
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
/// its manifest and the very archive `inspect_archive` validated — the
/// extraction reads through it, never a second parse (S-3).
struct Incoming<'f> {
    envelope: WorkspaceEnvelope,
    package: Option<(PackageManifest, ValidatedArchive<&'f File>)>,
}

fn read_incoming<'f>(
    file: &'f File,
    path: &Path,
    format: PackageFormat,
) -> Result<Incoming<'f>, EditorError> {
    let incoming = match format {
        PackageFormat::Portable => {
            let inspected = inspect_archive(file)?;
            Incoming {
                envelope: inspected.index.envelope,
                package: Some((inspected.index.manifest, inspected.archive)),
            }
        }
        PackageFormat::Lightweight => {
            let bytes = read_bounded(path, limits::MAX_PROJECT_JSON_BYTES)?;
            let envelope: WorkspaceEnvelope = serde_json::from_slice(&bytes)
                .map_err(|e| refuse_unparsable("The project file is not valid.", &e))?;
            validate_envelope(&envelope)?;
            Incoming {
                envelope,
                package: None,
            }
        }
    };
    if let Some(problem) = asset_id_problem(&incoming.envelope) {
        return Err(invalid_project(problem));
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
        let target = project_dir(root, id).ok_or_else(|| invalid_project("invalid project id"))?;
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
    /// Where it was extracted, in the build directory (what is re-probed).
    path: PathBuf,
    file: String,
    size: u64,
    sha256: String,
}

/// Extracts `entry` into `dir/name` (exclusive create — never onto a file
/// or a link already there) and PROVES it before anything else happens:
/// its local header at extraction, at most `size` bytes out, and exactly
/// `size` bytes whose SHA-256 is `sha256`.
///
/// `label` names the file by kind and manifest position ("media file 2"),
/// never by its name, which is the package author's text (S-11).
fn extract_one(
    extractor: &mut PackageExtractor<&File>,
    (entry, label): (&str, &str),
    dir: &Path,
    name: &str,
    size: u64,
    sha256: &str,
) -> Result<Extracted, EditorError> {
    let dest = join_contained(dir, name).ok_or_else(|| {
        invalid_project(format!(
            "The package's {label} cannot be saved as a file here."
        ))
    })?;
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
        return Err(invalid_project(format!(
            "The package's {label} does not match its manifest (size or SHA-256); the file is damaged."
        )));
    }
    out.sync_all().map_err(map_write_error)?;
    Ok(Extracted {
        path: dest,
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
    (product_id, label): (&str, &str),
    seen: &mut HashSet<String>,
) -> Result<&'a str, EditorError> {
    let filename = env
        .record
        .products
        .iter()
        .find(|p| p.id == product_id)
        .map(|p| p.filename.as_str())
        .ok_or_else(|| invalid_project(format!("The package's {label} is not in the project.")))?;
    let usable = validate_entry_name(&format!("products/{filename}")).is_ok()
        && !filename.contains('/')
        && !is_reserved_device_name(filename)
        && seen.insert(filename.to_lowercase());
    if usable {
        Ok(filename)
    } else {
        Err(invalid_project(format!(
            "The package's {label} has a file name that cannot be saved as a file."
        )))
    }
}

/// Every packaged media and product file, extracted and verified; returns
/// the media by asset id.
fn extract_all(
    archive: ValidatedArchive<&File>,
    manifest: &PackageManifest,
    env: &WorkspaceEnvelope,
    build: &ImportDir,
) -> Result<BTreeMap<String, Extracted>, EditorError> {
    let mut extractor = PackageExtractor::from_validated(archive)?;
    let mut media = BTreeMap::new();
    if !manifest.media.is_empty() {
        let dir = build.subdir("media")?;
        for (i, m) in manifest.media.iter().enumerate() {
            let ext = m.path.rsplit_once('.').map_or("bin", |(_, ext)| ext);
            let name = format!("{}.{ext}", m.asset_id);
            let label = format!("media file {}", i + 1);
            let entry = (m.path.as_str(), label.as_str());
            let got = extract_one(&mut extractor, entry, &dir, &name, m.size, &m.sha256)?;
            media.insert(m.asset_id.clone(), got);
        }
    }
    let mut seen = HashSet::new();
    if !manifest.products.is_empty() {
        let dir = build.subdir("products")?;
        for (i, p) in manifest.products.iter().enumerate() {
            let label = format!("product file {}", i + 1);
            let name = product_file_name(env, (&p.product_id, &label), &mut seen)?;
            let entry = (p.path.as_str(), label.as_str());
            extract_one(&mut extractor, entry, &dir, name, p.size, &p.sha256)?;
        }
    }
    Ok(media)
}

fn dimension(value: Option<&serde_json::Number>) -> Option<u32> {
    value
        .and_then(serde_json::Number::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

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

/// The kind the project's GRAPH gives an asset -- what a re-probe is asked
/// to confirm, and what the record must agree with.
fn asset_import_kind(asset: &Asset) -> ImportKind {
    match (asset.media_type == Some(MediaType::Image), asset.kind) {
        (true, _) => ImportKind::Image,
        (false, AssetKind::Audio) => ImportKind::Audio,
        (false, AssetKind::Video) => ImportKind::Video,
    }
}

/// GAP-215: the facts of a file the package CARRIED are what its extracted
/// bytes probe as, never the author's claim -- `hasAudio` decides
/// `detachAudio` and the render's audio map, and width/height/kind decide
/// the untouched-capture fast path (R1). A video or audio file goes through
/// the media import's own rule (`settle_av_import`: a "video" holding only
/// sound is audio); an image keeps its assigned length. A probe that cannot
/// say (`None`: no ffmpeg, an unreadable file) or that the rule refuses
/// leaves the carried facts standing -- today's behaviour, never a refusal.
fn reprobed(prober: &dyn Prober, path: &Path, asset: &Asset, carried: SourceFacts) -> SourceFacts {
    let kind = asset_import_kind(asset);
    let Some(probed) = prober.probe(path, kind) else {
        return carried;
    };
    if kind == ImportKind::Image {
        return SourceFacts {
            has_audio: false,
            has_video: true,
            width: probed.width,
            height: probed.height,
            media_kind: FactsMediaKind::Image,
            ..carried
        };
    }
    match settle_av_import(kind, probed) {
        Ok((settled, facts)) => SourceFacts {
            has_audio: facts.has_audio,
            has_video: facts.has_video,
            width: facts.width,
            height: facts.height,
            media_kind: if settled == ImportKind::Audio {
                FactsMediaKind::Audio
            } else {
                FactsMediaKind::Video
            },
            size: carried.size,
            duration_ms: facts.duration_ms,
        },
        Err(why) => {
            log::warn!(
                "editor import: packaged media {} probed unusably ({why}); its carried facts are kept",
                redact_path(path)
            );
            carried
        }
    }
}

/// A `sources.json` record for a file-backed asset: the extracted file when
/// the package carried it, a placeholder otherwise (module doc), with
/// `facts` (`build_sources` chooses them).
fn source_record(asset: &Asset, extracted: Option<&Extracted>, facts: SourceFacts) -> SourceRecord {
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

/// Every file-backed asset's record. Its facts are the exporting machine's
/// when the file carried them (`SOURCE_FACTS_KEY`), else `facts_from_asset`
/// -- and, for a file the package CARRIED, what its bytes probe as
/// (`reprobed`). A placeholder's media is not here to probe, so it keeps
/// what the file said until a reconnect re-probes it (Task 40).
fn build_sources(
    env: &WorkspaceEnvelope,
    extracted: &BTreeMap<String, Extracted>,
    facts: &BTreeMap<String, SourceFacts>,
    prober: &dyn Prober,
) -> BTreeMap<String, SourceRecord> {
    let defs = asset_definitions(env);
    file_backed_asset_ids(env)
        .into_iter()
        .filter_map(|id| {
            let asset = *defs.get(id.as_str())?.first()?;
            let carried = facts
                .get(&id)
                .cloned()
                .unwrap_or_else(|| facts_from_asset(asset));
            let got = extracted.get(&id);
            // MUTATION CHECK (`a_carried_file_is_reprobed_and_the_probe_outranks_its_carried_facts`,
            // `a_placeholder_is_never_probed_and_keeps_its_carried_facts`).
            let facts = match got {
                Some(x) => reprobed(prober, &x.path, asset, carried),
                None => carried,
            };
            Some((id, source_record(asset, got, facts)))
        })
        .collect()
}

fn write_json(dir: &Path, name: &str, value: &impl serde::Serialize) -> Result<(), EditorError> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| internal(format!("Could not encode {name}: {e}")))?;
    write_text(dir, name, &json)
}

fn write_text(dir: &Path, name: &str, json: &str) -> Result<(), EditorError> {
    write_atomic_replacing(&dir.join(name), json).map_err(map_write_error)
}

/// A sink that holds at most `max` bytes and fails the write that would
/// pass them, remembering why.
struct Capped {
    bytes: Vec<u8>,
    max: usize,
    over: bool,
}

impl io::Write for Capped {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(buf.len()) > self.max {
            self.over = true;
            return Err(io::Error::other("past the bound"));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// `value` pretty-printed exactly as `write_json` would store it, refused
/// once it passes `max` — the bound its reader enforces (M-V2). It stops
/// THERE, so a crafted file never makes the import build a pretty form
/// many times its own size.
fn stored_json(value: &impl serde::Serialize, max: u64) -> Result<String, EditorError> {
    let mut out = Capped {
        bytes: Vec::new(),
        max: usize::try_from(max).unwrap_or(usize::MAX),
        over: false,
    };
    match serde_json::to_writer_pretty(&mut out, value) {
        Ok(()) => String::from_utf8(out.bytes)
            .map_err(|e| internal(format!("Could not encode the project: {e}"))),
        Err(_) if out.over => Err(invalid_project(TOO_LARGE_TO_INSTALL)),
        Err(e) => Err(internal(format!("Could not encode the project: {e}"))),
    }
}

fn import_file(
    state: &EditorState,
    root: &Path,
    path: &Path,
    prober: &dyn Prober,
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
        return Err(invalid_project(
            "The chosen project file is not a plain file.",
        ));
    }
    let file = File::open(path).map_err(map_write_error)?;
    let Incoming {
        mut envelope,
        package,
    } = read_incoming(&file, path, format)?;

    let _open = lock_ignoring_poison(&state.open);
    // Transport only: taken OUT of the envelope before anything is stored.
    let facts = take_source_facts(&mut envelope).map_err(invalid_project)?;
    // Task 46: a product's file is `products\<productId>.mp4` and nothing
    // else -- refused here, before anything is built, like every other
    // name this file carries.
    // Neither the file name nor the product id is echoed: both are the
    // file author's text (S-11).
    if let Some(i) = envelope
        .record
        .products
        .iter()
        .position(|p| !has_canonical_file_name(p))
    {
        return Err(invalid_project(format!(
            "The file name of product {} in the project file is not its id followed by .mp4.",
            i + 1
        )));
    }
    let id = choose_id(root, &envelope.project.id);
    if id != envelope.project.id {
        rekey_envelope(&mut envelope, &id);
    }
    // `project.json` carries the SANITIZED blob too (fix round 1), exactly
    // as `editor_save_project` embeds it: untrusted package content never
    // lands in the store verbatim.
    let workspace = sanitize(&envelope.workspace);
    envelope.workspace = serde_json::to_value(&workspace)
        .map_err(|e| internal(format!("Could not encode the workspace: {e}")))?;
    // The ledger is the products' authority (Task 46, R5): an imported
    // project lists its retained products before any save. `project.json`
    // keeps them WITHOUT their snapshots, as a save does (fix round 1).
    // Both are measured here, before anything exists (M-V2).
    let ledger = if envelope.record.products.is_empty() {
        None
    } else {
        Some(stored_json(&envelope.record.products, LEDGER_MAX_BYTES)?)
    };
    for product in &mut envelope.record.products {
        product.snapshot = None;
    }
    // MUTATION CHECK (`a_compact_file_whose_stored_form_is_too_large_installs_nothing`):
    // not measuring it installs a project nothing can read back.
    let project_json = stored_json(&envelope, limits::MAX_PROJECT_JSON_BYTES)?;
    let build = ImportDir::create(root, &id)?;
    let extracted = match package {
        Some((manifest, archive)) => extract_all(archive, &manifest, &envelope, &build)?,
        None => BTreeMap::new(),
    };
    let sources = build_sources(&envelope, &extracted, &facts, prober);
    write_json(&build.dir, SOURCES_FILE, &sources)?;
    if let Some(ledger) = &ledger {
        write_text(&build.dir, PRODUCTS_FILE, ledger)?;
    }
    write_text(&build.dir, PROJECT_FILE, &project_json)?;
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
    prober: &dyn Prober,
) -> Result<Option<EditorOpenResult>, EditorError> {
    let Some(path) = chooser.package_to_open() else {
        return Ok(None);
    };
    import_file(state, root, &path, prober).map(Some)
}

#[cfg(test)]
#[path = "package_import_tests.rs"]
mod tests;
