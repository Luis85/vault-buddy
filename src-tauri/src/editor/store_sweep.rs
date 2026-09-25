//! The startup sweep of the project store's own crash leftovers (hardening
//! Task 7; GAP-174 item 2, GAP-189, GAP-191, review finding D-4). Every
//! running process removes these itself on success, cancel and failure;
//! only a process that ENDS mid-way — a crash, a kill, the buddy's quit
//! doors, which close no editor session — leaves them, and nothing reused
//! them, because every later job mints a fresh id.
//!
//! What it removes, and nothing else:
//!
//! - `editor-projects\.<id>.creating\` — a project mint that never reached
//!   its rename (`store_io::create_project`, D-4), an hour old;
//! - per project, `media\.<assetId>.<ext>.part` — an import's or a
//!   reconnect's copy cut short (GAP-174 item 2), an hour old;
//! - `jobs\<jobId>\` — a render's scratch (GAP-189), an hour old, unless it
//!   holds a `publish.json`: an interrupted publish's journal is reported on
//!   every start and never deleted (GAP-192);
//! - `products\<productId>.mp4` that `products.json` does not record
//!   (GAP-189), an hour old — and only when the ledger reads cleanly: a
//!   ledger that cannot be read removes nothing in `products\`, since then
//!   nothing can tell a leftover from a product;
//! - `cache\review-<jobId>.mp4` — a Review render (GAP-191), at any age: a
//!   review belongs to a session, and there is none.
//!
//! **Strictly ours.** A name is ours only when it is exactly the shape this
//! app mints, around a VALID id (`is_valid_id`). Every entry — and every
//! folder walked into (`media`, `jobs`, `products`, `cache` and the project
//! folder itself) — is inspected with `symlink_metadata` first: a symlink
//! or an NTFS junction (which Rust reports as a symlink) is never followed,
//! never removed and never walked through. A directory is removed only by
//! `store_io::remove_dir_no_follow` (never `remove_dir_all`), a file only
//! when it is a plain file. `takes\` is deliberately NOT swept: a take's
//! `.part` is the user's recording, not scratch (GAP-197).
//!
//! **When.** On the `editor-recovery-sweep` thread at startup, under
//! `EditorState::open` — before any session can exist, so no job, import,
//! render or review can be live. The hour is belt and braces;
//! `sweep_at_startup` skips the whole pass should a session exist anyway.
//! A VIEW-style walk: a failure is logged (never a path or a name) and the
//! sweep moves on.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use vault_buddy_core::editor::{is_valid_id, product_file_name};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::project_store::store_dir;
use super::publish::PUBLISH_JOURNAL;
use super::render_jobs::{read_ledger, JOBS_DIR, PRODUCTS_DIR};
use super::render_review::is_review_name;
use super::store_io::{creating_project_id, remove_dir_no_follow};
use super::EditorState;

/// A leftover this old is an abandoned one — the import sweep's hour.
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);

/// What one pass did: leftovers removed, and ours-shaped entries left in
/// place (too fresh, a link, a publish journal, an unreadable ledger, or a
/// removal that failed).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct SweepReport {
    pub removed: usize,
    pub kept: usize,
}

/// What a candidate must be to be removed.
#[derive(Clone, Copy)]
enum Kind {
    PlainFile,
    RealDir,
}

/// Is `name` an import's or a reconnect's copy part,
/// `.<assetId>.<ext>.part` (`media_import::copy_owned`)?
fn is_media_part(name: &str) -> bool {
    name.strip_prefix('.')
        .and_then(|rest| rest.strip_suffix(".part"))
        .and_then(|file| file.rsplit_once('.'))
        .is_some_and(|(id, ext)| {
            is_valid_id(id) && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

/// `dir/name` when it is a real directory — never a link to one.
fn real_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let path = dir.join(name);
    std::fs::symlink_metadata(&path)
        .is_ok_and(|m| m.is_dir())
        .then_some(path)
}

/// Every entry of `dir` as `(name, path)`; an unreadable folder is empty.
fn entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    read.flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect()
}

/// Remove `path` when it is `kind` (no-follow) and, with `aged`, at least
/// `STALE_AFTER` older than `now`; otherwise count it kept.
fn consider(path: &Path, kind: Kind, aged: bool, now: SystemTime, report: &mut SweepReport) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    let right_kind = match kind {
        Kind::PlainFile => meta.is_file(),
        Kind::RealDir => meta.is_dir(),
    };
    let stale = !aged || meta.modified().is_ok_and(|at| at + STALE_AFTER <= now);
    if !right_kind || !stale {
        report.kept += 1;
        return;
    }
    let removed = match kind {
        Kind::PlainFile => std::fs::remove_file(path),
        Kind::RealDir => remove_dir_no_follow(path),
    };
    match removed {
        Ok(()) => report.removed += 1,
        Err(e) => {
            report.kept += 1;
            log::warn!(
                "editor-recovery-sweep: could not remove a leftover ({:?})",
                e.kind()
            );
        }
    }
}

