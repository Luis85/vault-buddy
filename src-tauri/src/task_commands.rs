use std::path::{Path, PathBuf};
use vault_buddy_core::services::{self, ServicePaths, TaskDto};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_note, capture_paths, tasks, uri};

use crate::capture_commands::ConfigWriteLock;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksConfigDto {
    pub tasks_folder: Option<String>,
    /// The lists settings object: where unpicked new tasks land (None → the
    /// tasks root) and the display order for list sections/pickers.
    pub default_list: Option<String>,
    pub list_order: Vec<String>,
}

/// The vault's configured tasks folder (or None → the frontend shows the
/// default "Tasks") plus the lists settings object. Unknown vaults return
/// the defaults — never an error.
#[tauri::command]
pub fn get_tasks_config(id: String) -> TasksConfigDto {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    TasksConfigDto {
        tasks_folder: cfg.tasks_folder,
        default_list: cfg.default_list,
        list_order: cfg.list_order,
    }
}

/// Persist the vault's tasks folder. Validates the folder stays inside the
/// vault BEFORE writing (an invalid folder is an inline error, nothing is
/// saved), serialized behind ConfigWriteLock so a concurrent per-vault write
/// isn't lost. Read-modify-write preserves the vault's other config.
#[tauri::command]
pub fn set_tasks_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    tasks_folder: Option<String>,
) -> Result<(), String> {
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
    let _guard = lock_ignoring_poison(&lock.0);
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.tasks_folder = folder;
    capture_config::update_vault_config(&id, value)
}

/// Persist the vault's lists settings object (default list + list order),
/// preserving the tasks folder and every other per-vault field via the same
/// read-modify-write under ConfigWriteLock that set_tasks_config uses. Its
/// own command — not a widened set_tasks_config — so a lists-config failure
/// can't block the folder save and vice versa (the CaptureSettings pattern
/// of independent field-level saves).
///
/// ASYNC (GAP-22 class): the config write is fsync'd file I/O.
#[tauri::command]
pub async fn set_task_lists_config(
    lock: tauri::State<'_, ConfigWriteLock>,
    id: String,
    default_list: Option<String>,
    list_order: Vec<String>,
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
    let _guard = lock_ignoring_poison(&lock.0);
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.default_list = default_list;
    value.list_order = list_order;
    capture_config::update_vault_config(&id, value)
}

/// Read-only enumeration of a vault's list folders. Best-effort empty on an
/// unknown vault / unsafe root, mirroring list_tasks. Never writes.
///
/// ASYNC (GAP-22): a directory walk — off the main thread.
#[tauri::command]
pub async fn list_task_lists(id: String) -> Vec<String> {
    tauri::async_runtime::spawn_blocking(move || {
        services::list_task_lists(&ServicePaths::real(), &id)
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("list_task_lists: task failed: {e}");
        Vec::new()
    })
}

/// Create a list folder in the vault's tasks root; returns the created
/// list's relative name. Write-strict validation lives in core/services.
///
/// ASYNC (GAP-22 class): directory creation on a possibly-slow vault.
#[tauri::command]
pub async fn create_task_list(id: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        services::create_task_list(&ServicePaths::real(), &id, &name)
    })
    .await
    .map_err(|e| format!("create_task_list: task failed: {e}"))?
}

/// Move a task file into another list's folder; returns the landed absolute
/// path (which may carry a collision suffix the UI must adopt).
///
/// ASYNC (GAP-22 class): a vault file move (fsync-class I/O).
#[tauri::command]
pub async fn move_task_to_list(id: String, path: String, list: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        services::move_task_to_list(&ServicePaths::real(), &id, &path, &list)
    })
    .await
    .map_err(|e| format!("move_task_to_list: task failed: {e}"))?
}

/// Resolve a vault id to (vault path, lexically-safe tasks root). The shell
/// keeps its own copy for update_task/open_task (services' equivalent is
/// private); the canonical escape check is applied per-command since it
/// needs the folder to exist.
fn tasks_root_for(id: &str) -> Result<(PathBuf, PathBuf), String> {
    let vault = crate::commands::find_vault(id)?;
    let cfg = capture_config::vault_config(&capture_config::load_config(), id);
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), cfg.tasks_root())?;
    Ok((PathBuf::from(&vault.path), root))
}

/// Validate an optional due date for a write. Ok(None) when absent.
fn validated_due(due: Option<String>) -> Result<Option<String>, String> {
    match due {
        Some(d) if !tasks::is_valid_due(&d) => {
            Err(format!("Due date must be YYYY-MM-DD, got: {d}"))
        }
        other => Ok(other),
    }
}

