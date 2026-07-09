# Tasks Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an open-task counter badge to the vault-row Tasks button, move the tasks-folder setting into the per-vault settings view (relabeled "Vault settings"), add a `status: archived` value + a per-row Archive action that hides a task, and put a progress bar atop the Tasks view.

**Architecture:** Core gains a status-string toggle and an archived-exclusion in `list_tasks`; the shell exposes the string toggle + a `count_open_tasks` command; the frontend reworks `Tasks.vue` (progress bar, archive, string toggle, no folder input), adds a store-driven counter badge on `VaultList`, and relocates the tasks-folder field into `CaptureSettings.vue`.

**Tech Stack:** Rust (`vault_buddy_core` + Tauri shell), Vue 3 + Pinia + Tailwind 4, Vitest (`mockIPC`), `cargo test`.

## Global Constraints

- **Status values:** `new` (open), `done` (completed), `archived` (hidden). "Open" ≡ `status != "done" && status != "archived"`. Archived is never surfaced (no show-archived view this slice).
- **Frontmatter writes stay surgical:** the toggle still routes through `core::tasks::set_status` → `set_task_status` (only the `status:` line changes; canonicalize+containment+atomic-replace unchanged).
- **Rust↔TS contract:** `set_task_status` arg is `{ id, path, status }` (a string); `count_open_tasks` arg is `{ id }` and returns a number. TS `TaskItem.status` is a string.
- **The shell crate (`src-tauri/src/*.rs`) does not compile on Linux.** Mirror existing command patterns, run `cd src-tauri && cargo fmt --check`, rely on CI's `windows-app` job. Core + frontend build and test locally.
- **Commits:** Conventional Commits (`feat(tasks)`, `feat(ui)`, `refactor(tasks)`). Imperative subject; body explains the *why*. Git author must be `Claude <noreply@anthropic.com>` — run `git config user.email noreply@anthropic.com && git config user.name Claude` before committing if needed.
- **TDD:** failing test first, then implementation. Regression tests name their failure mode in a comment.
- Spec: `docs/superpowers/specs/2026-07-09-tasks-polish-design.md`.

---

### Task 1: Core — status-string toggle + exclude archived from the list

**Files:**
- Modify: `src-tauri/core/src/tasks.rs` (`set_task_status` ~248; `collect_tasks` ~120)
- Test: `src-tauri/core/src/tasks.rs` (`mod tests`)

**Interfaces:**
- Produces:
  - `tasks::set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String>` (was `done: bool`).
  - `tasks::list_tasks(root: &Path) -> Vec<TaskItem>` now omits files whose `status` is `"archived"`.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` (the `write(root, name, body)` helper exists):

```rust
#[test]
fn list_tasks_excludes_archived() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "open.md", "---\ntype: Task\nstatus: new\ntitle: \"Open\"\ncreated: 2026-07-08\n---\n");
    write(root, "done.md", "---\ntype: Task\nstatus: done\ntitle: \"Done\"\ncreated: 2026-07-07\n---\n");
    write(root, "arch.md", "---\ntype: Task\nstatus: archived\ntitle: \"Arch\"\ncreated: 2026-07-06\n---\n");
    let titles: Vec<String> = list_tasks(root).into_iter().map(|t| t.title).collect();
    assert_eq!(titles, vec!["Open", "Done"]); // archived is not surfaced
}

#[test]
fn set_task_status_writes_an_arbitrary_status() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    let p = create_task(&root, "Buy milk", "2026-07-08").unwrap();
    set_task_status(&root, &p, "archived").unwrap();
    assert!(std::fs::read_to_string(&p).unwrap().contains("status: archived\n"));
    set_task_status(&root, &p, "done").unwrap();
    assert!(std::fs::read_to_string(&p).unwrap().contains("status: done\n"));
}
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri/core && cargo test tasks::tests::list_tasks_excludes_archived tasks::tests::set_task_status_writes_an_arbitrary_status`
Expected: FAIL — `set_task_status` still takes `bool` (type error) and `list_tasks` still returns the archived task.

- [ ] **Step 3: Implement**

In `collect_tasks`, after the `is_task` guard and reading `status`, skip archived. Change the file-handling block so it reads `status` before deciding, then skips archived:

```rust
        if !is_task(&content) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
        let title = note_field(&content, "title").unwrap_or(stem);
        let status = note_field(&content, "status").unwrap_or_else(|| "new".to_string());
        // Archived tasks are removed from view — never surfaced in the list.
        if status == "archived" {
            continue;
        }
        let created = note_field(&content, "created").unwrap_or_default();
        let done = status == "done";
        out.push(TaskItem { path, title, status, created, done });
