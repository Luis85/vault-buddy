//! Shared service functions: ONE implementation of each user-visible
//! capability, called by both the Tauri IPC commands and the MCP tools
//! (spec: docs/superpowers/specs/2026-07-09-local-mcp-server-design.md).
//! Pure over `ServicePaths` so everything here tests on Linux; the caller
//! injects the clock (`date`/`today`) and the URI launcher.
//!
//! Split per domain so no single file carries the whole surface: `vault`
//! (registry / open / daily-note), `tasks` (list / add / edit / list-folder
//! lifecycle), `recordings` (read-only list). `ServicePaths` and the shared
//! `app_config` helper live here; every public item is re-exported so callers
//! keep using `services::X` unchanged.

mod recordings;
mod tasks;
mod vault;

pub use recordings::{list_recordings, RecordingDto};
pub use tasks::{
    add_task, count_open_tasks, create_task_list, delete_task_list, list_task_lists, list_tasks,
    move_task_to_list, rename_task_list, set_task_status, TaskDto,
};
pub use vault::{
    find_vault, list_vaults, list_vaults_with, open_daily_note, open_vault, DAILY_NOTE_CREATE_GATED,
};

use std::path::PathBuf;

use crate::{capture_config, discovery};

/// Where the real registry/config live. `real()` for the app; tests point
/// both at a temp dir. `None` degrades to empty/default (never an error) —
/// the same rule discovery follows for a missing obsidian.json.
#[derive(Clone, Debug, Default)]
pub struct ServicePaths {
    pub obsidian_json: Option<PathBuf>,
    pub config_json: Option<PathBuf>,
}

impl ServicePaths {
    pub fn real() -> Self {
        Self {
            obsidian_json: discovery::obsidian_config_path(),
            config_json: capture_config::config_path(),
        }
    }
}

/// Read the app-side config from `paths`, degrading to defaults when there is
/// none — the same "missing config is never an error" rule `ServicePaths`
/// documents for the registry.
pub(crate) fn app_config(paths: &ServicePaths) -> capture_config::AppConfig {
    match &paths.config_json {
        Some(p) => capture_config::load_config_from(p),
        None => capture_config::AppConfig::default(),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::ServicePaths;

    pub fn fixture(dir: &std::path::Path, vault_name: &str) -> (ServicePaths, std::path::PathBuf) {
        let vault = dir.join(vault_name);
        std::fs::create_dir_all(&vault).unwrap();
        let obsidian_json = dir.join("obsidian.json");
        let json = serde_json::json!({
            "vaults": { "deadbeef01234567": { "path": vault.to_string_lossy(), "open": true } }
        });
        std::fs::write(&obsidian_json, json.to_string()).unwrap();
        let config_json = dir.join("config.json");
        std::fs::write(&config_json, "{}").unwrap();
        (
            ServicePaths {
                obsidian_json: Some(obsidian_json),
                config_json: Some(config_json),
            },
            vault,
        )
    }
}