/// Validate an optional priority for a write. `normal` normalizes to None —
/// absent means normal, and a `priority: normal` line is never written.
fn validated_priority(priority: Option<String>) -> Result<Option<String>, String> {
    match priority.as_deref() {
        None | Some("normal") => Ok(None),
        Some("high") | Some("low") => Ok(priority),
        Some(other) => Err(format!("Unknown task priority: {other}")),
    }
}

/// Validate tags for a write: trim, strip a leading `#`, drop empties,
/// dedupe case-insensitively (first casing wins). Write validation is
/// STRICT where the read side is lenient — an invalid tag is an inline
/// error naming the token, so bad input can't silently vanish on save.
fn validated_tags(tags: Vec<String>) -> Result<Vec<String>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for raw in tags {
        let t = raw.trim();
        let t = t.strip_prefix('#').unwrap_or(t);
        if t.is_empty() {
            continue;
        }
        if !tasks::is_valid_tag(t) {
            return Err(format!(
                "Invalid tag (letters, digits, -, _ and / only; not all digits): {raw}"
            ));
        }
        if seen.insert(t.to_lowercase()) {
            out.push(t.to_string());
        }
    }
    Ok(out)
}

/// Read-only list of a vault's tasks. Unknown vault / unsafe folder / missing
/// folder → empty list, never an error (mirrors list_recordings). Never writes.
///
/// ASYNC (GAP-22): recursive tasks-folder walk — off the main thread.
#[tauri::command]
pub async fn list_tasks(id: String) -> Vec<TaskDto> {
    tauri::async_runtime::spawn_blocking(move || services::list_tasks(&ServicePaths::real(), &id))
        .await
        .unwrap_or_else(|e| {
            log::warn!("list_tasks: task failed: {e}");
            Vec::new()
        })
}

/// Create a task from a title (creating the tasks folder if needed). Rejects
/// an empty title; returns the created task so the UI can prepend it.
///
/// ASYNC (GAP-22 class, Codex PR #46): the fsync'd create + collision retry is
/// blocking disk I/O — offloaded so a slow/cloud/network vault can't freeze
/// the panel/buddy event loop. The cheap up-front validation stays inline so
/// a bad due/priority/tag errors before any thread hop.
#[tauri::command]
pub async fn add_task(
    id: String,
    title: String,
    due: Option<String>,
    priority: Option<String>,
    tags: Option<Vec<String>>,
    list: Option<String>,
) -> Result<TaskDto, String> {
    // Local calendar date (YYYY-MM-DD), matching every other date-sensitive
    // path in the app (capture uses chrono::Local::now().date_naive()). A UTC
    // date would name a task with tomorrow's/yesterday's date near local
    // midnight. Passed into the clock-free core so core stays testable.
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let due = validated_due(due)?;
    let priority = validated_priority(priority)?;
    let tags = validated_tags(tags.unwrap_or_default())?;
    // The list is validated in services (normalize_list_rel — the same gate
    // the move uses); None falls back to the vault's configured defaultList.
    tauri::async_runtime::spawn_blocking(move || {
        services::add_task(
            &ServicePaths::real(),
            &id,
            &title,
            &today,
            due.as_deref(),
            priority.as_deref(),
            &tags,
            list.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("add_task: task failed: {e}"))?
}

/// Set a task's status. `status` must be one of new/done/archived. The path
/// (from list_tasks) is re-validated inside the vault's tasks root by
/// `services::set_task_status`. That call also returns the task's display
/// title for a future MCP-write announce hook — unused here, so the
/// frontend's `Result<(), String>` contract stays unchanged.
///
/// ASYNC (GAP-22 class, Codex PR #46): the surgical fsync'd frontmatter
/// rewrite is offloaded — it fires on every checkbox toggle/archive, and a
/// slow vault must not stall the event loop.
#[tauri::command]
pub async fn set_task_status(id: String, path: String, status: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        services::set_task_status(&ServicePaths::real(), &id, &path, &status).map(|_title| ())
    })
    .await
    .map_err(|e| format!("set_task_status: task failed: {e}"))?
}