```

Change `set_task_status`'s signature and the status it writes:

```rust
pub fn set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String> {
```
and replace its `set_status(&content, if done { "done" } else { "new" })` call with `set_status(&content, new_status)`. Everything else in the function (canonicalize root+path, containment, atomic replacing write) is unchanged.

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cd src-tauri/core && cargo test tasks:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS. (The existing `set_task_status_writes_and_rejects_escape` and `set_task_status_rejects_symlinked_file_escaping_root` tests call `set_task_status(&root, &p, true)` / `..., true)` — update those two call sites to `"done"` and keep their assertions; the escape test's assertion doesn't depend on the value.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks.rs
git commit -m "feat(tasks): status-string toggle and exclude archived from the list"
```

---

### Task 2: Shell — string toggle + count_open_tasks command

**Files:**
- Modify: `src-tauri/src/task_commands.rs` (`set_task_status` ~161; add `count_open_tasks`)
- Modify: `src-tauri/src/lib.rs` (register `count_open_tasks` in `generate_handler!` ~288-292)

**Interfaces:**
- Consumes: `tasks::set_task_status(root, path, &str)`, `tasks::list_tasks` (Task 1).
- Produces (IPC): `set_task_status(id, path, status: String) -> Result<(), String>`; `count_open_tasks(id: String) -> usize`.

*Shell crate: not compilable on Linux — mirror the existing `list_tasks`/`set_task_status` command idioms and verify with `cargo fmt --check`.*

- [ ] **Step 1: Change `set_task_status` to take a status string**

Replace the command (currently `done: bool`) with:

```rust
/// Set a task's status. `status` must be one of new/done/archived. The path
/// (from list_tasks) is re-validated inside the vault's tasks root by
/// `tasks::set_task_status`.
#[tauri::command]
pub fn set_task_status(id: String, path: String, status: String) -> Result<(), String> {
    if !matches!(status.as_str(), "new" | "done" | "archived") {
        return Err(format!("Unknown task status: {status}"));
    }
    let (_vault_path, root) = tasks_root_for(&id)?;
    tasks::set_task_status(&root, Path::new(&path), &status)
}
```

- [ ] **Step 2: Add `count_open_tasks`**

Add below `list_tasks` in `task_commands.rs`:

```rust
/// Number of OPEN tasks (status != "done"; archived already excluded by
/// list_tasks) in a vault, for the vault-row badge. Unknown vault / unsafe or
/// missing folder / escape → 0, never an error (mirrors list_tasks).
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
```

- [ ] **Step 3: Register the command**

In `src-tauri/src/lib.rs`, after `task_commands::set_task_status,`:

```rust
            task_commands::set_task_status,
            task_commands::count_open_tasks,
```

- [ ] **Step 4: Format check + commit**

Run: `cd src-tauri && cargo fmt --check`
Expected: PASS (no diff). Compile is verified by CI's `windows-app` job.

```bash
git add src-tauri/src/task_commands.rs src-tauri/src/lib.rs
git commit -m "feat(tasks): status-string set_task_status + count_open_tasks command"
```

---

### Task 3: Frontend — Tasks view (progress bar, archive, string toggle, no folder input)

**Files:**
- Modify: `src/components/Tasks.vue`
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes (IPC): `list_tasks {id}`, `add_task {id, title}`, `set_task_status {id, path, status}`. (No longer uses `get_tasks_config`/`set_tasks_config` — those move to Vault settings, Task 5.)

