use std::path::{Path, PathBuf};

use super::{app_config, find_vault, ServicePaths};
use crate::{capture_config, capture_note, capture_paths, tasks};

mod lists;
pub use lists::{
    create_task_list, delete_task_list, list_task_lists, move_task_to_list, rename_task_list,
    MovedTask,
};

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
    /// The task's List: parent folder relative to the tasks root, `/`-joined,
    /// "" at the root. Additive for the frontend and MCP list_tasks alike.
    pub list: String,
    /// Manual rank from the `order:` frontmatter number; None = unranked.
    pub order: Option<f64>,
    /// The generated id under the vault's configured property; `None` when
    /// task IDs are off (the property is never read) or simply absent.
    pub id: Option<String>,
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
            list: t.list,
            order: t.order,
            id: t.id,
        }
    }
}

/// Resolve a vault id to (vault path, lexically-safe tasks root, the vault's
/// config). The config rides along because it is ALREADY loaded here for
/// `tasks_root()` — callers that need the id/archived fields would otherwise
/// re-read and re-parse config.json a second time per call (the shell's own
/// `tasks_root_for` returns it for the same reason). The canonical escape
/// check is applied per-command via `assert_root_if_exists` (warn-and-degrade
/// on reads, error on writes) since it needs the folder to exist.
fn tasks_root_for(
    paths: &ServicePaths,
    id: &str,
) -> Result<(PathBuf, PathBuf, capture_config::VaultCaptureConfig), String> {
    let vault = find_vault(paths, id)?;
    let cfg = capture_config::vault_config(&app_config(paths), id);
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), cfg.tasks_root())?;
    Ok((PathBuf::from(&vault.path), root, cfg))
}

/// The containment gate every task command applies after `tasks_root_for`:
/// canonicalize-and-assert only when the folder exists (a merely missing root
/// degrades quietly downstream — list_tasks returns empty, the writers create
/// it). One implementation instead of a per-command paste; the read/write
/// asymmetry stays at the call sites — read commands map an Err to their own
/// warn + empty/0, write commands propagate it with `?`.
fn assert_root_if_exists(vault_path: &Path, root: &Path) -> Result<(), String> {
    if root.exists() {
        capture_paths::assert_root_inside_vault(vault_path, root)?;
    }
    Ok(())
}

/// Read-only list of a vault's tasks. Unknown vault / unsafe folder / missing
/// folder → empty list, never an error (mirrors list_recordings). Never writes.
pub fn list_tasks(paths: &ServicePaths, id: &str) -> Vec<TaskDto> {
    let Ok((vault_path, root, cfg)) = tasks_root_for(paths, id) else {
        return Vec::new();
    };
    // Canonicalize before scanning: a symlinked tasks folder could otherwise
    // enumerate/read frontmatter outside the vault. A merely missing folder
    // degrades quietly (list_tasks returns empty); an escape is warned.
    if let Err(e) = assert_root_if_exists(&vault_path, &root) {
        log::warn!("list_tasks: tasks folder resolves outside the vault: {e}");
        return Vec::new();
    }
    // Same chokepoint add_task's generation uses (tasks::id_property_for_
    // generation): off, or a reserved/invalid configured property, both
    // yield None so the property is never read — a hand-edited config
    // pointing the id at a reserved key (e.g. "status") must not surface
    // that structured field's own value as the id (Codex, PR #59).
    let id_property =
        tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name());
    tasks::list_tasks(&root, id_property)
        .into_iter()
        .map(TaskDto::from_item)
        .collect()
}