/// Number of OPEN tasks (status != "done"; archived already excluded by
/// list_tasks) in a vault, for the vault-row badge. Unknown vault / unsafe or
/// missing folder / escape → 0, never an error (mirrors list_tasks). Read-only.
///
/// ASYNC (GAP-22): same walk as list_tasks, fanned out per vault by the
/// panel's badge refresh.
#[tauri::command]
pub async fn count_open_tasks(id: String) -> usize {
    tauri::async_runtime::spawn_blocking(move || {
        services::count_open_tasks(&ServicePaths::real(), &id)
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("count_open_tasks: task failed: {e}");
        0
    })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatchDto {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub clear_due: bool,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Manual rank (the drag-to-reorder write). Nothing un-ranks a task this
    /// slice, so there is no clear flag.
    #[serde(default)]
    pub order: Option<f64>,
}

/// Apply an inline-editor patch to a task: rename, set/clear the due date,
/// set the priority, set/clear tags — validated up front, then ONE surgical
/// multi-key frontmatter write (title quoted here; `priority: normal` and a
/// cleared due remove their lines; an empty tags list clears the
/// line/block). An empty patch is a no-op Ok.
///
/// ASYNC (GAP-22 class, Codex PR #46): validation + patch assembly are cheap
/// and stay inline (so a bad field errors before any thread hop), but the
/// vault resolution, containment canonicalize, read, and atomic fsync'd write
/// are offloaded — a save to a slow/cloud/network vault must not freeze the UI.
#[tauri::command]
pub async fn update_task(id: String, path: String, patch: TaskPatchDto) -> Result<(), String> {
    let mut updates: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(title) = &patch.title {
        let t = title.trim();
        if t.is_empty() {
            return Err("A task needs a title.".to_string());
        }
        updates.push(("title", Some(capture_note::yaml_quote(t))));
    }
    if patch.clear_due {
        updates.push(("due", None));
    } else if patch.due.is_some() {
        updates.push(("due", validated_due(patch.due.clone())?));
    }
    if patch.priority.is_some() {
        updates.push(("priority", validated_priority(patch.priority.clone())?));
    }
    if let Some(order) = patch.order {
        // Finite only: NaN/inf would serialize as unparseable YAML and the
        // lenient read would silently un-rank the task on the next list.
        if !order.is_finite() {
            return Err("Task order must be a finite number.".to_string());
        }
        // Rust's f64 Display is shortest-round-trip: 1536 not 1536.0, and
        // 1536.5 stays 1536.5 — the frontmatter stays human-readable.
        updates.push(("order", Some(format!("{order}"))));
    }
    if let Some(tags) = patch.tags {
        let tags = validated_tags(tags)?;
        if tags.is_empty() {
            // Explicit empty list clears — removes the line (or block).
            updates.push(("tags", None));
        } else {
            updates.push(("tags", Some(format!("[{}]", tags.join(", ")))));
        }
        // The read side (note_tags) honors a `tag:` singular alias when `tags:`
        // is absent, so every tags write must ALSO retire it: on an
        // alias-authored file, writing tags: without removing tag: would leave
        // dual keys (Obsidian shows the union, we'd show only tags:), and
        // clearing tags: alone would be a silent no-op — a missing tags: line
        // un-shadows the stale tag: alias on the next read. A missing tag:
        // line is a documented no-op, so this is safe on files that never had
        // the alias.
        updates.push(("tag", None));
    }
    if updates.is_empty() {
        return Ok(());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let (vault_path, root) = tasks_root_for(&id)?;
        if root.exists() {
            capture_paths::assert_root_inside_vault(&vault_path, &root)?;
        }
        let refs: Vec<(&str, Option<&str>)> =
            updates.iter().map(|(k, v)| (*k, v.as_deref())).collect();
        tasks::update_task_fields(&root, Path::new(&path), &refs)
    })
    .await
    .map_err(|e| format!("update_task: task failed: {e}"))?
}

/// Open a task document in Obsidian from its list row. Read-only: canonical
/// containment inside the vault's tasks root (list_tasks hands out canonical
/// paths, so the vault-relative part is computed against the CANONICAL vault
/// path or strip_prefix would fail on Windows' \\?\ form), then an
/// `obsidian://open` launch, logged by `uri::launch` like every vault open.
#[tauri::command]
pub fn open_task(id: String, path: String) -> Result<(), String> {
    let (vault_path, root) = tasks_root_for(&id)?;
    let canon_root =
        std::fs::canonicalize(&root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path = std::fs::canonicalize(Path::new(&path))
        .map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let canon_vault = std::fs::canonicalize(&vault_path)
        .map_err(|e| format!("Cannot resolve vault folder: {e}"))?;
    let rel = uri::vault_relative_no_ext(&canon_path, &canon_vault).ok_or_else(|| {
        log::warn!("open_task: {path} resolved outside its vault");
        "Task is outside its vault.".to_string()
    })?;
    uri::launch(&uri::open_file_uri(&id, &rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    // GAP-22: list_tasks/count_open_tasks must be async — the recursive
    // tasks-folder walk ran on the main thread on every panel open. The
    // lists commands walk/write the same folders, so they carry the same
    // pin (set_task_lists_config is async by construction — its State
    // parameter can't be built here).
    #[allow(dead_code)]
    fn task_list_commands_are_async() {
        fn is_future<F: std::future::Future>(_: fn(String) -> F) {}
        fn is_future2<F: std::future::Future>(_: fn(String, String) -> F) {}
        fn is_future3<F: std::future::Future>(_: fn(String, String, String) -> F) {}
        is_future(list_tasks);
        is_future(count_open_tasks);
        is_future(list_task_lists);
        is_future2(create_task_list);
        is_future3(move_task_to_list);
    }
}
