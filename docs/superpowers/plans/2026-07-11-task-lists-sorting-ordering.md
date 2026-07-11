# Task Lists, Sorting & Manual Ordering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Lists-as-folders for the tasks domain (with a per-vault `defaultList`/`listOrder` settings object), a user-controlled sort selector plus drag-to-reorder persisting an `order` frontmatter number, and the SelectMenu close-on-scroll fix.

**Architecture:** Spec: `docs/superpowers/specs/2026-07-11-task-lists-sorting-ordering-design.md`. Core (pure crate) gains list derivation/enumeration and two new sanctioned writes (list-folder create, in-root task move) on the existing never-clobber machinery; the shell adds four IPC commands (55→59) and widens `add_task`/`update_task`/`get_tasks_config`/`TaskDto`; the frontend restructures `Tasks.vue` behind pure utils (`taskSort`, `taskSections`, `taskOrder`) to stay under the 500-LOC cap and adds the Lists grouping, the sort control, the list pickers, and pointer/keyboard reordering.

**Tech Stack:** Rust (core/services/shell), Vue 3 + Pinia + Vitest (happy-dom, mockIPC), Tailwind 4.

## Global Constraints

- LOC gate: frontend files ≤ 500 nonblank, Rust ≤ 800; `capture_config.rs` grandfathered at exactly 900 — it may NOT grow (Task 4 extracts the MCP section first); `Tasks.vue` is at 474 — extractions land before features.
- Never-clobber: every new vault write uses exclusive-create / `rename_noreplace` + ` (N)` suffix retry; containment asserted canonically before AND after any `create_dir_all` (document-import discipline).
- Read-lenient, write-strict (the tags posture): any folder is a list on read; the buddy only creates validated single-segment list names.
- Sync commands never block: all new commands are async on the blocking pool (GAP-22 class).
- Config parsing stays per-field defensive; `serialize_config` round-trips every field; `set_capture_config` preserves the tasks fields.
- Conventional Commits with house scopes (`feat(tasks)`, `fix(ui)`, `docs(tasks)`, `style(core)` …); regression tests name the failure mode in a comment.
- Run `npx vitest run tests/<file>.test.ts` per frontend task; `cargo test` in the touched crate per Rust task; full gates in Task 14.

---

### Task 1: SelectMenu scroll fix

**Files:**
- Modify: `src/components/SelectMenu.vue:66-99` (listener wiring + handler)
- Test: `tests/select-menu.test.ts`

**Interfaces:**
- Produces: unchanged component API; new behavior — scroll inside popup keeps it open, outside scroll repositions instead of closing.

- [x] **Step 1: Write the failing tests** (append to `tests/select-menu.test.ts`, reusing its mount helpers)

```ts
// The user-reported bug: with enough options the popup itself scrolls
// (max-height ~220px), and the capture-phase window scroll listener closed
// the menu on the popup's OWN scroll — the lower options were unreachable.
it("stays open when the popup's own option list scrolls", async () => { /* open menu, dispatch bubbling scroll Event on the popup ul, expect listbox still present */ });
it("repositions instead of closing when the page behind scrolls", async () => { /* open menu, dispatch scroll on document/body, expect listbox still present */ });
it("still closes on outside pointerdown", async () => { /* unchanged behavior pin */ });
```

- [x] **Step 2: Run to verify the new tests fail** — `npx vitest run tests/select-menu.test.ts`

- [x] **Step 3: Implement** — replace the `closeMenu`-as-scroll-handler with:

```ts
function onWindowScroll(e: Event) {
  // A scroll inside the popup is navigation, not dismissal (the option list
  // overflows at ~220px). A scroll outside moves the trigger — track it.
  if (popupRef.value?.contains(e.target as Node)) return;
  positionPopup();
}
```

`openMenu` adds `window.addEventListener("scroll", onWindowScroll, true)`; `closeMenu`/`onBeforeUnmount` remove the same handler. Resize keeps closing.

- [x] **Step 4: Full file passes** — `npx vitest run tests/select-menu.test.ts` → PASS
- [x] **Step 5: Commit** — `fix(ui): keep SelectMenu open while its list or the page scrolls`