/// Create a task from a title (creating the tasks folder if needed). Rejects
/// an empty title; returns the created task so the UI can prepend it. `today`
/// (`YYYY-MM-DD`) is supplied by the caller — no clock in core. `due`,
/// `priority`, and `tags` are written only when present and are assumed
/// ALREADY VALIDATED by the caller's gate (the IPC command validates
/// strictly; a caller passing raw input would write it verbatim). `list`
/// picks the list folder the task lands in: `Some` is a caller's explicit
/// choice (write-strict — an escaping path is an inline error; `Some("")`
/// means the tasks root, overriding any default), `None` falls back to the
/// vault's configured `default_list` (read-lenient — a hand-edited bad
/// default degrades to the root with a warning; it must never block adds).
#[allow(clippy::too_many_arguments)]
pub fn add_task(
    paths: &ServicePaths,
    id: &str,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    list: Option<&str>,
) -> Result<TaskDto, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A task needs a title.".to_string());
    }
    let vault = find_vault(paths, id)?;
    let cfg = capture_config::vault_config(&app_config(paths), id);
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), cfg.tasks_root())?;
    let vault_path = PathBuf::from(&vault.path);
    let mut effective_list = match list {
        Some(l) => tasks::normalize_list_rel(l)?,
        None => {
            let default = cfg.default_list.as_deref().unwrap_or("");
            tasks::normalize_list_rel(default).unwrap_or_else(|e| {
                log::warn!("add_task: ignoring unsafe configured defaultList {default:?}: {e}");
                String::new()
            })
        }
    };
    // The registry can list a vault whose folder was moved/deleted; without
    // this guard the create_dir_all below would RESURRECT the missing vault
    // path (+ Tasks) and write a task into a directory that is no longer a
    // real vault. `start_capture` guards its recording write the same way.
    if !vault_path.is_dir() {
        // The absolute vault path stays in the log only — it once reached the
        // panel toast and MCP clients verbatim (GAP-26 remainder); the
        // user-facing copy now matches start_capture_blocking's own pattern.
        log::warn!("add_task: vault folder missing: {}", vault_path.display());
        return Err("Vault folder not found — was it moved or deleted?".to_string());
    }
    // Create + validate the tasks ROOT first, then validate the list subdir
    // against the RESOLVED root BEFORE creating it, so a list nested through a
    // symlink/junction that escapes the tasks root is rejected before
    // create_dir_all can follow the link and mkdir a stray folder outside the
    // root — not merely before the task file is written (vault is sacred). A
    // list can stay inside the vault yet escape the configured tasks root; the
    // read-side walkers (task_lists / list_tasks) canonicalize and skip such
    // folders, so a task written there would silently vanish from the view.
    // safe_recording_root already rejected `..`/absolute components lexically;
    // this mirrors move_task_to_list's create-then-canonicalize-then-check
    // order (Codex, PR #53 re-review).
    capture_paths::assert_path_inside_vault(&vault_path, &root)?;
    std::fs::create_dir_all(&root).map_err(|e| format!("Could not create tasks folder: {e}"))?;
    // Post-create assert closes the swap-in race on the root itself.
    capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    let canon_root =
        std::fs::canonicalize(&root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let target_root = if effective_list.is_empty() {
        root.clone()
    } else {
        let dir = root.join(&effective_list);
        // Pre-create: the nearest existing ancestor of the list dir must
        // resolve inside the tasks root — a symlink/junction at any ancestor is
        // caught before create_dir_all can follow it and mkdir outside the
        // root. Then create, then re-check (swap-in race, a junction planted
        // mid-flight).
        let resolved = capture_paths::assert_path_inside_vault(&canon_root, &dir)
            .and_then(|()| {
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("Could not create the list folder: {e}"))
            })
            .and_then(|()| capture_paths::assert_root_inside_vault(&canon_root, &dir));
        match resolved {
            Ok(()) => dir,
            // A CONFIGURED DEFAULT (list: None) that escapes the tasks root
            // degrades to the root — the same read-lenient posture normalize_
            // list_rel already applies to a lexically-unsafe default: a
            // hand-edited default (incl. one that is, or points through, a
            // symlink now resolving outside the root) must never break quick-
            // add for the whole vault. An explicit user pick still errors —
            // it named that exact target (Codex, PR #53 re-review).
            Err(e) if list.is_none() => {
                log::warn!(
                    "add_task: configured default list {effective_list:?} escapes the tasks \
                     root ({e}); landing in the tasks root instead"
                );
                effective_list = String::new(); // the task lands at the root, not the default
                root.clone()
            }
            Err(e) => return Err(e),
        }
    };
    // One gate for both write paths (tasks::id_property_for_generation): id
    // generation is off, or the resolved property is a valid non-reserved key.
    let id_property =
        tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name());
    let generated_id = id_property.is_some().then(tasks::new_task_id);
    let task_id = id_property.zip(generated_id.as_deref());
    // extra_frontmatter/body_template: None for now — a later task threads
    // cfg.task_extra_frontmatter/cfg.task_body_template here.
    let path = tasks::create_task(
        &target_root,
        title,
        today,
        due,
        priority,
        tags,
        task_id,
        None,
        None,
    )
    .map_err(|e| format!("Could not create task: {e}"))?;
    Ok(TaskDto {
        path: path.to_string_lossy().into_owned(),
        title: title.to_string(),
        status: "new".to_string(),
        created: today.to_string(),
        done: false,
        due: due.map(str::to_string),
        priority: priority.map(str::to_string),
        tags: tags.to_vec(),
        list: effective_list,
        order: None,
        // Already computed above for the write itself — reflects the id that
        // actually landed in the file (or None when IDs are off), not a
        // fresh read.
        id: generated_id,
    })
}

