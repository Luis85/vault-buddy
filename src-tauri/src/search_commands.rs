//! IPC surface for the panel's cross-vault Search view. Read-only: the scan
//! never writes, and opening a hit is delegated to Obsidian via the
//! launch-logged `obsidian://` path. See
//! docs/superpowers/specs/2026-07-09-vault-search-design.md.

use vault_buddy_core::{discovery, search, uri};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitDto {
    pub vault_id: String,
    pub vault_name: String,
    pub name: String,
    pub folder: String,
    pub file: String,
    pub snippet: Option<String>,
}

#[derive(Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponseDto {
    pub hits: Vec<SearchHitDto>,
    pub truncated: bool,
}

impl From<search::SearchResponse> for SearchResponseDto {
    fn from(r: search::SearchResponse) -> Self {
        Self {
            hits: r
                .hits
                .into_iter()
                .map(|h| SearchHitDto {
                    vault_id: h.vault_id,
                    vault_name: h.vault_name,
                    name: h.name,
                    folder: h.folder,
                    file: h.file,
                    snippet: h.snippet,
                })
                .collect(),
            truncated: r.truncated,
        }
    }
}

/// Live search across every registered vault. ASYNC on purpose — the one
/// deviation from this codebase's sync-command idiom: a sync command runs on
/// the main thread, and a multi-vault content scan there would freeze window
/// show/hide, drags and the upkeep tick. Running async keeps it off-main; it
/// touches no window APIs and takes no locks, so none of the main-thread
/// window invariants apply. The blocking filesystem walk is additionally
/// wrapped in `spawn_blocking` so it can't stall the async runtime's worker
/// threads either.
#[tauri::command]
pub async fn search_vaults(query: String) -> SearchResponseDto {
    let scanned = tauri::async_runtime::spawn_blocking(move || {
        // Same name-sorted list the panel shows; the `open` flag is
        // irrelevant here, so no process check is needed.
        let vaults = discovery::discover_vaults();
        search::search_vaults(&vaults, &query)
    })
    .await;
    match scanned {
        Ok(response) => response.into(),
        Err(e) => {
            // Degrade to empty but leave a trace (diagnostics invariant: no
            // swallowed error).
            log::warn!("search_vaults: scan task failed: {e}");
            SearchResponseDto::default()
        }
    }
}

/// Open one search hit in Obsidian. `file` is the URI-form vault-relative
/// path the search itself returned (note: extension dropped; attachment:
/// kept) — Obsidian resolves it inside the vault, so this performs no
/// filesystem access and never writes. Addressed by vault ID, never name.
#[tauri::command]
pub fn open_search_result(id: String, file: String) -> Result<(), String> {
    let vault = crate::commands::find_vault(&id)?;
    uri::launch(&uri::open_file_uri(&vault.id, &file))
}
