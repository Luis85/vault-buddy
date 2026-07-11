# Task Aggregation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A cross-vault "All tasks" view merging every vault's tasks into the existing task UI — full interactivity, per-row vault attribution, a vault picker on the add row, and an entry bar above the vault list.

**Architecture:** Frontend fan-out over the existing IPC surface — ZERO Rust changes. `Tasks.vue`'s prop widens to `vaultId: string | null`; both modes work on one enriched shape (`AggTask = TaskItem & {vaultId, vaultName}`) so every row action reads `task.vaultId` with no mode branches. Aggregate mode loads `list_vaults` then parallel per-vault `list_tasks` (best-effort per vault). The entry bar sums the store's already-loaded `taskCounts`.

**Tech Stack:** Vue 3 + Pinia, Vitest (`mockIPC`, happy-dom). No Rust.

## Global Constraints

- **No Rust/IPC changes.** Aggregation is client-side over `list_vaults` / `list_tasks` / existing action commands (`set_task_status`, `update_task`, `open_task`, `add_task` — all already take a vault `id` per call).
- **One internal shape for BOTH modes:** `AggTask = TaskItem & { vaultId: string; vaultName: string }`; per-vault mode enriches with `{vaultId: props.vaultId, vaultName: ""}`. Actions use `task.vaultId`, never `props.vaultId`.
- **Aggregate load is best-effort per vault:** a failing vault contributes nothing; ONE error toast names the failed vault(s); the blocking banner appears only when `list_vaults` fails or EVERY vault fails.
- **Sort:** existing comparator + final `vaultName.localeCompare` → `path.localeCompare` tiebreaks (both the open and done arms). Core sort untouched.
- **Aggregate-only UI:** vault chip on rows, vault picker on the add row, "All tasks" header title. Per-vault mode renders bit-identically to today.
- **Task paths are absolute and unique across vaults** — row keys and the per-path `busy` guard need no changes.
- **Commits:** Conventional Commits (`feat(ui)`, `docs`); git identity `Claude <noreply@anthropic.com>` is configured in the repo.
- **TDD:** failing test first; regression tests name the failure mode.
- Spec: `docs/superpowers/specs/2026-07-10-task-aggregation-design.md`.

---

### Task 1: Aggregate data layer in `Tasks.vue`

