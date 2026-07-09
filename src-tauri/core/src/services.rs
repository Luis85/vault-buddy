//! Shared service functions: ONE implementation of each user-visible
//! capability, called by both the Tauri IPC commands and the MCP tools
//! (spec: docs/superpowers/specs/2026-07-09-local-mcp-server-design.md).
//! Pure over `ServicePaths` so everything here tests on Linux; the caller
//! injects the clock (`date`/`today`) and the URI launcher.

use std::path::PathBuf;

use crate::{capture_config, daily_note_target, discovery, process, uri};

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

/// Registry parse + open-flag scrub, `obsidian_running` injected so the scrub
/// is deterministic under test (the process table is environment state).
pub fn list_vaults_with(paths: &ServicePaths, obsidian_running: bool) -> Vec<discovery::Vault> {
    let Some(config) = &paths.obsidian_json else {
        return Vec::new();
    };
    let mut vaults = discovery::discover_vaults_from(config);
    // obsidian.json keeps `open: true` across a full Obsidian quit (that's how
    // Obsidian restores vaults on relaunch) — only trust the flags while an
    // Obsidian process actually exists.
    if !obsidian_running {
        for vault in &mut vaults {
            vault.open = false;
        }
    }
    vaults
}

pub fn list_vaults(paths: &ServicePaths) -> Vec<discovery::Vault> {
    list_vaults_with(paths, process::obsidian_running())
}

pub fn find_vault(paths: &ServicePaths, id: &str) -> Result<discovery::Vault, String> {
    // The scrub is irrelevant for lookup; pass `true` to skip the process scan.
    list_vaults_with(paths, true)
        .into_iter()
        .find(|v| v.id == id)
        .ok_or_else(|| format!("vault not found: {id}"))
}

pub fn open_vault(
    paths: &ServicePaths,
    id: &str,
    launch: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let vault = find_vault(paths, id)?;
    // Address the vault by ID, not name — names can collide across vaults.
    launch(&uri::open_vault_uri(&vault.id))
}

/// Exact tool-error text for the gated daily-note create branch. A constant
/// so the MCP tool, the IPC layer, and the tests can never drift apart.
pub const DAILY_NOTE_CREATE_GATED: &str =
    "today's daily note doesn't exist; enable vault writes in Vault Buddy settings to let clients create it";

/// Open today's daily note. The create branch (`obsidian://new` for a missing
/// note) mutates the vault, so it is write-gated: `allow_create: false`
/// refuses it BEFORE any URI is built or launched. The human UI path passes
/// `true` (unchanged behavior); the MCP tool passes the live allow-writes
/// grant.
pub fn open_daily_note(
    paths: &ServicePaths,
    id: &str,
    date: chrono::NaiveDate,
    allow_create: bool,
    launch: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let vault = find_vault(paths, id)?;
    let vault_path = std::path::Path::new(&vault.path);
    let (rel, exists) = daily_note_target(vault_path, date);
    if exists {
        launch(&uri::open_file_uri(&vault.id, &rel))
    } else if allow_create {
        launch(&uri::new_file_uri(&vault.id, &rel))
    } else {
        Err(DAILY_NOTE_CREATE_GATED.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn fixture(dir: &std::path::Path, vault_name: &str) -> (ServicePaths, std::path::PathBuf) {
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

    fn date() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 7, 9).unwrap()
    }

    #[test]
    fn list_vaults_scrubs_open_flags_when_obsidian_is_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _) = fixture(dir.path(), "MyVault");
        let vaults = list_vaults_with(&paths, false);
        assert_eq!(vaults.len(), 1);
        assert_eq!(vaults[0].name, "MyVault");
        assert!(!vaults[0].open);
        let vaults = list_vaults_with(&paths, true);
        assert!(vaults[0].open);
    }

    #[test]
    fn list_vaults_degrades_to_empty_without_a_registry() {
        assert!(list_vaults_with(&ServicePaths::default(), true).is_empty());
    }

    #[test]
    fn open_vault_launches_the_id_addressed_uri() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _) = fixture(dir.path(), "MyVault");
        let launched = RefCell::new(Vec::new());
        let launch = |u: &str| {
            launched.borrow_mut().push(u.to_string());
            Ok(())
        };
        open_vault(&paths, "deadbeef01234567", &launch).unwrap();
        assert_eq!(
            launched.borrow().as_slice(),
            ["obsidian://open?vault=deadbeef01234567"]
        );
        assert!(open_vault(&paths, "nope", &launch).is_err());
    }

    #[test]
    fn open_daily_note_opens_an_existing_note_regardless_of_the_grant() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        std::fs::write(vault.join("2026-07-09.md"), "x").unwrap();
        let launched = RefCell::new(Vec::new());
        let launch = |u: &str| {
            launched.borrow_mut().push(u.to_string());
            Ok(())
        };
        open_daily_note(&paths, "deadbeef01234567", date(), false, &launch).unwrap();
        assert!(launched.borrow()[0].starts_with("obsidian://open?"));
    }

    // Codex review catch pinned as a test: the create branch is a WRITE. With
    // the grant off, a missing daily note must be an error and launch NOTHING.
    #[test]
    fn open_daily_note_gates_the_create_branch_behind_allow_create() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _) = fixture(dir.path(), "MyVault");
        let launched = RefCell::new(Vec::new());
        let launch = |u: &str| {
            launched.borrow_mut().push(u.to_string());
            Ok(())
        };
        let err = open_daily_note(&paths, "deadbeef01234567", date(), false, &launch).unwrap_err();
        assert_eq!(err, DAILY_NOTE_CREATE_GATED);
        assert!(launched.borrow().is_empty(), "must not launch anything");
        open_daily_note(&paths, "deadbeef01234567", date(), true, &launch).unwrap();
        assert!(launched.borrow()[0].starts_with("obsidian://new?"));
    }
}
