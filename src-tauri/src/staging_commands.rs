//! Staging as a WHOLE: what the directory is holding, and emptying it.
//!
//! The third module along a seam this feature already draws twice.
//! `export_commands` owns the export LIFECYCLE, `staged_commands` owns a
//! staged capture as an OBJECT — one capture, addressed by base — and this
//! owns the DIRECTORY: a number the settings card can show, and the bulk
//! clear spec §10 asks for beside it. It is also what the Rust LOC cap
//! forces: `staged_commands` sits at 686 of 800 nonblank, and this feature
//! plus its tests does not fit there.
//!
//! **Every rule here is borrowed, never re-grown.** Which captures exist is
//! `staged_commands::staged_summaries`; which files one owns is
//! `staging_files::capture_file_names`; whether a capture may be removed is
//! `staged_commands::discard_conflict`; the removal itself, with its
//! two-pass symlink refusal, is `staged_commands::discard_staged_files`.
//! A bulk clear that grew its own copy of any of those would be a second
//! answer to "is this file ours", on the one code path in the app that
//! deletes many of the user's recordings at once.
//!
//! **It deliberately does NOT refuse while a capture is recording.** A live
//! capture writes `.<base>.mp4.part`, which is not one of the three files a
//! staged capture owns, and it has no published `<base>.mp4`, so
//! `staged_summaries` cannot see it and a clear cannot reach it. Adding a
//! capture-guard refusal here would therefore block a safe operation and
//! diverge from the single discard, which refuses on the export alone.

use tauri::{AppHandle, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging_files::{self, StagingUsage};

use crate::export_commands::{emit_discarded, ExportState};
use crate::staged_commands::{
    discard_conflict, discard_staged_files, staged_summaries, staging_dir_for,
};

/// What a bulk clear actually did. Four numbers rather than a bare success,
/// because three different things can happen to a capture and a UI that
/// reported only "done" would be claiming the export-skipped one was
/// removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearStagedResultDto {
    pub cleared: usize,
    pub bytes_freed: u64,
    /// Left alone because an export is writing it right now.
    pub skipped: usize,
    /// Refused or errored — a symlinked leaf, or an unlink that failed.
    pub failed: usize,
}

/// What staging is holding.
///
/// ASYNC: a directory read plus a sidecar parse per capture, then a
/// `symlink_metadata` per file. Degrades to zero rather than erroring — a
/// settings card that cannot render because a size could not be measured is
/// worse than one reporting nothing to clear, and the Clear button beside it
/// is disabled by the same zero.
#[tauri::command]
pub async fn staging_usage(app: AppHandle) -> StagingUsage {
    let dir = match staging_dir_for(&app) {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("staging_usage: {e}");
            return StagingUsage::default();
        }
    };
    let measured = tauri::async_runtime::spawn_blocking(move || {
        let bases: Vec<String> = staged_summaries(&dir).into_iter().map(|s| s.base).collect();
        staging_files::usage(&dir, &bases)
    })
    .await;
    match measured {
        Ok(usage) => usage,
        Err(e) => {
            log::warn!("staging_usage: the staging scan failed: {e}");
            StagingUsage::default()
        }
    }
}

/// Discard every staged capture at once (spec §10's "Clear staged
/// captures").
///
/// Irreversible, and the widest-reaching destructive action in the app: the
/// settings card confirm-gates it in two steps, because spec §10's rule is
/// that nothing is ever deleted silently.
///
/// Each capture is measured BEFORE it is removed — afterwards the files are
/// gone and there is nothing left to weigh — and counted as freed only once
/// its removal has actually succeeded, so a refusal cannot inflate the
/// number the user is shown.
///
/// ASYNC: up to three unlinks per capture on a volume that may be slow.
#[tauri::command]
pub async fn clear_staged_captures(app: AppHandle) -> Result<ClearStagedResultDto, String> {
    let dir = staging_dir_for(&app)?;
    let exporting = lock_ignoring_poison(&app.state::<ExportState>().0)
        .as_ref()
        .map(|active| active.base.clone());

    let worked = tauri::async_runtime::spawn_blocking(move || {
        let mut result = ClearStagedResultDto::default();
        let mut cleared = Vec::new();
        for summary in staged_summaries(&dir) {
            if discard_conflict(exporting.as_deref(), &summary.base).is_some() {
                result.skipped += 1;
                continue;
            }
            let bytes = staging_files::capture_bytes(&dir, &summary.base);
            match discard_staged_files(&dir, &summary.base) {
                Ok(()) => {
                    result.cleared += 1;
                    result.bytes_freed += bytes;
                    cleared.push(summary.base);
                }
                Err(e) => {
                    log::warn!("clear_staged_captures: {} was kept: {e}", summary.base);
                    result.failed += 1;
                }
            }
        }
        (result, cleared)
    })
    .await;

    let (result, cleared) = worked.map_err(|e| format!("Staging could not be cleared: {e}"))?;
    // One `screen:discarded` per capture actually removed. Without it the
    // store's `lastStaged` keeps pointing at a base no longer on disk, the
    // capture bar goes on offering Edit, and the editor opens on a sidecar
    // that is gone — the exact breakage that event was added for.
    for base in &cleared {
        emit_discarded(&app, base);
    }
    log::info!(
        "screen staging cleared: {} removed, {} skipped, {} failed, {} bytes freed",
        result.cleared,
        result.skipped,
        result.failed,
        result.bytes_freed
    );
    Ok(result)
}