- [ ] **Step 1: Update the tests (TDD)**

In `tests/tasks.test.ts`: the `mountView` mock already handles `set_task_status` (returns null) and `list_tasks`. **Remove** `get_tasks_config`/`set_tasks_config` from the mock only if present in a handler default — leave the `mockIPC` switch returning `undefined` for unhandled commands (harmless). Change the existing toggle test's assertion from `done: true` to `status: "done"`, and add archive + progress + no-folder tests. Replace the `toggles a task via set_task_status` test body and add three tests:

```ts
it("toggles a task via set_task_status with a status string", async () => {
  const { wrapper, calls } = mountView();
  await flushPromises();
  await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
  await flushPromises();
  const call = calls.find((c) => c.cmd === "set_task_status");
  expect(call?.args).toMatchObject({ id: "v1", path: "C:/v/Tasks/2026-07-08-b.md", status: "done" });
});

it("archives a task: sends status archived and removes the row", async () => {
  const { wrapper, calls } = mountView();
  await flushPromises();
  await wrapper.get('[data-testid="task-archive"]').trigger("click"); // first row = "B open"
  await flushPromises();
  const call = calls.find((c) => c.cmd === "set_task_status");
  expect(call?.args).toMatchObject({ id: "v1", path: "C:/v/Tasks/2026-07-08-b.md", status: "archived" });
  expect(wrapper.text()).not.toContain("B open");
});

it("re-inserts the row and notifies when archive fails", async () => {
  const notifications = useNotificationsStore();
  const { wrapper } = mountView({ set_task_status: () => { throw new Error("disk full"); } });
  await flushPromises();
  await wrapper.get('[data-testid="task-archive"]').trigger("click");
  await flushPromises();
  expect(wrapper.text()).toContain("B open"); // restored
  expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
});

it("shows a progress bar of done/total and hides it at zero", async () => {
  const { wrapper } = mountView(); // sample = 1 open + 1 done → 1/2
  await flushPromises();
  const bar = wrapper.get('[data-testid="task-progress"]');
  expect(bar.text()).toContain("1 / 2");
  // Empty vault → no bar.
  const empty = mountView({ list_tasks: () => [] });
  await flushPromises();
  expect(empty.wrapper.find('[data-testid="task-progress"]').exists()).toBe(false);
});

it("no longer renders the tasks-folder input", async () => {
  const { wrapper } = mountView();
  await flushPromises();
  expect(wrapper.find('[data-testid="tasks-folder-input"]').exists()).toBe(false);
});
```

Note the `mountView` helper's `handlers` override already lets a test replace `list_tasks`/`set_task_status`; if `mountView` doesn't forward a `list_tasks` handler override, extend its switch so `if (handlers[cmd]) return handlers[cmd]!(args);` runs before the defaults (it already does per the file).

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — no `task-archive`/`task-progress` elements; toggle still sends `done`.

- [ ] **Step 3: Rework `Tasks.vue`**

Rewrite the `<script setup>` toggle/archive/remove-folder logic and the template. Replace the folder refs/functions and the `toggle` function, and remove `saveFolder`/`reload`/`folder`/`get_tasks_config`/`TasksConfig`:

Script — replace `toggle` and remove folder code; add `archive` and a `progress` computed:

```ts
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { TaskItem } from "../types";

const props = defineProps<{ vaultId: string }>();
const notifications = useNotificationsStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const tasks = ref<TaskItem[]>([]);
const newTitle = ref("");
const adding = ref(false);
// Paths with an in-flight set_task_status write — disable the row's controls
// and drop re-entrant actions so a toggle and an archive can't race.
const busy = ref(new Set<string>());
const isBusy = (path: string) => busy.value.has(path);

// done / total of the visible (non-archived) list; drives the progress bar.
const progress = computed(() => {
  const total = tasks.value.length;
  const done = tasks.value.filter((t) => t.done).length;
  return { total, done, pct: total === 0 ? 0 : Math.round((done / total) * 100) };
});

function sortInPlace() {
  tasks.value.sort(
    (a, b) =>
      Number(a.done) - Number(b.done) ||
      b.created.localeCompare(a.created) ||
      a.title.localeCompare(b.title),
  );
}

onMounted(async () => {
  try {
    tasks.value = await invoke<TaskItem[]>("list_tasks", { id: props.vaultId });
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});

async function add() {
  const title = newTitle.value.trim();
  if (!title || adding.value) return;
  adding.value = true;
  try {
    const created = await invoke<TaskItem>("add_task", { id: props.vaultId, title });
    tasks.value.unshift(created);
    sortInPlace();
    newTitle.value = "";
  } catch (e) {
    notifications.error(String(e));
    logWarning(`add_task failed: ${String(e)}`);
  } finally {
    adding.value = false;
  }
}

async function toggle(task: TaskItem) {
  if (busy.value.has(task.path)) return;
  const done = !task.done;
  task.done = done;
  task.status = done ? "done" : "new";
  sortInPlace();
  busy.value.add(task.path);
  try {
    await invoke("set_task_status", { id: props.vaultId, path: task.path, status: task.status });
  } catch (e) {
    task.done = !done;
    task.status = done ? "new" : "done";
    sortInPlace();
    notifications.error(String(e));
    logWarning(`set_task_status failed: ${String(e)}`);
  } finally {
    busy.value.delete(task.path);
  }
}

async function archive(task: TaskItem) {
  if (busy.value.has(task.path)) return;
  busy.value.add(task.path);
  // Optimistic: remove from the list; re-insert + notify on failure.
  const index = tasks.value.findIndex((t) => t.path === task.path);
  const removed = tasks.value.splice(index, 1)[0];
  try {
    await invoke("set_task_status", { id: props.vaultId, path: task.path, status: "archived" });
  } catch (e) {
    tasks.value.splice(index, 0, removed);
    notifications.error(String(e));
    logWarning(`archive failed: ${String(e)}`);
  } finally {
    busy.value.delete(task.path);
  }
}
```

Template — replace the folder-input block with a progress bar, keep the add-task input, and add the archive button + `:disabled="isBusy(...)"` on the checkbox:

