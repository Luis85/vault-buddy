# Local MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Embed an opt-in local MCP server (streamable HTTP, `127.0.0.1:22082`) in the running buddy so MCP clients (Claude Code, Claude Desktop, Cursor) can list vaults, open vaults/daily notes, list tasks/recordings, and — behind an "Allow vault writes" grant — add tasks and set task status.

**Architecture:** A new Linux-testable workspace crate `src-tauri/mcp/` (`vault_buddy_mcp`) implements the MCP service on the official `rmcp` SDK over shared service functions extracted into `vault_buddy_core::services`; the Tauri shell owns only lifecycle (named `"mcp-server"` thread, start/stop on settings changes) and settings IPC; a new `McpSettings.vue` section provides the UI. Spec: `docs/superpowers/specs/2026-07-09-local-mcp-server-design.md` (read it before starting).

**Tech Stack:** Rust (rmcp 2.x, axum 0.8, tokio, tokio-util, schemars, subtle, getrandom, base64), Tauri v2, Vue 3 + Vitest.

## Global Constraints

- Every task: failing test FIRST, then implementation (TDD; the repo's vendored superpowers rule).
- Commits: Conventional Commits, imperative subject, body explains the why. Scopes used here: `feat(core)`, `feat(mcp)`, `feat(shell)`, `feat(ui)`, `docs(...)`. End every commit message with the two trailers used on this branch:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_012X1y6GNn8F1qoqSejrEnjV`.
- Branch: `claude/buddy-local-mcp-8th5l0`. Never push elsewhere.
- Default MCP port: **22082**. Endpoint path: **`/mcp`**. Bind **127.0.0.1 only**.
- Exact user-facing strings (tests assert them):
  - Write gate: `Vault writes are disabled in Vault Buddy settings.`
  - Daily-note gate: `today's daily note doesn't exist; enable vault writes in Vault Buddy settings to let clients create it`
  - Settings toggle label: `Allow vault writes`
- All DTOs serialize camelCase (`#[serde(rename_all = "camelCase")]`), matching existing IPC DTOs.
- Vaults are addressed by **id**, never name.
- Every spawned thread is named (`std::thread::Builder`). No swallowed errors — failures go through `log::warn!`/`log::error!`.
- `core` stays clock-free: dates/`today` are always parameters. The `mcp` crate and shell may use `chrono::Local`.
- Tauri CLI is always `npx tauri <cmd>`, never an npm script alias.
- Rust gates per task (run from `src-tauri/`): `cargo fmt` (then `cargo fmt --check`), and for the crate you touched `cargo clippy --all-targets -- -D warnings` + `cargo test`.
- The shell crate compiles on Linux only after `npm run setup:linux` (once per container) + `npx tauri build --no-bundle`. Windows CI (`windows-app`) remains the behavior gate.
- `rmcp` is at 2.2.0 (July 2026). If a signature in this plan drifts from the pinned version, check https://docs.rs/rmcp — the intent (factory-per-session service, `LocalSessionManager`, cancellation-token shutdown, `#[tool_router]`/`#[tool]`/`#[tool_handler]` macros) is stable; adapt mechanically, don't redesign.
- `schemars` must match the major version rmcp uses (`cargo tree -p rmcp | grep schemars` → adjust the dep if it shows 0.8 vs 1.x).

---

### Task 1: `McpConfig` section in `config.json` (parse + serialize round-trip)

**Files:**
- Modify: `src-tauri/core/src/capture_config.rs`
- Test: same file (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing `AppConfig`, `parse_config`, `serialize_config`, `write_config`, `update_vault_config_at`.
- Produces (later tasks rely on these exact items):
  - `pub const DEFAULT_MCP_PORT: u16 = 22082;`
  - `pub struct McpConfig { pub enabled: bool, pub port: u16, pub token: String, pub allow_writes: bool }` (`Debug, Clone, PartialEq`, `Default` = `{ false, 22082, "", false }`)
  - `AppConfig` gains `pub mcp: McpConfig`
  - `pub fn load_config_from(path: &Path) -> AppConfig`
  - `pub fn update_mcp_config_at(path: &Path, mcp: McpConfig) -> std::io::Result<()>`

- [ ] **Step 1: Write the failing tests** (append inside the existing `mod tests` in `capture_config.rs`)

```rust
    #[test]
    fn mcp_config_defaults_when_absent_or_malformed() {
        let cfg = parse_config(r#"{ "vaults": {} }"#);
        assert_eq!(cfg.mcp, McpConfig::default());
        assert!(!cfg.mcp.enabled);
        assert_eq!(cfg.mcp.port, DEFAULT_MCP_PORT);
        // One malformed field defaults only itself — the file is hand-editable.
        let cfg = parse_config(
            r#"{ "mcp": { "enabled": true, "port": "not-a-number", "token": 5, "allowWrites": true } }"#,
        );
        assert!(cfg.mcp.enabled);
        assert_eq!(cfg.mcp.port, DEFAULT_MCP_PORT);
        assert_eq!(cfg.mcp.token, "");
        assert!(cfg.mcp.allow_writes);
    }

    #[test]
    fn mcp_config_round_trips_through_serialize() {
        let mut cfg = AppConfig::default();
        cfg.mcp = McpConfig {
            enabled: true,
            port: 4321,
            token: "abc_-123".to_string(),
            allow_writes: true,
        };
        let reparsed = parse_config(&serialize_config(&cfg));
        assert_eq!(reparsed.mcp, cfg.mcp);
    }

    #[test]
    fn default_mcp_section_is_omitted_from_the_file() {
        // The hand-editable file stays minimal: users who never enable MCP
        // never see the section.
        let json = serialize_config(&AppConfig::default());
        assert!(!json.contains("mcp"), "got: {json}");
    }

    // Regression: serialize_config used to emit ONLY the vaults section, so a
    // capture/tasks settings save would silently DELETE an mcp section.
    #[test]
    fn saving_a_vault_config_preserves_the_mcp_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{ "mcp": { "enabled": true, "port": 22082, "token": "tok", "allowWrites": false }, "vaults": {} }"#,
        )
        .unwrap();
        update_vault_config_at(&path, "vault1", VaultCaptureConfig::default()).unwrap();
        let cfg = load_config_from(&path);
        assert!(cfg.mcp.enabled);
        assert_eq!(cfg.mcp.token, "tok");
        assert!(cfg.vaults.contains_key("vault1"));
    }

    #[test]
    fn update_mcp_config_at_preserves_vaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        update_vault_config_at(&path, "vault1", VaultCaptureConfig::default()).unwrap();
        let mcp = McpConfig {
            enabled: true,
            port: DEFAULT_MCP_PORT,
            token: "tok".to_string(),
            allow_writes: false,
        };
        update_mcp_config_at(&path, mcp.clone()).unwrap();
        let cfg = load_config_from(&path);
        assert_eq!(cfg.mcp, mcp);
        assert!(cfg.vaults.contains_key("vault1"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test capture_config`
Expected: compile FAILURE — `McpConfig`, `DEFAULT_MCP_PORT`, `load_config_from`, `update_mcp_config_at` not found.

- [ ] **Step 3: Implement**

In `capture_config.rs`:

```rust
/// Default port for the embedded MCP server: 0x5642 = ASCII "VB".
pub const DEFAULT_MCP_PORT: u16 = 22082;

/// App-global settings for the embedded MCP server (spec:
/// docs/superpowers/specs/2026-07-09-local-mcp-server-design.md). Stored as
/// a top-level `mcp` section beside `vaults`; parsing is per-field defensive
/// for the same reason the vault entries are.
#[derive(Debug, Clone, PartialEq)]
pub struct McpConfig {
    pub enabled: bool,
    pub port: u16,
    /// Bearer token clients must send. Empty until first enable; the shell
    /// self-heals an enabled-but-tokenless config by generating one.
    pub token: String,
    /// The "Allow vault writes" grant: add_task, set_task_status, and the
    /// daily-note create branch.
    pub allow_writes: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_MCP_PORT,
            token: String::new(),
            allow_writes: false,
        }
    }
}
```

Add `pub mcp: McpConfig` to `AppConfig` (keep `#[derive(Debug, Clone, Default)]` working — `McpConfig` implements `Default`).

In `parse_config`, after the vaults loop:

```rust
    let mcp = value.get("mcp").map(mcp_entry).unwrap_or_default();
    AppConfig { vaults, mcp }
```

```rust
fn mcp_entry(entry: &serde_json::Value) -> McpConfig {
    let defaults = McpConfig::default();
    McpConfig {
        enabled: entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.enabled),
        port: entry
            .get("port")
            .and_then(|v| v.as_u64())
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(defaults.port),
        token: entry
            .get("token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_default(),
        allow_writes: entry
            .get("allowWrites")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.allow_writes),
    }
}
```

In `serialize_config`, replace the single-key root with a conditional two-key root (this is the regression fix — the serializer must round-trip every section it parses):

```rust
    let mut root = Map::new();
    if cfg.mcp != McpConfig::default() {
        let mut mcp = Map::new();
        mcp.insert("enabled".to_string(), json!(cfg.mcp.enabled));
        mcp.insert("port".to_string(), json!(cfg.mcp.port));
        mcp.insert("token".to_string(), json!(cfg.mcp.token));
        mcp.insert("allowWrites".to_string(), json!(cfg.mcp.allow_writes));
        root.insert("mcp".to_string(), Value::Object(mcp));
    }
    root.insert("vaults".to_string(), Value::Object(vaults));
    let mut out =
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_else(|_| "{}".to_string());
    out.push('\n');
    out
```

Add the two helpers next to `load_config` / `update_vault_config_at`:

```rust
pub fn load_config_from(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(json) => parse_config(&json),
        Err(_) => AppConfig::default(),
    }
}

/// Read-modify-write for the app-global mcp section, mirroring
/// update_vault_config_at (same no-own-lock rule: IPC callers serialize
/// behind ConfigWriteLock).
pub fn update_mcp_config_at(path: &Path, mcp: McpConfig) -> std::io::Result<()> {
    let mut cfg = load_config_from(path);
    cfg.mcp = mcp;
    write_config(path, &cfg)
}
```

Simplify `load_config` to reuse `load_config_from`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt`
Expected: all tests PASS (existing serialize tests must still pass — they assert vault fields, not the root shape), clippy clean.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/capture_config.rs
git commit -m "feat(core): mcp config section with serializer round-trip"
```
(Body: explain the clobber regression the round-trip test pins. Add the two trailers.)

---

### Task 2: `core::services` — vaults + open tools (with the gated daily-note create)

**Files:**
- Create: `src-tauri/core/src/services.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod services;` + `daily_note_target`)
- Modify: `src-tauri/src/commands.rs:486-521` (rewire `find_vault`, `list_vaults`, `open_vault`, `open_daily_note`)
- Test: `src-tauri/core/src/services.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: `discovery::{discover_vaults_from, obsidian_config_path, Vault}`, `process::obsidian_running()`, `uri::{open_vault_uri, open_file_uri, new_file_uri, launch}`, `daily_notes`.
- Produces (exact signatures later tasks call):
  - `pub struct ServicePaths { pub obsidian_json: Option<PathBuf>, pub config_json: Option<PathBuf> }` (`Clone, Debug, Default`) with `pub fn real() -> Self`
  - `pub fn list_vaults_with(paths: &ServicePaths, obsidian_running: bool) -> Vec<discovery::Vault>`
  - `pub fn list_vaults(paths: &ServicePaths) -> Vec<discovery::Vault>`
  - `pub fn find_vault(paths: &ServicePaths, id: &str) -> Result<discovery::Vault, String>`
  - `pub fn open_vault(paths: &ServicePaths, id: &str, launch: &dyn Fn(&str) -> Result<(), String>) -> Result<(), String>`
  - `pub fn open_daily_note(paths: &ServicePaths, id: &str, date: chrono::NaiveDate, allow_create: bool, launch: &dyn Fn(&str) -> Result<(), String>) -> Result<(), String>`
  - In `core/src/lib.rs`: `pub fn daily_note_target(vault_path: &Path, date: NaiveDate) -> (String, bool)` (rel path without `.md`, exists)
  - Exact gate message constant: `pub const DAILY_NOTE_CREATE_GATED: &str = "today's daily note doesn't exist; enable vault writes in Vault Buddy settings to let clients create it";`

- [ ] **Step 1: Write the failing tests** (bottom of the new `services.rs`)

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test services`
Expected: compile FAILURE — module doesn't exist yet.

- [ ] **Step 3: Implement**

In `core/src/lib.rs`, add `pub mod services;` to the module list, then refactor the daily-note helper (keep `daily_note_uri`'s behavior identical — its three existing tests must still pass):

```rust
/// The vault-relative daily-note path (no `.md`) for `date`, and whether the
/// note file already exists. Split from `daily_note_uri` so callers that must
/// gate creation (the MCP `open_daily_note` tool) can decide BEFORE a URI is
/// built.
pub fn daily_note_target(vault_path: &Path, date: NaiveDate) -> (String, bool) {
    let settings = daily_notes::load_settings(vault_path);
    let rel = daily_notes::daily_note_rel_path(&settings, date);
    let exists = vault_path.join(format!("{rel}.md")).exists();
    (rel, exists)
}

pub fn daily_note_uri(vault_id: &str, vault_path: &Path, date: NaiveDate) -> String {
    let (rel, exists) = daily_note_target(vault_path, date);
    if exists {
        uri::open_file_uri(vault_id, &rel)
    } else {
        uri::new_file_uri(vault_id, &rel)
    }
}
```

New `core/src/services.rs`:

```rust
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
```

Note: `discovery::Vault.path` is a `String` and `discover_vaults_from(&Path)` already exists (`core/src/discovery.rs:60`) — no discovery changes needed. Add `chrono` to core's deps only if it isn't already there (it is — `daily_note_uri` uses `NaiveDate`).

Rewire `src-tauri/src/commands.rs` (behavior identical; the commands become thin):

```rust
use vault_buddy_core::{services, uri};

#[tauri::command]
pub fn list_vaults() -> Vec<vault_buddy_core::discovery::Vault> {
    services::list_vaults(&services::ServicePaths::real())
}

#[tauri::command]
pub fn open_vault(id: String) -> Result<(), String> {
    services::open_vault(&services::ServicePaths::real(), &id, &|u| uri::launch(u))
}

#[tauri::command]
pub fn open_daily_note(id: String) -> Result<(), String> {
    let today = Local::now().date_naive();
    // allow_create: true — the human UI keeps its open-or-create behavior.
    services::open_daily_note(
        &services::ServicePaths::real(),
        &id,
        today,
        true,
        &|u| uri::launch(u),
    )
}
```

Delete the now-unused local `find_vault` and the old bodies (keep the doc comments' intent by moving them onto the service functions). Remove imports that became unused (`daily_note_uri`, `discovery`, `process`, `Path`) — `commands.rs` keeps `chrono::Local`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt`
Expected: PASS, including the three pre-existing `daily_note_uri` tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/lib.rs src-tauri/core/src/services.rs src-tauri/src/commands.rs
git commit -m "feat(core): services module for vaults + gated daily-note open"
```

---

### Task 3: `core::services` — tasks + recordings (move the command bodies)

**Files:**
- Modify: `src-tauri/core/src/services.rs` (add DTOs + task/recording functions)
- Modify: `src-tauri/src/task_commands.rs` (become thin wrappers)
- Modify: `src-tauri/src/capture_commands.rs` (`list_recordings` command body + `RecordingDto` moves out; keep a `pub use` if other shell code references it)
- Test: `src-tauri/core/src/services.rs`

**Interfaces:**
- Consumes: `ServicePaths`, `find_vault` (Task 2); `capture_config::{load_config_from, vault_config}`; `capture_paths::{safe_recording_root, assert_path_inside_vault, assert_root_inside_vault}`; `tasks::{list_tasks, create_task, set_task_status}`; `recordings::list_recordings`.
- Produces:
  - `pub struct TaskDto { pub path: String, pub title: String, pub status: String, pub created: String, pub done: bool }` (`Clone, serde::Serialize`, camelCase)
  - `pub struct RecordingDto { pub mp3: String, pub title: String, pub recorded_at: String, pub duration: Option<String>, pub recording_type: Option<String>, pub transcript_status: String }` (`Clone, serde::Serialize`, camelCase, `recording_type` renamed `"type"`)
  - `pub fn list_tasks(paths: &ServicePaths, id: &str) -> Vec<TaskDto>`
  - `pub fn add_task(paths: &ServicePaths, id: &str, title: &str, today: &str) -> Result<TaskDto, String>`
  - `pub fn set_task_status(paths: &ServicePaths, id: &str, task_path: &str, status: &str) -> Result<String, String>` — **returns the task's display title** (file stem of `task_path`), for the announce hook
  - `pub fn count_open_tasks(paths: &ServicePaths, id: &str) -> usize`
  - `pub fn list_recordings(paths: &ServicePaths, id: &str) -> Vec<RecordingDto>`

- [ ] **Step 1: Write the failing tests** (append to `services.rs` tests; reuse the `fixture` helper)

```rust
    #[test]
    fn add_list_and_toggle_tasks_through_the_service() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        let created = add_task(&paths, "deadbeef01234567", "Buy milk", "2026-07-09").unwrap();
        assert_eq!(created.title, "Buy milk");
        assert!(!created.done);
        assert!(vault.join("Tasks").is_dir());
        let listed = list_tasks(&paths, "deadbeef01234567");
        assert_eq!(listed.len(), 1);
        assert_eq!(count_open_tasks(&paths, "deadbeef01234567"), 1);
        let title = set_task_status(&paths, "deadbeef01234567", &created.path, "done").unwrap();
        assert_eq!(title, "Buy milk");
        assert_eq!(count_open_tasks(&paths, "deadbeef01234567"), 0);
    }

    #[test]
    fn task_service_errors_mirror_the_command_layer() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _) = fixture(dir.path(), "MyVault");
        assert!(add_task(&paths, "deadbeef01234567", "   ", "2026-07-09").is_err());
        assert!(add_task(&paths, "unknown", "x", "2026-07-09").is_err());
        assert!(
            set_task_status(&paths, "deadbeef01234567", "whatever.md", "bogus")
                .unwrap_err()
                .contains("Unknown task status")
        );
        assert!(list_tasks(&paths, "unknown").is_empty());
    }

    #[test]
    fn add_task_refuses_a_missing_vault_dir() {
        // A stale registry must not resurrect a deleted vault (same guard as
        // the IPC command).
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        std::fs::remove_dir_all(&vault).unwrap();
        assert!(add_task(&paths, "deadbeef01234567", "x", "2026-07-09").is_err());
    }

    #[test]
    fn list_recordings_degrades_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _) = fixture(dir.path(), "MyVault");
        assert!(list_recordings(&paths, "deadbeef01234567").is_empty());
        assert!(list_recordings(&paths, "unknown").is_empty());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test services`
Expected: compile FAILURE — the functions/DTOs don't exist.

- [ ] **Step 3: Implement**

Move the bodies from `src-tauri/src/task_commands.rs` into `services.rs`, replacing `discovery::discover_vaults()` with `find_vault(paths, id)` and `capture_config::load_config()` with a config read from `paths` (this internal helper):

```rust
fn app_config(paths: &ServicePaths) -> capture_config::AppConfig {
    match &paths.config_json {
        Some(p) => capture_config::load_config_from(p),
        None => capture_config::AppConfig::default(),
    }
}
```

The moved functions keep the existing guard sequences VERBATIM (they are the sanctioned-write discipline — copy the comments along):

- `tasks_root_for(paths, id) -> Result<(PathBuf, PathBuf), String>` — `find_vault` + `vault_config(&app_config(paths), id)` + `safe_recording_root(vault_path, cfg.tasks_root())`.
- `list_tasks` / `count_open_tasks` — the `root.exists()` + `assert_root_inside_vault` skip-with-warning, then `tasks::list_tasks(&root)` mapped to `TaskDto` (move `TaskDto::from_item`).
- `add_task` — trim/empty check (`"A task needs a title."`), `!vault_path.is_dir()` guard, `assert_path_inside_vault`, `create_dir_all`, `tasks::create_task(&root, title, today)`; **`today` is a parameter** (`&str`, `YYYY-MM-DD`) — no clock in core.
- `set_task_status` — the `matches!(status, "new" | "done" | "archived")` check (error text `Unknown task status: {status}`), root containment asserts, `tasks::set_task_status(&root, Path::new(task_path), status)`, then on success return the display title:

```rust
    // Display title for the announce hook: the file stem. create_task names
    // files after the title, so this matches for buddy-created tasks and is
    // an honest fallback for hand-authored ones — without re-parsing
    // frontmatter on the write path.
    let title = Path::new(task_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| task_path.to_string());
    Ok(title)