/// Set a task's status. `status` must be one of new/done/archived. The path
/// (from list_tasks) is re-validated inside the vault's tasks root by
/// `tasks::set_task_status`. Returns the task's display title (for the
/// announce hook), not `()` — callers that don't need it (the IPC command)
/// map it away.
pub fn set_task_status(
    paths: &ServicePaths,
    id: &str,
    task_path: &str,
    status: &str,
) -> Result<String, String> {
    if !matches!(status, "new" | "done" | "archived") {
        return Err(format!("Unknown task status: {status}"));
    }
    let (vault_path, root, _) = tasks_root_for(paths, id)?;
    // Mirror list_tasks/add_task: safe_recording_root is only lexical, so
    // canonicalize and reject a tasks folder that resolves outside the vault
    // before writing — keeps the "assert root inside vault before any write"
    // invariant uniform across all three task commands. (Core also
    // canonicalizes root + path and requires containment.)
    assert_root_if_exists(&vault_path, &root)?;
    tasks::set_task_status(&root, Path::new(task_path), status)?;
    // Display title for the announce hook ("Marked 'Buy milk' done…", per the
    // design spec): the frontmatter `title:` field, same extraction
    // `tasks::collect_tasks` uses for the list — create_task's filename is
    // slugified (spaces/case stripped, dated), so it can't stand in for the
    // title itself. Fall back to the file stem only when the title field is
    // absent (a hand-authored task) or the file became unreadable right after
    // the write above (warned, never swallowed) — an honest degrade, not the
    // primary source.
    let stem = Path::new(task_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| task_path.to_string());
    let title = match std::fs::read_to_string(task_path) {
        Ok(content) => capture_note::note_field(&content, "title").unwrap_or(stem),
        Err(e) => {
            log::warn!("set_task_status: could not re-read {task_path} for the title: {e}");
            stem
        }
    };
    Ok(title)
}

/// Number of OPEN tasks (status != "done"; archived-STATUS tasks already
/// excluded by list_tasks) in a vault, for the vault-row badge. Open tasks in
/// an ARCHIVED LIST are excluded too: the badge must agree with the default
/// Lists grouping, which hides them — counting them showed a nonzero badge
/// over an empty-looking view, the same phantom count the frontend's
/// visibleTasks fix removed one layer up (review, PR #59). The match mirrors
/// visibleTasks/listSections exactly: case-insensitive on the task's OWN list
/// (a nested sub-list of an archived list still renders, so it still counts).
/// Unknown vault / unsafe or missing folder / escape → 0, never an error
/// (mirrors list_tasks). Read-only.
pub fn count_open_tasks(paths: &ServicePaths, id: &str) -> usize {
    let Ok((vault_path, root, cfg)) = tasks_root_for(paths, id) else {
        return 0;
    };
    if let Err(e) = assert_root_if_exists(&vault_path, &root) {
        log::warn!("count_open_tasks: tasks folder resolves outside the vault: {e}");
        return 0;
    }
    let archived: std::collections::HashSet<String> = cfg
        .archived_lists
        .iter()
        .map(|a| a.to_lowercase())
        .collect();
    tasks::list_tasks(&root, None)
        .into_iter()
        .filter(|t| t.status != "done")
        .filter(|t| t.list.is_empty() || !archived.contains(&t.list.to_lowercase()))
        .count()
}

#[cfg(test)]
mod tests;