**Files:**
- Modify: `src/components/Tasks.vue` (props ~line 50; onMounted ~line 190; action invokes at lines ~206/238/258/270/332; `sortInPlace`; `Bucket` type)
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Produces: prop `vaultId: string | null`; `isAggregate` computed; internal `AggTask` type; `allVaults: Ref<Vault[]>` (consumed by Task 3's picker); actions reading `task.vaultId`. The add row is temporarily hidden in aggregate mode (`v-if="!isAggregate"` on its container) — Task 3 removes the gate and adds the picker.

- [ ] **Step 1: Add the aggregate mount helper + failing tests**

In `tests/tasks.test.ts`, after `mountView`, add:

```ts
const vaultsFixture = [
  { id: "va", name: "Alpha", path: "C:/va", open: false },
  { id: "vb", name: "Beta", path: "C:/vb", open: false },
];
const aggTask = (vault: "va" | "vb", title: string, created: string, extra: Partial<TaskItem> = {}): TaskItem => ({
  path: `C:/${vault}/Tasks/${title.replace(/\s+/g, "-")}.md`,
  title, status: "new", created, done: false, due: null, priority: null, tags: [], ...extra,
});

function mountAggregate(handlers: Partial<Record<string, (args: unknown) => unknown>> = {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "list_vaults") return vaultsFixture;
    if (cmd === "list_tasks") {
      const id = (args as { id: string }).id;
      return id === "va"
        ? [aggTask("va", "Alpha task", "2026-07-08")]
        : [aggTask("vb", "Beta task", "2026-07-09")];
    }
    if (cmd === "set_task_status") return null;
  });
  const wrapper = mount(Tasks, { props: { vaultId: null } });
  return { wrapper, calls };
}
```

Add tests:

```ts
it("aggregate mode merges every vault's tasks in global sort order", async () => {
  const { wrapper, calls } = mountAggregate();
  await flushPromises();
  expect(calls.filter((c) => c.cmd === "list_tasks").map((c) => (c.args as { id: string }).id).sort()).toEqual(["va", "vb"]);
  const rows = wrapper.findAll('[data-testid="task-row"]');
  // Newest created first: Beta task (07-09) before Alpha task (07-08).
  expect(rows[0].text()).toContain("Beta task");
  expect(rows[1].text()).toContain("Alpha task");
});

it("orders otherwise-equal tasks by vault name for cross-vault stability", async () => {
  const { wrapper, calls } = mountAggregate({
    list_tasks: (args) =>
      [(args as { id: string }).id === "va"
        ? aggTask("va", "Same", "2026-07-08")
        : aggTask("vb", "Same", "2026-07-08")],
  });
  await flushPromises();
  // Titles/created equal → vaultName tiebreak puts Alpha's copy first;
  // observable through which vault the first row's toggle hits.
  await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "set_task_status")?.args).toMatchObject({ id: "va" });
});

it("a failing vault degrades to a toast naming it, the rest render", async () => {
  const notifications = useNotificationsStore();
  const { wrapper } = mountAggregate({
    list_tasks: (args) => {
      if ((args as { id: string }).id === "vb") throw new Error("boom");
      return [aggTask("va", "Alpha task", "2026-07-08")];
    },
  });
  await flushPromises();
  expect(wrapper.text()).toContain("Alpha task");
  expect(notifications.items.some((n) => n.kind === "error" && n.message.includes("Beta"))).toBe(true);
  // No blocking banner — partial results render.
  expect(wrapper.text()).not.toContain("boom");
});

it("shows the blocking banner only when every vault fails", async () => {
  const { wrapper } = mountAggregate({
    list_tasks: () => {
      throw new Error("all gone");
    },
  });
  await flushPromises();
  expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(0);
  expect(wrapper.text()).toContain("Couldn't load tasks from any vault");
});

it("row actions carry the ROW's vault id in aggregate mode", async () => {
  const { wrapper, calls } = mountAggregate();
  await flushPromises();
  // First row is Beta task (vb): open + archive must hit vb, not va.
  await wrapper.get('[data-testid="task-open"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "open_task")?.args).toMatchObject({ id: "vb", path: "C:/vb/Tasks/Beta-task.md" });
  await wrapper.get('[data-testid="task-archive"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "set_task_status")?.args).toMatchObject({ id: "vb", status: "archived" });
});

it("aggregate mode hides the add row until the picker lands", async () => {
  const { wrapper } = mountAggregate();
  await flushPromises();
  expect(wrapper.find('[data-testid="task-input"]').exists()).toBe(false);
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — the component still requires `vaultId: string` (vue warn / no rows render in aggregate mounts).

- [ ] **Step 3: Implement**

In `src/components/Tasks.vue`:

Props/type/state (replace the `defineProps` line; add below it):

```ts
const props = defineProps<{ vaultId: string | null }>();
// Aggregate mode: one merged view across every vault (vaultId === null).
const isAggregate = computed(() => props.vaultId === null);

// A task enriched with its owning vault — the ONE internal shape for both
// modes, so every action reads task.vaultId and needs no mode branches.
type AggTask = TaskItem & { vaultId: string; vaultName: string };
```

Import `Vault` alongside the other types: `import type { TaskItem, TaskPatch, Vault } from "../types";`
Change `const tasks = ref<TaskItem[]>([]);` → `ref<AggTask[]>([])`, and add `const allVaults = ref<Vault[]>([]);`

Replace `onMounted`:

```ts
onMounted(async () => {
  try {
    if (props.vaultId !== null) {
      const items = await invoke<TaskItem[]>("list_tasks", { id: props.vaultId });
      const id = props.vaultId;
      tasks.value = items.map((t) => ({ ...t, vaultId: id, vaultName: "" }));
    } else {
      // Aggregate: fan out over every vault, best-effort per vault — the
      // same posture as the store's taskCounts load. A failed vault
      // contributes nothing and is named in ONE toast; the blocking banner
      // is reserved for list_vaults failing or EVERY vault failing.
      const vaults = await invoke<Vault[]>("list_vaults");
      allVaults.value = vaults;
      const failed: string[] = [];
      const results = await Promise.all(
        vaults.map(async (v) => {
          try {
            const items = await invoke<TaskItem[]>("list_tasks", { id: v.id });
            return items.map((t) => ({ ...t, vaultId: v.id, vaultName: v.name }));
          } catch (e) {
            failed.push(v.name);
            logWarning(`list_tasks failed for vault ${v.id}: ${String(e)}`);
            return [];
          }
        }),
      );
      if (vaults.length > 0 && failed.length === vaults.length) {
        loadError.value = "Couldn't load tasks from any vault.";
      } else {
        tasks.value = results.flat();
        sortInPlace();
        if (failed.length > 0) {
          notifications.error(`Couldn't load tasks from ${failed.join(", ")}.`);
        }
      }
    }
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});
```

Action call sites — replace `props.vaultId` with `task.vaultId` in `toggle`, `archive`, `openInObsidian`, and `saveEdit` (four invokes; `add()` keeps `props.vaultId` this task). Example (toggle):

```ts
    await invoke("set_task_status", { id: task.vaultId, path: task.path, status: task.status });
```

`sortInPlace` — append the tiebreaks to BOTH arms:

```ts
      (a.done
        ? b.created.localeCompare(a.created) ||
          a.title.localeCompare(b.title) ||
          a.vaultName.localeCompare(b.vaultName) ||
          a.path.localeCompare(b.path)
        : dueKey(a).localeCompare(dueKey(b)) ||
          rank(a) - rank(b) ||
          b.created.localeCompare(a.created) ||
          a.title.localeCompare(b.title) ||
          a.vaultName.localeCompare(b.vaultName) ||
          a.path.localeCompare(b.path)),
```

Type updates so the template's `task.vaultId` typechecks: `type Bucket = { key: string; label: string | null; tasks: AggTask[] }`, the `groups: Record<string, AggTask[]>` in the dates arm, the tag-mode `Map<string, { label: string; tasks: AggTask[] }>` + `notags`/`done` arrays, and `add()`'s created enrichment is Task 3 — for now `add()` is unreachable in aggregate (gate below). `toggle`/`archive`/`startEdit`/`saveEdit`/`openInObsidian` parameter types become `AggTask`.

Template — gate the add row and its options behind per-vault mode: wrap the add-row `<div class="flex items-center gap-1">` and the `<div v-if="showAddOptions" ...>` options row in `v-if="!isAggregate"` (on the add-row div, and change the options row to `v-if="!isAggregate && showAddOptions"`).

In `add()`, TypeScript will flag `props.vaultId` (string | null) — since the row is unreachable in aggregate, guard at the top: `if (props.vaultId === null) return;` (Task 3 replaces this with the picker).

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS — all pre-existing per-vault tests unchanged (mountView still passes `vaultId: "v1"`).

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): aggregate all-vaults task loading with per-row vault actions"
```

---

### Task 2: Vault chip on aggregate rows

**Files:**
- Modify: `src/components/Tasks.vue` (row template, inside the `task-open` button before the priority dot)
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes: `isAggregate`, `task.vaultName` (Task 1).

- [ ] **Step 1: Write the failing tests**

```ts
it("shows a vault chip with the vault initial on aggregate rows", async () => {
  const { wrapper } = mountAggregate();
  await flushPromises();
  const chips = wrapper.findAll('[data-testid="task-vault"]');
  expect(chips).toHaveLength(2);
  expect(chips[0].text()).toBe("B"); // first row = Beta task
  expect(chips[0].attributes("title")).toBe("Beta");
});

it("shows no vault chip in per-vault mode", async () => {
  const { wrapper } = mountView();
  await flushPromises();
  expect(wrapper.find('[data-testid="task-vault"]').exists()).toBe(false);
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — no `task-vault` elements.

- [ ] **Step 3: Implement**

Inside the `task-open` button, as its FIRST child (before the priority-dot span):

```html
                <span
                  v-if="isAggregate"
                  data-testid="task-vault"
                  class="flex h-4 w-4 shrink-0 items-center justify-center rounded bg-violet-600/80 text-[9px] font-bold text-white"
                  :title="task.vaultName"
                >{{ task.vaultName.charAt(0).toUpperCase() }}</span>
```

(Same avatar treatment as the vault list's initial, scaled down; `title` carries the full vault name. Not `aria-hidden` — the `title` is the accessible vault hint on the row.)

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): vault chip on aggregate task rows"
```

---

### Task 3: Add row with a vault picker in aggregate mode

**Files:**
- Modify: `src/components/Tasks.vue` (remove the Task-1 add gate; picker state + template; `add()` routing/enrichment)
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes: `allVaults` (Task 1), `SelectMenu` (`src/components/SelectMenu.vue`: props `modelValue`, `options: {value, label}[]`, `ariaLabel`, `dataTestid`; emits `update:modelValue`; renders its dropdown through a Teleport to `document.body`).

- [ ] **Step 1: Write the failing tests**

The picker's dropdown teleports to `document.body`, so these tests mount with `attachTo` and clean up. Add a variant helper + tests:

```ts
function mountAggregateAttached(handlers: Partial<Record<string, (args: unknown) => unknown>> = {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "list_vaults") return vaultsFixture;
    if (cmd === "list_tasks") return [];
    if (cmd === "add_task") {
      const a = args as { id: string; title: string };
      return { path: `C:/${a.id}/Tasks/new.md`, title: a.title, status: "new", created: "2026-07-10", done: false, due: null, priority: null, tags: [] };
    }
  });
  const wrapper = mount(Tasks, { props: { vaultId: null }, attachTo: document.body });
  return { wrapper, calls };
}

it("aggregate add routes to the picked vault and merges the created task", async () => {
  const { wrapper, calls } = mountAggregateAttached();
  await flushPromises();
  // Picker defaults to the first vault (Alpha).
  expect(wrapper.get('[data-testid="task-add-vault"]').text()).toContain("Alpha");
  // Pick Beta from the teleported menu.
  await wrapper.get('[data-testid="task-add-vault"]').trigger("click");
  (document.body.querySelector('[data-testid="task-add-vault-option-vb"]') as HTMLElement).click();
  await flushPromises();
  await wrapper.get('[data-testid="task-input"]').setValue("Cross task");
  await wrapper.get('[data-testid="task-add"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "add_task")?.args).toMatchObject({ id: "vb", title: "Cross task" });
  // Created task renders enriched with Beta's chip.
  const row = wrapper.findAll('[data-testid="task-row"]').find((r) => r.text().includes("Cross task"))!;
  expect(row.get('[data-testid="task-vault"]').attributes("title")).toBe("Beta");
  wrapper.unmount();
  document.body.innerHTML = "";
});
```

Also DELETE the Task-1 test `"aggregate mode hides the add row until the picker lands"` (the gate it pins is removed this task) and replace it with:

```ts
it("aggregate mode shows the add row with the vault picker", async () => {
  const { wrapper } = mountAggregate();
  await flushPromises();
  expect(wrapper.find('[data-testid="task-input"]').exists()).toBe(true);
  expect(wrapper.find('[data-testid="task-add-vault"]').exists()).toBe(true);
});

it("per-vault mode has no vault picker", async () => {
  const { wrapper } = mountView();
  await flushPromises();
  expect(wrapper.find('[data-testid="task-add-vault"]').exists()).toBe(false);
});
```

(SelectMenu's option test ids follow the `${dataTestid}-option-${value}` pattern — verify against `src/components/SelectMenu.vue` and `tests/capture-settings.test.ts`'s `pickOption` helper; if the actual pattern differs, match it.)

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — no `task-add-vault`; the add row is still gated off in aggregate.

- [ ] **Step 3: Implement**

Script:

```ts
import SelectMenu from "./SelectMenu.vue";
```

```ts
// Aggregate add: which vault receives the new task. Defaults to the first
// vault; component-local, no persistence across opens (YAGNI per spec).
const addVaultId = ref("");
const vaultOptions = computed(() =>
  allVaults.value.map((v) => ({ value: v.id, label: v.name })),
);
```

In the aggregate branch of `onMounted`, after `allVaults.value = vaults;`: `addVaultId.value = vaults[0]?.id ?? "";`

Rework `add()`'s target + enrichment (replacing the Task-1 `props.vaultId === null` early return):

```ts
async function add() {
  const title = newTitle.value.trim();
  const targetVault = isAggregate.value ? addVaultId.value : props.vaultId;
  if (!title || adding.value || !targetVault) return;
  adding.value = true;
  try {
    const args: Record<string, unknown> = { id: targetVault, title };
    if (addDue.value) args.due = addDue.value;
    if (addPriority.value !== "normal") args.priority = addPriority.value;
    const tags = parseTagsInput(addTags.value);
    if (tags.length > 0) args.tags = tags;
    const created = await invoke<TaskItem>("add_task", args);
    tasks.value.unshift({
      ...created,
      vaultId: targetVault,
      vaultName: allVaults.value.find((v) => v.id === targetVault)?.name ?? "",
    });
    sortInPlace();
    newTitle.value = "";
    addDue.value = "";
    addPriority.value = "normal";
    addTags.value = "";
    showAddOptions.value = false;
  } catch (e) {
    notifications.error(String(e));
    logWarning(`add_task failed: ${String(e)}`);
  } finally {
    adding.value = false;
  }
}
```

Template: remove the `!isAggregate` gates from the add row and options row (restore `v-if="showAddOptions"` on the options row), and insert the picker as the FIRST child of the add-row flex div:

```html
      <SelectMenu
        v-if="isAggregate"
        v-model="addVaultId"
        :options="vaultOptions"
        aria-label="Vault for the new task"
        data-testid="task-add-vault"
      />
```

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS (per-vault add tests unchanged — `targetVault` degenerates to `props.vaultId`).

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): aggregate add-task with a vault picker"
```

---

### Task 4: Entry bar, store action, header title

**Files:**
- Modify: `src/stores/vaults.ts` (add `openAllTasks` beside `openTasks` ~line 166)
- Modify: `src/components/ActionPanel.vue` (header ternary ~lines 83-85; list container ~line 229)
- Test: `tests/vaults-store.test.ts`, `tests/action-panel.test.ts`

**Interfaces:**
- Consumes: `store.taskCounts` (existing), `Tasks.vue`'s `vaultId: string | null` prop (Task 1; the `<Tasks :vault-id="store.tasksVaultId" />` mount already passes it).
- Produces: `vaults.openAllTasks()` (view `"tasks"`, `tasksVaultId: null`).

- [ ] **Step 1: Write the failing tests**

`tests/vaults-store.test.ts`:

```ts
it("openAllTasks opens the tasks view in aggregate mode", () => {
  const store = useVaultsStore();
  store.openAllTasks();
  expect(store.view).toBe("tasks");
  expect(store.tasksVaultId).toBeNull();
  store.back();
  expect(store.view).toBe("list");
});
```

`tests/action-panel.test.ts` (mirror the file's existing store-driven mounts; stub `Tasks`):

```ts
it("shows the All-tasks entry bar with the summed open count and opens aggregate mode", async () => {
  const store = useVaultsStore();
  store.vaults = sampleVaults;
  store.loaded = true;
  store.taskCounts = { d4e5f6: 2, a1b2c3: 3 };
  const wrapper = mount(ActionPanel, { global: { stubs: { Tasks: true } } });
  expect(wrapper.get('[data-testid="all-tasks-count"]').text()).toBe("5");
  await wrapper.get('[data-testid="all-tasks"]').trigger("click");
  expect(store.view).toBe("tasks");
  expect(store.tasksVaultId).toBeNull();
  expect(wrapper.text()).toContain("All tasks");
});

it("hides the count badge at zero and the bar without vaults", () => {
  const store = useVaultsStore();
  store.vaults = sampleVaults;
  store.loaded = true;
  store.taskCounts = {};
  const empty = mount(ActionPanel);
  expect(empty.get('[data-testid="all-tasks"]').text()).toContain("All tasks");
  expect(empty.find('[data-testid="all-tasks-count"]').exists()).toBe(false);
  const store2 = useVaultsStore();
  store2.vaults = [];
  store2.loaded = true;
  // fresh pinia per test — this second mount block goes in its own test if
  // the file's beforeEach resets pinia; then assert:
  // expect(wrapper.find('[data-testid="all-tasks"]').exists()).toBe(false);
});
```

(Adapt the second test to the file's one-pinia-per-test setup: one test for badge-hidden-at-zero, one for bar-absent-with-no-vaults.)

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/vaults-store.test.ts tests/action-panel.test.ts`
Expected: FAIL — `openAllTasks` undefined; no `all-tasks` element.

- [ ] **Step 3: Implement**

`src/stores/vaults.ts`, after `openTasks`:

```ts
    /** The cross-vault "All tasks" view — tasks view with no vault selected. */
    openAllTasks() {
      this.view = "tasks";
      this.tasksVaultId = null;
    },
```

`src/components/ActionPanel.vue` script: add

```ts
const totalOpenTasks = computed(() =>
  Object.values(store.taskCounts).reduce((a, b) => a + b, 0),
);
```

Header ternary — replace the `"tasks"` arm:

```
                    : view === "tasks"
                      ? store.tasksVaultId === null
                        ? "All tasks"
                        : "Tasks"
                      : "Vaults"
```

Entry bar — inside the list-view container (`<div v-else class="panel-scroll ...">`), as its FIRST child before `<VaultList`:

```html
      <button
        v-if="store.vaults.length > 0"
        type="button"
        data-testid="all-tasks"
        class="mb-2 flex w-full cursor-pointer items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1.5 text-left text-sm text-slate-200 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        aria-label="All tasks across every vault"
        @click="store.openAllTasks()"
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M9 11l3 3 8-8" />
          <path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h9" />
        </svg>
        <span class="min-w-0 flex-1 truncate">All tasks</span>
        <span
          v-if="totalOpenTasks > 0"
          data-testid="all-tasks-count"
          class="shrink-0 rounded-full bg-violet-500 px-1.5 text-center text-[10px] font-semibold leading-4 text-white"
        >{{ totalOpenTasks }}</span>
      </button>
```

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/vaults-store.test.ts tests/action-panel.test.ts && npm test && npm run build`
Expected: PASS. (If an existing action-panel test counts `.panel-scroll button` elements — "lists each vault with both actions and a count badge" expects 10 — the new bar adds one: update that expectation to 11 with a comment naming the All-tasks bar.)

- [ ] **Step 5: Commit**

```bash
git add src/stores/vaults.ts src/components/ActionPanel.vue tests/vaults-store.test.ts tests/action-panel.test.ts
git commit -m "feat(ui): All-tasks entry bar opening the cross-vault view"
```

---

### Task 5: Docs + full verification sweep

**Files:**
- Modify: `AGENTS.md` (tasks-domain frontend paragraph)
- Modify: `README.md` (the Track-tasks bullet)
- Modify: `docs/use-cases/aggregated-task-dashboard-and-lists.md` (+ the catalog row in `docs/use-cases/README.md`)

- [ ] **Step 1: Update the docs**

- **AGENTS.md** (tasks-domain frontend paragraph): add that `Tasks.vue` now takes `vaultId: string | null` — `null` is the cross-vault aggregate mode (fan-out over `list_vaults` + per-vault `list_tasks`, best-effort per vault with one toast naming failures; banner only when all fail); both modes share one enriched task shape so every row action carries the row's own vault id; aggregate-only UI = vault chip, add-row vault picker, "All tasks" header; entry = the All-tasks bar above the vault list summing `taskCounts`; `openAllTasks()` in the store; ZERO new IPC commands.
- **README.md**: extend the Track-tasks bullet with one clause — the vault list also has an **All tasks** view merging every vault (with the summed open count), where adding picks a target vault.
- **`docs/use-cases/aggregated-task-dashboard-and-lists.md`**: retitle the status section to "Partially shipped — the aggregated view (v0.5.4)"; state what shipped (cross-vault merged view with full interactivity, vault attribution, vault-picking add, entry bar) and what remains (user-defined lists, Quick Task modal, bulk operations, full-text search, dashboard widget rows). Update frontmatter `status: planned` → `status: partially-shipped` and add `shipped_in: v0.5.4`. In `docs/use-cases/README.md`, move its row from the Planned table to the Shipped table with `v0.5.4 (aggregated view; lists/modal/bulk/search remain planned)`.

- [ ] **Step 2: Full verification sweep + commit**

Run: `npm test && npm run build && cd src-tauri && cargo fmt --check && cd core && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all PASS (Rust untouched — the sweep guards against accidental edits).

```bash
git add AGENTS.md README.md docs/use-cases/
git commit -m "docs: document the cross-vault All-tasks aggregation"
```

---

## Self-Review

**Spec coverage:**
- Frontend fan-out, zero Rust → Task 1 (+ Task 5 sweep guard). ✓
- One `AggTask` shape both modes; actions read `task.vaultId` → Task 1. ✓
- Best-effort per vault, toast naming failures, banner only on total failure → Task 1. ✓
- Sort vaultName→path tiebreak, both arms → Task 1. ✓
- Vault chip aggregate-only → Task 2. ✓
- Add with vault picker (default first vault, created task enriched+merged) → Task 3. ✓
- Entry bar + summed badge + `openAllTasks` + "All tasks" header → Task 4. ✓
- Buckets/tag mode/filters/progress/busy-guard untouched → no task touches them (Task 1's type changes only). ✓
- Docs → Task 5. ✓

**Placeholder scan:** none. The Task 3 note about SelectMenu's option-testid pattern and Task 4's note about the button-count test are concrete verify-and-adapt instructions against named files, not deferred work.

**Type consistency:** `AggTask` defined in Task 1, consumed by Tasks 2-3 (`task.vaultName`, enrichment in `add()`); `allVaults: Ref<Vault[]>` Task 1 → Task 3 (`vaultOptions`); `vaultId: string | null` prop Task 1 ↔ Task 4's store `tasksVaultId: null`; test ids `task-vault` (T2), `task-add-vault` (T3), `all-tasks`/`all-tasks-count` (T4) used consistently; `openAllTasks()` name identical in store, ActionPanel, and tests.