```

- `list_recordings` — move the body of `capture_commands::list_recordings` (the per-folder `safe_recording_root` warn-and-skip loop over `cfg.recording_roots()`, then `recordings::list_recordings(&roots)` mapped to `RecordingDto`); move the `RecordingDto` struct (with its `#[serde(rename = "type")]`) and `TaskDto` into `services.rs`.

Rewire the shell:

`src-tauri/src/task_commands.rs` shrinks to wrappers (the `get_tasks_config`/`set_tasks_config` commands stay as they are — settings, not services):

```rust
use vault_buddy_core::services::{self, ServicePaths, TaskDto};

#[tauri::command]
pub fn list_tasks(id: String) -> Vec<TaskDto> {
    services::list_tasks(&ServicePaths::real(), &id)
}

#[tauri::command]
pub fn add_task(id: String, title: String) -> Result<TaskDto, String> {
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    services::add_task(&ServicePaths::real(), &id, &title, &today)
}

#[tauri::command]
pub fn set_task_status(id: String, path: String, status: String) -> Result<(), String> {
    services::set_task_status(&ServicePaths::real(), &id, &path, &status).map(|_title| ())
}

#[tauri::command]
pub fn count_open_tasks(id: String) -> usize {
    services::count_open_tasks(&ServicePaths::real(), &id)
}
```