```html
<template>
  <div class="flex flex-col gap-2">
    <div
      v-if="!loading && !loadError && progress.total > 0"
      data-testid="task-progress"
      class="flex items-center gap-2"
    >
      <div class="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-white/10">
        <div class="h-full rounded-full bg-violet-500 transition-all" :style="{ width: `${progress.pct}%` }"></div>
      </div>
      <span class="shrink-0 text-xs tabular-nums text-slate-400">{{ progress.done }} / {{ progress.total }}</span>
    </div>

    <div class="flex items-center gap-1">
      <input
        v-model="newTitle"
        data-testid="task-input"
        type="text"
        placeholder="Add a task…"
        aria-label="New task title"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        @keydown.enter="add"
      />
      <button
        type="button"
        data-testid="task-add"
        :disabled="adding || newTitle.trim() === ''"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
        @click="add"
      >
        Add
      </button>
    </div>

    <p v-if="loading" class="text-xs text-slate-400">Loading…</p>
    <p v-else-if="loadError" class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200">
      {{ loadError }}
    </p>
    <p v-else-if="tasks.length === 0" class="text-xs text-slate-400">No tasks yet.</p>
    <ul v-else class="flex flex-col gap-1">
      <li
        v-for="task in tasks"
        :key="task.path"
        data-testid="task-row"
        class="flex items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1"
      >
        <input
          type="checkbox"
          data-testid="task-checkbox"
          :checked="task.done"
          :disabled="isBusy(task.path)"
          :aria-label="`Mark ${task.title} ${task.done ? 'not done' : 'done'}`"
          class="shrink-0 cursor-pointer accent-violet-500 disabled:cursor-default disabled:opacity-50"
          @change="toggle(task)"
        />
        <span
          class="min-w-0 flex-1 truncate text-sm"
          :class="task.done ? 'text-slate-500 line-through' : 'text-slate-100'"
          :title="task.title"
        >
          {{ task.title }}
        </span>
        <button
          type="button"
          data-testid="task-archive"
          :disabled="isBusy(task.path)"
          :aria-label="`Archive ${task.title}`"
          title="Archive"
          class="shrink-0 cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
          @click="archive(task)"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <rect x="3" y="4" width="18" height="4" rx="1" />
            <path d="M5 8v11a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8M10 12h4" />
          </svg>
        </button>
      </li>
    </ul>
  </div>
</template>
```

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS (all green; `vue-tsc` clean).

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): Tasks view progress bar, archive action, status-string toggle"
```

---

### Task 4: Frontend — open-task counter badge on the vault row

**Files:**
- Modify: `src/stores/vaults.ts` (state + `refresh`)
- Modify: `src/components/ActionPanel.vue` (`<VaultList>` props ~224-233)
- Modify: `src/components/VaultList.vue` (props + Tasks button badge)
- Test: `tests/vaults-store.test.ts`, `tests/vault-list.test.ts`

**Interfaces:**
- Consumes (IPC): `count_open_tasks {id} -> number` (Task 2).
- Produces: `vaults` store `taskCounts: Record<string, number>`; `VaultList` prop `taskCounts`.

- [ ] **Step 1: Write the failing tests**

Add to `tests/vaults-store.test.ts` (mirror its `mockIPC` setup; it already mocks `list_vaults`):

```ts
it("refresh populates taskCounts from count_open_tasks", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "list_vaults") return [{ id: "v1", name: "A", path: "/a", open: false }];
    if (cmd === "count_open_tasks") return (args as { id: string }).id === "v1" ? 3 : 0;
  });
  const store = useVaultsStore();
  await store.refresh();
  expect(store.taskCounts).toEqual({ v1: 3 });
});
```

Add to `tests/vault-list.test.ts`:

```ts
it("shows the open-task badge when the count is > 0", () => {
  const wrapper = mountList({ taskCounts: { v1: 4 } }); // adapt to the file's mount helper
  expect(wrapper.get('[data-testid="task-count"]').text()).toBe("4");
});

it("hides the badge when the open-task count is 0 or missing", () => {
  const wrapper = mountList({ taskCounts: {} });
  expect(wrapper.find('[data-testid="task-count"]').exists()).toBe(false);
});
```

If `tests/vault-list.test.ts` has a shared `mountList(overrideProps)` helper, pass `taskCounts` through it; otherwise mount `VaultList` directly with the existing props plus `taskCounts`, using a single `v1` vault.

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/vaults-store.test.ts tests/vault-list.test.ts`
Expected: FAIL — `taskCounts` undefined; no `task-count` element.

- [ ] **Step 3: Implement the store**

In `src/stores/vaults.ts` state, after `error: null as string | null,` add:

```ts
    // Open-task count per vault id (status new; done/archived excluded), for
    // the vault-row Tasks badge. Refreshed on every panel open.
    taskCounts: {} as Record<string, number>,
```

In `refresh()`, after `await this.loadVaults();` add `await this.loadTaskCounts();`. Add the action (beside `loadVaults`):

```ts
    async loadTaskCounts() {
      // Best-effort, in parallel; a failed/absent count is treated as 0. Replace
      // the map wholesale so a removed vault's stale count can't linger.
      const entries = await Promise.all(
        this.vaults.map(async (v) => {
          try {
            return [v.id, await invoke<number>("count_open_tasks", { id: v.id })] as const;
          } catch {
            return [v.id, 0] as const;
          }
        }),
      );
      this.taskCounts = Object.fromEntries(entries);
    },
```

- [ ] **Step 4: Wire ActionPanel + VaultList**

In `src/components/ActionPanel.vue`, on the `<VaultList>` element add:

```html
        :task-counts="store.taskCounts"
```

In `src/components/VaultList.vue` `defineProps`, add `taskCounts: Record<string, number>;`. On the Tasks button (the `data-testid="open-tasks"` one), make the button `relative` and add a badge; replace the button's opening tag class and insert the badge as the first child inside the button:

```html
        <button
          type="button"
          data-testid="open-tasks"
          class="relative mr-1 shrink-0 cursor-pointer rounded-lg p-1.5 text-slate-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
          :disabled="busyVaultId !== null"
          :aria-label="`Tasks in ${accessibleName(vault)}${(taskCounts[vault.id] ?? 0) > 0 ? ` (${taskCounts[vault.id]} open)` : ''}`"
          title="Tasks"
          @click="$emit('open-tasks', vault.id)"
        >
          <span
            v-if="(taskCounts[vault.id] ?? 0) > 0"
            data-testid="task-count"
            class="absolute -right-0.5 -top-0.5 min-w-[14px] rounded-full bg-violet-500 px-1 text-center text-[9px] font-semibold leading-[14px] text-white"
          >{{ taskCounts[vault.id] }}</span>
          <!-- existing checklist SVG stays here -->
```

Keep the existing checklist `<svg>` inside the button unchanged.

- [ ] **Step 5: Run tests + full suite + build**

Run: `npx vitest run tests/vaults-store.test.ts tests/vault-list.test.ts && npm test && npm run build`
Expected: PASS. (If any existing `VaultList` mount in `tests/vault-list.test.ts` or `tests/action-panel.test.ts` fails on a missing `taskCounts` prop, pass `taskCounts: {}` — Vue tolerates a missing prop as `undefined`, and `taskCounts[vault.id]` on `undefined` throws, so add a default: in `defineProps` this is non-optional; give the ActionPanel/tests a `{}`. To be safe, read as `props.taskCounts?.[vault.id] ?? 0` in the badge expressions.)

- [ ] **Step 6: Commit**

```bash
git add src/stores/vaults.ts src/components/ActionPanel.vue src/components/VaultList.vue tests/vaults-store.test.ts tests/vault-list.test.ts
git commit -m "feat(ui): open-task count badge on the vault-row Tasks button"
```

---

### Task 5: Frontend — move the tasks folder into Vault settings

**Files:**
- Modify: `src/components/CaptureSettings.vue` (add a Tasks section + `tasksFolder` load/save)
- Modify: `src/components/ActionPanel.vue` (header title `captureSettings` → "Vault settings")
- Test: `tests/capture-settings.test.ts`, `tests/action-panel.test.ts`

**Interfaces:**
- Consumes (IPC): `get_tasks_config {id} -> { tasksFolder }`, `set_tasks_config {id, tasksFolder}`.

- [ ] **Step 1: Write the failing tests**

Add to `tests/capture-settings.test.ts` (mirror its `mockIPC` — it already mocks `get_capture_config`/`list_audio_devices`; add `get_tasks_config`/`set_tasks_config`):

```ts
it("loads and saves the tasks folder via the tasks config commands", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config") return defaultCaptureCfg; // the file's existing default
    if (cmd === "list_audio_devices") return { inputs: [], outputs: [] };
    if (cmd === "get_tasks_config") return { tasksFolder: "Inbox/Tasks" };
    if (cmd === "set_tasks_config") return null;
  });
  const wrapper = mount(CaptureSettings, { props: { vaultId: "v1" } });
  await flushPromises();
  const input = wrapper.get('[data-testid="tasks-folder-input"]');
  expect((input.element as HTMLInputElement).value).toBe("Inbox/Tasks");
  await input.setValue("Work/Tasks");
  await wrapper.get('[data-testid="tasks-folder-save"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "set_tasks_config")).toEqual({
    cmd: "set_tasks_config",
    args: { id: "v1", tasksFolder: "Work/Tasks" },
  });
});
```

Add to `tests/action-panel.test.ts` (the file drives `store.view`):

```ts
it("titles the per-vault settings view 'Vault settings'", async () => {
  const store = useVaultsStore();
  store.openCaptureSettings("v1");
  const wrapper = mountPanel(); // the file's existing mount helper (stub CaptureSettings)
  await flushPromises();
  expect(wrapper.text()).toContain("Vault settings");
});
```

