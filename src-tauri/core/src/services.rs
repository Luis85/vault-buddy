//! Shared service functions: ONE implementation of each user-visible
//! capability, called by both the Tauri IPC commands and the MCP tools
//! (spec: docs/superpowers/specs/2026-07-09-local-mcp-server-design.md).
//! Pure over `ServicePaths` so everything here tests on Linux; the caller
//! injects the clock (`date`/`today`) and the URI launcher.

use std::path::{Path, PathBuf};

use crate::{
    capture_config, capture_note, capture_paths, daily_note_target, discovery, process, recordings,
    tasks, uri,
};

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
    // The error is the panel's own copy (the spec requires MCP/IPC failures to
    // carry the same user-facing messages the panel shows) with the id
    // appended — MCP clients and logs still need the failing key.
    list_vaults_with(paths, true)
        .into_iter()
        .find(|v| v.id == id)
        .ok_or_else(|| format!("Vault not found — was it removed from Obsidian? (id: {id})"))
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
/// grant. Returns whether the note was CREATED (`true` only when the
/// `obsidian://new` branch actually launched) — reported from the branch
/// taken, not from a separate existence probe, so a caller that treats a
/// create as a vault write (the MCP tool's on_write hook and `created`
/// flag) can never disagree with this function's own exists check under a
/// race. Callers that don't care (the IPC command) map the bool away.
pub fn open_daily_note(
    paths: &ServicePaths,
    id: &str,
    date: chrono::NaiveDate,
    allow_create: bool,
    launch: &dyn Fn(&str) -> Result<(), String>,
) -> Result<bool, String> {
    let vault = find_vault(paths, id)?;
    let vault_path = std::path::Path::new(&vault.path);
    let (rel, exists) = daily_note_target(vault_path, date);
    if exists {
        launch(&uri::open_file_uri(&vault.id, &rel))?;
        Ok(false)
    } else if allow_create {
        launch(&uri::new_file_uri(&vault.id, &rel))?;
        Ok(true)
    } else {
        Err(DAILY_NOTE_CREATE_GATED.to_string())
    }
}

/// Read the app-side config from `paths`, degrading to defaults when there is
/// none — the same "missing config is never an error" rule `ServicePaths`
/// documents for the registry.
fn app_config(paths: &ServicePaths) -> capture_config::AppConfig {
    match &paths.config_json {
        Some(p) => capture_config::load_config_from(p),
        None => capture_config::AppConfig::default(),
    }
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

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    pub mp3: String,
    pub title: String,
    pub recorded_at: String,
    pub duration: Option<String>,
    // `type` is a Rust keyword — expose the camelCase `type` the frontend wants.
    #[serde(rename = "type")]
    pub recording_type: Option<String>,
    pub transcript_status: String,
}

/// Resolve a vault id to (vault path, lexically-safe tasks root). Shared by
/// list/add/toggle so folder resolution lives in one place; the canonical
/// escape check is applied per-command (skip-on-read, error-on-write) since
/// it needs the folder to exist.
fn tasks_root_for(paths: &ServicePaths, id: &str) -> Result<(PathBuf, PathBuf), String> {
    let vault = find_vault(paths, id)?;
    let cfg = capture_config::vault_config(&app_config(paths), id);
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), cfg.tasks_root())?;
    Ok((PathBuf::from(&vault.path), root))
}