`src-tauri/src/capture_commands.rs`: `list_recordings` becomes `services::list_recordings(&ServicePaths::real(), &id)`; delete the local `RecordingDto` and import it from services (`use vault_buddy_core::services::RecordingDto;`) — `open_recording`/`open_transcript` and the tests keep compiling because the DTO shape is identical.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt && cd .. && npm test`
Expected: core PASS; `npm test` PASS (frontend untouched — this proves the DTO shapes didn't drift).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/services.rs src-tauri/src/task_commands.rs src-tauri/src/capture_commands.rs
git commit -m "feat(core): move task and recording command bodies into services"
```

---

### Task 4: `vault_buddy_mcp` crate skeleton — token + HTTP guard

**Files:**
- Create: `src-tauri/mcp/Cargo.toml`, `src-tauri/mcp/src/lib.rs`, `src-tauri/mcp/src/token.rs`, `src-tauri/mcp/src/http.rs`
- Modify: `src-tauri/Cargo.toml` (workspace `members` += `"mcp"`)
- Test: inline `#[cfg(test)]` in `token.rs` and `http.rs`

**Interfaces:**
- Produces:
  - `pub fn token::generate_token() -> String` (43-char base64url, no padding)
  - `pub fn http::origin_ok(origin: Option<&str>) -> bool`
  - `pub fn http::auth_ok(header: Option<&str>, token: &str) -> bool`
  - `pub fn http::length_ok(content_length: Option<&str>) -> bool` (limit 1 MiB)
  - `pub const http::MAX_BODY_BYTES: u64 = 1_048_576;`

- [ ] **Step 1: Create the crate and write the failing tests**

`src-tauri/Cargo.toml`: `members = ["core", "capture", "transcribe", "mcp"]`.

`src-tauri/mcp/Cargo.toml`:

```toml
[package]
name = "vault_buddy_mcp"
version = "0.1.0"
edition = "2021"

[dependencies]
vault_buddy_core = { path = "../core" }
rmcp = { version = "2.2", features = [
    "server",
    "macros",
    "transport-streamable-http-server",
] }
axum = "0.8"
tokio = { version = "1", features = ["rt", "net", "macros", "time"] }
tokio-util = "0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
# Must match rmcp's schemars major — verify with `cargo tree -p rmcp | grep schemars`.
schemars = "1"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
log = "0.4"
getrandom = "0.3"
base64 = "0.22"
subtle = "2"

[dev-dependencies]
rmcp = { version = "2.2", features = [
    "client",
    "transport-streamable-http-client",
    "reqwest",
] }
reqwest = { version = "0.12", default-features = false, features = ["json"] }
tempfile = "3"
tokio = { version = "1", features = ["rt-multi-thread"] }
```

`src-tauri/mcp/src/lib.rs`:

```rust
//! Embedded MCP server for Vault Buddy (streamable HTTP on 127.0.0.1).
//! Spec: docs/superpowers/specs/2026-07-09-local-mcp-server-design.md.
//! Tauri-free by design: the shell wires lifecycle + events, this crate owns
//! protocol, tools, and the HTTP guard — all testable on Linux.

pub mod http;
pub mod token;
```

Tests in `token.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_43_char_base64url_and_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 43); // 32 bytes, base64url, no padding
        assert_ne!(a, b);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
```

Tests in `http.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_absent_or_localhost_passes_everything_else_fails() {
        assert!(origin_ok(None)); // CLI clients send no Origin
        for ok in [
            "http://localhost",
            "http://localhost:5173",
            "https://localhost:1234",
            "http://127.0.0.1:8080",
            "http://[::1]:9000",
        ] {
            assert!(origin_ok(Some(ok)), "{ok} should pass");
        }
        for bad in ["http://evil.test", "https://localhost.evil.test", "null", "file://x"] {
            assert!(!origin_ok(Some(bad)), "{bad} should fail");
        }
    }

    #[test]
    fn auth_requires_the_exact_bearer_token() {
        assert!(auth_ok(Some("Bearer sekret"), "sekret"));
        assert!(!auth_ok(Some("Bearer wrong"), "sekret"));
        assert!(!auth_ok(Some("sekret"), "sekret")); // scheme required
        assert!(!auth_ok(None, "sekret"));
        assert!(!auth_ok(Some("Bearer "), "sekret"));
        assert!(!auth_ok(Some("Bearer sekret"), "")); // empty token never matches
    }

    #[test]
    fn length_cap_is_one_mebibyte() {
        assert!(length_ok(None)); // no header → let the request through
        assert!(length_ok(Some("1048576")));
        assert!(!length_ok(Some("1048577")));
        assert!(!length_ok(Some("not-a-number"))); // unparseable → reject
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/mcp && cargo test`
Expected: compile FAILURE — functions not defined.

- [ ] **Step 3: Implement**

`token.rs`:

```rust
use base64::Engine;

/// 32 random bytes as unpadded base64url — the bearer token MCP clients must
/// present. Generated shell-side on first enable and stored in config.json
/// (user-profile ACLs; same trust level as the rest of that file).
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    // getrandom only fails on broken OS RNG — a panic here is correct
    // (an unguessable token is the whole security model).
    getrandom::fill(&mut bytes).expect("OS RNG unavailable");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}
```

(`getrandom 0.3` renamed `getrandom::getrandom` to `getrandom::fill`; if the resolver picks 0.2, use `getrandom::getrandom(&mut bytes)`.)

`http.rs` — the three pure guards (the axum wiring joins them in Task 6):

```rust
use subtle::ConstantTimeEq;

/// Hard cap on request bodies (1 MiB): tool calls are small JSON; anything
/// bigger is a misbehaving client. Enforced via Content-Length — the endpoint
/// is localhost + bearer-token'd, so a chunked-encoding attacker is outside
/// the threat model; the cap exists to keep an honest client's mistake from
/// ballooning memory.
pub const MAX_BODY_BYTES: u64 = 1_048_576;

/// MCP-spec DNS-rebinding defense: no Origin (CLI clients) or a localhost
/// origin passes; any web origin is rejected before auth work.
pub fn origin_ok(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let rest = if let Some(r) = origin.strip_prefix("http://") {
        r
    } else if let Some(r) = origin.strip_prefix("https://") {
        r
    } else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    // Strip a port; [::1] needs the bracket form kept intact.
    let host = if let Some(h) = host.strip_prefix('[') {
        h.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Constant-time bearer check. An empty configured token never matches —
/// "not yet generated" must not mean "open".
pub fn auth_ok(header: Option<&str>, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let Some(presented) = header.and_then(|h| h.strip_prefix("Bearer ")) else {
        return false;
    };
    presented.as_bytes().ct_eq(token.as_bytes()).into()
}

pub fn length_ok(content_length: Option<&str>) -> bool {
    match content_length {
        None => true,
        Some(v) => v.parse::<u64>().map(|n| n <= MAX_BODY_BYTES).unwrap_or(false),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/mcp && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt`
