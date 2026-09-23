//! The portable tutorial-project package, `vault-buddy-project-package/1`
//! (F-40, ADR R9): one ZIP holding a manifest, the saved workspace envelope
//! and the project's original media (and optionally its rendered
//! products). Pure -- no Tauri types, no filesystem -- so the whole
//! threat surface (A22) is tested on Linux; the shell (Task 39) owns the
//! dialogs, the temp files and the store install.
//!
//! Layout, and the order `write_package` emits it in:
//!
//! ```text
//! package.json          the manifest (PackageManifest) -- ALWAYS entry 0, Deflated
//! workspace.json        the WorkspaceEnvelope, Deflated
//! media/<assetId>.<ext> an original source, Stored (already compressed)
//! products/<id>.mp4     a rendered product, Stored
//! ```
//!
//! **Naming hazard.** The package's `workspace.json` is the saved
//! WORKSPACE ENVELOPE (`{schema, project, workspace, record, saved_at}`,
//! `validate_envelope`). A project DIRECTORY's `workspace.json` (Task 18,
//! `editor::prefs_commands`) is something else entirely: the sanitized UI
//! view-preference blob (`workspace::sanitize`). Same file name, different
//! document -- never read one with the other's reader, and never extract a
//! package's `workspace.json` into a project directory under that name.
//!
//! `inspect_archive` decides, BEFORE anything is extracted, whether an
//! untrusted archive may be imported at all (ADR R9, `DATA-MODEL.md` §
//! Validation order step 1 "bound total input bytes, archive count/
//! expansion ... before allocating unbounded buffers"): the archive length,
//! the entry count, every name, every declared size and ratio, the entry
//! kinds, the manifest (first, parsed and cross-checked) and the envelope
//! (`validate_envelope`, which is also where GAP-179's caption/chapter
//! bounds now apply). Declared sizes are what an attacker writes, so every
//! later read goes through `extract_entry_bounded`, which counts the bytes
//! actually produced and never trusts the header.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{self, Read, Seek, SeekFrom, Write};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::error::{EditorError, EditorErrorCode};
use super::fingerprint::assets_referenced;
use super::ids::is_valid_id;
use super::limits;
use super::model::{Asset, Builtin};
use super::model_cues::WorkspaceEnvelope;
use super::validate::validate_envelope;
use super::PACKAGE_SCHEMA;

/// The manifest's entry name; always the archive's first entry.
pub const MANIFEST_NAME: &str = "package.json";
/// The saved workspace ENVELOPE's entry name -- see the module doc's
/// naming hazard: not the project directory's UI-preference file.
pub const WORKSPACE_NAME: &str = "workspace.json";
const MEDIA_DIR: &str = "media";
const PRODUCTS_DIR: &str = "products";
/// An entry name's cap, in bytes (the brief's ≤ 255; also comfortably
/// inside every filesystem's own component limit once extracted).
const MAX_ENTRY_NAME_BYTES: usize = 255;
/// The manifest lists at most `MAX_ASSETS + MAX_PRODUCTS` files of a few
/// hundred bytes each; 1 MiB is two orders of magnitude of headroom and
/// still bounds the one read that happens before anything is validated.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// A media file's extension: 1-8 ASCII alphanumerics (`mp4`, `webm`,
/// `wav`, `png`...). The name is otherwise fixed to the asset id, so the
/// extension is the only free text in a media path.
const MAX_EXTENSION_CHARS: usize = 8;
/// The ZIP end-of-central-directory record's fixed size (APPNOTE 4.3.16).
const END_RECORD_LEN: u64 = 22;
/// `st_mode`'s file-type bits and the one type a package entry may carry.
const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;

/// `package.json` (camelCase, like every other wire DTO in the repo). Its
/// own fields are closed (`deny_unknown_fields`): unlike the project graph
/// (R3's round-trip policy), the manifest is an index this crate alone
/// writes, and a field it does not understand is a field it cannot check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageManifest {
    pub schema: String,
    pub project_id: String,
    pub saved_at: String,
    /// Always `"workspace.json"`; carried so a reader never guesses.
    pub workspace: String,
    pub media: Vec<PackageMedia>,
    pub products: Vec<PackageProduct>,
}

/// One packaged original: `path` is exactly `media/<assetId>.<ext>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageMedia {
    pub asset_id: String,
    pub path: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the file's bytes.
    pub sha256: String,
}