### Task 2: Core — `TaskItem.list` + `TaskItem.order`

**Files:**
- Modify: `src-tauri/core/src/tasks/list.rs` (struct + `collect_task_file` + walk callsite), `src-tauri/core/src/tasks/mod.rs` (exports unchanged)
- Modify: `src-tauri/core/src/services.rs:123-147` (`TaskDto` + `from_item`; `add_task`'s literal DTO gets `list`/`order` in Task 5)

**Interfaces:**
- Produces: `TaskItem { …, list: String, order: Option<f64> }`; `TaskDto { …, list: String, order: Option<f64> }` (camelCase serialize). `list` = parent dir relative to canonical root, `/`-joined, `""` at root.

- [x] **Step 1: Failing tests** (in `list.rs` tests)

```rust
#[test]
fn list_tasks_derives_list_from_subfolder() {
    // root task → "", one level → "work", nested → "work/q3" (always `/`,
    // never the platform separator — the identity crosses IPC).
}
#[test]
fn list_tasks_reads_order_leniently() {
    // order: 1536 → Some(1536.0); order: 1536.5 → Some(1536.5);
    // order: soon / missing → None (unranked, never an error).
}
```

- [x] **Step 2: Run** — `cd src-tauri/core && cargo test tasks::` → FAIL (missing fields)
- [x] **Step 3: Implement** — thread `canon_root` into `collect_task_file`:

```rust
let list = path
    .parent()
    .and_then(|dir| dir.strip_prefix(canon_root).ok())
    .map(|rel| {
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/")
    })
    .unwrap_or_default();
let order = scalar_field(&content, "order").and_then(|v| v.parse::<f64>().ok().filter(|f| f.is_finite()));
```

Widen `TaskDto`/`from_item` in `services.rs` the same way.

- [x] **Step 4: Run** — `cargo test` in `core` → PASS (existing tests updated only where they construct `TaskItem` literally)
- [x] **Step 5: Commit** — `feat(tasks): derive each task's list folder and manual order rank`

### Task 3: Core — `tasks/lists.rs` (enumerate, create, move)

**Files:**
- Create: `src-tauri/core/src/tasks/lists.rs`
- Modify: `src-tauri/core/src/tasks/mod.rs` (add `mod lists; pub use lists::{task_lists, create_task_list, move_task_to_list, is_valid_list_name};`)

**Interfaces:**
- Produces:
  - `pub fn task_lists(root: &Path) -> Vec<String>` — every directory under the canonical root (incl. empty), relative `/`-joined, name-ordered, dot-dirs skipped, symlink-escape and cycles guarded (mirrors `vault_walk`'s dir discipline; missing root → empty).
  - `pub fn is_valid_list_name(name: &str) -> bool` — trimmed non-empty, no `/` or `\`, no leading `.`.
  - `pub fn create_task_list(root: &Path, name: &str) -> Result<String, String>` — validates, `create_dir_all(root)` then asserts, creates `root/name`, re-asserts containment after creation; returns the relative name. Existing folder → `Ok` (idempotent).
  - `pub fn move_task_to_list(root: &Path, path: &Path, list: &str) -> Result<PathBuf, String>` — canonicalize root+path + containment (exactly `update_task_fields`' gate); lexically validate `list` (relative, `Normal`/`CurDir` components only — multi-segment allowed); ensure the target dir with pre/post asserts; same-dir → `Ok(path)`; else `rename_noreplace(src, target)` with the ` (N)` suffix retry loop (N in 2..=99, mirroring `write_note_collision_safe`'s naming), returning the landed path.

- [x] **Step 1: Failing tests** (inline `#[cfg(test)]`), covering:

```text
task_lists: missing root → empty; flat + nested folders (name order,
"a" < "a/b" placement documented by the test); dot-dir skipped; (unix)
symlinked dir out of root not listed; (unix) cycle terminates.
create_task_list: creates + returns name; idempotent on existing;
rejects "", "a/b", "\\", ".hidden", " . "; (unix) rejects when the tasks
root is a symlink out of the vault-shaped tempdir (containment via
canonicalize of root parent — create-side assert).
move_task_to_list: moves root→list and list→list keeping basename;
same-list no-op returns original path; collision lands " (2)" suffix and
leaves the occupant untouched; recreates a deleted list folder; rejects a
source outside root; (unix) rejects a symlinked source escaping root;
rejects list "../x".
```

- [x] **Step 2: Run** — `cargo test tasks::lists` → FAIL (module missing)
- [x] **Step 3: Implement** (shape):

```rust
pub fn task_lists(root: &Path) -> Vec<String> {
    let Ok(canon_root) = std::fs::canonicalize(root) else { return Vec::new(); };
    let mut out = Vec::new();
    let mut walked = std::collections::HashSet::new();
    collect_dirs(&canon_root, &canon_root, &mut walked, &mut out);
    out
}
fn collect_dirs(dir: &Path, canon_root: &Path, walked: &mut HashSet<PathBuf>, out: &mut Vec<String>) {
    if !walked.insert(dir.to_path_buf()) { return; }
    let mut entries = crate::transcript::dir_entries(dir);
    entries.sort_by(|a, b| a.2.cmp(&b.2));
    for (path, ft, name) in entries {
        if !ft.is_dir() || name.starts_with('.') { continue; }
        match std::fs::canonicalize(&path) {
            Ok(child) if child.starts_with(canon_root) => {
                if let Ok(rel) = child.strip_prefix(canon_root) {
                    out.push(rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
                }
                collect_dirs(&child, canon_root, walked, out);
            }
            _ => continue,
        }
    }
}
```

`move_task_to_list` core:

```rust
let canon_root = std::fs::canonicalize(root).map_err(...)?;
let canon_path = std::fs::canonicalize(path).map_err(...)?;
if !canon_path.starts_with(&canon_root) { return Err("Task file is outside the vault's tasks folder".into()); }
// lexical target validation: every component Normal/CurDir (safe_recording_root's rule)
let target_dir = canon_root.join(list); // "" joins to root itself
crate::capture_paths::assert_path_inside_vault(&canon_root, &target_dir)?;   // pre-create
std::fs::create_dir_all(&target_dir).map_err(...)?;
crate::capture_paths::assert_root_inside_vault(&canon_root, &target_dir)?;   // post-create
if target_dir == canon_path.parent()... { return Ok(canon_path); }
// exclusive landing: rename_noreplace + " (N)" suffix retry on AlreadyExists
```

- [x] **Step 4: Run** — `cargo test -p vault_buddy_core` → PASS
- [x] **Step 5: Commit** — `feat(tasks): list-folder enumeration, creation, and the in-root task move`

### Task 4: Config — extract `mcp_config.rs`, add `default_list` + `list_order`

**Files:**
- Create: `src-tauri/core/src/mcp_config.rs` (pure move: `DEFAULT_MCP_PORT`, `McpConfig` + `Default`, `mcp_entry` → `pub(crate) fn entry`, the mcp-specific parse tests)
- Modify: `src-tauri/core/src/capture_config.rs` (re-export `pub use crate::mcp_config::{DEFAULT_MCP_PORT, McpConfig};` so every caller keeps compiling; new fields), `src-tauri/core/src/lib.rs` (`pub mod mcp_config;`)

**Interfaces:**
- Produces: `VaultCaptureConfig { …, default_list: Option<String>, list_order: Vec<String> }`; JSON keys `defaultList` (string) / `listOrder` (array of strings; non-strings dropped); both serialized only when non-default; preserved by `set_capture_config` (shell side untouched — it read-modify-writes the whole struct, add the regression test anyway).

- [x] **Step 1: Move first (no behavior change)** — extract the MCP block + its tests; run `cargo test -p vault_buddy_core` → PASS; `git commit -m "style(core): split McpConfig out of capture_config (LOC headroom for lists)"`
- [x] **Step 2: Failing tests** (capture_config tests)

```rust
#[test]
fn vault_entry_reads_default_list_and_list_order_defensively() {
    // {"defaultList":"Inbox","listOrder":["Next",5,"Waiting"]} → Some("Inbox"), ["Next","Waiting"]
    // malformed types default only themselves
}
#[test]
fn serialize_round_trips_lists_settings() {
    // serialize → parse → equal; absent when default (file stays minimal)
}
#[test]
fn capture_save_preserves_lists_settings() {
    // update_vault_config with a capture-shaped value keeps default_list/list_order
    // (the set_capture_config clobber class — regression-named)
}
```

- [x] **Step 3: Run** → FAIL; **Step 4: Implement** parse arm:

```rust
default_list: entry.get("defaultList").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
list_order: entry.get("listOrder").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()).unwrap_or_default(),
```

serialize arm (inside the existing per-vault map build): emit `defaultList` when `Some`, `listOrder` when non-empty.

- [x] **Step 5: Run + LOC check** — `cargo test -p vault_buddy_core` → PASS; `node scripts/check-loc.mjs` must pass with capture_config.rs BELOW 900 → run `npm run check:loc -- --update` to shrink the entry; commit baseline in the same commit.
- [x] **Step 6: Commit** — `feat(tasks): per-vault defaultList/listOrder settings object in config.json`

### Task 5: Services — lists plumbing + `add_task` into a list

**Files:**
- Modify: `src-tauri/core/src/services.rs` (new fns + widened `add_task`), `src-tauri/mcp/src/service.rs:392` (call site gains one `None`)

**Interfaces:**
- Produces:
  - `services::add_task(paths, id, title, today, due, priority, tags, list: Option<&str>)` — `None` → config `default_list`; `Some("")` → root; the effective list is lexically validated (reuse `safe_recording_root(vault, cfg.tasks_root()/<list>)` shape: validate by joining and re-running the lexical gate), the task is created in `root/<list>` (containment pre/post via existing asserts + `create_dir_all`), and the returned `TaskDto` carries `list` + `order: None`.
  - `services::list_task_lists(paths, id) -> Vec<String>` — resolve root like `list_tasks` (best-effort empty; canonical escape warned + empty).
  - `services::create_task_list(paths, id, name) -> Result<String, String>` — vault must exist on disk (the add_task missing-vault guard), root resolved + `assert_path_inside_vault`, then `tasks::create_task_list`.
  - `services::move_task_to_list(paths, id, path, list) -> Result<String, String>` — root resolved, `assert_root_inside_vault` when it exists, then `tasks::move_task_to_list`, returning the landed path as `String`.

- [x] **Step 1: Failing tests** (services tests, tempdir vault + registry fixture already exist in the file):
  add-into-list (file lands under `Tasks/Inbox/`, DTO `list == "Inbox"`), add honors config default when `None`, add `Some("")` overrides a configured default back to root, list_task_lists best-effort, move returns the landed path, create validates.
- [x] **Step 2: Run** → FAIL; **Step 3: Implement**; MCP call site: `services::add_task(&deps.paths, &vault_id, &title, &today, None, None, &[], None)`.
- [x] **Step 4: Run** — `cargo test -p vault_buddy_core && cargo test -p vault_buddy_mcp` → PASS
- [x] **Step 5: Commit** — `feat(tasks): services for list enumeration/create/move; add_task lands in a list`

### Task 6: Shell IPC — four commands + widened surface

**Files:**
- Modify: `src-tauri/src/task_commands.rs`, `src-tauri/src/lib.rs` (generate_handler 55→59)

**Interfaces:**
- Produces (all async on `spawn_blocking`, mirroring `list_tasks`/`add_task`):

```rust
#[tauri::command] pub async fn list_task_lists(id: String) -> Vec<String>
#[tauri::command] pub async fn create_task_list(id: String, name: String) -> Result<String, String>
#[tauri::command] pub async fn move_task_to_list(id: String, path: String, list: String) -> Result<String, String>
#[tauri::command] pub async fn set_task_lists_config(lock: State<ConfigWriteLock>, id: String, default_list: Option<String>, list_order: Vec<String>) -> Result<(), String>
```

  - `add_task` gains `list: Option<String>` (passed through; cheap validation stays inline: reject a `list` containing `..` early for a friendly message — services re-validates).
  - `update_task`'s `TaskPatchDto` gains `order: Option<f64>`; non-finite → `Err("Task order must be a finite number")`; pushed as `("order", Some(format!("{v}")))` (Rust f64 Display is shortest-round-trip: `1536` not `1536.0`).
  - `get_tasks_config` returns `TasksConfigDto { tasks_folder, default_list, list_order }`; `set_tasks_config` (single-field) now also re-reads under the lock so it can't clobber the new fields (it already read-modify-writes the whole `VaultCaptureConfig` — pin with a test-comment, behavior already correct).
  - `set_task_lists_config` validates `default_list` lexically inside the vault (same join gate as `set_tasks_config` validates the folder), trims/drops empty `list_order` entries, read-modify-writes under `ConfigWriteLock`.

- [x] **Steps:** failing shell tests where testable without Tauri runtime (the async-signature `is_future` pin extends to the new commands; validation helpers unit-tested) → implement → `cargo test -p vault-buddy --lib` needs the Linux GUI libs, so for THIS task run `cd src-tauri && cargo check` after `npm run setup:linux` (once) — full shell tests run in Task 14's gate pass.
- [x] **Commit** — `feat(tasks): IPC for task lists (enumerate/create/move/settings) and order writes`

### Task 7: Frontend — types + `taskSort.ts` (pure extraction + factory)

**Files:**
- Create: `src/utils/taskSort.ts`
- Modify: `src/types.ts` (`TaskItem` gains `list: string; order: number | null;`; `TaskPatch` gains `order?: number`; `TasksConfig` gains `defaultList: string | null; listOrder: string[];`), `src/components/Tasks.vue` (replace inline comparator with the factory), `tests/tasks.test.ts` (fixtures gain `list: "", order: null`)
- Test: `tests/task-sort.test.ts`

**Interfaces:**
- Produces:

```ts
export type SortKey = "default" | "due" | "priority" | "created" | "title" | "manual";
export type SortDir = "asc" | "desc";
export interface TaskSortPref { key: SortKey; dir: SortDir }
export const NATURAL_DIR: Record<SortKey, SortDir>; // due/priority/title asc, created desc, default/manual asc (unused)
export function taskComparator(pref: TaskSortPref): (a: AggTask, b: AggTask) => number;
export function loadSortPref(viewKey: string): TaskSortPref;   // localStorage "vault-buddy:task-sort", defensive parse
export function saveSortPref(viewKey: string, pref: TaskSortPref): void;
export function directionApplies(key: SortKey): boolean;        // false for default/manual
```

Comparator contract: done-last is universal; `default` is byte-identical to today's chain (dueKey → priority rank → created desc → title → vaultName → path; done: created desc → title → vaultName → path); explicit keys compare their field first (absent due/order always last regardless of dir; priority absent = normal middle; dir flips only the field comparison) then fall through to the default open-chain; `manual` = ranked by `order` asc first, unranked after (default chain among unranked).

- [x] **Step 1: Failing tests** — `tests/task-sort.test.ts` with a fixture list pinning: default-equals-today (copy the exact expected order from an existing tasks.test.ts sort expectation), due asc/desc with no-due always last, priority natural, created desc natural, title asc, manual ranked-then-unranked, persistence round-trip + garbage localStorage → default.
- [x] **Step 2: Run** → FAIL; **Step 3: Implement + swap Tasks.vue's `sortInPlace` to `tasks.value.sort(taskComparator(pref))` with a hardcoded `{key:"default"}` pref this task (no UI yet).**
- [x] **Step 4: Run** — `npx vitest run tests/task-sort.test.ts tests/tasks.test.ts` → PASS (existing suite proves Default is bit-identical)
- [x] **Step 5: Commit** — `feat(ui): extract the task comparator into a sort factory with persistence`

### Task 8: Frontend — sort control UI

**Files:**
- Modify: `src/components/Tasks.vue` (grouping row gains the control; pref state + persistence wiring), `tests/tasks.test.ts`

**Interfaces:**
- Consumes: Task 7's factory/persistence. View key = `props.vaultId ?? "all"`.
- Produces: `data-testid="task-sort"` SelectMenu (options Default/Due date/Priority/Created/Title/Manual) + `data-testid="task-sort-dir"` toggle button (disabled for default/manual; aria-label announces direction; ↑/↓ glyph). Changing either re-sorts in place and saves the pref; mount loads it.

- [x] **Steps:** failing tests (select due → order changes accordingly; dir button flips; persisted pref survives remount via localStorage; manual shows ranked-first) → implement → run `tests/tasks.test.ts` → commit `feat(ui): user-selectable task sorting persisted per view`

### Task 9: Frontend — `taskSections.ts` + Lists grouping + lists fan-out

**Files:**
- Create: `src/utils/taskSections.ts` (move the dates/tags bucket builders out of `Tasks.vue`, add lists)
- Modify: `src/components/Tasks.vue` (grouping toggle gains Lists; fetch lists; sections via util), `tests/tasks.test.ts`
- Test: `tests/task-sections.test.ts`

**Interfaces:**
- Produces:

```ts
export type Bucket = { key: string; label: string | null; tasks: AggTask[] };
export function dateBuckets(tasks: AggTask[], today: string): Bucket[];      // extracted verbatim
export function tagSections(tasks: AggTask[]): Bucket[];                     // extracted verbatim
export function listSections(tasks: AggTask[], knownLists: string[], listOrder: string[], opts: { includeEmpty: boolean }): Bucket[];
```

`listSections`: one section per list — `listOrder` names first (those that exist in `knownLists ∪ task lists`), remainder alphabetical (case-insensitive); merge case-insensitively (first-seen casing labels, tags precedent); then "No list" (open, `list === ""`), then "Done". `includeEmpty: true` in per-vault mode (a fresh list shows as an empty section), `false` in aggregate. Keys `list:<lowered>` / `nolist` / `done`.
- Tasks.vue: `lists = ref(new Map<string, string[]>())` — single-vault: one `list_task_lists` invoke on mount; aggregate: fan out per vault in the existing `Promise.all` (best-effort, `logWarning` only). `listOrder` from `get_tasks_config` (single-vault mount; aggregate uses `[]` — cross-vault order union is YAGNI, alphabetical).

- [x] **Steps:** failing util tests (ordering, merge casing, empty sections, done placement) + component tests (toggle shows Lists; sections render; aggregate merge) → implement → run → commit `feat(tasks): Lists grouping mode over list folders`

### Task 10: Frontend — `TaskListPicker.vue` + composer integration

**Files:**
- Create: `src/components/TaskListPicker.vue`
- Modify: `src/components/TaskComposer.vue` (options row gains the picker), `src/components/Tasks.vue` (lists/config state down, create-list handler, `add()` passes `list`), `tests/tasks.test.ts`
- Test: `tests/task-list-picker.test.ts`

**Interfaces:**
- Produces (presentational, mirrors TaskComposer's no-invoke rule):

```ts
props: { modelValue: string; lists: string[]; busy?: boolean; dataTestid?: string }
emits: { "update:modelValue": [string]; create: [name: string] }
```

Renders a SelectMenu of `[{value:"",label:"No list"}, ...lists, {value:"__new__",label:"New list…"}]`; picking `__new__` swaps to an inline name input (Enter/✓ emits `create`, Escape/✕ restores the previous pick — IME-guarded like the composer title, GAP-31 class).
- Composer: `addList = ref("")`, seeded from a new `defaultList` prop (watch, like the vault default); payload gains `list: addList.value`; reset keeps the pick (like the vault). Aggregate: `lists`/`defaultList` props follow the picked vault; Tasks.vue lazily fetches `get_tasks_config` per vault (cached map).
- Tasks.vue `createList(vaultId, name): Promise<string | null>` invokes `create_task_list`, updates the lists map, toasts on error; the picker's parent sets the model to the returned name.
- `add()` sends `list` (always a string; `""` = root — explicit, overriding any config default) and unshifts the returned DTO (which now carries `list`).

- [x] **Steps:** failing picker tests (options order, No list default, new-list emit + cancel restore) + composer/container tests (payload carries list; default seeds from config; aggregate follows vault; create-list flow selects the new list) → implement → run → commit `feat(tasks): pick a list when adding a task, with inline list creation`

### Task 11: Frontend — editor list move

**Files:**
- Modify: `src/components/TaskEditor.vue` (picker seeded with the row's list; patch carries `list?: string` — a TS-only field, stripped before `update_task`), `src/components/Tasks.vue` (`onEditorSave` orchestrates patch-then-move), `tests/tasks.test.ts`

**Interfaces:**
- Consumes: `move_task_to_list` IPC (Task 6), `TaskListPicker` (Task 10).
- Produces: editor emits `save` with `TaskPatch & { list?: string }` only when the pick differs from `task.list`. Container: (1) strip `list`; (2) non-empty field patch → `update_task` at the OLD path; (3) list change → `move_task_to_list`, then update `task.path` to the returned landed path and `task.list`; busy keyed on the old path for the whole sequence, re-keyed after. A move failure after a successful patch keeps the patch and toasts `Saved fields, but couldn't move to "<list>": <err>` (never silently half-applied). Editor closes on save as today.

- [x] **Steps:** failing tests (move invoked with right args; row re-keys to landed path incl. collision-suffix path; partial-failure keeps patch + toast; same-list → no move call) → implement → run → commit `feat(tasks): move a task between lists from the inline editor`

### Task 12: Frontend — manual ordering (drag + keyboard)

**Files:**
- Create: `src/utils/taskOrder.ts`, `src/composables/useTaskReorder.ts`
- Modify: `src/components/TaskRow.vue` (grip handle slot-side, `reorderable` prop), `src/components/Tasks.vue` (wiring, indicator, writes), `tests/tasks.test.ts`
- Test: `tests/task-order.test.ts`, `tests/task-reorder.test.ts`

**Interfaces:**
- Produces:

```ts
// taskOrder.ts
export const RANK_STEP = 1024;
export function rankBetween(before: number | undefined, after: number | undefined): number | null;
// both → midpoint, null when no representable gap (mid <= before || mid >= after)
// only before → before + RANK_STEP; only after → after - RANK_STEP; neither → RANK_STEP
export function materializeRanks(paths: string[]): Map<string, number>; // i → (i+1)*RANK_STEP
export function planReorder(section: AggTask[], fromIndex: number, toIndex: number):
  | { kind: "single"; order: number }
  | { kind: "materialize"; orders: Map<string, number> } // section in new visual order
  | null; // no-op (same slot)
// "materialize" when either adjacent neighbor at the target slot is unranked, or rankBetween → null
```

```ts
// useTaskReorder.ts — pointer state machine over the rendered rows
export function useTaskReorder(opts: {
  enabled: () => boolean;                      // sort===manual && no filters && !reordering
  commit: (sectionKey: string, fromIndex: number, toIndex: number) => Promise<void>;
}): {
  onHandlePointerDown(e: PointerEvent, sectionKey: string, index: number): void;
  dragState: Ref<{ sectionKey: string; fromIndex: number; toIndex: number } | null>;
  onHandleKeydown(e: KeyboardEvent, sectionKey: string, index: number): void; // ArrowUp/Down → commit(index, index∓1)
}
```

Pointer flow: pointerdown captures the pointer on the handle; pointermove maps `clientY` against the section's row rects (rows carry `data-reorder="<sectionKey>:<index>"`; query within the section list); Escape or pointercancel aborts; pointerup with a changed slot calls `commit`. Drop indicator: rows expose `:class` when `dragState.toIndex` targets them (violet top border); the dragged row dims.
- Tasks.vue `commit`: build the section's task array (same data the render used), `planReorder`; `single` → optimistic `task.order = order`, re-sort, `update_task {order}`, revert + toast on failure (per-path busy); `materialize` → view-level `reordering` flag, optimistic orders, then serialized `update_task` per changed task (each task's own `vaultId`), revert ALL + one toast on any failure.
- TaskRow: grip button (`data-testid="task-drag"`, aria-label `Reorder <title>`; `@pointerdown`/`@keydown` forwarded up) rendered when `reorderable` prop — Tasks.vue passes `sort.key === 'manual' && !filter && !tagFilter`.

- [x] **Steps:** failing `task-order.test.ts` (midpoint, ends, first-rank, no-gap → null, planReorder single/materialize/no-op cases with unranked neighbors) → implement utils → failing `task-reorder.test.ts` + component tests (keyboard ArrowDown writes the right order; pointer drag across two rows commits; disabled when filtered; materialize writes every changed row; failure reverts) → implement → run — `npx vitest run tests/task-order.test.ts tests/task-reorder.test.ts tests/tasks.test.ts` → commit `feat(tasks): drag-to-reorder with a persisted order rank (manual sort)`

### Task 13: Frontend — per-vault lists settings UI

**Files:**
- Create: `src/components/TaskListSettings.vue`
- Modify: `src/components/CaptureSettings.vue` (render it inside the existing Tasks `VaultFolderSetting` section area, below the folder input), `tests/capture-settings.test.ts`
- Test: `tests/task-list-settings.test.ts`

**Interfaces:**
- Consumes: `get_tasks_config` (now `{tasksFolder, defaultList, listOrder}`), `list_task_lists`, `set_task_lists_config`.
- Produces: self-contained component (`props: { vaultId: string }`, own load/save — the McpSettings/DocumentImportSettings precedent, keeping CaptureSettings' growth to an import + tag): Default-list `TaskListPicker` (without the New-list option — pass the existing lists only; a `allowCreate=false` prop on the picker) and a list-order editor (the vault's lists as rows with ↑/↓ buttons; order = `listOrder` first then the rest alphabetical, exactly what the sections render); Save persists `{defaultList, listOrder}` via `set_task_lists_config`, field-level error + saved state (house pattern).

- [x] **Steps:** failing tests (loads + renders lists in effective order; ↑/↓ reorders; save invokes with the right payload; default-list picker seeds from config) → implement → run → commit `feat(tasks): per-vault default list and list order settings`

### Task 14: Docs + full gates + push + PR

**Files:**
- Modify: `AGENTS.md` (IPC table 55→59 + tasks-domain section + sanctioned-writes list + repo map `mcp_config.rs` + frontend-state notes), `CONTEXT.md` (List, Order terms), `docs/DEVELOPMENT.md` (config reference: `defaultList`, `listOrder`), `docs/prds/task-management.md` (folders decision replaces "metadata rather than physical folders"; roadmap tick), `docs/use-cases/aggregated-task-dashboard-and-lists.md` (lists shipped scope), `docs/Gaps.md` (GAP-17 extended to list enumeration; SelectMenu scroll fix noted; accepted residuals)

- [x] **Step 1: Docs edits** (each doc mirrors what shipped — reconcile, don't aspire)
- [x] **Step 2: Full frontend gates in CI order** — `npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage` (no `coverage/` dir before `check:quality`; `--update` any baseline my change legitimately shrinks, with justification)
- [x] **Step 3: Rust gates** — `cd src-tauri && cargo fmt --check && cargo machete .`; clippy `-D warnings` + tests for `core` and `mcp`; `npx tauri build --no-bundle` (shell compile gate) then `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test -p vault-buddy --lib`
- [x] **Step 4: `npm run build`** (vue-tsc) → PASS
- [x] **Step 5: Commit docs** — `docs(tasks): reconcile AGENTS/PRD/use-cases/Gaps with lists + sorting`
- [x] **Step 6: Push** — `git push -u origin claude/task-lists-sorting-vault-crms0i` (retry w/ backoff on network failure)
- [x] **Step 7: Open the PR** (ready for review) summarizing the three workstreams, gate results, and the spec/plan paths; subscribe to PR activity.

## Self-Review

- **Spec coverage:** lists model → T2/T3/T5; settings object → T4/T6/T13; composer/editor → T10/T11; grouping/aggregate merge → T9; sort → T7/T8; manual order → T12; SelectMenu → T1; MCP delta → T5; docs → T14. Rows-gain-no-list-chip: no task (deliberate, spec Out-of-scope adjacent). Empty-list sections per-vault → T9 `includeEmpty`.
- **Type consistency:** `list: String`/`""`-root everywhere; `order: Option<f64>` ↔ `number | null`; landed-path returns are `String` absolute paths; `TaskPatch.order?: number`; picker sentinel `__new__` only inside TaskListPicker.
- **Placeholder scan:** none — every step names its files, tests, and commands.
