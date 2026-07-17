# Tasks Polish — List Lifecycle, Copy-ID, Drag-to-Move, Grouping Persistence — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add list rename/delete/archive, a copyable task-ID in the editor, drag-a-task-between-lists, per-view grouping persistence, and remove drag grips from the aggregate view.

**Architecture:** Backend-first (core `tasks::lists` gains rename/delete; a per-vault `archived_lists` config field; `list_tasks` surfaces the id), then frontend (section-header menu, archived filtering, copy-ID, cross-list drag, grouping persistence, aggregate-no-grips). Every new vault write rides the existing containment + never-clobber rails.

**Tech Stack:** Rust (Tauri v2 shell + pure `core` crate), Vue 3 + Pinia + Tailwind, Vitest.

## Global Constraints

- **Spec:** `docs/superpowers/specs/2026-07-16-tasks-polish-lists-lifecycle-copy-id-drag-to-move-design.md`. Every task implements part of it.
- **TDD:** failing test first, minimal code, regression tests name the failure mode.
- **Commits:** Conventional Commits (`feat(tasks)`, `feat(ui)`, `fix(ui)`, `docs(tasks)`, …), imperative subject, body explains the *why*. Signing is configured — just `git commit`.
- **Delete semantics:** delete moves a list's own `type: Task` files to *No list* (the tasks root), then removes the folder ONLY if empty; a folder still holding sub-lists or foreign files is kept + reported. Foreign files are NEVER moved or deleted.
- **Rename:** single-segment new name at the same parent; **refuse if the target exists** (never clobber/merge).
- **Archive** = reversibly hide the list via `archived_lists` config; the folder + tasks stay on disk. Archived lists are excluded from Lists grouping (section AND member tasks) and from pickers; their tasks still show under Dates/Tags.
- **Drag is per-vault only** — no grips in the aggregate view (`isAggregate`).
- **Copy-ID is display-only** — `list_tasks` reads the vault's configured id property; the id crosses IPC as `TaskItem.id: string | null` (null when IDs are off).
- **Never grow a baseline silently:** `npm run check:loc` (guards Rust files too), `check:quality`, and coverage floors are shrink-only — ratchet with a justification only for genuine this-branch growth. **Run `check:loc` on every gate pass.**
- **All new writes** ride the existing atomic/never-clobber machinery (`rename_noreplace`, `candidate`, `assert_path_inside_vault`, `assert_root_inside_vault`); no new vault capability.

## File structure

Created:
- `src/utils/taskGrouping.ts` — per-view grouping persistence (mirrors `taskSort.ts`).
- `src/components/TaskSectionMenu.vue` — the ⋯ rename/archive/delete menu for a Lists-view section header.

Modified (Rust):
- `src-tauri/core/src/vault_config.rs` — `archived_lists` field + parse/serialize/preserve.
- `src-tauri/core/src/tasks/lists.rs` — `rename_task_list`, `delete_task_list`, `DeleteListOutcome`.
- `src-tauri/core/src/tasks/mod.rs` — export the two new fns + the outcome.
- `src-tauri/core/src/tasks/list.rs` — `list_tasks` id-property param → `TaskItem.id`.
- `src-tauri/core/src/services.rs` — `rename_task_list`/`delete_task_list` wrappers; `list_tasks` threads the id property; `TaskDto.id`.
- `src-tauri/src/task_commands.rs` — new commands; `TasksConfigDto.archived_lists`; `set_task_lists_config`/`get_tasks_config` carry it.
- `src-tauri/src/capture_config_commands.rs` — preserve `archived_lists`.
- `src-tauri/src/lib.rs` — register `rename_task_list`, `delete_task_list`.

Modified (frontend):
- `src/types.ts` — `TaskItem.id`; `TasksConfig.archivedLists`.
- `src/utils/taskSections.ts` — `listSections` excludes archived; buckets carry the raw list name.
- `src/composables/useTaskLists.ts` — expose the archived set; filter pickers.
- `src/composables/useTaskReorder.ts` — over-section tracking for cross-list drag.
- `src/components/Tasks.vue` — grouping persistence, aggregate-no-grips, section-menu wiring, cross-list-drop decision.
- `src/components/TaskEditor.vue` — copy-ID row.
- `src/components/TaskListSettings.vue` — archived-list management.
- Tests + docs + baselines.

---

### Task 1: `archived_lists` per-vault config field

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs` (struct, `Default`, `vault_entry`, `serialize_vault_entry`, tests)
- Modify: `src-tauri/src/capture_config_commands.rs` (`set_capture_config` preserve block)

**Interfaces:**
- Produces: `VaultCaptureConfig.archived_lists: Vec<String>`; JSON key `archivedLists`.

- [ ] **Step 1: Write the failing test** — add to `vault_config.rs` tests:

```rust
    #[test]
    fn archived_lists_round_trip_and_defensive_parse() {
        let cfg = parse_config(
            r#"{ "vaults": { "a": { "archivedLists": ["Old", 5, "  ", "Done/Q1"] } } }"#,
        );
        assert_eq!(vault_config(&cfg, "a").archived_lists, vec!["Old", "Done/Q1"]);
        let mut c = AppConfig::default();
        c.vaults.insert(
            "a".into(),
            VaultCaptureConfig { archived_lists: vec!["Old".into()], ..VaultCaptureConfig::default() },
        );
        assert_eq!(parse_config(&serialize_config(&c)).vaults, c.vaults);
        let mut d = AppConfig::default();
        d.vaults.insert("b".into(), VaultCaptureConfig::default());
        assert!(!serialize_config(&d).contains("archivedLists")); // omitted when empty
    }