/// One packaged rendered product: `path` is exactly
/// `products/<productId>.mp4`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageProduct {
    pub product_id: String,
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// What `inspect_archive` proved about an archive, before extraction.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageIndex {
    pub manifest: PackageManifest,
    /// The parsed, fully validated `workspace.json` envelope.
    pub envelope: WorkspaceEnvelope,
    /// Asset ids the project (current graph OR a retained product's
    /// snapshot, A17) depends on that ship no bytes here: the lightweight
    /// semantics -- reported for reconnection, never silently pruned.
    pub missing: Vec<String>,
}

/// What `extract_entry_bounded` actually produced (never the header's
/// claim): the byte count and the lowercase hex SHA-256 of those bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedEntry {
    pub bytes: u64,
    pub sha256: String,
}

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidProject, message)
}

fn unreadable(e: impl std::fmt::Display) -> EditorError {
    invalid(format!(
        "the project package is not a readable archive: {e}"
    ))
}

/// A write-side I/O failure: a full disk keeps its own retryable code;
/// anything else is ours to report, not the package's fault.
fn write_failed(e: &io::Error) -> EditorError {
    let code = if e.kind() == io::ErrorKind::StorageFull {
        EditorErrorCode::DiskFull
    } else {
        EditorErrorCode::Internal
    };
    EditorError::new(code, format!("writing the project package failed: {e}"))
}

fn zip_write_failed(e: ZipError) -> EditorError {
    match e {
        ZipError::Io(io) => write_failed(&io),
        other => EditorError::new(
            EditorErrorCode::Internal,
            format!("writing the project package failed: {other}"),
        ),
    }
}

/// Whether `name` may appear in a package at all: relative, `/`-separated,
/// at most 255 bytes, no NUL/control character, no absolute, drive, UNC or
/// backslash form, no empty/`.`/`..` segment, and only `package.json`,
/// `workspace.json` or something under `media/` or `products/`. A
/// backslash is refused anywhere (not only in front): it is a separator on
/// Windows, so `media\..\..\x` would traverse there. A `:` is refused
/// anywhere too -- a drive prefix, or an NTFS alternate data stream.
pub fn validate_entry_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("an entry name is empty".to_string());
    }
    if name.len() > MAX_ENTRY_NAME_BYTES {
        return Err(format!("longer than {MAX_ENTRY_NAME_BYTES} bytes"));
    }
    if name.chars().any(char::is_control) {
        return Err("contains a NUL or control character".to_string());
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return Err("is an absolute or UNC path".to_string());
    }
    if name.contains('\\') {
        return Err("contains a backslash".to_string());
    }
    if name.contains(':') {
        return Err("contains a drive or stream separator".to_string());
    }
    let segments: Vec<&str> = name.split('/').collect();
    if segments
        .iter()
        .any(|s| s.is_empty() || *s == "." || *s == "..")
    {
        return Err("has an empty, `.` or `..` path segment".to_string());
    }
    let allowed = match segments.as_slice() {
        [only] => *only == MANIFEST_NAME || *only == WORKSPACE_NAME,
        [dir, _, ..] => *dir == MEDIA_DIR || *dir == PRODUCTS_DIR,
        [] => false,
    };
    if !allowed {
        return Err("is not a package entry".to_string());
    }
    Ok(())
}

/// The ZIP end record, read by hand from the archive's last 22 bytes.
///
/// `zip::ZipArchive` indexes entries by name and silently keeps ONE of two
/// entries that share an exact name (its central directory is an
/// `IndexMap` keyed on the name), so a second `workspace.json` could hide
/// behind the first. Comparing this record's own entry count with the
/// crate's `len()` is the only way to see that collapse. Requiring the
/// record to be the final 22 bytes (no archive comment, nothing trailing,
/// which `write_package` never emits) pins it to the one record the crate
/// tries first, so the two cannot be reading different directories.
struct EndRecord {
    entries: u64,
    directory_offset: u64,
}

