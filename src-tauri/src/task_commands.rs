use std::path::{Path, PathBuf};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_note, capture_paths, discovery, tasks, uri};

use crate::capture_commands::ConfigWriteLock;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksConfigDto {
    pub tasks_folder: Option<String>,
}

/// The vault's configured tasks folder (or None → the frontend shows the
/// default "Tasks"). Unknown vaults return None — never an error.
#[tauri::command]
pub fn get_tasks_config(id: String) -> TasksConfigDto {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    TasksConfigDto {
        tasks_folder: cfg.tasks_folder,
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
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
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

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub path: String,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
    pub due: Option<String>,
    pub priority: Option<String>,
    pub tags: Vec<String>,
}

impl TaskDto {
    fn from_item(t: tasks::TaskItem) -> Self {
        Self {
            path: t.path.to_string_lossy().into_owned(),
            title: t.title,
            status: t.status,
            created: t.created,
            done: t.done,
            due: t.due,
            priority: t.priority,
            tags: t.tags,
        }
    }
}

/// Resolve a vault id to (vault path, lexically-safe tasks root). Shared by
/// list/add/toggle so folder resolution lives in one place; the canonical
/// escape check is applied per-command (skip-on-read, error-on-write) since
/// it needs the folder to exist.
fn tasks_root_for(id: &str) -> Result<(PathBuf, PathBuf), String> {
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
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
#[tauri::command]
pub fn list_tasks(id: String) -> Vec<TaskDto> {
    let Ok((vault_path, root)) = tasks_root_for(&id) else {
        return Vec::new();
    };
    // Canonicalize before scanning: a symlinked tasks folder could otherwise
    // enumerate/read frontmatter outside the vault. A merely missing folder
    // degrades quietly (list_tasks returns empty); an escape is warned.
    if root.exists() {
        if let Err(e) = capture_paths::assert_root_inside_vault(&vault_path, &root) {
            log::warn!("list_tasks: tasks folder resolves outside the vault: {e}");
            return Vec::new();
        }
    }
    tasks::list_tasks(&root)
        .into_iter()
        .map(TaskDto::from_item)
        .collect()
}

/// Create a task from a title (creating the tasks folder if needed). Rejects
/// an empty title; returns the created task so the UI can prepend it.
#[tauri::command]
pub fn add_task(
    id: String,
    title: String,
    due: Option<String>,
    priority: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<TaskDto, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A task needs a title.".to_string());
    }
    let due = validated_due(due)?;
    let priority = validated_priority(priority)?;
    let tags = validated_tags(tags.unwrap_or_default())?;
    let (vault_path, root) = tasks_root_for(&id)?;
    // The registry can list a vault whose folder was moved/deleted; without
    // this guard the create_dir_all below would RESURRECT the missing vault
    // path (+ Tasks) and write a task into a directory that is no longer a
    // real vault. `start_capture` guards its recording write the same way.
    if !vault_path.is_dir() {
        return Err(format!("Vault folder not found: {}", vault_path.display()));
    }
    // Validate the folder resolves inside the vault BEFORE creating it: this
    // canonicalizes the nearest existing ancestor, so a symlink/junction at any
    // ancestor is caught even when the leaf doesn't exist yet — create_dir_all
    // then can't create a directory (or write a task) outside the vault. The
    // lexical safe_recording_root already rejected `..`/absolute components.
    capture_paths::assert_path_inside_vault(&vault_path, &root)?;
    std::fs::create_dir_all(&root).map_err(|e| format!("Could not create tasks folder: {e}"))?;
    // Local calendar date (YYYY-MM-DD), matching every other date-sensitive
    // path in the app (capture uses chrono::Local::now().date_naive()). A UTC
    // date would name a task with tomorrow's/yesterday's date near local
    // midnight. Passed into the clock-free core so core stays testable.
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let path = tasks::create_task(
        &root,
        title,
        &today,
        due.as_deref(),
        priority.as_deref(),
        &tags,
    )
    .map_err(|e| format!("Could not create task: {e}"))?;
    Ok(TaskDto {
        path: path.to_string_lossy().into_owned(),
        title: title.to_string(),
        status: "new".to_string(),
        created: today,
        done: false,
        due,
        priority,
        tags,
    })
}

/// Set a task's status. `status` must be one of new/done/archived. The path
/// (from list_tasks) is re-validated inside the vault's tasks root by
/// `tasks::set_task_status`.
#[tauri::command]
pub fn set_task_status(id: String, path: String, status: String) -> Result<(), String> {
    if !matches!(status.as_str(), "new" | "done" | "archived") {
        return Err(format!("Unknown task status: {status}"));
    }
    let (vault_path, root) = tasks_root_for(&id)?;
    // Mirror list_tasks/add_task: safe_recording_root is only lexical, so
    // canonicalize and reject a tasks folder that resolves outside the vault
    // before writing — keeps the "assert root inside vault before any write"
    // invariant uniform across all three task commands. (Core also
    // canonicalizes root + path and requires containment.)
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
    tasks::set_task_status(&root, Path::new(&path), &status)
}

/// Number of OPEN tasks (status != "done"; archived already excluded by
/// list_tasks) in a vault, for the vault-row badge. Unknown vault / unsafe or
/// missing folder / escape → 0, never an error (mirrors list_tasks). Read-only.
#[tauri::command]
pub fn count_open_tasks(id: String) -> usize {
    let Ok((vault_path, root)) = tasks_root_for(&id) else {
        return 0;
    };
    if root.exists() {
        if let Err(e) = capture_paths::assert_root_inside_vault(&vault_path, &root) {
            log::warn!("count_open_tasks: tasks folder resolves outside the vault: {e}");
            return 0;
        }
    }
    tasks::list_tasks(&root)
        .into_iter()
        .filter(|t| t.status != "done")
        .count()
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
}

/// Apply an inline-editor patch to a task: rename, set/clear the due date,
/// set the priority — validated up front, then ONE surgical multi-key
/// frontmatter write (title quoted here; `priority: normal` and a cleared due
/// remove their lines). An empty patch is a no-op Ok.
#[tauri::command]
pub fn update_task(id: String, path: String, patch: TaskPatchDto) -> Result<(), String> {
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
    if let Some(tags) = patch.tags {
        let tags = validated_tags(tags)?;
        if tags.is_empty() {
            // Explicit empty list clears — removes the line (or block).
            updates.push(("tags", None));
        } else {
            updates.push(("tags", Some(format!("[{}]", tags.join(", ")))));
        }
    }
    if updates.is_empty() {
        return Ok(());
    }
    let (vault_path, root) = tasks_root_for(&id)?;
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
    let refs: Vec<(&str, Option<&str>)> = updates.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    tasks::update_task_fields(&root, Path::new(&path), &refs)
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
    let rel = uri::vault_relative_no_ext(&canon_path, &canon_vault)
        .ok_or_else(|| format!("task is outside its vault: {path}"))?;
    uri::launch(&uri::open_file_uri(&id, &rel))
}
