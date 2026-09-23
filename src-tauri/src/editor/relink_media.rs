//! The reconnect pipeline behind `editor_relink_media` (Task 40; F-03;
//! A19; SCREENS 07) — the `AppHandle`-free half, so it runs on a tempdir.
//! `relink_commands.rs` owns the command, its named thread and the dialog.
//!
//! **In order, and where each rule lives:**
//! 1. `check_request` / `targets` — BEFORE the dialog: every id names a
//!    graph asset whose `sources.json` record is file-backed and really
//!    missing. A present source is not "reconnected" over; a builtin has no
//!    file; a staged capture's file belongs to the capture staging folder,
//!    and moving its record into `media\` would cut the project's link to
//!    that capture (its pin, its `sourceBase`) — refused (GAP-183).
//! 2. `examine` — each picked file probed (the import's own `describe`, so
//!    a still image's length is the same default an import records) and
//!    SHA-256 hashed, on the caller's `editor-relink` thread. A file that
//!    cannot be read is a `perFile` problem, never the whole call's.
//! 3. `core::editor::relink::match_candidates` decides. Ambiguous and
//!    mismatched sources are left EXACTLY as they were and reported.
//! 4. A mismatch is applied only with `confirmReplace` (one source, one
//!    file), and even then only when the replacement cannot break the edit:
//!    the same kind, and at least as long as the asset the graph describes —
//!    the graph keeps its asset (id, length, every clip and cue on it), so a
//!    shorter file would leave clips pointing past its end.
//! 5. Each applied file is copied into `media\<assetId>.<ext>` through the
//!    import's owned `.part` + `rename_noreplace`, and the copy's hash must
//!    equal the one matched (a file changed in between is not reconnected).
//! 6. Under the session's SAVE lock: `sources.json` is rewritten from the
//!    chosen file's own probe (never the old record's facts), then ONE
//!    `InternalCommand::RelinkAssets` bumps the revision — rolled back
//!    (records restored, copies removed) if it is refused — and the
//!    relinked assets' cached thumbnails and waveforms are purged
//!    (GAP-176: a thumbnail is not tied to the file it was cut from).
//!
//! Every message names files and assets by DISPLAY name, never a path.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use vault_buddy_core::editor::commands::payloads::RelinkAssetsPayload;
use vault_buddy_core::editor::import_io::{copy_hashing, display_name};
use vault_buddy_core::editor::probe::{classify_extension, ImportKind};
use vault_buddy_core::editor::relink::{
    match_candidates, CandidateFacts, ExpectedSource, NamedReport, RelinkFileProblem,
    RelinkReportDto, RelinkedFile, DURATION_TOLERANCE_MS,
};
use vault_buddy_core::editor::{
    is_valid_id, limits, EditorError, EditorErrorCode, EditorProjection, InternalCommand,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::authz::require_session;
use super::media_import::{
    copy_owned, describe, extension_of, file_name_of, media_dir, remove_quietly, source_size,
    ImportIo,
};
use super::prefs_commands::project_id_for;
use super::project_store::{
    project_dir, resolve_source, ReplacedFrom, SourceLocator, SourceMediaKind, SourceRecord,
};
use super::save_commands::session_save_lock;
use super::session_commands::missing_media;
use super::store_io::{load_sources, write_sources};
use super::EditorState;

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidRequest, message)
}

fn internal(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::Internal, message)
}

/// Everything one reconnect runs against.
pub(crate) struct RelinkJob<'a> {
    pub state: &'a EditorState,
    pub root: &'a Path,
    pub session_id: &'a str,
    pub io: &'a dyn ImportIo,
}

/// One missing source the request names, as it stood before the dialog.
pub(crate) struct Target {
    pub asset_id: String,
    /// The asset's display name — what the dialog title and every message
    /// call it.
    pub name: String,
    pub record: SourceRecord,
    /// The length the GRAPH gives the asset: a replacement must cover it.
    graph_duration_ms: u64,
}

/// The request's own shape, before anything is read.
pub(crate) fn check_request(
    asset_ids: &[String],
    confirm_replace: bool,
) -> Result<(), EditorError> {
    if asset_ids.is_empty() || asset_ids.len() > limits::MAX_ASSETS {
        return Err(invalid(
            "Name between one and 200 media items to reconnect.",
        ));
    }
    let mut seen = HashSet::new();
    if !asset_ids
        .iter()
        .all(|id| is_valid_id(id) && seen.insert(id))
    {
        return Err(invalid("The media to reconnect were not named correctly."));
    }
    if confirm_replace && asset_ids.len() != 1 {
        return Err(invalid(
            "A replacement is chosen for one media item at a time.",
        ));
    }
    Ok(())
}