Stub `CaptureSettings` in the action-panel mount (extend the file's `stubs`) so the title test doesn't hit IPC.

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/capture-settings.test.ts tests/action-panel.test.ts`
Expected: FAIL — no `tasks-folder-input` in CaptureSettings; header still says "Capture settings".

- [ ] **Step 3: Implement**

In `src/components/ActionPanel.vue`, change the header title ternary arm for `captureSettings` from `"Capture settings"` to `"Vault settings"`.

In `src/components/CaptureSettings.vue`:
- Add a ref: `const tasksFolder = ref("");` and `const tasksFolderError = ref<string | null>(null);` beside the other refs.
- In `onMounted`, after the capture config load, fetch the tasks folder (a separate invoke so a tasks-config failure can't block the capture form):

```ts
    try {
      const tcfg = await invoke<{ tasksFolder: string | null }>("get_tasks_config", { id: props.vaultId });
      tasksFolder.value = tcfg.tasksFolder ?? "";
    } catch (e) {
      logWarning(`get_tasks_config failed (vault ${props.vaultId}): ${String(e)}`);
    }
```

- Add a save function:

```ts
async function saveTasksFolder() {
  tasksFolderError.value = null;
  const value = tasksFolder.value.trim();
  try {
    await invoke("set_tasks_config", { id: props.vaultId, tasksFolder: value === "" ? null : value });
  } catch (e) {
    tasksFolderError.value = String(e);
    logWarning(`set_tasks_config failed (vault ${props.vaultId}): ${String(e)}`);
  }
}
```

- In the template, add a `Tasks` `<section>` (e.g. after the recording-folder section, before or after transcription — keep it a sibling `<section>` inside the `<form>`):

```html
    <section>
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        Tasks
      </h2>
      <div class="flex items-center gap-1">
        <input
          v-model="tasksFolder"
          data-testid="tasks-folder-input"
          type="text"
          placeholder="Tasks"
          aria-label="Tasks folder"
          class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
          @keydown.enter.prevent="saveTasksFolder"
        />
        <button
          type="button"
          data-testid="tasks-folder-save"
          class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          @click="saveTasksFolder"
        >
          Save
        </button>
      </div>
      <p v-if="tasksFolderError" class="mt-1 text-xs text-red-300">{{ tasksFolderError }}</p>
    </section>
```

Note: the tasks-folder input is `type="button"`-saved separately from the capture form's `@submit`; the `@keydown.enter.prevent` stops Enter from submitting the capture form.

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/capture-settings.test.ts tests/action-panel.test.ts && npm test && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/CaptureSettings.vue src/components/ActionPanel.vue tests/capture-settings.test.ts tests/action-panel.test.ts
git commit -m "feat(ui): move the tasks folder into Vault settings"
```

---

## Self-Review

**Spec coverage:**
- Open-task counter → Task 2 (`count_open_tasks`) + Task 4 (store + badge). ✓
- Move tasks folder to Vault settings → Task 5. ✓
- `status: archived` + Archive action removing from list → Task 1 (exclude archived, string toggle) + Task 2 (validate archived) + Task 3 (Archive button + optimistic remove). ✓
- Progress bar → Task 3. ✓
- Status model (`new`/`done`/`archived`, open = not-done-not-archived) → Tasks 1–4. ✓
- "Open" excludes archived (list already excludes) → Task 1 + count filter in Task 2. ✓

**Placeholder scan:** No TBD/TODO. The "adapt to the file's mount helper" notes in Tasks 4/5 tests are concrete adaptation instructions (the helper exists per the referenced files), not deferred work; the code steps show full code.

**Type consistency:** `set_task_status` arg is `{id, path, status}` (string) across Task 2 (Rust), Task 3 (Tasks.vue `invoke`), and the tests. `count_open_tasks` → number in Task 2, Task 4 store, and tests. `taskCounts: Record<string, number>` consistent across store (Task 4), ActionPanel prop, and VaultList prop. `tasksFolder`/`tasksFolder`-keyed `set_tasks_config` consistent between Task 5 and the existing command. `status` values `new`/`done`/`archived` consistent Task 1↔2↔3.
