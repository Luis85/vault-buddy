use std::path::{Path, PathBuf};
use vault_buddy_core::services::{self, ServicePaths, TaskDto};
use vault_buddy_core::{capture_config, capture_note, capture_paths, tasks, uri};

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
/// path (which may carry a collision suffix the UI must adopt) and the task's
/// current id (freshly stamped when the vault opts in and it lacked one), so
/// the drag / editor-move callers reveal copy-ID without a reload.
///
/// ASYNC (GAP-22 class): a vault file move (fsync-class I/O).
#[tauri::command]
pub async fn move_task_to_list(
    id: String,
    path: String,
    list: String,
) -> Result<services::MovedTask, String> {
    tauri::async_runtime::spawn_blocking(move || {
        services::move_task_to_list(&ServicePaths::real(), &id, &path, &list)
    })
    .await
    .map_err(|e| format!("move_task_to_list: task failed: {e}"))?
}

/// Rename a list folder; returns the new relative list name. Write-strict
/// validation (name shape + never-clobber) lives in core/services.
///
/// ASYNC (GAP-22 class): a vault directory rename (fsync-class I/O).
#[tauri::command]
pub async fn rename_task_list(id: String, from: String, to: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        services::rename_task_list(&ServicePaths::real(), &id, &from, &to)
    })
    .await
    .map_err(|e| format!("rename_task_list: task failed: {e}"))?
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteListDto {
    pub moved: usize,
    pub folder_removed: bool,
}

/// Delete a list folder: its own direct tasks move to the tasks root (No
/// list), then the now-empty folder is removed (a folder still holding
/// sub-lists or foreign files is kept — see core::tasks::delete_task_list).
///
/// ASYNC (GAP-22 class): a vault-wide task move + directory removal.
#[tauri::command]
pub async fn delete_task_list(id: String, list: String) -> Result<DeleteListDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        services::delete_task_list(&ServicePaths::real(), &id, &list).map(|o| DeleteListDto {
            moved: o.moved,
            folder_removed: o.folder_removed,
        })
    })
    .await
    .map_err(|e| format!("delete_task_list: task failed: {e}"))?
}

/// Resolve a vault id to (vault path, lexically-safe tasks root, the loaded
/// vault config). The shell keeps its own copy for update_task/open_task
/// (services' equivalent is private); the canonical escape check is applied
/// per-command since it needs the folder to exist. Callers that also need
/// per-vault config fields (e.g. update_task's task-id stamping) get it here
/// instead of re-reading config.json a second time.
fn tasks_root_for(
    id: &str,
) -> Result<(PathBuf, PathBuf, capture_config::VaultCaptureConfig), String> {
    let vault = crate::commands::find_vault(id)?;
    let cfg = capture_config::vault_config(&capture_config::load_config(), id);
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), cfg.tasks_root())?;
    Ok((PathBuf::from(&vault.path), root, cfg))
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

