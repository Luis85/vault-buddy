use std::path::{Path, PathBuf};

use super::{app_config, find_vault, ServicePaths};
use crate::{capture_config, capture_note, capture_paths, tasks};

mod id_config;
mod lists;
mod parent;
mod update;
pub use id_config::{count_parent_links, set_task_id_config};
pub use lists::{
    create_task_list, delete_task_list, list_task_lists, move_task_to_list, rename_task_list,
    MovedTask,
};
pub use parent::{set_task_parent, ParentSet};
pub use update::{update_task, ParentOp, TaskWriteResult};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub path: String,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
    pub due: Option<String>,
    /// The do/plan date, distinct from `due`. `None` when unset. Additive for
    /// the frontend and MCP `list_tasks` alike.
    pub scheduled: Option<String>,
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
    /// Free-text detail (the `description:` frontmatter field). `None` when
    /// absent. Additive for the frontend and MCP `list_tasks` alike.
    pub description: Option<String>,
    /// The parent Task's stable id (`parent-id`); `None` when the Task has no
    /// parent. Additive for the frontend and MCP `list_tasks` alike.
    pub parent_id: Option<String>,
    /// The parent's Obsidian link, for display/navigation only.
    pub parent_link: Option<String>,
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
            scheduled: t.scheduled,
            priority: t.priority,
            tags: t.tags,
            list: t.list,
            order: t.order,
            id: t.id,
            description: t.description,
            parent_id: t.parent_id,
            parent_link: t.parent_link,
        }
    }
}

/// `add_task`'s result: the created task's fields, flattened for wire
/// compatibility (every existing `add_task` caller keeps reading the task's
/// fields at the top level — only the new boolean is added), plus whether
/// THIS call turned Task IDs on for the vault. Add subtask is very often a
/// vault's FIRST hierarchy operation, so it is the path most likely to flip
/// that setting as a side effect — and a bare `TaskDto` gives the frontend no
/// way to know (Codex P2, PR #77).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddTaskResult {
    #[serde(flatten)]
    pub task: TaskDto,
    pub ids_enabled: bool,
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

/// The archived-inclusive counterpart of `list_tasks` — the one additional
/// read the subtasks vault-UX-polish increment needed (see core::tasks::
/// list_tasks_including_archived's own doc comment): an archived task can
/// still be somebody's PARENT, and a resolver built only from the archived-
/// EXCLUDED view can never see that edge, wrongly reporting no relationship
/// for an active child whose parent was later archived — inviting a silent
/// overwrite. Same containment/degrade posture as `list_tasks`: still a
/// best-effort VIEW, not a write-time guard, so this shares every gate
/// `list_tasks` applies rather than opening a second, unguarded path.
pub fn list_tasks_including_archived(paths: &ServicePaths, id: &str) -> Vec<TaskDto> {
    let Ok((vault_path, root, cfg)) = tasks_root_for(paths, id) else {
        return Vec::new();
    };
    if let Err(e) = assert_root_if_exists(&vault_path, &root) {
        log::warn!("list_tasks_including_archived: tasks folder resolves outside the vault: {e}");
        return Vec::new();
    }
    let id_property =
        tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name());
    tasks::list_tasks_including_archived(&root, id_property)
        .into_iter()
        .map(TaskDto::from_item)
        .collect()
}

/// Create a task from a title (creating the tasks folder if needed). Rejects
/// an empty title; returns the created task so the UI can prepend it. `today`
/// (`YYYY-MM-DD`) is supplied by the caller — no clock in core. `due`,
/// `scheduled`, `priority`, and `tags` are written only when present and are
/// assumed ALREADY VALIDATED by the caller's gate (the IPC command validates
/// strictly; a caller passing raw input would write it verbatim). `list`
/// picks the list folder the task lands in: `Some` is a caller's explicit
/// choice (write-strict — an escaping path is an inline error; `Some("")`
/// means the tasks root, overriding any default), `None` falls back to the
/// vault's configured `default_list` (read-lenient — a hand-edited bad
/// default degrades to the root with a warning; it must never block adds).
/// `parent_path` (last, appended to the existing 9-parameter list) is the
/// prospective parent Task's PATH: `Some` runs the FULL shared
/// `parent::add_subtask` path (validate, lock, re-check, enable, stamp,
/// compose the link) — not phase 1's read-only validation alone, since Add
/// subtask is very often a vault's FIRST hierarchy operation (design spec
/// §2, Codex P1, PR #77) — while `None` reproduces today's exact bodyless,
/// parentless create untouched.
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
    scheduled: Option<&str>,
    parent_path: Option<&Path>,
) -> Result<AddTaskResult, String> {
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
    // Two disjoint create paths, branching on whether a parent was named.
    // Both must produce (path, the child's own id, parent_id, parent_link,
    // ids_enabled) — a parentless add reproduces today's exact bodyless
    // output byte-for-byte (unchanged from before Task 7); a parented add
    // runs the whole shared resolve-the-parent path (see `add_subtask`'s doc
    // comment for why phase-1 validation alone is not enough here).
    let (path, generated_id, parent_id, parent_link, ids_enabled) = match parent_path {
        None => {
            // One gate for both write paths (tasks::id_property_for_generation):
            // id generation is off, or the resolved property is a valid
            // non-reserved key.
            let id_property =
                tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name());
            let generated_id = id_property.is_some().then(tasks::new_task_id);
            let task_id = id_property.zip(generated_id.as_deref());
            // The vault's additive task template (None/empty → today's exact
            // output, unchanged — see render_task's byte-identical-with-no-
            // template test).
            let path = tasks::create_task(
                &target_root,
                title,
                today,
                due,
                priority,
                tags,
                task_id,
                cfg.task_extra_frontmatter.as_deref(),
                cfg.task_body_template.as_deref(),
                scheduled,
                None,
            )
            .map_err(|e| format!("Could not create task: {e}"))?;
            (path, generated_id, None, None, false)
        }
        Some(parent_path) => {
            let (resolved, path, child_id) = parent::add_subtask(
                paths,
                id,
                &vault_path,
                &root,
                &cfg,
                parent_path,
                &target_root,
                title,
                today,
                due,
                priority,
                tags,
                scheduled,
            )?;
            (
                path,
                Some(child_id),
                Some(resolved.parent_id),
                Some(resolved.link),
                resolved.ids_enabled,
            )
        }
    };
    Ok(AddTaskResult {
        task: TaskDto {
            path: path.to_string_lossy().into_owned(),
            title: title.to_string(),
            status: "new".to_string(),
            created: today.to_string(),
            done: false,
            due: due.map(str::to_string),
            scheduled: scheduled.map(str::to_string),
            priority: priority.map(str::to_string),
            tags: tags.to_vec(),
            list: effective_list,
            order: None,
            // Already computed above for the write itself — reflects the id
            // that actually landed in the file (or None when IDs are off),
            // not a fresh read.
            id: generated_id,
            // A newly created task has no description — it is a MANAGED
            // detail-view field, reserved from templates (like due/status),
            // and is set later in the detail view (Codex PR #76).
            description: None,
            parent_id,
            parent_link,
        },
        ids_enabled,
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