fn sweep_media(project: &Path, now: SystemTime, report: &mut SweepReport) {
    let Some(media) = real_dir(project, "media") else {
        return;
    };
    for (name, path) in entries(&media) {
        if is_media_part(&name) {
            consider(&path, Kind::PlainFile, true, now, report);
        }
    }
}

fn sweep_jobs(project: &Path, now: SystemTime, report: &mut SweepReport) {
    let Some(jobs) = real_dir(project, JOBS_DIR) else {
        return;
    };
    for (name, path) in entries(&jobs) {
        if !is_valid_id(&name) {
            continue;
        }
        if std::fs::symlink_metadata(path.join(PUBLISH_JOURNAL)).is_ok() {
            report.kept += 1;
            continue;
        }
        consider(&path, Kind::RealDir, true, now, report);
    }
}

fn sweep_products(
    root: &Path,
    id: &str,
    project: &Path,
    now: SystemTime,
    report: &mut SweepReport,
) {
    let Some(products) = real_dir(project, PRODUCTS_DIR) else {
        return;
    };
    let candidates: Vec<(String, PathBuf)> = entries(&products)
        .into_iter()
        .filter(|(name, _)| name.strip_suffix(".mp4").is_some_and(is_valid_id))
        .collect();
    let recorded: HashSet<String> = match read_ledger(root, id) {
        Ok(ledger) => ledger
            .iter()
            .map(|p| product_file_name(&p.id).to_lowercase())
            .collect(),
        Err(_) => {
            report.kept += candidates.len();
            log::warn!(
                "editor-recovery-sweep: project {id}'s product ledger could not be read; its \
                 products folder was left as it is"
            );
            return;
        }
    };
    for (name, path) in candidates {
        // Case-insensitively recorded: when in doubt, keep.
        if !recorded.contains(&name.to_lowercase()) {
            consider(&path, Kind::PlainFile, true, now, report);
        }
    }
}

fn sweep_reviews(project: &Path, now: SystemTime, report: &mut SweepReport) {
    let Some(cache) = real_dir(project, "cache") else {
        return;
    };
    for (name, path) in entries(&cache) {
        if is_review_name(&name) {
            consider(&path, Kind::PlainFile, false, now, report);
        }
    }
}

/// Sweep the whole store (see the module doc for the rules).
pub(super) fn sweep_project_leftovers(root: &Path, now: SystemTime) -> SweepReport {
    let mut report = SweepReport::default();
    for (name, path) in entries(&store_dir(root)) {
        if creating_project_id(&name).is_some() {
            consider(&path, Kind::RealDir, true, now, &mut report);
            continue;
        }
        if !is_valid_id(&name) || !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        sweep_media(&path, now, &mut report);
        sweep_jobs(&path, now, &mut report);
        sweep_products(root, &name, &path, now, &mut report);
        sweep_reviews(&path, now, &mut report);
    }
    report
}

/// The startup pass, or `None` when a session is already open — then
/// something in the store may be in use, and the sweep waits for the next
/// start rather than race it. The caller holds `open`, so no session can
/// appear during the pass.
pub(super) fn sweep_at_startup(
    state: &EditorState,
    root: &Path,
    now: SystemTime,
) -> Option<SweepReport> {
    if !lock_ignoring_poison(&state.by_project).is_empty() {
        return None;
    }
    Some(sweep_project_leftovers(root, now))
}

#[cfg(test)]
#[path = "store_sweep_tests.rs"]
mod tests;