Expected: PASS. (First build downloads rmcp/axum — a few minutes.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/mcp
git commit -m "feat(mcp): crate skeleton with token generation and http guards"
```

---

### Task 5: The MCP service — seven tools over `core::services`

**Files:**
- Create: `src-tauri/mcp/src/service.rs`
- Modify: `src-tauri/mcp/src/lib.rs` (`pub mod service;` + re-exports)
- Test: inline `#[cfg(test)]` in `service.rs`

**Interfaces:**
- Consumes: `core::services` (Tasks 2-3), `token`/`http` (Task 4).
- Produces (Task 6 and the shell consume these exactly):
  - `pub struct Deps { pub paths: ServicePaths, pub app_version: String, pub allow_writes: Arc<AtomicBool>, pub launch: Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>, pub on_write: Arc<dyn Fn(WriteEvent) + Send + Sync> }` (`Clone`)
  - `pub struct WriteEvent { pub kind: WriteKind, pub title: String, pub vault_name: String }` (`Clone, Debug, serde::Serialize`, camelCase)
  - `pub enum WriteKind { AddTask, SetTaskStatus, CreateDailyNote }` (`Clone, Copy, Debug, serde::Serialize`, camelCase)
  - `pub struct VaultBuddyMcp` with `pub fn new(deps: Deps) -> Self` — write tools included in the router only when `allow_writes` is true at construction (per-session), and re-checked live on every call
  - `pub const WRITES_DISABLED: &str = "Vault writes are disabled in Vault Buddy settings.";`

- [ ] **Step 1: Write the failing tests**

The `#[tool]` methods are plain methods — test them directly, no HTTP needed:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn fixture_deps(dir: &std::path::Path, allow_writes: bool) -> (Deps, Arc<Mutex<Vec<String>>>, Arc<Mutex<Vec<WriteEvent>>>) {
        let vault = dir.join("MyVault");
        std::fs::create_dir_all(&vault).unwrap();
        let obsidian_json = dir.join("obsidian.json");
        std::fs::write(
            &obsidian_json,
            serde_json::json!({
                "vaults": { "deadbeef01234567": { "path": vault.to_string_lossy() } }
            })
            .to_string(),
        )
        .unwrap();
        let config_json = dir.join("config.json");
        std::fs::write(&config_json, "{}").unwrap();
        let launched = Arc::new(Mutex::new(Vec::new()));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let l2 = launched.clone();
        let w2 = writes.clone();
        let deps = Deps {
            paths: vault_buddy_core::services::ServicePaths {
                obsidian_json: Some(obsidian_json),
                config_json: Some(config_json),
            },
            app_version: "0.0.0-test".to_string(),
            allow_writes: Arc::new(AtomicBool::new(allow_writes)),
            launch: Arc::new(move |u: &str| {
                l2.lock().unwrap().push(u.to_string());
                Ok(())
            }),
            on_write: Arc::new(move |ev| w2.lock().unwrap().push(ev)),
        };
        (deps, launched, writes)
    }

    fn text_of(result: &rmcp::model::CallToolResult) -> String {
        serde_json::to_string(&result.content).unwrap_or_default()
    }

    #[test]
    fn write_tools_are_listed_only_with_the_grant() {
        let dir = tempfile::tempdir().unwrap();
        let with = VaultBuddyMcp::new(fixture_deps(dir.path(), true).0);
        let without = VaultBuddyMcp::new(fixture_deps(dir.path(), false).0);
        let names = |s: &VaultBuddyMcp| {
            let mut n: Vec<String> = s.tool_router.list_all().into_iter().map(|t| t.name.to_string()).collect();
            n.sort();
            n
        };
        assert_eq!(
            names(&with),
            ["add_task", "list_recordings", "list_tasks", "list_vaults", "open_daily_note", "open_vault", "set_task_status"]
        );
        assert_eq!(
            names(&without),
            ["list_recordings", "list_tasks", "list_vaults", "open_daily_note", "open_vault"]
        );
    }

    #[tokio::test]
    async fn add_task_writes_fires_the_hook_and_respects_the_live_gate() {
        let dir = tempfile::tempdir().unwrap();
        let (deps, _launched, writes) = fixture_deps(dir.path(), true);
        let svc = VaultBuddyMcp::new(deps.clone());
        let result = svc
            .add_task(rmcp::handler::server::wrapper::Parameters(AddTaskParams {
                vault_id: "deadbeef01234567".into(),
                title: "Buy milk".into(),
            }))
            .await
            .unwrap();
        assert_ne!(result.is_error, Some(true), "{}", text_of(&result));
        assert!(dir.path().join("MyVault/Tasks").is_dir());
        assert_eq!(writes.lock().unwrap().len(), 1);
        assert_eq!(writes.lock().unwrap()[0].vault_name, "MyVault");

        // Grant revoked AFTER session construction: the call-time gate is
        // authoritative even though the tool is still in this session's list.
        deps.allow_writes.store(false, Ordering::Relaxed);
        let result = svc
            .add_task(rmcp::handler::server::wrapper::Parameters(AddTaskParams {
                vault_id: "deadbeef01234567".into(),
                title: "Nope".into(),
            }))
            .await
            .unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(text_of(&result).contains(WRITES_DISABLED));
    }

    #[tokio::test]
    async fn open_daily_note_gates_creation_and_reports_it_as_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let (deps, launched, writes) = fixture_deps(dir.path(), false);
        let svc = VaultBuddyMcp::new(deps.clone());
        let params = || rmcp::handler::server::wrapper::Parameters(VaultIdParams {
            vault_id: "deadbeef01234567".into(),
        });
        let result = svc.open_daily_note(params()).await.unwrap();
        assert_eq!(result.is_error, Some(true));
        assert!(text_of(&result).contains("enable vault writes"));
        assert!(launched.lock().unwrap().is_empty());

        deps.allow_writes.store(true, Ordering::Relaxed);
        let result = svc.open_daily_note(params()).await.unwrap();
        assert_ne!(result.is_error, Some(true), "{}", text_of(&result));
        assert!(launched.lock().unwrap()[0].starts_with("obsidian://new?"));
        assert_eq!(writes.lock().unwrap().len(), 1); // create counted as a write
    }

    #[tokio::test]
    async fn list_vaults_returns_the_registry() {
        let dir = tempfile::tempdir().unwrap();
        let (deps, _, _) = fixture_deps(dir.path(), false);
        let svc = VaultBuddyMcp::new(deps);
        let result = svc.list_vaults().await.unwrap();
        let text = text_of(&result);
        assert!(text.contains("deadbeef01234567") && text.contains("MyVault"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/mcp && cargo test service`
Expected: compile FAILURE.

- [ ] **Step 3: Implement `service.rs`**

```rust
//! The MCP service: seven tools over core::services. Vaults are addressed by
//! ID, never name. Write tools ride two gates: session-construction router
//! filtering (advisory — clients cache tools/list) and a live atomic check on
//! every call (authoritative).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use vault_buddy_core::services::{self, ServicePaths};

pub const WRITES_DISABLED: &str = "Vault writes are disabled in Vault Buddy settings.";

const INSTRUCTIONS: &str = "Vault Buddy exposes the user's Obsidian vaults. Always call \
list_vaults first and address vaults by their `id` (never by name). Write tools (add_task, \
set_task_status, and creating a missing daily note) work only while the user has enabled \
'Allow vault writes' in Vault Buddy settings.";

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WriteKind {
    AddTask,
    SetTaskStatus,
    CreateDailyNote,
}

/// Emitted after every successful vault write so the shell can have the buddy
/// announce what an AI client just did.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteEvent {
    pub kind: WriteKind,
    pub title: String,
    pub vault_name: String,
}

/// Everything the tools need from the outside world, injectable for tests:
/// real paths + uri::launch + a Tauri event emitter in the app, temp dirs +
/// recording closures in tests.
#[derive(Clone)]
pub struct Deps {
    pub paths: ServicePaths,
    pub app_version: String,
    pub allow_writes: Arc<AtomicBool>,
    pub launch: Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>,
    pub on_write: Arc<dyn Fn(WriteEvent) + Send + Sync>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VaultIdParams {
    /// The vault's id from list_vaults.
    pub vault_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddTaskParams {
    /// The vault's id from list_vaults.
    pub vault_id: String,
    /// The task's title.
    pub title: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetTaskStatusParams {
    /// The vault's id from list_vaults.
    pub vault_id: String,
    /// The task file's path, from list_tasks.
    pub path: String,
    /// One of: new, done, archived.
    pub status: String,
}

#[derive(Clone)]
pub struct VaultBuddyMcp {
    deps: Deps,
    pub(crate) tool_router: ToolRouter<Self>,
}

impl VaultBuddyMcp {
    /// Router chosen at construction — the HTTP layer constructs one service
    /// per SESSION, so a toggled grant shows up in tools/list on the next
    /// connect; the live atomic check in each write tool covers sessions that
    /// initialized before the flip.
    pub fn new(deps: Deps) -> Self {
        let mut tool_router = Self::read_tools_router();
        if deps.allow_writes.load(Ordering::Relaxed) {
            tool_router = tool_router + Self::write_tools_router();
        }
        Self { deps, tool_router }
    }

    fn writes_allowed(&self) -> bool {
        self.deps.allow_writes.load(Ordering::Relaxed)
    }

    /// Uniform result shape: pretty JSON as text content — every target
    /// client renders text blocks. Domain failures are tool errors
    /// (isError: true), never protocol errors.
    fn ok_json<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let json = serde_json::to_string_pretty(value)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    fn tool_error(message: impl Into<String>) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::error(vec![ContentBlock::text(
            message.into(),
        )]))
    }

    /// Audit line for every tool call — names + outcome, never argument
    /// values (title lengths only), per the spec's redaction rule.
    fn audit(tool: &str, vault_id: &str, outcome: &Result<(), String>) {
        match outcome {
            Ok(()) => log::info!("mcp: tool={tool} vault={vault_id} ok"),
            Err(e) => log::warn!("mcp: tool={tool} vault={vault_id} failed: {e}"),
        }
    }

    fn today() -> chrono::NaiveDate {
        chrono::Local::now().date_naive()
    }
}

#[tool_router(router = read_tools_router, vis = "pub")]
impl VaultBuddyMcp {
    #[tool(
        description = "List the user's Obsidian vaults: id, name, path, and whether the vault is open now. Call this first — every other tool takes a vault id from here.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_vaults(&self) -> Result<CallToolResult, McpError> {
        let vaults = services::list_vaults(&self.deps.paths);
        Self::audit("list_vaults", "-", &Ok(()));
        Self::ok_json(&vaults)
    }

    #[tool(
        description = "List a vault's tasks (todo documents): path, title, status, created date, done.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_tasks(
        &self,
        Parameters(p): Parameters<VaultIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let tasks = services::list_tasks(&self.deps.paths, &p.vault_id);
        Self::audit("list_tasks", &p.vault_id, &Ok(()));
        Self::ok_json(&tasks)
    }

    #[tool(
        description = "List a vault's recordings (meetings and voice notes): title, recorded time, duration, type, transcript status. Metadata only.",
        annotations(read_only_hint = true)
    )]
    pub async fn list_recordings(
        &self,
        Parameters(p): Parameters<VaultIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let recordings = services::list_recordings(&self.deps.paths, &p.vault_id);
        Self::audit("list_recordings", &p.vault_id, &Ok(()));
        Self::ok_json(&recordings)
    }

    #[tool(description = "Open a vault in Obsidian (focuses/launches the Obsidian app).")]
    pub async fn open_vault(
        &self,
        Parameters(p): Parameters<VaultIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let outcome = services::open_vault(&self.deps.paths, &p.vault_id, &*self.deps.launch);
        Self::audit("open_vault", &p.vault_id, &outcome);
        match outcome {
            Ok(()) => Self::ok_json(&serde_json::json!({ "opened": true })),
            Err(e) => Self::tool_error(e),
        }
    }

    #[tool(
        description = "Open today's daily note in Obsidian. If the note doesn't exist yet, creating it counts as a vault write and requires the 'Allow vault writes' grant in Vault Buddy settings."
    )]
    pub async fn open_daily_note(
        &self,
        Parameters(p): Parameters<VaultIdParams>,
    ) -> Result<CallToolResult, McpError> {
        let allow_create = self.writes_allowed();
        let date = Self::today();
        // Peek at existence first so a create (a vault write) can fire on_write.
        let vault = match services::find_vault(&self.deps.paths, &p.vault_id) {
            Ok(v) => v,
            Err(e) => {
                Self::audit("open_daily_note", &p.vault_id, &Err(e.clone()));
                return Self::tool_error(e);
            }
        };
        let (_, existed) =
            vault_buddy_core::daily_note_target(std::path::Path::new(&vault.path), date);
        let outcome = services::open_daily_note(
            &self.deps.paths,
            &p.vault_id,
            date,
            allow_create,
            &*self.deps.launch,
        );
        Self::audit("open_daily_note", &p.vault_id, &outcome);
        match outcome {
            Ok(()) => {
                if !existed {
                    (self.deps.on_write)(WriteEvent {
                        kind: WriteKind::CreateDailyNote,
                        title: date.format("%Y-%m-%d").to_string(),
                        vault_name: vault.name,
                    });
                }
                Self::ok_json(&serde_json::json!({ "opened": true, "created": !existed }))
            }
            Err(e) => Self::tool_error(e),
        }
    }
}