```

- [ ] **Step 2: Run to verify it fails** — `cd src-tauri/core && cargo test archived_lists` → FAIL (`no field archived_lists`).

- [ ] **Step 3: Add the field.** In the struct (after `list_order`):

```rust
    /// `/`-joined relative names of lists hidden from the Lists grouping and
    /// the pickers (the folder + tasks stay on disk). Read-lenient, write-strict.
    pub archived_lists: Vec<String>,
```

`Default`: add `archived_lists: Vec::new(),`. In `vault_entry` (after `list_order`):

```rust
        archived_lists: entry
            .get("archivedLists")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
```

In `serialize_vault_entry` (after the `list_order` block):

```rust
    if !v.archived_lists.is_empty() {
        entry.insert("archivedLists".to_string(), json!(v.archived_lists));
    }
```

Also add `archived_lists: existing.archived_lists,` to the full `config_round_trips…` test construct if present (compiler will flag it).

- [ ] **Step 4: Preserve in `set_capture_config`.** In `capture_config_commands.rs`, after `list_order: existing.list_order,`:

```rust
        archived_lists: existing.archived_lists,
```

- [ ] **Step 5: Run** — `cd src-tauri/core && cargo test` → PASS.

- [ ] **Step 6: Commit**

```bash
cd src-tauri && cargo fmt && git add src-tauri/core/src/vault_config.rs src-tauri/src/capture_config_commands.rs
git commit -m "feat(tasks): add per-vault archivedLists config field"
```

---

### Task 2: `rename_task_list` + `delete_task_list` (core)

**Files:**
- Modify: `src-tauri/core/src/tasks/lists.rs`
- Modify: `src-tauri/core/src/tasks/mod.rs`

**Interfaces:**
- Produces: `rename_task_list(root: &Path, from: &str, to: &str) -> Result<String, String>` (returns the new `/`-joined rel name); `delete_task_list(root: &Path, list: &str) -> Result<DeleteListOutcome, String>` with `pub struct DeleteListOutcome { pub moved: usize, pub folder_removed: bool }`.

- [ ] **Step 1: Write the failing tests** — add to `lists.rs` tests (`TASK`/`write` helpers already exist):

```rust
    #[test]
    fn rename_task_list_moves_folder_and_refuses_existing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Inbox"), "t.md", TASK);
        assert_eq!(rename_task_list(&root, "Inbox", "Later").unwrap(), "Later");
        assert!(root.join("Later").join("t.md").exists());
        assert!(!root.join("Inbox").exists());
        // Same-parent nesting: renames the leaf only.
        write(&root.join("work/q3"), "x.md", TASK);
        assert_eq!(rename_task_list(&root, "work/q3", "q4").unwrap(), "work/q4");
        assert!(root.join("work/q4").join("x.md").exists());
        // Refuse an invalid name and a collision (never clobber).
        assert!(rename_task_list(&root, "Later", "a/b").is_err());
        std::fs::create_dir_all(root.join("Taken")).unwrap();
        assert!(rename_task_list(&root, "Later", "Taken").is_err());
    }

    #[test]
    fn delete_task_list_moves_tasks_to_root_then_removes_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Inbox"), "a.md", TASK);
        write(&root.join("Inbox"), "b.md", TASK);
        let out = delete_task_list(&root, "Inbox").unwrap();
        assert_eq!(out.moved, 2);
        assert!(out.folder_removed);
        assert!(!root.join("Inbox").exists());
        assert!(root.join("a.md").exists() && root.join("b.md").exists());
    }

    #[test]
    fn delete_task_list_keeps_a_folder_with_sublists_or_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        write(&root.join("Proj"), "t.md", TASK);
        write(&root.join("Proj/Sub"), "s.md", TASK); // nested sub-list
        std::fs::write(root.join("Proj").join("notes.txt"), "keep me").unwrap(); // foreign
        let out = delete_task_list(&root, "Proj").unwrap();
        assert_eq!(out.moved, 1); // only Proj's own direct task
        assert!(!out.folder_removed);
        assert!(root.join("Proj").exists()); // kept — not empty
        assert!(root.join("Proj").join("notes.txt").exists()); // foreign untouched
        assert!(root.join("Proj/Sub").join("s.md").exists()); // sub-list untouched
        assert!(root.join("t.md").exists()); // the moved task landed at the root
    }
```

- [ ] **Step 2: Run to verify they fail** — `cd src-tauri/core && cargo test task_list` → FAIL (not defined).

- [ ] **Step 3: Implement.** Add to `lists.rs` (uses the existing `move_task_to_list`, `assert_path_inside_vault`, `assert_root_inside_vault`, `dir_entries`, `normalize_list_rel`, `is_valid_list_name`, `super::doc::is_task`):

```rust
/// Outcome of deleting a list: how many of its own tasks were moved to the
/// tasks root, and whether the (now-empty) folder was removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteListOutcome {
    pub moved: usize,
    pub folder_removed: bool,
}

