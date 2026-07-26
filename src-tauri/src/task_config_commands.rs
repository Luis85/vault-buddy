//! Per-vault tasks-domain SETTINGS IPC: the tasks folder, the lists settings
//! object (default list / list order / archived lists), Task IDs, and the
//! task-document template. Split out of `task_commands.rs` (its shrink-only
//! LOC cap — it was at 707 nonblank / 740 raw against the 800 line gate, and
//! this task adds two DTO fields, a helper, two result types, and their
//! tests) for headroom, the `capture_config_commands`/`vault_config`/
//! `mcp_config`/`document_import_config` split-out precedent. No behavior
//! change: the IPC surface is unchanged, only the defining module moves
//! (`lib.rs`'s `generate_handler!` repoints to here). The list-LIFECYCLE
//! commands (`list_task_lists`/`create_task_list`/`rename_task_list`/
//! `delete_task_list`/`move_task_to_list`) and the task-document commands
//! stay in `task_commands.rs` — this file is settings only.

use std::path::Path;
use vault_buddy_core::services::{self, ServicePaths};
use vault_buddy_core::{capture_config, capture_paths, tasks};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksConfigDto {
    pub tasks_folder: Option<String>,
    /// The lists settings object: where unpicked new tasks land (None → the
    /// tasks root) and the display order for list sections/pickers.
    pub default_list: Option<String>,
    pub list_order: Vec<String>,
    /// `/`-joined relative names of lists hidden from the Lists grouping and
    /// pickers (the folder + tasks stay on disk).
    pub archived_lists: Vec<String>,
    /// Whether generated task IDs are enabled for this vault.
    pub task_id_enabled: bool,
    /// The RESOLVED id property name (default "task-id" when unset) — the UI
    /// shows it as the placeholder/current value.
    pub task_id_property: String,
    /// Additive per-vault task-document template. None → today's exact
    /// create_task output (identity frontmatter only, no body).
    pub task_extra_frontmatter: Option<String>,
    pub task_body_template: Option<String>,
}

/// The vault's configured tasks folder (or None → the frontend shows the
/// default "Tasks") plus the lists settings object. Unknown vaults return
/// the defaults — never an error.
#[tauri::command]
pub fn get_tasks_config(id: String) -> TasksConfigDto {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    let task_id_property = cfg.task_id_property_name().to_string();
    TasksConfigDto {
        task_id_enabled: cfg.task_id_enabled,
        task_id_property,
        tasks_folder: cfg.tasks_folder,
        default_list: cfg.default_list,
        list_order: cfg.list_order,
        archived_lists: cfg.archived_lists,
        task_extra_frontmatter: cfg.task_extra_frontmatter,
        task_body_template: cfg.task_body_template,
    }
}

/// Persist the vault's tasks folder. Validates the folder stays inside the
/// vault BEFORE writing (an invalid folder is an inline error, nothing is
/// saved), serialized behind `config_write_lock()` so a concurrent per-vault
/// write isn't lost. Read-modify-write preserves the vault's other config.
///
/// ASYNC (GAP-22 class, widened by the config-lock collapse): this vault's
/// own `config_write_lock()` can now be held across a full recursive scan of
/// every task file in the vault (`services::set_task_parent` — see
/// `core/src/services/tasks/parent/mod.rs`), so a debounced folder-setting
/// autosave landing mid-scan must not freeze the main thread behind it. The
/// lock's scope is unchanged (still synchronous, no `.await` in between) —
/// only the command itself moves off the main thread, mirroring
/// `set_task_lists_config` right below.
#[tauri::command]
pub async fn set_tasks_config(id: String, tasks_folder: Option<String>) -> Result<(), String> {
    let vault = crate::commands::find_vault(&id)?;
    let folder = tasks_folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string);
    // Validate the folder that will ACTUALLY be used — the explicit one, or the
    // default "Tasks" when the field is cleared — against a symlink/junction at
    // any existing ancestor (even when the leaf doesn't exist yet; the lexical
    // check can't see through a link). Clearing to a default that is itself a
    // symlink outside the vault must be rejected up front too, not just custom
    // folders, else the setting saves but list/add/toggle can't use it.
    // ("Tasks" mirrors VaultCaptureConfig::tasks_root()'s default.)
    let effective = folder.as_deref().unwrap_or("Tasks");
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), effective)?;
    capture_paths::assert_path_inside_vault(Path::new(&vault.path), &root)?;
    let _guard = capture_config::config_write_lock();
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.tasks_folder = folder;
    capture_config::update_vault_config(&id, value)
}