/// Validate an optional do/plan date for a write (same shape as `due`).
/// Ok(None) when absent.
fn validated_scheduled(scheduled: Option<String>) -> Result<Option<String>, String> {
    match scheduled {
        Some(d) if !tasks::is_valid_due(&d) => Err(format!("Do date must be YYYY-MM-DD, got: {d}")),
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
/// `parent_path` (optional, appended last) is the prospective parent Task's
/// PATH — never its id: with Task IDs off (the default) no id is surfaced
/// anywhere, so a path is the only identity the frontend can supply (design
/// spec §2). `Some` runs the FULL shared resolve-the-parent path in
/// `services::add_task` (validate, lock, re-check, enable, stamp, compose the
/// link), not read-only validation alone — Add subtask is very often a
/// vault's FIRST hierarchy operation. The returned `AddTaskResult` flattens
/// the created task's fields (backward-compatible wire shape) and adds
/// `idsEnabled`, true only when THIS call turned Task IDs on — the frontend
/// cannot infer that from a bare task (Codex P2, PR #77).
///
/// ASYNC (GAP-22 class, Codex PR #46): the fsync'd create + collision retry is
/// blocking disk I/O — offloaded so a slow/cloud/network vault can't freeze
/// the panel/buddy event loop. The cheap up-front validation stays inline so
/// a bad due/scheduled/priority/tag errors before any thread hop.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn add_task(
    id: String,
    title: String,
    due: Option<String>,
    priority: Option<String>,
    tags: Option<Vec<String>>,
    list: Option<String>,
    scheduled: Option<String>,
    parent_path: Option<String>,
) -> Result<services::AddTaskResult, String> {
    // Local calendar date (YYYY-MM-DD), matching every other date-sensitive
    // path in the app (capture uses chrono::Local::now().date_naive()). A UTC
    // date would name a task with tomorrow's/yesterday's date near local
    // midnight. Passed into the clock-free core so core stays testable.
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let due = validated_due(due)?;
    let scheduled = validated_scheduled(scheduled)?;
    let priority = validated_priority(priority)?;
    let tags = validated_tags(tags.unwrap_or_default())?;
    // The list is validated in services (normalize_list_rel — the same gate
    // the move uses); None falls back to the vault's configured defaultList.
    // The parent path's containment/is_task/self-parent/cycle validation all
    // live in services too — the shell never reaches into the tasks folder.
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
            scheduled.as_deref(),
            parent_path.as_deref().map(Path::new),
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

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatchDto {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub clear_due: bool,
    #[serde(default)]
    pub scheduled: Option<String>,
    #[serde(default)]
    pub clear_scheduled: bool,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    /// Manual rank (the drag-to-reorder write). Nothing un-ranks a task this
    /// slice, so there is no clear flag.
    #[serde(default)]
    pub order: Option<f64>,
    /// Free-text detail written as an escaped single-line scalar via
    /// `yaml_quote_multiline` (multi-line, `#`-safe).
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub clear_description: bool,
    /// The parent Task's PATH (never its id — with IDs disabled, the default,
    /// no id is surfaced anywhere, so a path is the only identity the frontend
    /// can supply; design spec §2).
    #[serde(default)]
    pub parent_path: Option<String>,
    #[serde(default)]
    pub clear_parent: bool,
}

/// Whether a patch has nothing to do: no ordinary field set/cleared AND no
/// parent relationship change. Checked BEFORE any per-field validation or
/// vault I/O so a truly empty patch costs nothing. A parent-only patch — the
/// Parent picker's Change/Clear sends exactly `{parentPath}` or
/// `{clearParent}` with no ordinary field — must NOT read as empty, or every
/// picker action silently no-ops (Codex P1, PR #77). Extracted so the
/// emptiness decision is testable and has ONE definition.
fn patch_is_empty(patch: &TaskPatchDto) -> bool {
    patch.title.is_none()
        && !patch.clear_due
        && patch.due.is_none()
        && !patch.clear_scheduled
        && patch.scheduled.is_none()
        && patch.priority.is_none()
        && patch.tags.is_none()
        && patch.order.is_none()
        && patch.description.is_none()
        && !patch.clear_description
        && patch.parent_path.is_none()
        && !patch.clear_parent
}

/// Apply an inline-editor patch to a task: rename, set/clear the due date,
/// set/clear the do (scheduled) date, set the priority, set/clear tags, and/or
/// set/clear the parent — validated up front, then dispatched to
/// `services::update_task` (core), which runs the ordinary field write and
/// the parent relationship change in the phase order its own doc comment
/// describes (validate the parent before any write; the field write lands
/// before the parent write). An empty patch is a no-op Ok — see
/// `patch_is_empty`, which now ALSO covers `parentPath`/`clearParent` so the
/// Parent picker's Change/Clear (which sends no ordinary field) never no-ops
/// (Codex P1, PR #77).
///
/// ASYNC (GAP-22 class, Codex PR #46): validation + patch assembly are cheap
/// and stay inline (so a bad field errors before any thread hop), but vault
/// resolution, containment, the surgical write(s), and the ID stamp are
/// offloaded (now inside `services::update_task`) — a save to a slow/cloud/
/// network vault must not freeze the UI.
///
/// Returns a `TaskWriteResult`: `id` keeps its pre-Task-7 meaning (the task's
/// current effective id, `None` when IDs are off); `parentId`/`parentLink`
/// are the pair actually written THIS call (`None` when the patch carried no
/// relationship change); `idsEnabled` is true only when THIS call turned Task
/// IDs on for the vault.
#[tauri::command]
pub async fn update_task(
    id: String,
    path: String,
    patch: TaskPatchDto,
) -> Result<services::TaskWriteResult, String> {
    if patch_is_empty(&patch) {
        return Ok(services::TaskWriteResult {
            id: None,
            parent_id: None,
            parent_link: None,
            ids_enabled: false,
        });
    }
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
    if patch.clear_scheduled {
        updates.push(("scheduled", None));
    } else if patch.scheduled.is_some() {
        updates.push(("scheduled", validated_scheduled(patch.scheduled.clone())?));
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
    if patch.clear_description {
        updates.push(("description", None));
    } else if let Some(desc) = &patch.description {
        updates.push((
            "description",
            Some(capture_note::yaml_quote_multiline(desc)),
        ));
    }
    // Clear wins over set — the same precedence clearDue/clearScheduled/
    // clearDescription already use above.
    let parent_op = if patch.clear_parent {
        services::ParentOp::Clear
    } else if let Some(p) = patch.parent_path {
        services::ParentOp::Set(PathBuf::from(p))
    } else {
        services::ParentOp::Keep
    };
    tauri::async_runtime::spawn_blocking(move || {
        let refs: Vec<(&str, Option<&str>)> =
            updates.iter().map(|(k, v)| (*k, v.as_deref())).collect();
        services::update_task(
            &ServicePaths::real(),
            &id,
            Path::new(&path),
            &refs,
            parent_op,
        )
    })
    .await
    .map_err(|e| format!("update_task: task failed: {e}"))?
}

/// Permanently delete a task file. The app's first destructive vault write —
/// gated behind a hardened confirm in the detail view; see docs/Gaps.md for
/// the deliberate departure from vault-is-sacred. ASYNC: the fs removal is off
/// the main thread like the other task writes.
#[tauri::command]
pub async fn delete_task(id: String, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (vault_path, root, _cfg) = tasks_root_for(&id)?;
        if root.exists() {
            capture_paths::assert_root_inside_vault(&vault_path, &root)?;
        }
        tasks::delete_task(&root, Path::new(&path))
    })
    .await
    .map_err(|e| format!("delete_task: task failed: {e}"))?
}

/// Duplicate a task file into the same list. Returns the landed (possibly
/// suffixed) path so the detail view's success toast can offer to open it.
/// ASYNC: the read + collision-safe fsync'd write is off the main thread.
#[tauri::command]
pub async fn duplicate_task(id: String, path: String) -> Result<String, String> {
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let (vault_path, root, cfg) = tasks_root_for(&id)?;
        if root.exists() {
            capture_paths::assert_root_inside_vault(&vault_path, &root)?;
        }
        // Touch the id property only when the configured name is a valid,
        // non-reserved id key — never a foreign/reserved field. `ids_enabled`
        // then decides regenerate (on) vs. strip (off) inside the core fn.
        let prop_name = cfg.task_id_property_name();
        let id_property = tasks::is_valid_id_property(prop_name).then_some(prop_name);
        let new_path = tasks::duplicate_task(
            &root,
            Path::new(&path),
            &today,
            id_property,
            cfg.task_id_enabled,
        )?;
        Ok(new_path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("duplicate_task: task failed: {e}"))?
}

/// Open a task document in Obsidian from its list row. Read-only: canonical
/// containment inside the vault's tasks root (list_tasks hands out canonical
/// paths, so the vault-relative part is computed against the CANONICAL vault
/// path or strip_prefix would fail on Windows' \\?\ form), then an
/// `obsidian://open` launch, logged by `uri::launch` like every vault open.
#[tauri::command]
pub fn open_task(id: String, path: String) -> Result<(), String> {
    let (vault_path, root, _cfg) = tasks_root_for(&id)?;
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

    #[test]
    fn validated_scheduled_accepts_dates_and_rejects_junk() {
        assert_eq!(validated_scheduled(None).unwrap(), None);
        assert_eq!(
            validated_scheduled(Some("2026-07-20".to_string())).unwrap(),
            Some("2026-07-20".to_string())
        );
        assert!(validated_scheduled(Some("next week".to_string())).is_err());
    }

    #[test]
    fn a_parent_only_patch_is_not_treated_as_empty() {
        // The Parent picker's Change/Clear sends {parentPath} / {clearParent}
        // with NO ordinary field updates. update_task no-ops an empty patch, so
        // unless the relationship fields count toward "is there anything to do",
        // every picker action is a silent no-op (Codex P1, PR #77).
        let patch = TaskPatchDto {
            parent_path: Some("/v/Tasks/p.md".into()),
            ..Default::default()
        };
        assert!(!patch_is_empty(&patch));
        let clearing = TaskPatchDto {
            clear_parent: true,
            ..Default::default()
        };
        assert!(!patch_is_empty(&clearing));
        assert!(patch_is_empty(&TaskPatchDto::default()));
    }

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
        is_future2(delete_task_list);
        is_future3(move_task_to_list);
        is_future3(rename_task_list);
    }
}