/// Rename a list folder's leaf to `to` (a single valid segment) at the same
/// parent, moving every contained task with it. Refuses a collision (never
/// clobber). Returns the new `/`-joined relative name.
pub fn rename_task_list(root: &Path, from: &str, to: &str) -> Result<String, String> {
    if !is_valid_list_name(to) {
        return Err(
            "List names need at least one character and cannot contain / or \\ or start with a dot."
                .to_string(),
        );
    }
    let from_rel = normalize_list_rel(from)?;
    if from_rel.is_empty() {
        return Err("The tasks root is not a list and cannot be renamed.".to_string());
    }
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let from_dir = canon_root.join(&from_rel);
    if !from_dir.is_dir() {
        return Err("That list no longer exists — reopen the list to refresh.".to_string());
    }
    // New rel = the from's parent joined with the `to` leaf.
    let parent = Path::new(&from_rel).parent();
    let new_rel = match parent.map(|p| p.to_string_lossy().into_owned()).filter(|p| !p.is_empty()) {
        Some(p) => format!("{p}/{}", to.trim()),
        None => to.trim().to_string(),
    };
    let to_dir = canon_root.join(&new_rel);
    crate::capture_paths::assert_path_inside_vault(&canon_root, &to_dir)?;
    if to_dir.exists() {
        return Err(format!("A list named \"{}\" already exists.", to.trim()));
    }
    std::fs::rename(&from_dir, &to_dir).map_err(|e| format!("Could not rename the list: {e}"))?;
    crate::capture_paths::assert_root_inside_vault(&canon_root, &to_dir)?;
    Ok(new_rel)
}

/// Delete a list: move its OWN direct `type: Task` files to the tasks root
/// (No list), then remove the folder if it is now empty. A folder still
/// holding nested sub-lists or foreign (non-task) files is kept — those are
/// never moved or deleted.
pub fn delete_task_list(root: &Path, list: &str) -> Result<DeleteListOutcome, String> {
    let rel = normalize_list_rel(list)?;
    if rel.is_empty() {
        return Err("The tasks root is not a list and cannot be deleted.".to_string());
    }
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let list_dir = canon_root.join(&rel);
    crate::capture_paths::assert_path_inside_vault(&canon_root, &list_dir)?;
    if !list_dir.is_dir() {
        return Err("That list no longer exists — reopen the list to refresh.".to_string());
    }
    // Collect the direct task files first (don't mutate while iterating).
    let mut task_files: Vec<PathBuf> = Vec::new();
    for (path, ft, name) in crate::transcript::dir_entries(&list_dir) {
        if ft.is_file() && name.ends_with(".md") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if super::doc::is_task(&content) {
                    task_files.push(path);
                }
            }
        }
    }
    let mut moved = 0;
    for f in &task_files {
        move_task_to_list(&canon_root, f, "")?; // to No list; rails already never-clobber
        moved += 1;
    }
    // Remove only if empty; a folder with sub-lists / foreign files stays.
    let folder_removed = std::fs::remove_dir(&list_dir).is_ok();
    Ok(DeleteListOutcome { moved, folder_removed })
}
```

Export in `mod.rs`:

```rust
pub use lists::{
    create_task_list, delete_task_list, is_valid_list_name, move_task_to_list, normalize_list_rel,
    rename_task_list, task_lists, DeleteListOutcome,
};
```

- [ ] **Step 4: Run** — `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings` → PASS.

- [ ] **Step 5: Commit**

```bash
cd src-tauri && cargo fmt && git add src-tauri/core/src/tasks/lists.rs src-tauri/core/src/tasks/mod.rs
git commit -m "feat(tasks): rename and delete list folders (move-to-root delete, never clobber)"
```

---

### Task 3: Services + commands for rename/delete + archivedLists config

**Files:**
- Modify: `src-tauri/core/src/services.rs` (rename/delete wrappers)
- Modify: `src-tauri/src/task_commands.rs` (commands; `TasksConfigDto.archived_lists`; `set_task_lists_config`/`get_tasks_config`)
- Modify: `src-tauri/src/lib.rs` (register the two commands)

**Interfaces:**
- Produces IPC: `rename_task_list(id, from, to) -> Result<String, String>`; `delete_task_list(id, list) -> Result<DeleteListDto, String>` where `DeleteListDto { moved: usize, folder_removed: bool }`; `set_task_lists_config`/`get_tasks_config` carry `archived_lists`.

- [ ] **Step 1: Write the failing tests** — add to `services.rs` tests (using the `fixture` helper):

```rust
    #[test]
    fn rename_and_delete_lists_through_the_service() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        add_task(&paths, "deadbeef01234567", "A", "2026-07-09", None, None, &[], Some("Inbox")).unwrap();
        assert_eq!(rename_task_list(&paths, "deadbeef01234567", "Inbox", "Later").unwrap(), "Later");
        assert!(vault.join("Tasks").join("Later").is_dir());
        let out = delete_task_list(&paths, "deadbeef01234567", "Later").unwrap();
        assert_eq!(out.moved, 1);
        assert!(out.folder_removed);
        assert!(list_tasks(&paths, "deadbeef01234567").iter().all(|t| t.list.is_empty()));
        assert!(rename_task_list(&paths, "unknown", "a", "b").is_err());
    }
```

- [ ] **Step 2: Run to verify it fails** — `cd src-tauri/core && cargo test rename_and_delete` → FAIL.

- [ ] **Step 3: Add services wrappers** (`services.rs`, near `move_task_to_list`):

```rust
/// Rename a list folder (see `tasks::rename_task_list`). Adds the vault-level
/// root assert every list write shares. Returns the new relative list name.
pub fn rename_task_list(paths: &ServicePaths, id: &str, from: &str, to: &str) -> Result<String, String> {
    let (vault_path, root) = tasks_root_for(paths, id)?;
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
    tasks::rename_task_list(&root, from, to)
}