fn to_import_kind(kind: SourceMediaKind) -> ImportKind {
    match kind {
        SourceMediaKind::Video => ImportKind::Video,
        SourceMediaKind::Audio => ImportKind::Audio,
        SourceMediaKind::Image => ImportKind::Image,
    }
}

/// Every requested source, checked (step 1 of the module doc), with the
/// project it belongs to.
pub(crate) fn targets(
    job: &RelinkJob,
    asset_ids: &[String],
) -> Result<(String, Vec<Target>), EditorError> {
    let project_id = project_id_for(job.state, job.session_id)?;
    let graph: BTreeMap<String, (String, u64)> = require_session(job.state, job.session_id)?
        .get(job.session_id)
        .map(|s| {
            s.project()
                .assets
                .iter()
                .map(|a| (a.id.clone(), (a.name.clone(), a.duration_ms)))
                .collect()
        })
        .unwrap_or_default();
    let mut sources = load_sources(job.root, &project_id)?;
    let mut out = Vec::new();
    for id in asset_ids {
        let (name, graph_duration_ms) = graph
            .get(id)
            .cloned()
            .ok_or_else(|| invalid("That media is not part of this project."))?;
        let record = sources
            .remove(id)
            .ok_or_else(|| invalid(format!("“{name}” has no file of its own to reconnect.")))?;
        match record.locator {
            SourceLocator::Builtin => {
                return Err(invalid(format!("“{name}” has no file to reconnect.")))
            }
            SourceLocator::Staging { .. } => {
                return Err(invalid(format!(
                    "“{name}” is a screen recording kept with your captures, so it cannot be reconnected here."
                )))
            }
            SourceLocator::Media { .. } | SourceLocator::Takes { .. } => {}
        }
        if resolve_source(job.root, &project_id, &record).is_some_and(|p| p.is_file()) {
            return Err(invalid(format!("“{name}” is not missing.")));
        }
        out.push(Target {
            asset_id: id.clone(),
            name,
            record,
            graph_duration_ms,
        });
    }
    Ok((project_id, out))
}

/// One picked file, examined: what it is, the record it would get, and
/// where it is.
struct Examined {
    facts: CandidateFacts,
    record: SourceRecord,
    path: PathBuf,
    ext: String,
}

fn hash_file(io: &dyn ImportIo, path: &Path) -> Result<String, EditorError> {
    let mut reader = io.open_source(path).map_err(|e| {
        log::warn!(
            "editor relink: a picked file could not be opened ({:?})",
            e.kind()
        );
        invalid("The file could not be opened.")
    })?;
    copy_hashing(&mut *reader, &mut io::sink())
        .map(|(_, sha)| sha)
        .map_err(|e| {
            log::warn!(
                "editor relink: a picked file could not be read ({:?})",
                e.kind()
            );
            invalid("The file could not be read.")
        })
}

/// Probe first (cheap, and it refuses what is not media at all), then hash.
fn examine(io: &dyn ImportIo, path: &Path) -> Result<Examined, EditorError> {
    let full = file_name_of(path);
    let name = display_name(&full);
    let kind = classify_extension(&full).ok_or_else(|| {
        EditorError::new(
            EditorErrorCode::UnsupportedMedia,
            "This is not a supported video, audio or image type.",
        )
    })?;
    let size = source_size(path)?;
    if kind != ImportKind::Image {
        io.av_ready()?;
    }
    let (asset, record) = describe(io, path, kind, size, "relink".into(), name.clone())?;
    let sha256 = hash_file(io, path)?;
    Ok(Examined {
        facts: CandidateFacts {
            name,
            sha256: sha256.clone(),
            size,
            duration_ms: asset.duration_ms,
            kind: to_import_kind(record.media_kind),
        },
        record: SourceRecord {
            sha256: Some(sha256),
            ..record
        },
        path: path.to_path_buf(),
        ext: extension_of(&full),
    })
}

/// A file about to be applied: which target, which candidate, and whether
/// it is a confirmed replacement.
struct Apply {
    target: usize,
    candidate: usize,
    replace: bool,
}