/// Persist the vault's lists settings object (default list + list order +
/// archived lists), preserving the tasks folder and every other per-vault
/// field via the same read-modify-write under `config_write_lock()` that
/// set_tasks_config uses. Its own command — not a widened set_tasks_config —
/// so a lists-config failure can't block the folder save and vice versa (the
/// CaptureSettings pattern of independent field-level saves).
///
/// `archived_lists` is OPTIONAL (Codex, PR #59 regression): Task 3 first
/// added it as a REQUIRED `Vec<String>`, but the existing caller
/// (`TaskListSettings.vue`, wired before the archive UI existed) invokes
/// this command with only `id`/`defaultList`/`listOrder` — Tauri v2 rejects
/// an invoke missing a required argument before the command body ever runs,
/// so every default-list/list-order save started erroring and persisting
/// nothing. A missing `Option<T>` argument deserializes to `None`, so
/// today's caller keeps working (preserves the stored set, see
/// `resolve_archived_lists`) and the future archive/unarchive UI (Tasks
/// 9/10) passes `Some(_)` to replace it.
///
/// ASYNC (GAP-22 class): the config write is fsync'd file I/O.
#[tauri::command]
pub async fn set_task_lists_config(
    id: String,
    default_list: Option<String>,
    list_order: Vec<String>,
    archived_lists: Option<Vec<String>>,
) -> Result<(), String> {
    crate::commands::find_vault(&id)?;
    // Write-strict on the default list (the settings UI offers existing
    // lists, so anything unsafe is bad input, not hand-edited config):
    // normalize rejects `..`/absolute forms; empty → None (the tasks root).
    let default_list = match default_list.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(l) => Some(tasks::normalize_list_rel(l)?).filter(|n| !n.is_empty()),
    };
    let list_order: Vec<String> = list_order
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let _guard = capture_config::config_write_lock();
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.default_list = default_list;
    value.list_order = list_order;
    value.archived_lists = resolve_archived_lists(value.archived_lists.clone(), archived_lists);
    capture_config::update_vault_config(&id, value)
}

/// Resolve the `archived_lists` field for a `set_task_lists_config` save.
/// `Some(incoming)` normalizes and REPLACES it — same best-effort posture as
/// `list_order` rather than command-failing: a stale name left over from a
/// since renamed/deleted list must not block saving the rest of the
/// picker's selections, so an unsafe or empty entry (the tasks-root
/// sentinel, `""`, is not a real list) is dropped rather than erroring.
/// `None` (an omitting caller — today's `TaskListSettings.vue`, which
/// predates the archive UI) returns `existing` untouched, which is what
/// lets that caller keep saving the default list / list order without
/// silently wiping a previously-stored archived set.
fn resolve_archived_lists(existing: Vec<String>, incoming: Option<Vec<String>>) -> Vec<String> {
    let Some(incoming) = incoming else {
        return existing;
    };
    incoming
        .into_iter()
        .filter_map(|s| match tasks::normalize_list_rel(s.trim()) {
            Ok(n) if !n.is_empty() => Some(n),
            Ok(_) => None,
            Err(e) => {
                log::warn!("set_task_lists_config: dropping unsafe archived list {s:?}: {e}");
                None
            }
        })
        .collect()
}