/// Delete a list folder (see `tasks::delete_task_list`). Returns the outcome.
pub fn delete_task_list(
    paths: &ServicePaths,
    id: &str,
    list: &str,
) -> Result<tasks::DeleteListOutcome, String> {
    let (vault_path, root) = tasks_root_for(paths, id)?;
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
    tasks::delete_task_list(&root, list)
}
```

- [ ] **Step 4: Add the commands + extend the DTO** (`task_commands.rs`). Add `archived_lists` to `TasksConfigDto`:

```rust
    pub archived_lists: Vec<String>,
```

`get_tasks_config`: add `archived_lists: cfg.archived_lists,`. Extend `set_task_lists_config`'s signature with `archived_lists: Vec<String>`, normalize each (same `normalize_list_rel`, drop empties) and set `value.archived_lists = ...`. Add the commands:

```rust
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

#[tauri::command]
pub async fn delete_task_list(id: String, list: String) -> Result<DeleteListDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        services::delete_task_list(&ServicePaths::real(), &id, &list)
            .map(|o| DeleteListDto { moved: o.moved, folder_removed: o.folder_removed })
    })
    .await
    .map_err(|e| format!("delete_task_list: task failed: {e}"))?
}
```

Register in `lib.rs` after `set_task_lists_config`: `task_commands::rename_task_list, task_commands::delete_task_list,`. Update the `set_task_lists_config` call sites/tests that pass its args (the frontend passes `archivedLists` in Task 10).

- [ ] **Step 5: Run** — `cd src-tauri/core && cargo test`; `cd src-tauri && cargo build -p vault-buddy && cargo clippy -p vault-buddy --all-targets -- -D warnings && cargo test -p vault-buddy --lib`. PASS.

- [ ] **Step 6: Commit**

```bash
cd src-tauri && cargo fmt && git add src-tauri/core/src/services.rs src-tauri/src/task_commands.rs src-tauri/src/lib.rs
git commit -m "feat(tasks): rename/delete list commands and archivedLists config plumbing"
```

---

### Task 4: `list_tasks` surfaces the task id

**Files:**
- Modify: `src-tauri/core/src/tasks/list.rs` (`list_tasks` param + `TaskItem.id`)
- Modify: `src-tauri/core/src/services.rs` (`TaskDto.id`; thread the property)
- Modify: `src/types.ts` (`TaskItem.id`)

**Interfaces:**
- Produces: `TaskItem.id: Option<String>`; `list_tasks(root: &Path, id_property: Option<&str>) -> Vec<TaskItem>`; `TaskDto.id`; TS `TaskItem.id: string | null`.

- [ ] **Step 1: Write the failing test** (`list.rs` tests):

```rust
    #[test]
    fn list_tasks_reads_the_configured_id_property_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "t.md", "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\ntask-id: abc12345\n---\n");
        assert_eq!(list_tasks(root, Some("task-id"))[0].id.as_deref(), Some("abc12345"));
        assert_eq!(list_tasks(root, None)[0].id, None); // off → no read
    }
```

- [ ] **Step 2: Run to verify it fails** — `cd src-tauri/core && cargo test list_tasks_reads_the_configured_id` → FAIL (arity + no field).

- [ ] **Step 3: Implement.** Add `pub id: Option<String>,` to `TaskItem`. Change `list_tasks` to `pub fn list_tasks(root: &Path, id_property: Option<&str>) -> Vec<TaskItem>` and thread `id_property` into `collect_task_file` (add a param); in it, `let id = id_property.and_then(|p| scalar_field(&content, p));` and set `id` on the pushed `TaskItem`. Update all in-crate `list_tasks(root)` callers (this file's tests, and `count_open_tasks` in services) to pass `None` except where the id is wanted.

- [ ] **Step 4: Thread through services.** In `services.rs`: add `pub id: Option<String>,` to `TaskDto` and `id: t.id,` in `from_item`. In `list_tasks`, compute the property and pass it:

```rust
    let id_property = cfg.task_id_enabled.then(|| cfg.task_id_property_name().to_string());
    tasks::list_tasks(&root, id_property.as_deref())
        .into_iter()
        .map(TaskDto::from_item)
        .collect()
```

(`cfg` is already loaded via `tasks_root_for`/`app_config`; load it in `list_tasks` — currently it uses `tasks_root_for`; add `let cfg = capture_config::vault_config(&app_config(paths), id);`.) `count_open_tasks` passes `None`.

- [ ] **Step 5: TS type.** In `src/types.ts`, add to `TaskItem`: `/** The generated id under the vault's configured property; null when IDs are off. */ id: string | null;`. Update the test fixtures in `tests/helpers/taskMount.ts` (`aggTask`, `sample`, `many`) and any inline TaskItem literals to include `id: null` (the compiler + failing tests will list them).

- [ ] **Step 6: Run** — `cd src-tauri/core && cargo test`; `cargo test -p vault_buddy_mcp` (shares TaskDto); `npm run build` (TS). PASS.

- [ ] **Step 7: Commit**

```bash
cd src-tauri && cargo fmt && git add src-tauri/core/src/tasks/list.rs src-tauri/core/src/services.rs src/types.ts tests/helpers/taskMount.ts
git commit -m "feat(tasks): surface the task id on TaskItem/TaskDto (display-only)"
```

---

### Task 5: Persist the grouping choice per view

**Files:**
- Create: `src/utils/taskGrouping.ts`
- Modify: `src/components/Tasks.vue`
- Test: `tests/task-grouping.test.ts`

**Interfaces:**
- Produces: `loadGrouping(viewKey: string): Grouping`; `saveGrouping(viewKey: string, value: Grouping): void`; `type Grouping = "dates" | "tags" | "lists"`.

- [ ] **Step 1: Write the failing test** (`tests/task-grouping.test.ts`):

```ts
import { afterEach, describe, expect, it } from "vitest";
import { loadGrouping, saveGrouping } from "../src/utils/taskGrouping";