/// Read-only list of a vault's tasks. Unknown vault / unsafe folder / missing
/// folder → empty list, never an error (mirrors list_recordings). Never writes.
pub fn list_tasks(paths: &ServicePaths, id: &str) -> Vec<TaskDto> {
    let Ok((vault_path, root)) = tasks_root_for(paths, id) else {
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
/// an empty title; returns the created task so the UI can prepend it. `today`
/// (`YYYY-MM-DD`) is supplied by the caller — no clock in core. `due`,
/// `priority`, and `tags` are written only when present and are assumed
/// ALREADY VALIDATED by the caller's gate (the IPC command validates
/// strictly; a caller passing raw input would write it verbatim).
pub fn add_task(
    paths: &ServicePaths,
    id: &str,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
) -> Result<TaskDto, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A task needs a title.".to_string());
    }
    let (vault_path, root) = tasks_root_for(paths, id)?;
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
    let path = tasks::create_task(&root, title, today, due, priority, tags)
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
    let (vault_path, root) = tasks_root_for(paths, id)?;
    // Mirror list_tasks/add_task: safe_recording_root is only lexical, so
    // canonicalize and reject a tasks folder that resolves outside the vault
    // before writing — keeps the "assert root inside vault before any write"
    // invariant uniform across all three task commands. (Core also
    // canonicalizes root + path and requires containment.)
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
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

/// Number of OPEN tasks (status != "done"; archived already excluded by
/// list_tasks) in a vault, for the vault-row badge. Unknown vault / unsafe or
/// missing folder / escape → 0, never an error (mirrors list_tasks). Read-only.
pub fn count_open_tasks(paths: &ServicePaths, id: &str) -> usize {
    let Ok((vault_path, root)) = tasks_root_for(paths, id) else {
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

/// Read-only list of a vault's past recordings for the Recordings view.
/// Scans the vault's recording roots (custom folder, or both mode defaults)
/// and reads each recording's companion note for type/duration. An unknown
/// vault or unreadable roots yield an empty list — never an error (mirrors
/// discovery's degrade-to-empty rule). Never writes into the vault.
pub fn list_recordings(paths: &ServicePaths, id: &str) -> Vec<RecordingDto> {
    let Ok(vault) = find_vault(paths, id) else {
        return Vec::new();
    };
    let cfg = capture_config::vault_config(&app_config(paths), id);
    // No swallowed error: a rejected (unsafe) folder is skipped WITH a warning,
    // matching run_recovery/scan_and_enqueue — a silent filter_map would hide it.
    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in cfg.recording_roots() {
        let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), folder) else {
            log::warn!("list_recordings: skipping unsafe recording folder {folder:?}");
            continue;
        };
        // Canonicalize before scanning: safe_recording_root is only lexical,
        // so a recording folder that is a symlink/junction out of the vault
        // would otherwise be scanned — enumerating capture MP3s and reading
        // companion-note frontmatter outside the vault. Same read guard as
        // list_tasks (and recovery asserts the same before its scan): a
        // merely missing root stays pushed — scanning it yields nothing — an
        // escape is warned and skipped, never failing the whole call.
        if root.exists() {
            if let Err(e) = capture_paths::assert_root_inside_vault(Path::new(&vault.path), &root) {
                log::warn!("list_recordings: recording folder resolves outside the vault: {e}");
                continue;
            }
        }
        roots.push(root);
    }
    recordings::list_recordings(&roots)
        .into_iter()
        .map(|e| RecordingDto {
            mp3: e.mp3_path.to_string_lossy().into_owned(),
            title: e.title,
            recorded_at: e.recorded_at,
            duration: e.duration,
            recording_type: e.recording_type,
            transcript_status: e.transcript_status.as_dto_str().to_string(),
        })
        .collect()
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
        let created = open_daily_note(&paths, "deadbeef01234567", date(), false, &launch).unwrap();
        assert!(!created, "opening an existing note is not a create");
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
        let created = open_daily_note(&paths, "deadbeef01234567", date(), true, &launch).unwrap();
        assert!(created, "the create branch must report created=true");
        assert!(launched.borrow()[0].starts_with("obsidian://new?"));
    }

    #[test]
    fn add_list_and_toggle_tasks_through_the_service() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        let created = add_task(
            &paths,
            "deadbeef01234567",
            "Buy milk",
            "2026-07-09",
            None,
            None,
            &[],
        )
        .unwrap();
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
        assert!(add_task(
            &paths,
            "deadbeef01234567",
            "   ",
            "2026-07-09",
            None,
            None,
            &[]
        )
        .is_err());
        // The spec requires MCP/IPC failures to carry the same user-facing
        // message the panel shows ("was it removed from Obsidian?"), not a
        // terse internal one that leaks only a raw hex id.
        let err = add_task(&paths, "unknown", "x", "2026-07-09", None, None, &[])
            .err()
            .expect("unknown vault must fail");
        assert!(err.contains("was it removed from Obsidian?"), "got: {err}");
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
        assert!(add_task(
            &paths,
            "deadbeef01234567",
            "x",
            "2026-07-09",
            None,
            None,
            &[]
        )
        .is_err());
    }

    #[test]
    fn list_recordings_degrades_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _) = fixture(dir.path(), "MyVault");
        assert!(list_recordings(&paths, "deadbeef01234567").is_empty());
        assert!(list_recordings(&paths, "unknown").is_empty());
    }

    // Codex review catch pinned as a test: safe_recording_root is only
    // lexical, so a recording folder that is a symlink out of the vault would
    // be scanned — enumerating capture MP3s and reading companion-note
    // frontmatter OUTSIDE the vault. The read must skip it (warn, degrade to
    // empty), same guard as list_tasks — and this path is MCP-exposed.
    #[cfg(unix)]
    #[test]
    fn list_recordings_skips_a_symlinked_root_outside_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        // A real capture layout OUTSIDE the vault, reachable only through the
        // symlinked default folder <vault>/Meetings.
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("2026").join("07")).unwrap();
        std::fs::write(
            outside
                .join("2026")
                .join("07")
                .join("2026-07-09 1405 Meeting.mp3"),
            "mp3",
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, vault.join("Meetings")).unwrap();
        assert!(list_recordings(&paths, "deadbeef01234567").is_empty());
    }
}