fn read_end_record<R: Read + Seek>(reader: &mut R, len: u64) -> Result<EndRecord, EditorError> {
    if len < END_RECORD_LEN {
        return Err(invalid("the project package is too short to be an archive"));
    }
    let mut rec = [0u8; END_RECORD_LEN as usize];
    reader
        .seek(SeekFrom::Start(len - END_RECORD_LEN))
        .and_then(|_| reader.read_exact(&mut rec))
        .map_err(unreadable)?;
    let u16_at = |i: usize| u64::from(u16::from_le_bytes([rec[i], rec[i + 1]]));
    let u32_at = |i: usize| {
        u64::from(u32::from_le_bytes([
            rec[i],
            rec[i + 1],
            rec[i + 2],
            rec[i + 3],
        ]))
    };
    if rec[..4] != [0x50, 0x4b, 0x05, 0x06] || u16_at(20) != 0 {
        return Err(invalid(
            "the project package must end with a plain ZIP end record (no comment or trailing data)",
        ));
    }
    let (entries, directory_size, directory_offset) = (u16_at(10), u32_at(12), u32_at(16));
    if u16_at(4) != 0 || u16_at(6) != 0 || u16_at(8) != entries {
        return Err(invalid("the project package spans several disks"));
    }
    if entries == 0xFFFF || directory_offset == 0xFFFF_FFFF || directory_size == 0xFFFF_FFFF {
        return Err(invalid(
            "the project package uses ZIP64, which no package needs",
        ));
    }
    if directory_offset + directory_size != len - END_RECORD_LEN {
        return Err(invalid(
            "the project package's central directory does not end at its end record",
        ));
    }
    Ok(EndRecord {
        entries,
        directory_offset,
    })
}

/// The facts the per-entry scan keeps: every name's declared size, and the
/// first entry's name.
struct EntryScan {
    sizes: HashMap<String, u64>,
    first: String,
}

/// Every per-entry rule that needs only the central directory and the
/// local header offsets -- nothing is decompressed here.
fn scan_entries<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<EntryScan, EditorError> {
    let mut sizes = HashMap::new();
    let mut folded = HashSet::new();
    let mut first = String::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let file = archive.by_index_raw(i).map_err(unreadable)?;
        let name = file.name().to_string();
        if std::str::from_utf8(file.name_raw()) != Ok(name.as_str()) {
            return Err(invalid(format!(
                "package entry {name:?} is not allowed: its name is not UTF-8"
            )));
        }
        validate_entry_name(&name)
            .map_err(|why| invalid(format!("package entry {name:?} is not allowed: {why}")))?;
        if !folded.insert(name.to_lowercase()) {
            return Err(invalid(format!(
                "package entry {name:?} is a duplicate (names are compared case-insensitively)"
            )));
        }
        if file
            .unix_mode()
            .is_some_and(|m| m & S_IFMT != 0 && m & S_IFMT != S_IFREG)
        {
            return Err(invalid(format!(
                "package entry {name:?} is not a regular file (a symlink or special entry)"
            )));
        }
        if file.encrypted() {
            return Err(invalid(format!("package entry {name:?} is encrypted")));
        }
        let (size, compressed) = (file.size(), file.compressed_size());
        match file.compression() {
            CompressionMethod::Stored if size != compressed => {
                return Err(invalid(format!(
                    "package entry {name:?} is stored with inconsistent sizes"
                )));
            }
            CompressionMethod::Stored | CompressionMethod::Deflated => {}
            other => {
                return Err(invalid(format!(
                    "package entry {name:?} uses unsupported compression {other}"
                )));
            }
        }
        if size > compressed.max(1).saturating_mul(limits::MAX_ENTRY_RATIO) {
            return Err(invalid(format!(
                "package entry {name:?} expands {size} bytes from {compressed}, past the {}:1 ratio limit",
                limits::MAX_ENTRY_RATIO
            )));
        }
        total = total.saturating_add(size);
        if total > limits::MAX_PACKAGE_BYTES {
            return Err(invalid(format!(
                "the project package expands past {} bytes",
                limits::MAX_PACKAGE_BYTES
            )));
        }
        if i == 0 {
            first = name.clone();
        }
        sizes.insert(name, size);
    }
    Ok(EntryScan { sizes, first })
}