afterEach(() => localStorage.clear());

describe("taskGrouping", () => {
  it("defaults to lists and round-trips per view", () => {
    expect(loadGrouping("v1")).toBe("lists");
    saveGrouping("v1", "dates");
    saveGrouping("all", "tags");
    expect(loadGrouping("v1")).toBe("dates");
    expect(loadGrouping("all")).toBe("tags");
    expect(loadGrouping("v2")).toBe("lists"); // unset
  });
  it("degrades a corrupt value to lists", () => {
    localStorage.setItem("vault-buddy:task-grouping", "not json");
    expect(loadGrouping("v1")).toBe("lists");
  });
});
```

- [ ] **Step 2: Run to verify it fails** — `npx vitest run tests/task-grouping.test.ts` → FAIL (module missing).

- [ ] **Step 3: Implement** `src/utils/taskGrouping.ts` (mirror `taskSort.ts`'s load/save shape):

```ts
import { logWarning } from "../logging";

export type Grouping = "dates" | "tags" | "lists";
const STORAGE_KEY = "vault-buddy:task-grouping";
const DEFAULT: Grouping = "lists";
const VALID = new Set<Grouping>(["dates", "tags", "lists"]);

function readAll(): Record<string, unknown> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) return parsed as Record<string, unknown>;
  } catch (e) {
    logWarning(`task grouping: load failed: ${String(e)}`);
  }
  return {};
}

export function loadGrouping(viewKey: string): Grouping {
  const v = readAll()[viewKey];
  return typeof v === "string" && VALID.has(v as Grouping) ? (v as Grouping) : DEFAULT;
}

export function saveGrouping(viewKey: string, value: Grouping): void {
  const all = readAll();
  all[viewKey] = value;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch (e) {
    logWarning(`task grouping: save failed: ${String(e)}`);
  }
}
```

- [ ] **Step 4: Wire into `Tasks.vue`.** Replace `const grouping = ref<...>("lists")` with a persisted ref keyed by `sortViewKey` (`props.vaultId ?? "all"`, already defined):

```ts
const grouping = ref<Grouping>(loadGrouping(sortViewKey));
watch(grouping, (g) => saveGrouping(sortViewKey, g));
```

(Import `loadGrouping`/`saveGrouping`/`Grouping` and `watch`.)

- [ ] **Step 5: Run** — `npx vitest run tests/task-grouping.test.ts tests/tasks.test.ts` → PASS (update the tasks.test.ts "defaults to lists" assertion is unaffected; grouping still starts on lists when unset).

- [ ] **Step 6: Commit**

```bash
git add src/utils/taskGrouping.ts src/components/Tasks.vue tests/task-grouping.test.ts
git commit -m "feat(ui): persist the tasks grouping choice per view"
```

---

### Task 6: No drag grips in the aggregate view

**Files:**
- Modify: `src/components/Tasks.vue` (`reorderView` computed)
- Modify: `docs/Gaps.md` (GAP-63)
- Test: `tests/tasks.test.ts`

- [ ] **Step 1: Write the failing test** (`tests/tasks.test.ts`, using `mountAggregate`):

```ts
  it("shows no drag grips in the aggregate view", async () => {
    const { wrapper } = mountAggregate();
    await flushPromises();
    expect(wrapper.find('[data-testid="task-grip"]').exists()).toBe(false);
  });
```

(Confirm the grip's testid by reading `TaskRow.vue`/`TaskDragHandle.vue`; adjust the selector to the real one.)

- [ ] **Step 2: Run to verify it fails** — `npx vitest run tests/tasks.test.ts -t "no drag grips"` → FAIL (aggregate defaults to manual sort → grips show).

- [ ] **Step 3: Implement.** In `Tasks.vue`, add `&& !isAggregate.value` to the `reorderView` computed:

```ts
const reorderView = computed(
  () =>
    !isAggregate.value &&
    sortPref.value.key === "manual" &&
    filter.value.trim() === "" &&
    tagFilter.value === null,
);
```

- [ ] **Step 4: Run** — `npx vitest run tests/tasks.test.ts` → PASS.

- [ ] **Step 5: Update GAP-63** — mark its aggregate-reorder bullet resolved (grips are now suppressed in the aggregate view).

- [ ] **Step 6: Commit**

```bash
git add src/components/Tasks.vue docs/Gaps.md tests/tasks.test.ts
git commit -m "fix(ui): drop drag grips in the aggregate tasks view (resolves GAP-63)"
```

---

### Task 7: Copy-ID in the inline editor

**Files:**
- Modify: `src/components/TaskEditor.vue`
- Test: `tests/task-editor.test.ts` (or the editor's existing test file)

**Interfaces:**
- Consumes: `props.task.id` (from Task 4).

- [ ] **Step 1: Write the failing test.** Mount `TaskEditor` with `task.id = "abc12345"`; assert a `[data-testid="task-edit-id"]` shows the id and clicking `[data-testid="task-edit-id-copy"]` calls `navigator.clipboard.writeText` with it; with `task.id = null`, assert the row is absent. (Mock `navigator.clipboard.writeText` with `vi.fn()`.)

- [ ] **Step 2: Run to verify it fails** — FAIL (no id row).

- [ ] **Step 3: Implement.** In `TaskEditor.vue`, add a copy handler:

```ts
async function copyId() {
  if (!props.task.id) return;
  try {
    await navigator.clipboard.writeText(props.task.id);
  } catch (e) {
    // best-effort; a clipboard failure is non-fatal
  }
}
```

Add markup before the Cancel/Save row, shown only when `task.id`:

```html
    <div
      v-if="task.id"
      class="flex items-center gap-1"
    >
      <span class="shrink-0 text-[10px] uppercase tracking-wider text-slate-500">ID</span>
      <code data-testid="task-edit-id" class="min-w-0 flex-1 truncate rounded bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-300">{{ task.id }}</code>
      <button
        type="button"
        data-testid="task-edit-id-copy"
        aria-label="Copy task ID"
        title="Copy ID"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="copyId"
      >
        Copy
      </button>
    </div>
