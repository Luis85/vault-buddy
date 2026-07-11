//! IPC surface for the panel's cross-vault Search view. Read-only: the scan
//! never writes, and opening a hit is delegated to Obsidian via the
//! launch-logged `obsidian://` path. See
//! docs/superpowers/specs/2026-07-09-vault-search-design.md and the polish
//! follow-up docs/superpowers/specs/2026-07-09-search-polish-design.md.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use vault_buddy_core::{discovery, search, uri};

/// Bumped by every `search_vaults` call; a scan whose generation is no
/// longer current aborts at its next per-file poll instead of running a
/// stale multi-vault walk to completion. Relaxed suffices — it's a
/// freshness hint, not a synchronization point.
static SCAN_GENERATION: AtomicU64 = AtomicU64::new(0);

/// The process-lifetime search content cache. Lazily created on first use and
/// shared by every `search_vaults` call and the `search-prewarm` thread, so
/// searches reuse note content read once. Touched ONLY inside `spawn_blocking`
/// / the prewarm thread — never on the main thread — so no window invariant is
/// affected.
static SEARCH_CACHE: OnceLock<search::SearchCache> = OnceLock::new();

pub(crate) fn search_cache() -> &'static search::SearchCache {
    SEARCH_CACHE.get_or_init(search::SearchCache::new)
}

/// Live search across every registered vault. ASYNC on purpose — the one
/// deviation from this codebase's sync-command idiom: a sync command runs on
/// the main thread, and a multi-vault content scan there would freeze window
/// show/hide, drags and the upkeep tick. Running async keeps it off-main; it
/// touches no window APIs and takes no window-state or main-thread locks (the
/// scan's `SearchCache` mutex lives off-main inside `spawn_blocking`, never
/// held across a window call), so none of the main-thread window invariants
/// apply. The blocking walk runs under `spawn_blocking` so
/// it can't stall the async runtime's workers either. Returns `Err` on an
/// infrastructure failure (panicked scan task): an empty SUCCESS would blank
/// a working result list, while the frontend's error path keeps the previous
/// results up.
#[tauri::command]
pub async fn search_vaults(query: String) -> Result<search::SearchResponse, String> {
    let my_gen = SCAN_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let scanned = tauri::async_runtime::spawn_blocking(move || {
        // Same name-sorted list the panel shows; the `open` flag is
        // irrelevant here, so no process check is needed.
        let vaults = discovery::discover_vaults();
        let stale = move || SCAN_GENERATION.load(Ordering::Relaxed) != my_gen;
        search::search_vaults_with_cache(&vaults, &query, search_cache(), &stale)
    })
    .await;
    match scanned {
        Ok(response) => Ok(response),
        Err(e) => {
            log::warn!("search_vaults: scan task failed: {e}");
            Err("Search failed — see the logs for details.".to_string())
        }
    }
}

/// Open one search hit in Obsidian. `file` is the URI-form vault-relative
/// path the search itself returned (exactly-".md" note: extension dropped;
/// anything else: kept) — Obsidian resolves it inside the vault, so this
/// performs no filesystem access and never writes. Addressed by vault ID,
/// never name.
///
/// `keep_open` is the Ctrl-open multi-open flow: skipping the frontend's
/// `close_panel` is not enough on its own, because Obsidian grabs foreground
/// focus while handling the URI and the panel's focus-out check would hide
/// the panel moments later — so the command pins the panel open across that
/// grab (see `lib.rs::pin_panel_open`). Sync command → main thread, where
/// the pin's writer is expected to run.
#[tauri::command]
pub fn open_search_result(id: String, file: String, keep_open: bool) -> Result<(), String> {
    let vault = crate::commands::find_vault(&id)?;
    uri::launch(&uri::open_file_uri(&vault.id, &file))?;
    if keep_open {
        crate::pin_panel_open();
    }
    Ok(())
}