#[tool_router(router = write_tools_router, vis = "pub")]
impl VaultBuddyMcp {
    #[tool(
        description = "Create a task document in a vault's tasks folder. Requires the 'Allow vault writes' grant."
    )]
    pub async fn add_task(
        &self,
        Parameters(p): Parameters<AddTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.writes_allowed() {
            return Self::tool_error(WRITES_DISABLED);
        }
        let vault = match services::find_vault(&self.deps.paths, &p.vault_id) {
            Ok(v) => v,
            Err(e) => return Self::tool_error(e),
        };
        let today = Self::today().format("%Y-%m-%d").to_string();
        let outcome = services::add_task(&self.deps.paths, &p.vault_id, &p.title, &today);
        Self::audit(
            "add_task",
            &p.vault_id,
            &outcome.as_ref().map(|_| ()).map_err(Clone::clone),
        );
        match outcome {
            Ok(task) => {
                (self.deps.on_write)(WriteEvent {
                    kind: WriteKind::AddTask,
                    title: task.title.clone(),
                    vault_name: vault.name,
                });
                Self::ok_json(&task)
            }
            Err(e) => Self::tool_error(e),
        }
    }

    #[tool(
        description = "Set a task's status to new, done, or archived. Requires the 'Allow vault writes' grant."
    )]
    pub async fn set_task_status(
        &self,
        Parameters(p): Parameters<SetTaskStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        if !self.writes_allowed() {
            return Self::tool_error(WRITES_DISABLED);
        }
        let vault = match services::find_vault(&self.deps.paths, &p.vault_id) {
            Ok(v) => v,
            Err(e) => return Self::tool_error(e),
        };
        let outcome = services::set_task_status(&self.deps.paths, &p.vault_id, &p.path, &p.status);
        Self::audit(
            "set_task_status",
            &p.vault_id,
            &outcome.as_ref().map(|_| ()).map_err(Clone::clone),
        );
        match outcome {
            Ok(title) => {
                (self.deps.on_write)(WriteEvent {
                    kind: WriteKind::SetTaskStatus,
                    title: title.clone(),
                    vault_name: vault.name,
                });
                Self::ok_json(&serde_json::json!({ "path": p.path, "status": p.status, "title": title }))
            }
            Err(e) => Self::tool_error(e),
        }
    }
}

#[tool_handler]
impl ServerHandler for VaultBuddyMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vault-buddy", &self.deps.app_version))
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_instructions(INSTRUCTIONS.to_string())
    }
}
```

Add to `lib.rs`: `pub mod service;` and `pub use service::{Deps, VaultBuddyMcp, WriteEvent, WriteKind};` plus `pub use service::WRITES_DISABLED;`.

API-drift notes (mechanical adaptations only): if `ContentBlock::text` is `Content::text` in the pinned version, or `annotations(read_only_hint = true)` isn't accepted by the `#[tool]` macro, or `ServerInfo::new`/`Implementation::new`/`ProtocolVersion::LATEST` differ — mirror the pinned version's `examples/servers` counter exactly; the tool bodies and gates above stay as written. If `ToolRouter` addition is `merge` instead of `+`, use `merge`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/mcp && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/mcp/src/lib.rs src-tauri/mcp/src/service.rs
git commit -m "feat(mcp): seven vault tools with double-gated writes"
```

---

### Task 6: HTTP server runner + client round-trip integration test

**Files:**
- Modify: `src-tauri/mcp/src/http.rs` (add the runner + axum wiring)
- Modify: `src-tauri/mcp/src/lib.rs` (re-export `start`, `RunningServer`)
- Test: `src-tauri/mcp/tests/roundtrip.rs`

**Interfaces:**
- Consumes: `VaultBuddyMcp::new(Deps)` (Task 5), guards (Task 4).
- Produces (the shell consumes exactly):
  - `pub fn start(deps: Deps, port: u16, token: String) -> Result<RunningServer, String>` — `port == 0` binds an ephemeral port; returns after the bind is confirmed (a taken port is an `Err`, synchronously)
  - `pub struct RunningServer { pub port: u16, /* private: cancel + join */ }` with `pub fn stop(self)`

- [ ] **Step 1: Write the failing integration test** (`src-tauri/mcp/tests/roundtrip.rs`)

```rust
//! Client-agnostic spec-level validation: a real MCP client (rmcp's own,
//! co-versioned with the server) drives initialize → tools/list → tools/call
//! over streamable HTTP against a temp-dir vault, and the task file actually
//! lands on disk.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use vault_buddy_mcp::{start, Deps};

const TOKEN: &str = "test-token-test-token-test-token-test-token";

fn fixture_deps(dir: &std::path::Path, allow_writes: bool) -> Deps {
    let vault = dir.join("MyVault");
    std::fs::create_dir_all(&vault).unwrap();
    let obsidian_json = dir.join("obsidian.json");
    std::fs::write(
        &obsidian_json,
        serde_json::json!({
            "vaults": { "deadbeef01234567": { "path": vault.to_string_lossy() } }
        })
        .to_string(),
    )
    .unwrap();
    let config_json = dir.join("config.json");
    std::fs::write(&config_json, "{}").unwrap();
    Deps {
        paths: vault_buddy_core::services::ServicePaths {
            obsidian_json: Some(obsidian_json),
            config_json: Some(config_json),
        },
        app_version: "0.0.0-test".to_string(),
        allow_writes: Arc::new(AtomicBool::new(allow_writes)),
        launch: Arc::new(|_uri: &str| Ok(())),
        on_write: Arc::new(|_ev| {}),
    }
}

fn authed_http_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {TOKEN}").parse().unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn full_round_trip_with_writes_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), true), 0, TOKEN.to_string()).unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", server.port);

    let transport = StreamableHttpClientTransport::with_client(
        authed_http_client(),
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            url.clone(),
        ),
    );
    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("roundtrip-test", "0.0.0"),
    );
    let client = client_info.serve(transport).await.expect("initialize");

    let tools = client.list_tools(Default::default()).await.unwrap();
    let mut names: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(
        names,
        ["add_task", "list_recordings", "list_tasks", "list_vaults", "open_daily_note", "open_vault", "set_task_status"]
    );

    let result = client
        .call_tool(
            CallToolRequestParams::new("add_task").with_arguments(
                serde_json::json!({ "vaultId": "deadbeef01234567", "title": "Buy milk" })
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");

    // The write is REAL: a task document exists in the temp vault.
    let tasks_dir = dir.path().join("MyVault/Tasks");
    let files: Vec<_> = std::fs::read_dir(&tasks_dir).unwrap().collect();
    assert_eq!(files.len(), 1);

    client.cancel().await.unwrap();
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn writes_off_hides_and_rejects_write_tools() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", server.port);

    let transport = StreamableHttpClientTransport::with_client(
        authed_http_client(),
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            url.clone(),
        ),
    );
    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("roundtrip-test", "0.0.0"),
    );
    let client = client_info.serve(transport).await.expect("initialize");

    let tools = client.list_tools(Default::default()).await.unwrap();
    let names: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    assert!(!names.contains(&"add_task".to_string()), "{names:?}");
    assert!(names.contains(&"list_vaults".to_string()));

    client.cancel().await.unwrap();
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_without_the_token_or_with_an_evil_origin_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", server.port);
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });

    let plain = reqwest::Client::new();
    let resp = plain.post(&url).json(&body).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    let resp = plain
        .post(&url)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Origin", "http://evil.test")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_closes_the_listener_even_with_a_pinned_open_stream() {
    // Codex review catch: a client holding a streamable-HTTP stream open must
    // not keep the old endpoint (and old token) alive past a disable/restart.
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let port = server.port;
    let url = format!("http://127.0.0.1:{port}/mcp");
    // Pin a GET (rmcp's standalone SSE notification stream) and never read it
    // to completion; if the pinned GET is refused sessionless (4xx), holding
    // the response still exercises an open connection.
    let client = authed_http_client();
    let _pinned = client
        .get(&url)
        .header("Accept", "text/event-stream")
        .send()
        .await;
    let started = std::time::Instant::now();
    tokio::task::spawn_blocking(move || server.stop())
        .await
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "stop() must be bounded by the drain grace"
    );
    // The port must actually be free again — the old listener is gone.
    let rebind = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
    assert!(rebind.is_ok(), "old listener still owns the port");
}

#[test]
fn a_taken_port_is_a_synchronous_error() {
    let dir = tempfile::tempdir().unwrap();
    let a = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let err = start(fixture_deps(dir.path(), false), a.port, TOKEN.to_string());
    assert!(err.is_err(), "second bind on port {} must fail", a.port);
    a.stop();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri/mcp && cargo test --test roundtrip`
Expected: compile FAILURE — `start`/`RunningServer` don't exist.

- [ ] **Step 3: Implement the runner** (append to `http.rs`)

```rust
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;

use crate::service::{Deps, VaultBuddyMcp};

#[derive(Clone)]
struct Guard {
    token: Arc<String>,
}

fn header_str<'a>(headers: &'a HeaderMap, name: header::HeaderName) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