```

- [ ] **Step 4: Run** — the editor test + `npx vitest run tests/tasks.test.ts` → PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/TaskEditor.vue tests/task-editor.test.ts
git commit -m "feat(ui): show and copy a task's id in the inline editor"
```

---

### Task 8: Archived-list filtering (sections + pickers)

**Files:**
- Modify: `src/utils/taskSections.ts` (`listSections` archived arg + bucket carries the list name)
- Modify: `src/composables/useTaskLists.ts` (expose archived set; filter pickers)
- Modify: `src/components/Tasks.vue` (pass archived set to `listSections`)
- Test: `tests/task-sections.test.ts`, `tests/tasks-lists.test.ts`

**Interfaces:**
- Produces: `listSections(tasks, knownLists, listOrder, opts: { includeEmpty: boolean; archived: string[] })`; each list `Bucket` gains `list?: string` (the raw name).

- [ ] **Step 1: Write the failing test** (`tests/task-sections.test.ts`):

```ts
  it("excludes an archived list's section AND its tasks", () => {
    const tasks = [
      { ...base, path: "a", list: "Old", title: "Hidden" },
      { ...base, path: "b", list: "Keep", title: "Shown" },
    ];
    const secs = listSections(tasks as any, ["Old", "Keep"], [], { includeEmpty: true, archived: ["Old"] });
    expect(secs.map((s) => s.label)).not.toContain("Old");
    expect(secs.flatMap((s) => s.tasks.map((t: any) => t.title))).not.toContain("Hidden");
    const keep = secs.find((s) => s.label === "Keep");
    expect(keep?.list).toBe("Keep"); // bucket carries the raw list name
  });
```

(`base` = a minimal AggTask; add `id: null`.)

- [ ] **Step 2: Run to verify it fails** — `npx vitest run tests/task-sections.test.ts` → FAIL (arity + no `list`).

- [ ] **Step 3: Implement `listSections`.** Add `archived: string[]` to `opts`; build a lowercased archived set; skip archived lists in `ensure`/`includeEmpty`; and skip a task whose `t.list` is archived (case-insensitive) rather than bucketing it. Emit `list` on each list bucket:

```ts
export function listSections(
  tasks: AggTask[],
  knownLists: string[],
  listOrder: string[],
  opts: { includeEmpty: boolean; archived: string[] },
): Bucket[] {
  const archived = new Set(opts.archived.map((a) => a.toLowerCase()));
  // ... existing byList map ...
  if (opts.includeEmpty) for (const l of knownLists) if (!archived.has(l.toLowerCase())) ensure(l);
  // ... in the task loop:
  //   else if (t.list === "") nolist.push(t);
  //   else if (archived.has(t.list.toLowerCase())) continue; // hidden with its list
  //   else ensure(t.list).tasks.push(t);
  // ... in the sections map, add `list: label` to each list bucket:
  //   return { key: `list:${key}`, label, list: label, tasks: byList.get(key)?.tasks ?? [] };
}
```

Add `list?: string` to the `Bucket` type.