/// Persist the vault's Task ID settings (enable + frontmatter property).
/// Validation, the parent-links guard, and the read-modify-write itself all
/// live in `services::set_task_id_config` (core) now — see its doc comment
/// and design spec §2a. That move is what lets the guard share ONE lock
/// (`capture_config::config_write_lock()`) with `services::set_task_parent`
/// across both the scan and the commit.
///
/// This command takes `find_vault` but deliberately does NOT also take
/// `config_write_lock()`: `services::set_task_id_config` acquires it
/// internally, the lock is not reentrant (a second `.lock()` from the same
/// thread while the first guard is still alive blocks forever), and every
/// OTHER config-write command in this file takes the lock directly. This is
/// the one site where doing the same would self-deadlock the very call it is
/// about to make.
///
/// ASYNC + OFFLOADED (GAP-22 class): unlike its siblings in this file (a
/// small config.json read-modify-write each), `services::set_task_id_config`
/// can hold `config_write_lock()` across a full recursive scan of every task
/// file in the vault (`count_parent_links`, guarding a disable/re-point) BEFORE
/// its own fsync'd write — the same "unbounded vault-wide work" shape as
/// `add_task`/`update_task`/`move_task_to_list` in task_commands.rs, all of
/// which run on `tauri::async_runtime::spawn_blocking`. Being `async fn` alone
/// only gets this off the MAIN thread; without spawn_blocking the scan+write
/// still runs inline on whichever Tauri async-runtime worker polls this
/// future, occupying it (and, on that large/slow vault, delaying unrelated
/// IPC queued behind it) for the scan's whole duration. The guard's lock is
/// acquired INSIDE `services::set_task_id_config`, so it must move onto the
/// blocking pool together with the call that takes it — splitting the lock
/// from the work it protects would let a caller "await" past the point where
/// the lock is actually held.
#[tauri::command]
pub async fn set_task_id_config(
    id: String,
    enabled: bool,
    property: Option<String>,
) -> Result<(), String> {
    crate::commands::find_vault(&id)?;
    tauri::async_runtime::spawn_blocking(move || {
        services::set_task_id_config(&ServicePaths::real(), &id, enabled, property.as_deref())
    })
    .await
    .map_err(|e| format!("set_task_id_config: task failed: {e}"))?
}