/// Step 4: the confirmed mismatch, if it cannot break the edit.
fn confirmed_replacement(target: &Target, candidate: &CandidateFacts) -> Result<(), EditorError> {
    let (file, name) = (&candidate.name, &target.name);
    if to_import_kind(target.record.media_kind) != candidate.kind {
        return Err(invalid(format!(
            "“{file}” cannot replace “{name}”: it is a different kind of media."
        )));
    }
    if candidate.duration_ms + DURATION_TOLERANCE_MS < target.graph_duration_ms {
        return Err(invalid(format!(
            "“{file}” is shorter than “{name}”, so your edit could run past its end. Choose a file at least as long."
        )));
    }
    Ok(())
}

/// A copy in place, and the record it gets.
struct Copied {
    asset_id: String,
    dest: PathBuf,
    record: SourceRecord,
}

fn copy_in(
    job: &RelinkJob,
    project_id: &str,
    target: &Target,
    examined: &Examined,
    replace: bool,
) -> Result<Copied, EditorError> {
    let file = format!("{}.{}", target.asset_id, examined.ext);
    let media = media_dir(job.root, project_id)?;
    let (dest, sha256) = copy_owned(job.io, &examined.path, &media, &file)?;
    if sha256 != examined.facts.sha256 {
        remove_quietly(&dest);
        return Err(invalid(format!(
            "“{}” changed while it was being reconnected. Choose it again.",
            examined.facts.name
        )));
    }
    let old = &target.record;
    let replaced_from = if replace {
        old.replaced_from.clone().or_else(|| {
            Some(ReplacedFrom {
                sha256: old.sha256.clone(),
                size: old.size,
                duration_ms: old.duration_ms,
                media_kind: old.media_kind,
            })
        })
    } else {
        old.replaced_from.clone()
    };
    Ok(Copied {
        asset_id: target.asset_id.clone(),
        dest,
        record: SourceRecord {
            locator: SourceLocator::Media { file },
            replaced_from,
            ..examined.record.clone()
        },
    })
}

/// Is `name` one of `record_id`'s derived cache files —
/// `<id>-<digits>.jpg` (a thumbnail) or `<id>.peaks.<digits>.json` (a
/// waveform)? `media_derive`'s own two name shapes, exactly.
fn derived_of(name: &str, record_id: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let Some(rest) = name.strip_prefix(record_id) else {
        return false;
    };
    let thumb = rest
        .strip_prefix('-')
        .and_then(|r| r.strip_suffix(".jpg"))
        .is_some_and(digits);
    let peaks = rest
        .strip_prefix(".peaks.")
        .and_then(|r| r.strip_suffix(".json"))
        .is_some_and(digits);
    thumb || peaks
}

/// GAP-176: drop `record_ids`' cached thumbnails and waveforms — our own
/// plain files only (no-follow), never anything else in `cache\`.
fn purge_derived(root: &Path, project_id: &str, record_ids: &[&str]) {
    let Some(cache) = project_dir(root, project_id).map(|d| d.join("cache")) else {
        return;
    };
    let entries = match std::fs::read_dir(&cache) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return,
        Err(e) => return log::warn!("editor relink: the cache could not be listed: {e}"),
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = record_ids.iter().any(|id| derived_of(&name, id));
        let plain = std::fs::symlink_metadata(entry.path()).is_ok_and(|m| m.is_file());
        if ours && plain {
            remove_quietly(&entry.path());
        }
    }
}

/// Step 6, under the save lock. Returns the projection after the edit.
fn commit(
    job: &RelinkJob,
    project_id: &str,
    targets: &[Target],
    copied: &[Copied],
) -> Result<(), EditorError> {
    let lock = session_save_lock(job.state, job.session_id)?;
    let _no_save_or_discard_meanwhile = lock_ignoring_poison(&lock);
    let before = load_sources(job.root, project_id)?;
    let unchanged = targets
        .iter()
        .filter(|t| copied.iter().any(|c| c.asset_id == t.asset_id))
        .all(|t| before.get(&t.asset_id) == Some(&t.record));
    if !unchanged {
        return Err(EditorError::new(
            EditorErrorCode::RevisionConflict,
            "The project's media changed while the files were being checked. Try again.",
        ));
    }
    let mut after = before.clone();
    for c in copied {
        after.insert(c.asset_id.clone(), c.record.clone());
    }
    write_sources(job.root, project_id, &after)
        .map_err(|e| internal(format!("The project's media list could not be saved: {e}")))?;
    let ids: Vec<String> = copied.iter().map(|c| c.asset_id.clone()).collect();
    let applied = require_session(job.state, job.session_id).and_then(|mut sessions| {
        let session = sessions
            .get_mut(job.session_id)
            .ok_or_else(|| internal("session vanished under its own lock"))?;
        session.execute_internal(&InternalCommand::RelinkAssets(RelinkAssetsPayload {
            asset_ids: ids.clone(),
        }))
    });
    if let Err(e) = applied {
        if let Err(w) = write_sources(job.root, project_id, &before) {
            log::warn!("editor relink: sources.json not restored after a refusal: {w}");
        }
        return Err(e);
    }
    let record_ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    purge_derived(job.root, project_id, &record_ids);
    Ok(())
}