async fn guard(
    State(g): State<Guard>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let headers = req.headers();
    if !origin_ok(header_str(headers, header::ORIGIN)) {
        return Err(StatusCode::FORBIDDEN);
    }
    if !auth_ok(header_str(headers, header::AUTHORIZATION), &g.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !length_ok(header_str(headers, header::CONTENT_LENGTH)) {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Ok(next.run(req).await)
}

/// How long in-flight requests get after a shutdown request before the
/// listener is closed by force. Bounds `RunningServer::stop()` by
/// construction — the disable/regenerate path must be able to PROVE the old
/// socket (and old token) are gone before reporting success.
const DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// A live server: the bound port plus the handles to stop it. Dropping
/// without `stop()` leaves the thread running until process exit — fine for
/// app shutdown (the OS reclaims the listener), wrong for a settings change.
pub struct RunningServer {
    pub port: u16,
    cancel: CancellationToken,
    join: std::thread::JoinHandle<()>,
}

impl RunningServer {
    /// Cancel + join. The join is bounded by DRAIN_GRACE by construction
    /// (the runner force-closes after the drain), so this returns promptly
    /// and only once the listener is actually released.
    pub fn stop(self) {
        self.cancel.cancel();
        if self.join.join().is_err() {
            log::error!("mcp-server thread panicked during shutdown");
        }
    }
}

/// Bind 127.0.0.1:`port` (0 = ephemeral) and serve MCP on a dedicated named
/// thread with its own current-thread tokio runtime. Returns only after the
/// bind outcome is known, so "port already in use" is a synchronous,
/// user-visible error — never a silently dead server.
pub fn start(deps: Deps, port: u16, token: String) -> Result<RunningServer, String> {
    let cancel = CancellationToken::new();
    let ct = cancel.clone();
    // std channel: the caller is synchronous (a Tauri command / setup), the
    // sender is inside the runtime — a oneshot over threads.
    let (bind_tx, bind_rx) = std::sync::mpsc::channel::<Result<u16, String>>();

    let join = std::thread::Builder::new()
        .name("mcp-server".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = bind_tx.send(Err(format!("tokio runtime: {e}")));
                    return;
                }
            };
            rt.block_on(async move {
                let listener =
                    match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                        Ok(l) => l,
                        Err(e) => {
                            let _ = bind_tx.send(Err(format!(
                                "could not bind 127.0.0.1:{port}: {e}"
                            )));
                            return;
                        }
                    };
                let actual = match listener.local_addr() {
                    Ok(a) => a.port(),
                    Err(e) => {
                        let _ = bind_tx.send(Err(format!("local_addr: {e}")));
                        return;
                    }
                };
                let service = StreamableHttpService::new(
                    move || Ok(VaultBuddyMcp::new(deps.clone())),
                    LocalSessionManager::default().into(),
                    StreamableHttpServerConfig::default()
                        .with_cancellation_token(ct.child_token()),
                );
                let router = axum::Router::new()
                    .nest_service("/mcp", service)
                    .layer(middleware::from_fn_with_state(
                        Guard {
                            token: Arc::new(token),
                        },
                        guard,
                    ));
                let _ = bind_tx.send(Ok(actual));
                log::info!("mcp: serving on 127.0.0.1:{actual}/mcp");
                let shutdown = ct.clone();
                let serve = axum::serve(listener, router)
                    .with_graceful_shutdown(async move { shutdown.cancelled().await });
                let forced = ct.clone();
                tokio::select! {
                    result = serve => {
                        if let Err(e) = result {
                            log::error!("mcp: server exited with error: {e}");
                        }
                    }
                    _ = async {
                        forced.cancelled().await;
                        tokio::time::sleep(DRAIN_GRACE).await;
                    } => {
                        // Dropping the serve future hard-closes the listener and
                        // every connection. A client pinning an SSE stream open
                        // must not keep the old endpoint (and old token) alive
                        // after the UI reports the server stopped.
                        log::warn!("mcp: graceful drain timed out; forcing close");
                    }
                }
                log::info!("mcp: server stopped");
            });
        })
        .map_err(|e| format!("could not spawn mcp-server thread: {e}"))?;

    match bind_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(port)) => Ok(RunningServer { port, cancel, join }),
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => Err("mcp-server did not report its bind status within 10s".to_string()),
    }
}
```

Add to `lib.rs`: `pub use http::{start, RunningServer};`.

API-drift notes: if `StreamableHttpClientTransportConfig::with_uri` is a struct-literal config in the pinned version, construct it as `StreamableHttpClientTransportConfig { uri: url.into(), ..Default::default() }`; if `with_cancellation_token` doesn't exist, set the field on the config struct. Nothing else may change.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/mcp && cargo test && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt`
Expected: all four roundtrip tests PASS plus Task 4/5 suites.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/mcp/src/http.rs src-tauri/mcp/src/lib.rs src-tauri/mcp/tests/roundtrip.rs src-tauri/Cargo.lock
git commit -m "feat(mcp): streamable http runner with guarded endpoint and round-trip test"
```

---

### Task 7: Shell lifecycle — `mcp_commands.rs`, state, wiring

**Files:**
- Create: `src-tauri/src/mcp_commands.rs`
- Modify: `src-tauri/src/lib.rs` (module, `.manage`, 3 commands, setup hook)
- Modify: `src-tauri/Cargo.toml` (`vault_buddy_mcp = { path = "mcp" }`)
- Test: compile gates (this file is Tauri-bound; its logic lives in already-tested crates)

**Interfaces:**
- Consumes: `vault_buddy_mcp::{start, RunningServer, Deps, WriteEvent}`, `token::generate_token`, `capture_config::{load_config, update_mcp_config_at, config_path, McpConfig, DEFAULT_MCP_PORT}`, `ConfigWriteLock`.
- Produces IPC (frontend consumes these names + camelCase DTOs):
  - `get_mcp_config() -> McpConfigDto`
  - `set_mcp_config(input: McpConfigInput) -> Result<McpConfigDto, String>`
  - `regenerate_mcp_token() -> Result<McpConfigDto, String>`
  - Events: `mcp:status` (`McpStatusDto`), `mcp:write` (`WriteEvent`)
  - `McpConfigDto { enabled, port, allowWrites, token, status: { state: "running"|"stopped"|"error", port?, message? } }`

- [ ] **Step 1: Write `mcp_commands.rs`**

```rust
//! Lifecycle + settings IPC for the embedded MCP server. The protocol/tool
//! logic lives in the Tauri-free `vault_buddy_mcp` crate; this file only
//! starts/stops it, persists its config, and bridges its write events to the
//! buddy's announcements.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, services, uri};

use crate::capture_commands::ConfigWriteLock;

#[derive(Default)]
pub struct McpServerState(pub Mutex<McpInner>);