/// Reads a JSON entry into memory, bounded twice: its DECLARED size is
/// checked first (a cheap refusal), and the copy is bounded again by
/// `extract_entry_bounded` because the declaration may lie.
fn read_json_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    scan: &EntryScan,
    name: &str,
    max: u64,
) -> Result<Vec<u8>, EditorError> {
    let declared = *scan
        .sizes
        .get(name)
        .ok_or_else(|| invalid(format!("the project package has no {name}")))?;
    if declared > max {
        return Err(invalid(format!(
            "{name} declares {declared} bytes, past its {max}-byte limit"
        )));
    }
    let mut out = Vec::new();
    extract_entry_bounded(archive, name, &mut out, max)?;
    Ok(out)
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `media/<assetId>.<ext>` with `ext` 1-8 ASCII alphanumerics.
fn is_media_path(path: &str, asset_id: &str) -> bool {
    path.strip_prefix("media/")
        .and_then(|rest| rest.strip_prefix(asset_id))
        .and_then(|rest| rest.strip_prefix('.'))
        .is_some_and(|ext| {
            (1..=MAX_EXTENSION_CHARS).contains(&ext.len())
                && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// One manifest file entry, media or product, as `check_listed_file` sees it.
struct Listed<'a> {
    kind: &'static str,
    id: &'a str,
    path: &'a str,
    /// Whether `path` is exactly the name this kind of entry must carry.
    path_ok: bool,
    size: u64,
    sha256: &'a str,
}

/// One manifest file entry's shared rules: a valid, unique id, the exact
/// path shape, a well-formed digest, and a size that agrees with the
/// archive's own entry.
fn check_listed_file(
    f: &Listed<'_>,
    scan: &EntryScan,
    seen: &mut HashSet<String>,
) -> Result<(), EditorError> {
    let (kind, id, path) = (f.kind, f.id, f.path);
    if !is_valid_id(id) || !seen.insert(id.to_string()) {
        return Err(invalid(format!(
            "manifest {kind} {id:?}: invalid or duplicate id"
        )));
    }
    if !f.path_ok {
        return Err(invalid(format!(
            "manifest {kind} {id}: path {path:?} is not its expected name"
        )));
    }
    if !is_sha256_hex(f.sha256) {
        return Err(invalid(format!(
            "manifest {kind} {id}: sha256 is not 64 lowercase hex digits"
        )));
    }
    match scan.sizes.get(path) {
        None => Err(invalid(format!(
            "manifest {kind} {id}: {path} is missing from the archive"
        ))),
        Some(&actual) if actual != f.size => Err(invalid(format!(
            "manifest {kind} {id}: declares size {} but the archive entry holds {actual}",
            f.size
        ))),
        Some(_) => Ok(()),
    }
}

/// The manifest's own rules and its agreement with the archive: every
/// listed file present at its declared size, and nothing in the archive
/// that the manifest does not list.
fn check_manifest(m: &PackageManifest, scan: &EntryScan) -> Result<(), EditorError> {
    if m.schema != PACKAGE_SCHEMA {
        return Err(invalid(format!(
            "manifest: schema must be {PACKAGE_SCHEMA:?}, got {:?}",
            m.schema
        )));
    }
    if !is_valid_id(&m.project_id) {
        return Err(invalid("manifest: projectId is not a valid id"));
    }
    if chrono::DateTime::parse_from_rfc3339(&m.saved_at).is_err() {
        return Err(invalid("manifest: savedAt is not an RFC 3339 timestamp"));
    }
    if m.workspace != WORKSPACE_NAME {
        return Err(invalid(format!(
            "manifest: workspace must be {WORKSPACE_NAME:?}"
        )));
    }
    if m.media.len() > limits::MAX_ASSETS || m.products.len() > limits::MAX_PRODUCTS {
        return Err(invalid("manifest: too many media or product files"));
    }
    let mut seen = HashSet::new();
    for f in &m.media {
        let listed = Listed {
            kind: "media",
            id: &f.asset_id,
            path: &f.path,
            path_ok: is_media_path(&f.path, &f.asset_id),
            size: f.size,
            sha256: &f.sha256,
        };
        check_listed_file(&listed, scan, &mut seen)?;
    }
    let mut seen = HashSet::new();
    for f in &m.products {
        let listed = Listed {
            kind: "product",
            id: &f.product_id,
            path: &f.path,
            path_ok: f.path == format!("products/{}.mp4", f.product_id),
            size: f.size,
            sha256: &f.sha256,
        };
        check_listed_file(&listed, scan, &mut seen)?;
    }
    let media_bytes = m
        .media
        .iter()
        .map(|f| f.size)
        .chain(m.products.iter().map(|f| f.size))
        .fold(0u64, u64::saturating_add);
    if media_bytes > limits::MAX_PACKAGE_MEDIA_BYTES {
        return Err(invalid(format!(
            "the package's media and products total {media_bytes} bytes, past the {}-byte portable limit",
            limits::MAX_PACKAGE_MEDIA_BYTES
        )));
    }
    let listed: HashSet<&str> = m
        .media
        .iter()
        .map(|f| f.path.as_str())
        .chain(m.products.iter().map(|f| f.path.as_str()))
        .chain([MANIFEST_NAME, WORKSPACE_NAME])
        .collect();
    if let Some(extra) = scan.sizes.keys().find(|n| !listed.contains(n.as_str())) {
        return Err(invalid(format!(
            "package entry {extra:?} is not listed in the manifest"
        )));
    }
    Ok(())
}

/// Cross-checks the manifest against the validated envelope and returns
/// the missing asset ids (A17's lightweight semantics).
///
/// An asset needs no packaged bytes when it is a `card` builtin (the one
/// asset kind the native app synthesizes from the project itself) or a
/// `linked_asset` (detached audio: its bytes are its source's, and that
/// source is itself in the referenced set). Every OTHER builtin kind
/// counts as media: natively a `screen` builtin is a staged capture backed
/// by a real file (`validate_media::check_linked_assets`' own note), and a
/// reference-sample builtin has no native generator -- exempting either
/// would prune a real source from the package without a word.
fn cross_check(m: &PackageManifest, env: &WorkspaceEnvelope) -> Result<Vec<String>, EditorError> {
    if m.project_id != env.project.id {
        return Err(invalid(format!(
            "manifest: projectId {} does not match the workspace's project {}",
            m.project_id, env.project.id
        )));
    }
    let products: HashSet<&str> = env.record.products.iter().map(|p| p.id.as_str()).collect();
    if let Some(f) = m
        .products
        .iter()
        .find(|f| !products.contains(f.product_id.as_str()))
    {
        return Err(invalid(format!(
            "manifest product {} is not a product in the workspace record",
            f.product_id
        )));
    }
    let referenced: BTreeSet<String> = assets_referenced(&env.project, &env.record.products);
    if let Some(f) = m.media.iter().find(|f| !referenced.contains(&f.asset_id)) {
        return Err(invalid(format!(
            "manifest media {} is not used by the project or any retained product",
            f.asset_id
        )));
    }
    let snapshots = env
        .record
        .products
        .iter()
        .filter_map(|p| p.snapshot.as_deref());
    let mut assets: HashMap<&str, &Asset> = HashMap::new();
    for asset in std::iter::once(&env.project)
        .chain(snapshots)
        .flat_map(|p| &p.assets)
    {
        assets.entry(asset.id.as_str()).or_insert(asset);
    }
    let packaged: HashSet<&str> = m.media.iter().map(|f| f.asset_id.as_str()).collect();
    Ok(referenced
        .into_iter()
        .filter(|id| !packaged.contains(id.as_str()))
        .filter(|id| {
            assets
                .get(id.as_str())
                .is_none_or(|a| a.builtin != Some(Builtin::Card) && a.linked_asset.is_none())
        })
        .collect())
}

/// Validates an untrusted package end to end WITHOUT extracting any media:
/// see the module doc for the order and why it is that order. `Err` is
/// always `invalidProject` and names the first rule the archive breaks.
pub fn inspect_archive<R: Read + Seek>(mut reader: R) -> Result<PackageIndex, EditorError> {
    let len = reader.seek(SeekFrom::End(0)).map_err(unreadable)?;
    if len > limits::MAX_PACKAGE_BYTES {
        return Err(invalid(format!(
            "the project package is {len} bytes, past the {}-byte limit",
            limits::MAX_PACKAGE_BYTES
        )));
    }
    let end = read_end_record(&mut reader, len)?;
    if end.entries > limits::MAX_PACKAGE_ENTRIES as u64 {
        return Err(invalid(format!(
            "the project package has {} entries, past the {} limit",
            end.entries,
            limits::MAX_PACKAGE_ENTRIES
        )));
    }
    reader.seek(SeekFrom::Start(0)).map_err(unreadable)?;
    let mut archive = ZipArchive::new(reader).map_err(unreadable)?;
    if archive.len() as u64 != end.entries
        || archive.central_directory_start() != end.directory_offset
        || archive.offset() != 0
    {
        return Err(invalid(
            "the project package holds duplicate entry names (its directory lists more entries than distinct names)",
        ));
    }
    let scan = scan_entries(&mut archive)?;
    if scan.first != MANIFEST_NAME {
        return Err(invalid(format!(
            "the project package's first entry must be {MANIFEST_NAME}, found {:?}",
            scan.first
        )));
    }
    let raw = read_json_entry(&mut archive, &scan, MANIFEST_NAME, MAX_MANIFEST_BYTES)?;
    let manifest: PackageManifest =
        serde_json::from_slice(&raw).map_err(|e| invalid(format!("manifest: {e}")))?;
    check_manifest(&manifest, &scan)?;
    let raw = read_json_entry(
        &mut archive,
        &scan,
        WORKSPACE_NAME,
        limits::MAX_PROJECT_JSON_BYTES,
    )?;
    let envelope: WorkspaceEnvelope =
        serde_json::from_slice(&raw).map_err(|e| invalid(format!("{WORKSPACE_NAME}: {e}")))?;
    validate_envelope(&envelope)?;
    let missing = cross_check(&manifest, &envelope)?;
    Ok(PackageIndex {
        manifest,
        envelope,
        missing,
    })
}

/// Copies entry `name` into `dest`, failing as soon as MORE than `max`
/// bytes come out -- regardless of what the entry's header declares, since
/// a deflate stream decodes to whatever length it encodes and the ZIP
/// reader does not stop at the declared size. Nothing past `max` is ever
/// written to `dest`. Returns the true count and its SHA-256, which a
/// caller compares against the manifest.
pub fn extract_entry_bounded<R: Read + Seek, W: Write>(
    archive: &mut ZipArchive<R>,
    name: &str,
    dest: &mut W,
    max: u64,
) -> Result<ExtractedEntry, EditorError> {
    let mut file = archive
        .by_name(name)
        .map_err(|e| invalid(format!("package entry {name:?}: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| invalid(format!("package entry {name:?} is corrupt: {e}")))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > max {
            return Err(invalid(format!(
                "package entry {name:?} expands past its {max}-byte limit"
            )));
        }
        hasher.update(&buf[..n]);
        dest.write_all(&buf[..n]).map_err(|e| write_failed(&e))?;
    }
    let sha256 = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(ExtractedEntry {
        bytes: total,
        sha256,
    })
}

/// Writes a package: `package.json` first, then `workspace.json` (both
/// Deflated), then each `(name, reader)` of `files` Stored and streamed --
/// media is already compressed, and Stored keeps our own output far from
/// the ratio guard. Every file must be a manifest-listed path, written
/// once, at exactly its listed size, and every listed path must be
/// written: a package this function produces always passes
/// `inspect_archive`'s structural rules, or it is not produced at all.
pub fn write_package<W, N, R>(
    writer: W,
    manifest: &PackageManifest,
    workspace_json: &[u8],
    files: impl IntoIterator<Item = (N, R)>,
) -> Result<W, EditorError>
where
    W: Write + Seek,
    N: AsRef<str>,
    R: Read,
{
    let internal = |m: String| EditorError::new(EditorErrorCode::Internal, m);
    let mut pending: HashMap<&str, u64> = manifest
        .media
        .iter()
        .map(|f| (f.path.as_str(), f.size))
        .chain(manifest.products.iter().map(|f| (f.path.as_str(), f.size)))
        .collect();
    let json = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let manifest_json = serde_json::to_vec_pretty(manifest)
        .map_err(|e| internal(format!("serializing the package manifest failed: {e}")))?;

    let mut zip = ZipWriter::new(writer);
    for (name, bytes) in [
        (MANIFEST_NAME, &manifest_json[..]),
        (WORKSPACE_NAME, workspace_json),
    ] {
        zip.start_file(name, json).map_err(zip_write_failed)?;
        zip.write_all(bytes).map_err(|e| write_failed(&e))?;
    }
    for (name, mut reader) in files {
        let name = name.as_ref();
        validate_entry_name(name).map_err(|why| internal(format!("{name:?} {why}")))?;
        let expected = pending
            .remove(name)
            .ok_or_else(|| internal(format!("{name} is not an unwritten manifest path")))?;
        zip.start_file(name, stored).map_err(zip_write_failed)?;
        let copied = io::copy(&mut reader, &mut zip).map_err(|e| write_failed(&e))?;
        if copied != expected {
            return Err(internal(format!(
                "{name} is {copied} bytes but the manifest lists {expected}"
            )));
        }
    }
    if let Some(name) = pending.keys().next() {
        return Err(internal(format!("manifest path {name} was never written")));
    }
    zip.finish().map_err(zip_write_failed)
}

#[cfg(test)]
#[path = "package_tests.rs"]
mod tests;