- [ ] **Step 4: Expose archived in `useTaskLists`** — read `archivedLists` from the vault config (already loaded via `loadVaultConfig`); expose a `computed archivedLists` (per-vault; `[]` in aggregate for simplicity) and filter `listsForVault`/`composerLists` to drop archived names (except a task's own current list in the editor — handled by the editor picker keeping its `modelValue`). Pass the archived set into `Tasks.vue`'s `listSections` call.

- [ ] **Step 5: Wire `Tasks.vue`** — `listSections(filteredTasks.value, knownLists.value, listOrder.value, { includeEmpty: !isAggregate.value && !filterActive.value, archived: archivedLists.value })`.

- [ ] **Step 6: Run** — `npx vitest run tests/task-sections.test.ts tests/tasks.test.ts tests/tasks-lists.test.ts` → PASS.

- [ ] **Step 7: Commit**

```bash
git add src/utils/taskSections.ts src/composables/useTaskLists.ts src/components/Tasks.vue tests/task-sections.test.ts
git commit -m "feat(ui): hide archived lists from the Lists grouping and pickers"
```

---

### Task 9: List section menu (rename / archive / delete)

**Files:**
- Create: `src/components/TaskSectionMenu.vue`
- Modify: `src/components/Tasks.vue` (render the menu on real list sections; wire commands)
- Modify: `src/composables/useTaskLists.ts` (rename/delete/archive actions)
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes: `rename_task_list`, `delete_task_list` commands; `set_task_lists_config` (archive).
- Produces: `TaskSectionMenu.vue` emitting `rename`/`archive`/`delete`.

- [ ] **Step 1: Write the failing tests** (`tests/tasks.test.ts`): in per-vault Lists mode with a list "Inbox", the section header shows a menu (`[data-testid="task-section-menu-Inbox"]`); opening it and clicking Rename → an inline input → confirm calls `rename_task_list` with `{ id:"v1", from:"Inbox", to:"Later" }`; Archive calls `set_task_lists_config` with `archivedLists` including "Inbox"; Delete (after a confirm) calls `delete_task_list` with `{ id:"v1", list:"Inbox" }`. Assert `No list`/`Done` sections have NO menu.

- [ ] **Step 2: Run to verify they fail** — FAIL (no menu).

- [ ] **Step 3: Implement `TaskSectionMenu.vue`** — a presentational ⋯ button that toggles a small popover with Rename / Archive / Delete. Rename opens an inline input (IME-guarded Enter, Escape stopPropagation, mirrors `TaskListPicker`'s create input); Delete asks a confirm (a second click / a confirm sub-state); each emits `rename(name)` / `archive` / `delete`. Props: `list: string`, `busy: boolean`, `resetNonce: number` (close on success, the Task-8/PR-#59 precedent).

- [ ] **Step 4: Add actions in `useTaskLists`:**

```ts
async function renameList(from: string, to: string): Promise<string | null> {
  const id = vaultId; if (id === null) return null;
  try {
    const landed = await invoke<string>("rename_task_list", { id, from, to });
    await loadVaultLists(id); // paths changed
    return landed;
  } catch (e) { notifications.error(String(e)); return null; }
}
async function deleteList(list: string): Promise<boolean> {
  const id = vaultId; if (id === null) return false;
  try {
    await invoke("delete_task_list", { id, list });
    await loadVaultLists(id);
    return true;
  } catch (e) { notifications.error(String(e)); return false; }
}
async function archiveList(list: string): Promise<boolean> {
  const id = vaultId; if (id === null) return false;
  const cfg = /* current tasks config for this vault */;
  const next = [...(cfg?.archivedLists ?? []), list];
  try {
    await invoke("set_task_lists_config", { id, defaultList: cfg?.defaultList ?? null, listOrder: cfg?.listOrder ?? [], archivedLists: next });
    // update the cached config so the section hides immediately
    return true;
  } catch (e) { notifications.error(String(e)); return false; }
}
```

(Adjust to read/update the cached `vaultConfigs` map; after a successful rename/delete, `Tasks.vue` reloads tasks so the moved/renamed rows re-home.)

- [ ] **Step 5: Render in `Tasks.vue`** — for a bucket in Lists grouping with `bucket.list` set and `!isAggregate`, render `<TaskSectionMenu>` in the `<h3>` header; wire `@rename`/`@archive`/`@delete` to the composable actions, then reload tasks (`list_tasks`) on rename/delete success.

- [ ] **Step 6: Run** — `npx vitest run tests/tasks.test.ts` → PASS; `npm run build`.

- [ ] **Step 7: Commit**

```bash
git add src/components/TaskSectionMenu.vue src/components/Tasks.vue src/composables/useTaskLists.ts tests/tasks.test.ts
git commit -m "feat(ui): rename/archive/delete a list from its section header"
```

---

### Task 10: Archived-list management in Vault settings

**Files:**
- Modify: `src/components/TaskListSettings.vue`
- Modify: `src/composables/useAutosave` usage / `set_task_lists_config` call (carry `archivedLists`)
- Test: `tests/task-list-settings.test.ts`

- [ ] **Step 1: Write the failing test** — mount `TaskListSettings` with a config whose `archivedLists: ["Old"]`; assert an "Old" archived row shows with an Unarchive button; clicking it calls `set_task_lists_config` with `archivedLists: []`.

- [ ] **Step 2: Run to verify it fails** — FAIL.

- [ ] **Step 3: Implement.** In `TaskListSettings.vue`, load `archivedLists` from `get_tasks_config`; render a section listing them with an Unarchive button per row; the autosave `set_task_lists_config` call now sends `archivedLists` too (removing the unarchived name). Keep the existing default-list/order UI; thread `archivedLists` through the same save.

- [ ] **Step 4: Run** — `npx vitest run tests/task-list-settings.test.ts tests/tasks-config-tab.test.ts` → PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/TaskListSettings.vue tests/task-list-settings.test.ts
git commit -m "feat(ui): manage archived lists (unarchive) in Vault settings"
```

---

### Task 11: Drag a task between lists

**Files:**
- Modify: `src/composables/useTaskReorder.ts` (over-section tracking)
- Modify: `src/components/Tasks.vue` (section drop zones; move-vs-reorder decision)
- Test: `tests/task-reorder.test.ts`

**Interfaces:**
- Produces: `DragState.overSectionKey: string | null`; `useTaskReorder` opts gain `sectionAt(clientX: number, clientY: number): string | null`; `commit(fromSection, fromIndex, toIndex, overSection)`.

- [ ] **Step 1: Write the failing test** (`tests/task-reorder.test.ts`): in per-vault Lists+Manual mode with lists "A" and "B", simulate a pointer drag of a task from section `list:a` releasing over section `list:b`; assert `move_task_to_list` is called with the task path and `"B"` (the target). A drop within the same section still reorders (existing behavior). (Hit-testing is stubbed via the `sectionAt` opt in the test.)

- [ ] **Step 2: Run to verify it fails** — FAIL.

- [ ] **Step 3: Implement.** Extend `useTaskReorder`: add `overSectionKey` to `DragState`, initialize to the origin, update it in `onMove` via `opts.sectionAt(ev.clientX, ev.clientY)`; change `commit` to `commit(sectionKey, fromIndex, toIndex, overSectionKey)`. In `Tasks.vue`, supply `sectionAt` by hit-testing rendered section containers (add `data-section-key` to each section wrapper) and rewrite `commitReorder`:

```ts
async function commitReorder(sectionKey: string, fromIndex: number, toIndex: number, overSectionKey: string | null) {
  const section = buckets.value.find((b) => b.key === sectionKey);
  const task = section?.tasks[fromIndex];
  const over = buckets.value.find((b) => b.key === overSectionKey);
  // Cross-list move: dropped on a DIFFERENT list section (Lists grouping only).
  if (task && over && over.key !== sectionKey && over.list !== undefined && grouping.value === "lists") {
    await moveTaskToList(task, over.list); // move_task_to_list; optimistic + revert
    return;
  }
  // ... existing within-section reorder (planReorder etc.) ...
}
```

Add a `moveTaskToList(task, list)` helper (optimistic list change + `invoke("move_task_to_list", { id: task.vaultId, path: task.path, list })`, adopt the landed path, revert + toast on failure). Cross-list move stays Lists-grouping + per-vault (grips already suppressed in aggregate by Task 6).

- [ ] **Step 4: Run** — `npx vitest run tests/task-reorder.test.ts tests/tasks.test.ts` → PASS; `npm run build`.

- [ ] **Step 5: Commit**

```bash
git add src/composables/useTaskReorder.ts src/components/Tasks.vue tests/task-reorder.test.ts
git commit -m "feat(ui): drag a task onto another list's section to move it"
```

---

### Task 12: Docs, baselines, and full gate run

**Files:** `AGENTS.md`, `CONTEXT.md`, `docs/Gaps.md`, baselines.

- [ ] **Step 1: AGENTS.md** — tasks domain: the two new list writes (rename/delete + move-to-root/never-clobber semantics), the `archivedLists` config field + archive-hides-the-list semantics, the id surfaced on `TaskItem`, the section-header menu, drag-to-move, per-view grouping persistence, aggregate-no-grips. Add `rename_task_list`/`delete_task_list` to the IPC surface table and bump the command count (verify against `lib.rs`'s `generate_handler!`).
- [ ] **Step 2: CONTEXT.md** — a List can be archived (hidden); "delete a list" preserves its tasks (moves them to No list).
- [ ] **Step 3: docs/Gaps.md** — record any residual (rename is single-segment/same-parent; delete keeps a foreign-holding folder; archived lists' tasks still show under Dates/Tags).
- [ ] **Step 4: Full gate run** (include `check:loc`, which guards Rust files too):

```bash
npm run lint && npm run check:loc && npm run check:quality && npm run build && npm run test:coverage
cd src-tauri && cargo fmt --check && (cd core && cargo clippy --all-targets -- -D warnings && cargo test) && cargo test -p vault_buddy_mcp && cargo machete . && cargo build -p vault-buddy && cargo clippy -p vault-buddy --all-targets -- -D warnings && cargo test -p vault-buddy --lib && cargo llvm-cov -p vault_buddy_core --fail-under-lines 94
```

Ratchet any baseline (`loc-baseline.json`, `quality-baseline.json`, coverage floors) only for genuine this-branch growth, with a justification.

- [ ] **Step 5: Commit**

```bash
git add AGENTS.md CONTEXT.md docs/Gaps.md scripts/loc-baseline.json scripts/quality-baseline.json vite.config.ts
git commit -m "docs(tasks): document list lifecycle, copy-id, drag-to-move, grouping persistence"
```

---

## Self-Review

**Spec coverage:** List rename/delete/archive → Tasks 1-3, 8-10. Copy-ID → Tasks 4, 7. Cross-list drag → Task 11. Grouping persistence → Task 5. Aggregate no-grips → Task 6. Docs/baselines → Task 12. ✓

**Placeholder scan:** Steps show exact signatures, code, and commands. Task 9's `archiveList` reads/updates the cached config — the implementer wires it to `useTaskLists`'s `vaultConfigs` map (named there); Task 8's picker-filtering and Task 11's `sectionAt` hit-testing are described with the exact DOM hooks (`data-section-key`). The one grip-testid in Task 6 is confirmed against `TaskDragHandle.vue` at implementation time (noted in the step).

**Type consistency:** `archived_lists`/`archivedLists`, `DeleteListOutcome`/`DeleteListDto { moved, folder_removed }`, `rename_task_list(id, from, to) -> String`, `delete_task_list(id, list) -> DeleteListDto`, `TaskItem.id`/`id: string | null`, `listSections(..., { includeEmpty, archived })` with `Bucket.list`, and `commit(fromSection, fromIndex, toIndex, overSection)` are used consistently across the tasks that define and consume them.