/// Run one reconnect over the files the user picked (module doc).
pub(crate) fn relink_in(
    job: &RelinkJob,
    asset_ids: &[String],
    confirm_replace: bool,
    files: &[PathBuf],
) -> Result<RelinkReportDto, EditorError> {
    check_request(asset_ids, confirm_replace)?;
    if confirm_replace && files.len() != 1 {
        return Err(invalid("Choose exactly one file as the replacement."));
    }
    let (project_id, targets) = targets(job, asset_ids)?;
    let mut examined = Vec::new();
    let mut per_file = Vec::new();
    for path in files {
        match examine(job.io, path) {
            Ok(one) => examined.push(one),
            Err(e) => per_file.push(RelinkFileProblem {
                name: display_name(&file_name_of(path)),
                error: e.message,
            }),
        }
    }
    let expected: Vec<(String, ExpectedSource)> = targets
        .iter()
        .map(|t| {
            (
                t.asset_id.clone(),
                ExpectedSource {
                    sha256: t.record.sha256.clone(),
                    size: t.record.size,
                    duration_ms: t.record.duration_ms,
                    kind: to_import_kind(t.record.media_kind),
                },
            )
        })
        .collect();
    let facts: Vec<CandidateFacts> = examined.iter().map(|e| e.facts.clone()).collect();
    let mut report = match_candidates(&expected, &facts);
    let index = |id: &str| targets.iter().position(|t| t.asset_id == id);
    let mut applies: Vec<Apply> = report
        .matched
        .iter()
        .filter_map(|(id, c)| {
            Some(Apply {
                target: index(id)?,
                candidate: *c,
                replace: false,
            })
        })
        .collect();
    if confirm_replace {
        if let Some((id, c, _)) = report.mismatched.first().cloned() {
            let target = index(&id).ok_or_else(|| internal("a report named an unknown asset"))?;
            confirmed_replacement(&targets[target], &facts[c])?;
            report.mismatched.clear();
            applies.push(Apply {
                target,
                candidate: c,
                replace: true,
            });
        }
    }
    let mut copied = Vec::new();
    let mut replaced = Vec::new();
    let mut matched_names = NamedReport::of(&report, &facts);
    for a in &applies {
        let target = &targets[a.target];
        match copy_in(job, &project_id, target, &examined[a.candidate], a.replace) {
            Ok(c) => {
                copied.push(c);
                if a.replace {
                    replaced.push(RelinkedFile {
                        asset_id: target.asset_id.clone(),
                        file: facts[a.candidate].name.clone(),
                    });
                }
            }
            Err(e) => {
                matched_names
                    .matched
                    .retain(|m| m.asset_id != target.asset_id);
                matched_names.unmatched.push(target.asset_id.clone());
                per_file.push(RelinkFileProblem {
                    name: facts[a.candidate].name.clone(),
                    error: e.message,
                });
            }
        }
    }
    if !copied.is_empty() {
        if let Err(e) = commit(job, &project_id, &targets, &copied) {
            for c in &copied {
                remove_quietly(&c.dest);
            }
            return Err(e);
        }
        super::recovery::note_acknowledged(job.state, job.root, job.session_id);
    }
    reply(job, &project_id, matched_names, replaced, per_file)
}

fn reply(
    job: &RelinkJob,
    project_id: &str,
    named: NamedReport,
    replaced: Vec<RelinkedFile>,
    per_file: Vec<RelinkFileProblem>,
) -> Result<RelinkReportDto, EditorError> {
    let sources = load_sources(job.root, project_id)?;
    let sessions = require_session(job.state, job.session_id)?;
    let session = sessions
        .get(job.session_id)
        .ok_or_else(|| internal("session vanished under its own lock"))?;
    Ok(RelinkReportDto {
        projection: EditorProjection::of(session),
        missing: missing_media(job.root, session.project(), &sources),
        matched: named.matched,
        replaced,
        ambiguous: named.ambiguous,
        unmatched: named.unmatched,
        mismatched: named.mismatched,
        per_file,
    })
}

#[cfg(test)]
#[path = "relink_media_tests.rs"]
mod tests;