/// Persist the vault's per-vault task template (extra frontmatter + body).
/// Independent field-save (the set_task_id_config pattern): a template save
/// can't block the folder/lists/id saves and vice versa. Blank→None. ASYNC —
/// fsync'd config write.
#[tauri::command]
pub async fn set_task_template_config(
    id: String,
    extra_frontmatter: Option<String>,
    body_template: Option<String>,
) -> Result<(), String> {
    crate::commands::find_vault(&id)?;
    let clean = |s: Option<String>| {
        s.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let _guard = capture_config::config_write_lock();
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.task_extra_frontmatter = clean(extra_frontmatter);
    value.task_body_template = clean(body_template);
    capture_config::update_vault_config(&id, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Codex, PR #59 (P2): Task 3 first added `archived_lists` to
    // set_task_lists_config as a REQUIRED `Vec<String>`. TaskListSettings.vue
    // predates the archive UI and invokes the command without an
    // `archivedLists` argument at all, so Tauri v2 rejected the whole invoke
    // (missing required argument) before the command body ever ran — every
    // default-list/list-order save started erroring and persisting nothing.
    // These two tests pin resolve_archived_lists, the pure helper the command
    // body now delegates to for the field: it is the one place that can be
    // exercised without a real vault/config.json (set_task_lists_config
    // itself takes tauri::State and calls ServicePaths::real(), so it can
    // only run against a developer's real app config — not unit-testable
    // here, same as every other State-taking command in this file).

    #[test]
    fn resolve_archived_lists_none_preserves_existing() {
        // None is what an omitting caller (today's TaskListSettings.vue)
        // deserializes to — it must leave the stored archived set untouched,
        // not wipe it.
        let existing = vec!["Inbox".to_string(), "Archive/Old".to_string()];
        assert_eq!(resolve_archived_lists(existing.clone(), None), existing);
    }

    #[test]
    fn resolve_archived_lists_some_normalizes_and_replaces() {
        // Some(_) is the future archive/unarchive UI (Tasks 9/10): it
        // REPLACES the stored set (existing "Stale" must not survive) after
        // normalizing exactly like list_order — trimmed, the tasks-root
        // sentinel ("") and unsafe entries (dot-prefixed, escaping) dropped
        // best-effort rather than failing the whole save.
        let existing = vec!["Stale".to_string()];
        let incoming = vec![
            "Inbox".to_string(),
            "  Work/Q3  ".to_string(),
            "".to_string(),
            ".hidden".to_string(),
            "../escape".to_string(),
        ];
        assert_eq!(
            resolve_archived_lists(existing, Some(incoming)),
            vec!["Inbox".to_string(), "Work/Q3".to_string()]
        );
    }

    // Fix 2 (this round): set_task_id_config called services::set_task_id_config
    // — a recursive parent-link scan (count_parent_links) plus an fsync'd
    // config write — directly inside the async command body, so a large or
    // slow vault occupied a Tauri async-runtime worker for the whole scan.
    // Every OTHER filesystem-heavy task command in this codebase
    // (task_commands.rs: list_tasks, add_task, update_task, move_task_to_list,
    // delete_task_list, ...) offloads via
    // `tauri::async_runtime::spawn_blocking`; this pins the same shape here.
    //
    // WHY A SOURCE SCAN, not a compile-time coercion check (the
    // `task_list_commands_are_async`-style pin task_commands.rs uses) or a
    // runtime/timing test: spawn_blocking usage isn't visible in the
    // function's TYPE signature — both the fixed and the broken form are
    // `async fn(String, bool, Option<String>) -> Result<(), String>` — so a
    // coercion check would keep compiling, and passing, even reverted. A
    // scheduling/timing test COULD distinguish them (spawn_blocking frees the
    // polling worker even on a single-worker runtime; inline blocking I/O
    // doesn't) but would need `tokio` as a direct dev-dependency (not in this
    // workspace's Cargo.toml — `tauri::async_runtime`'s re-exports don't
    // include its `#[tokio::test]` macro or `Builder`) and a controlled
    // runtime flavor that `tauri::async_runtime`'s lazily-initialized,
    // process-global singleton can't guarantee once shared across every test
    // in this binary. A source scan is the established alternative for
    // exactly this situation — `config_lock_guard.rs` is this codebase's
    // precedent for pinning a structural invariant the type system can't
    // express, including that file's own disclosed caveat: this is a text
    // scan, not a parser — it cannot prove the call is reachable or that the
    // work inside actually moved, only that the source names the mechanism.
    #[test]
    fn set_task_id_config_offloads_the_scan_and_write_to_the_blocking_pool() {
        let src = include_str!("task_config_commands.rs");
        let start = src
            .find("pub async fn set_task_id_config")
            .expect("set_task_id_config not found in its own source file");
        // Bounded by the START of the NEXT command (set_task_template_config)
        // so a match doesn't wander into unrelated code further down the file.
        let body = &src[start..];
        let body = &body[..body.find("#[tauri::command]").unwrap_or(body.len())];
        assert!(
            body.contains("spawn_blocking"),
            "set_task_id_config must offload services::set_task_id_config's recursive \
             parent-link scan + fsync'd write via tauri::async_runtime::spawn_blocking, \
             matching every other filesystem-heavy task command (task_commands.rs) — \
             calling it inline occupies a Tauri async-runtime worker for the scan's \
             duration on a large/slow vault"
        );
    }
}