#[derive(Default)]
pub struct McpInner {
    running: Option<vault_buddy_mcp::RunningServer>,
    last_error: Option<String>,
    /// Shared with the running server's Deps so an allow-writes flip takes
    /// effect live, without a restart.
    allow_writes: Arc<AtomicBool>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatusDto {
    pub state: String, // "running" | "stopped" | "error"
    pub port: Option<u16>,
    pub message: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigDto {
    pub enabled: bool,
    pub port: u16,
    pub allow_writes: bool,
    pub token: String,
    pub status: McpStatusDto,
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigInput {
    pub enabled: bool,
    pub port: u16,
    pub allow_writes: bool,
}

fn status_of(inner: &McpInner) -> McpStatusDto {
    match (&inner.running, &inner.last_error) {
        (Some(s), _) => McpStatusDto {
            state: "running".into(),
            port: Some(s.port),
            message: None,
        },
        (None, Some(e)) => McpStatusDto {
            state: "error".into(),
            port: None,
            message: Some(e.clone()),
        },
        (None, None) => McpStatusDto {
            state: "stopped".into(),
            port: None,
            message: None,
        },
    }
}

fn snapshot(app: &AppHandle) -> McpConfigDto {
    let cfg = capture_config::load_config().mcp;
    let state = app.state::<McpServerState>();
    let inner = lock_ignoring_poison(&state.0);
    McpConfigDto {
        enabled: cfg.enabled,
        port: cfg.port,
        allow_writes: cfg.allow_writes,
        token: cfg.token,
        status: status_of(&inner),
    }
}

fn emit_status(app: &AppHandle) {
    let state = app.state::<McpServerState>();
    let status = status_of(&lock_ignoring_poison(&state.0));
    if let Err(e) = app.emit("mcp:status", &status) {
        log::warn!("mcp: could not emit status: {e}");
    }
}

fn deps_for(app: &AppHandle, allow_writes: Arc<AtomicBool>) -> vault_buddy_mcp::Deps {
    let emitter = app.clone();
    vault_buddy_mcp::Deps {
        paths: services::ServicePaths::real(),
        app_version: app.package_info().version.to_string(),
        allow_writes,
        launch: Arc::new(|u: &str| uri::launch(u)),
        // The buddy window's announcer (useBuddyAnnouncements) listens for
        // this and speaks it — gated frontend-side on "Buddy messages", the
        // same gate every other announcement rides.
        on_write: Arc::new(move |ev: vault_buddy_mcp::WriteEvent| {
            if let Err(e) = emitter.emit("mcp:write", &ev) {
                log::warn!("mcp: could not emit write event: {e}");
            }
        }),
    }
}

/// Start the server from the persisted config. Never fails the caller: a
/// bind error lands in `last_error`/`mcp:status` and the log.
fn start_from_config(app: &AppHandle, cfg: &capture_config::McpConfig) {
    let state = app.state::<McpServerState>();
    let mut inner = lock_ignoring_poison(&state.0);
    if inner.running.is_some() {
        return;
    }
    inner.allow_writes.store(cfg.allow_writes, Ordering::Relaxed);
    let deps = deps_for(app, inner.allow_writes.clone());
    match vault_buddy_mcp::start(deps, cfg.port, cfg.token.clone()) {
        Ok(server) => {
            inner.running = Some(server);
            inner.last_error = None;
        }
        Err(e) => {
            log::error!("mcp: start failed: {e}");
            inner.last_error = Some(e);
        }
    }
    drop(inner);
    emit_status(app);
}

/// Stop off the calling thread's critical path: take the handle under the
/// lock, join outside it.
fn stop_running(app: &AppHandle) {
    let state = app.state::<McpServerState>();
    let server = lock_ignoring_poison(&state.0).running.take();
    if let Some(server) = server {
        server.stop();
    }
    emit_status(app);
}

/// Called once from setup. Self-heals an enabled config with no token (the
/// enable normally generates one; a hand-edited file may lack it).
pub fn start_if_enabled(app: &AppHandle) {
    let mut cfg = capture_config::load_config().mcp;
    if !cfg.enabled {
        return;
    }
    if cfg.token.is_empty() {
        cfg.token = vault_buddy_mcp::token::generate_token();
        if let Err(e) = persist(app, cfg.clone()) {
            log::error!("mcp: could not persist a self-healed token: {e}");
            return;
        }
    }
    start_from_config(app, &cfg);
}

fn persist(app: &AppHandle, cfg: capture_config::McpConfig) -> Result<(), String> {
    let lock = app.state::<ConfigWriteLock>();
    let _guard = lock_ignoring_poison(&lock.0);
    let path = capture_config::config_path().ok_or("Cannot resolve the config directory")?;
    capture_config::update_mcp_config_at(&path, cfg)
        .map_err(|e| format!("Could not save MCP settings: {e}"))
}

#[tauri::command]
pub fn get_mcp_config(app: AppHandle) -> McpConfigDto {
    snapshot(&app)
}

/// Async: stopping joins the server thread — that wait belongs on the async
/// runtime, not the main thread (the sync-command rule exists for window
/// APIs, which this never touches).
#[tauri::command]
pub async fn set_mcp_config(
    app: AppHandle,
    input: McpConfigInput,
) -> Result<McpConfigDto, String> {
    if input.port < 1024 {
        return Err("Port must be between 1024 and 65535.".to_string());
    }
    let previous = capture_config::load_config().mcp;
    let mut next = previous.clone();
    next.enabled = input.enabled;
    next.port = input.port;
    next.allow_writes = input.allow_writes;
    if next.enabled && next.token.is_empty() {
        next.token = vault_buddy_mcp::token::generate_token();
    }
    persist(&app, next.clone())?;

    // Mirror the grant into shared state first — the call-time authority for
    // any session that lives through the transition.
    {
        let state = app.state::<McpServerState>();
        let inner = lock_ignoring_poison(&state.0);
        inner.allow_writes.store(next.allow_writes, Ordering::Relaxed);
    }
    let needs_restart = next.enabled != previous.enabled
        || next.port != previous.port
        || next.token != previous.token
        // A grant flip restarts too: sessions end, clients re-initialize and
        // fetch a fresh tools/list, so newly granted write tools actually
        // become discoverable (clients cache tool lists per session and v1
        // sends no listChanged push).
        || next.allow_writes != previous.allow_writes;
    if needs_restart {
        let app2 = app.clone();
        let next2 = next.clone();
        tauri::async_runtime::spawn_blocking(move || {
            stop_running(&app2);
            if next2.enabled {
                start_from_config(&app2, &next2);
            }
        })
        .await
        .map_err(|e| format!("MCP server restart task failed: {e}"))?;
    }
    Ok(snapshot(&app))
}

#[tauri::command]
pub async fn regenerate_mcp_token(app: AppHandle) -> Result<McpConfigDto, String> {
    let mut cfg = capture_config::load_config().mcp;
    cfg.token = vault_buddy_mcp::token::generate_token();
    persist(&app, cfg.clone())?;
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        stop_running(&app2);
        if cfg.enabled {
            start_from_config(&app2, &cfg);
        }
    })
    .await
    .map_err(|e| format!("MCP server restart task failed: {e}"))?;
    Ok(snapshot(&app))
}
```

Expose `pub mod token;`'s `generate_token` through the mcp crate root if not already (`pub use token::generate_token;` — Task 4 left it as `token::generate_token`, referenced here as `vault_buddy_mcp::token::generate_token`, which works with `pub mod token`).

- [ ] **Step 2: Wire the shell**

`src-tauri/Cargo.toml` (dependencies): `vault_buddy_mcp = { path = "mcp" }`.

`src-tauri/src/lib.rs`:
- `mod mcp_commands;` (module list at the top)
- `.manage(mcp_commands::McpServerState::default())` (next to the other `.manage` calls, `lib.rs:191-193`)
- Register in `invoke_handler` (after the task commands): `mcp_commands::get_mcp_config, mcp_commands::set_mcp_config, mcp_commands::regenerate_mcp_token`
- In `setup`, after `transcription::run_transcription(app.handle());`: `mcp_commands::start_if_enabled(app.handle());`

- [ ] **Step 3: Compile gates**

Run: `npm run setup:linux` (once per container, installs GTK/WebView deps), then `npx tauri build --no-bundle`
Expected: shell crate compiles (this catches IPC signature drift and the new module wiring).
Run: `cd src-tauri && cargo fmt --check && cd core && cargo test`
Expected: clean, PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/src/mcp_commands.rs
git commit -m "feat(shell): mcp server lifecycle, settings commands, write-event bridge"
```

---

### Task 8: Frontend — `McpSettings.vue` + buddy announcements for MCP writes

**Files:**
- Create: `src/components/McpSettings.vue`
- Modify: `src/components/BuddySettings.vue` (render it after `<DiagnosticsSettings />`)
- Modify: `src/buddyMessages.ts` (add `mcpWriteMessage`)
- Modify: `src/composables/useBuddyAnnouncements.ts` (listen for `mcp:write`)
- Test: `tests/mcp-settings.test.ts` (create), `tests/buddy-announcements.test.ts` (extend)

**Interfaces:**
- Consumes IPC: `get_mcp_config`, `set_mcp_config`, `regenerate_mcp_token`; events `mcp:status`, `mcp:write` (Task 7 DTOs, camelCase).
- Produces: `McpConfig` TS type local to the component; `mcpWriteMessage(payload: { kind: string; title: string; vaultName: string }): string`.

- [ ] **Step 1: Write the failing tests**

`tests/mcp-settings.test.ts`:

```typescript
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

// The component listens for mcp:status pushes; capture the handler so tests
// can fire one.
const listeners: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners[name] = cb;
    return Promise.resolve(() => delete listeners[name]);
  },
}));

import McpSettings from "../src/components/McpSettings.vue";

const baseConfig = {
  enabled: false,
  port: 22082,
  allowWrites: false,
  token: "",
  status: { state: "stopped", port: null, message: null },
};

describe("McpSettings", () => {
  beforeEach(() => clearMocks());
  afterEach(() => clearMocks());

  it("loads config on mount and renders the stopped status", async () => {
    mockIPC((cmd) => (cmd === "get_mcp_config" ? { ...baseConfig } : undefined));
    const wrapper = mount(McpSettings);
    await flushPromises();
    expect(wrapper.text()).toContain("MCP server");
    expect(wrapper.text()).toContain("Stopped");
    expect(wrapper.text()).toContain("Allow vault writes");
  });

  it("enabling saves via set_mcp_config and shows the running port", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_mcp_config") return { ...baseConfig };
      if (cmd === "set_mcp_config")
        return {
          ...baseConfig,
          enabled: true,
          token: "tok123",
          status: { state: "running", port: 22082, message: null },
        };
      return undefined;
    });
    const wrapper = mount(McpSettings);
    await flushPromises();
    await wrapper.find('[data-testid="mcp-enabled"]').setValue(true);
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_mcp_config");
    expect(set).toBeTruthy();
    expect((set!.args as { input: { enabled: boolean } }).input.enabled).toBe(true);
    expect(wrapper.text()).toContain("Running on 127.0.0.1:22082");
    // Client snippets render the live port + token.
    expect(wrapper.text()).toContain("claude mcp add");
    expect(wrapper.text()).toContain("tok123");
  });

  it("regenerate calls the command and mcp:status pushes update the badge", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "get_mcp_config")
        return { ...baseConfig, enabled: true, token: "old" };
      if (cmd === "regenerate_mcp_token")
        return { ...baseConfig, enabled: true, token: "fresh" };
      return undefined;
    });
    const wrapper = mount(McpSettings);
    await flushPromises();
    await wrapper.find('[data-testid="mcp-regenerate"]').trigger("click");
    await flushPromises();
    expect(calls).toContain("regenerate_mcp_token");
    expect(wrapper.text()).toContain("fresh");
    listeners["mcp:status"]?.({
      payload: { state: "error", port: null, message: "could not bind" },
    });
    await flushPromises();
    expect(wrapper.text()).toContain("could not bind");
  });
});
```

Extend `tests/buddy-announcements.test.ts`: following that file's existing mock pattern (it already mocks `@tauri-apps/api/event` and asserts `announce` IPC calls — mirror it exactly), add:

```typescript
  it("announces an mcp write through the buddy-messages gate", async () => {
    // fire the captured "mcp:write" listener with
    // { kind: "addTask", title: "Buy milk", vaultName: "Notes" }
    // → expect an announce IPC call whose text contains "Buy milk" and "Notes";
    // with buddyMessagesEnabled=false expect NO announce call.
  });
```

(Write it as real code against that file's helpers — the comment above states the required behavior; the file's existing structure dictates the mechanics.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/mcp-settings.test.ts tests/buddy-announcements.test.ts`
Expected: FAIL — component/message/listener don't exist.

- [ ] **Step 3: Implement**

`src/buddyMessages.ts` — add:

```typescript
/** What an AI client just did in a vault, spoken by the buddy. */
export function mcpWriteMessage(payload: {
  kind: string;
  title: string;
  vaultName: string;
}): string {
  const { kind, title, vaultName } = payload;
  if (kind === "addTask") return `Added task "${title}" to ${vaultName}`;
  if (kind === "setTaskStatus") return `Updated task "${title}" in ${vaultName}`;
  if (kind === "createDailyNote") return `Created today's note in ${vaultName}`;
  return `An AI client updated ${vaultName}`;
}
```

`src/composables/useBuddyAnnouncements.ts` — add (with the existing imports):

```typescript
import { listen } from "@tauri-apps/api/event";
import { mcpWriteMessage } from "../buddyMessages";

// inside useBuddyAnnouncements(), after the watchers:
  // MCP writes: Rust emits mcp:write after an AI client's sanctioned vault
  // write. Announced here (buddy window only — same exactly-once rule as the
  // capture watchers above); `announce` itself applies the Buddy-messages
  // setting.
  void listen<{ kind: string; title: string; vaultName: string }>(
    "mcp:write",
    (event) => announce(mcpWriteMessage(event.payload)),
  );
```

`src/components/McpSettings.vue`:

```vue
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { logError } from "../logging";

type McpStatus = { state: string; port: number | null; message: string | null };
type McpConfig = {
  enabled: boolean;
  port: number;
  allowWrites: boolean;
  token: string;
  status: McpStatus;
};

const cfg = ref<McpConfig | null>(null);
const error = ref<string | null>(null);
let unlisten: (() => void) | null = null;

onMounted(async () => {
  try {
    cfg.value = await invoke<McpConfig>("get_mcp_config");
  } catch (e) {
    // not running under Tauri (unit tests) or IPC failure — leave the card empty
    logError("mcp settings: get_mcp_config failed", e);
  }
  try {
    unlisten = await listen<McpStatus>("mcp:status", (event) => {
      if (cfg.value) cfg.value.status = event.payload;
    });
  } catch (e) {
    logError("mcp settings: listen failed", e);
  }
});
onUnmounted(() => unlisten?.());

async function save(patch: Partial<Pick<McpConfig, "enabled" | "port" | "allowWrites">>) {
  if (!cfg.value) return;
  error.value = null;
  const input = {
    enabled: cfg.value.enabled,
    port: cfg.value.port,
    allowWrites: cfg.value.allowWrites,
    ...patch,
  };
  try {
    cfg.value = await invoke<McpConfig>("set_mcp_config", { input });
  } catch (e) {
    error.value = String(e);
  }
}

async function regenerate() {
  error.value = null;
  try {
    cfg.value = await invoke<McpConfig>("regenerate_mcp_token");
  } catch (e) {
    error.value = String(e);
  }
}

function copy(text: string) {
  void navigator.clipboard?.writeText(text).catch(() => {});
}

const statusLabel = computed(() => {
  const s = cfg.value?.status;
  if (!s) return "";
  if (s.state === "running") return `Running on 127.0.0.1:${s.port}`;
  if (s.state === "error") return s.message ?? "Error";
  return "Stopped";
});

const url = computed(() => `http://127.0.0.1:${cfg.value?.port ?? 22082}/mcp`);
const claudeSnippet = computed(
  () =>
    `claude mcp add --transport http vault-buddy ${url.value} --header "Authorization: Bearer ${cfg.value?.token ?? ""}"`,
);
const cursorSnippet = computed(() =>
  JSON.stringify(
    {
      mcpServers: {
        "vault-buddy": {
          url: url.value,
          headers: { Authorization: `Bearer ${cfg.value?.token ?? ""}` },
        },
      },
    },
    null,
    2,
  ),
);
const claudeDesktopSnippet = computed(() =>
  JSON.stringify(
    {
      mcpServers: {
        "vault-buddy": {
          command: "npx",
          args: [
            "mcp-remote",
            url.value,
            "--header",
            `Authorization: Bearer ${cfg.value?.token ?? ""}`,
          ],
        },
      },
    },
    null,
    2,
  ),
);
</script>

<template>
  <section v-if="cfg">
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      AI integrations — MCP server
    </h2>
    <div class="flex flex-col gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between gap-2">
        <label for="mcp-enabled" class="text-sm text-slate-200">
          Local MCP server
          <span class="block text-xs text-slate-500">{{ statusLabel }}</span>
        </label>
        <input
          id="mcp-enabled"
          data-testid="mcp-enabled"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="cfg.enabled"
          @change="save({ enabled: ($event.target as HTMLInputElement).checked })"
        />
      </div>
      <div class="flex items-center justify-between gap-2">
        <label for="mcp-port" class="text-sm text-slate-200">Port</label>
        <input
          id="mcp-port"
          data-testid="mcp-port"
          type="number"
          min="1024"
          max="65535"
          class="w-24 rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-right text-sm text-slate-200"
          :value="cfg.port"
          @change="save({ port: Number(($event.target as HTMLInputElement).value) })"
        />
      </div>
      <div class="flex items-center justify-between gap-2">
        <label for="mcp-writes" class="text-sm text-slate-200">
          Allow vault writes
          <span class="block text-xs text-slate-500">
            AI clients may add tasks, update task status, and create today's daily note
          </span>
        </label>
        <input
          id="mcp-writes"
          data-testid="mcp-writes"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="cfg.allowWrites"
          @change="save({ allowWrites: ($event.target as HTMLInputElement).checked })"
        />
      </div>
      <div v-if="cfg.token" class="flex items-center justify-between gap-2">
        <span class="text-sm text-slate-200">Token</span>
        <span class="flex items-center gap-1">
          <code class="max-w-40 truncate text-xs text-slate-400">{{ cfg.token }}</code>
          <button
            type="button"
            data-testid="mcp-copy-token"
            class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 hover:bg-white/10"
            @click="copy(cfg.token)"
          >
            Copy
          </button>
          <button
            type="button"
            data-testid="mcp-regenerate"
            class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 hover:bg-white/10"
            @click="regenerate"
          >
            Regenerate
          </button>
        </span>
      </div>
      <details v-if="cfg.enabled && cfg.token" class="text-xs text-slate-400">
        <summary class="cursor-pointer select-none text-slate-300">Client setup</summary>
        <div class="mt-1.5 flex flex-col gap-2">
          <div>
            <div class="mb-0.5 flex items-center justify-between">
              <span>Claude Code</span>
              <button type="button" class="cursor-pointer text-slate-300 hover:text-slate-100" @click="copy(claudeSnippet)">Copy</button>
            </div>
            <pre class="overflow-x-auto rounded-lg bg-black/30 p-1.5">{{ claudeSnippet }}</pre>
          </div>
          <div>
            <div class="mb-0.5 flex items-center justify-between">
              <span>Cursor (.cursor/mcp.json)</span>
              <button type="button" class="cursor-pointer text-slate-300 hover:text-slate-100" @click="copy(cursorSnippet)">Copy</button>
            </div>
            <pre class="overflow-x-auto rounded-lg bg-black/30 p-1.5">{{ cursorSnippet }}</pre>
          </div>
          <div>
            <div class="mb-0.5 flex items-center justify-between">
              <span>Claude Desktop (via mcp-remote)</span>
              <button type="button" class="cursor-pointer text-slate-300 hover:text-slate-100" @click="copy(claudeDesktopSnippet)">Copy</button>
            </div>
            <pre class="overflow-x-auto rounded-lg bg-black/30 p-1.5">{{ claudeDesktopSnippet }}</pre>
          </div>
        </div>
      </details>
      <p v-if="error" class="text-xs text-rose-400">{{ error }}</p>
    </div>
  </section>
</template>
```

`src/components/BuddySettings.vue`: `import McpSettings from "./McpSettings.vue";` and render `<McpSettings />` after `<DiagnosticsSettings />`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm test && npm run build`
Expected: full Vitest suite PASS (including the extended announcements test), typecheck + build clean.

- [ ] **Step 5: Commit**

```bash
git add src/components/McpSettings.vue src/components/BuddySettings.vue src/buddyMessages.ts src/composables/useBuddyAnnouncements.ts tests/mcp-settings.test.ts tests/buddy-announcements.test.ts
git commit -m "feat(ui): MCP server settings card and buddy announcements for AI writes"
```

---

### Task 9: Docs, Windows verification checklist, final gates, push

**Files:**
- Modify: `AGENTS.md` (compile-where table row for `src-tauri/mcp/`; IPC list += `get_mcp_config`, `set_mcp_config`, `regenerate_mcp_token`; a short "MCP server domain" subsection: double write gate, id addressing, config `mcp` section must round-trip through `serialize_config`, `"mcp-server"` named thread, guard order origin→auth→length)
- Modify: `README.md` (feature blurb: opt-in local MCP server, disabled by default)
- Modify: `docs/DEVELOPMENT.md` (config.json `mcp` keys, default port 22082, token handling, the three client snippets)
- Modify: `docs/use-cases/mcp-server-and-runtime.md` (status: first slice shipped — embedded streamable-HTTP MCP server with 7 tools; the full runtime/event-bus vision remains open)
- Create: `docs/superpowers/specs/2026-07-09-local-mcp-server-windows-verification.md`

**Interfaces:** none new — documentation of everything above.

- [ ] **Step 1: Write the docs** (content per the parenthetical notes above; keep AGENTS.md's terse invariant style — one paragraph per invariant, say *why*)

- [ ] **Step 2: Write the Windows verification checklist** (this repo's `*-windows-verification.md` pattern: numbered manual steps with expected results)

Cover, as checkboxes for a manual Windows run: enable in settings → status shows running; `claude mcp add --transport http vault-buddy http://127.0.0.1:22082/mcp --header "Authorization: Bearer <token>"` → `/mcp` in Claude Code lists vault-buddy connected; `list_vaults` returns real vaults; `add_task` with writes OFF → error naming the setting (and the tool absent from the client's list); toggle "Allow vault writes" → the server restarts, the client reconnects on next use and now lists `add_task`; `add_task` creates a file the panel's Tasks view shows, buddy announces it; `open_daily_note` on a missing note with writes off → gated error; Cursor + Claude Desktop snippets connect; MCP Inspector connects and lists tools; disable → status stopped within ~3 s, client calls fail to connect and the port is released; quit app mid-enabled → relaunch → server auto-starts.

- [ ] **Step 3: Run the full gate suite**

```bash
npm test && npm run build
cd src-tauri && cargo fmt --check
cd core && cargo clippy --all-targets -- -D warnings && cargo test && cd ..
cd mcp && cargo clippy --all-targets -- -D warnings && cargo test && cd ..
npx tauri build --no-bundle
```
Expected: everything green.

- [ ] **Step 4: Commit and push**

```bash
git add AGENTS.md README.md docs/DEVELOPMENT.md docs/use-cases/mcp-server-and-runtime.md docs/superpowers/specs/2026-07-09-local-mcp-server-windows-verification.md
git commit -m "docs(mcp): document the embedded MCP server across agent and user docs"
git push -u origin claude/buddy-local-mcp-8th5l0
```

- [ ] **Step 5: Update PR #43** — check off the test-plan boxes that now ran (Vitest, build, fmt, clippy+tests, mcp tests; leave the Windows box unchecked with a note pointing at the verification doc), and watch CI + Codex review on the push.

---

## Plan Self-Review (performed at write time)

- **Spec coverage:** config section + round-trip (Task 1), services + gated daily note (Task 2), task/recording services + DTO move (Task 3), token/auth/origin/body-cap (Task 4), seven tools + double write gate + audit + instructions + annotations (Task 5), named-thread runner + bounded-drain shutdown (graceful, then forced close — stop() provably releases the socket) + synchronous bind errors + round-trip test (Task 6), lifecycle/IPC/status events/self-heal/write bridge + restart-on-grant-flip for tool discoverability (Task 7), settings card + snippets + announce (Task 8), docs + Windows verification (Task 9). Deliberately not implemented, per spec: `mcp:status` has no listener outside the settings card; no `listChanged` push (the grant-flip restart covers discoverability).
- **Type consistency:** `ServicePaths`/`Deps`/`WriteEvent`/DTO field names cross-checked across Tasks 2-8; the exact gate strings live in constants (`DAILY_NOTE_CREATE_GATED`, `WRITES_DISABLED`) so they cannot drift.
- **Placeholder scan:** the two intentional freedom points are marked "API-drift notes" (rmcp pre-pin) and the announcements-test mechanics (must follow that file's existing harness) — both state the required behavior exactly.
