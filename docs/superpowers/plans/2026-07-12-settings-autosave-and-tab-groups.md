# Settings Auto-Save & Tab Groups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the two panel settings views from a manual Save button to auto-save (blur + debounce, with a transient header status), and split their long scrolls into 3-tab groups.

**Architecture:** Three small new frontend units — a reusable `TabGroup.vue` (eager `v-show` panels), a `useAutosave` composable (debounce + immediate + in-flight serialization + status), and a tiny `settingsStatus` Pinia store the panel header reads. `BuddySettings` gains tabs only (its controls already persist instantly). `CaptureSettings` splits into three self-contained tab components (`RecordingConfigTab`, `TasksConfigTab`, `DocumentsConfigTab`) that each load their own config and auto-save; the old load/edited race-guards and `useOptionalFolderField` are deleted.

**Tech Stack:** Vue 3 `<script setup>`, Pinia, Tailwind 4, Tauri IPC (`@tauri-apps/api/core` `invoke`), Vitest + happy-dom + `@vue/test-utils` + `@tauri-apps/api/mocks` (`mockIPC`).

**Spec:** `docs/superpowers/specs/2026-07-12-settings-autosave-and-tab-groups-design.md`

## Global Constraints

- **Frontend-only slice.** No Rust/IPC surface change. Commands used, all pre-existing: `get_capture_config`/`set_capture_config`, `get_tasks_config`/`set_tasks_config`, `set_task_lists_config`/`list_task_lists`, `get_documents_config`/`set_documents_config`, `list_audio_devices`, `get_autostart`/`set_autostart`.
- **Debounce = 600 ms** for typed fields; toggles/selects save immediately; blur (`@focusout`) flushes a pending debounced save.
- **Save trigger only fires from user edits**, never from a mount-time load assignment ("does not save on mount" is a required test for every auto-saving tab).
- **Failed load → inline error, no editable fields** (a default seed must never be auto-saved over a value that failed to read).
- **LOC cap:** frontend files ≤ 500 nonblank LOC (`scripts/loc-baseline.json`). Every new file must stay under it.
- **Coverage floors** (`vite.config.ts`, rise-only): statements 93 / branches 90 / functions 90 / lines 95. Keep or raise; never lower.
- **No swallowed errors:** every caught failure goes through `logWarning` (`src/logging.ts`).
- **Commits:** Conventional Commits, scope `ui` (e.g. `feat(ui):`, `refactor(ui):`, `test(ui):`). Imperative subject; body explains the why.
- **Terminology:** a Task is a document, a Todo is a checklist line (CONTEXT.md). Copy stays in the app's existing voice.

---

### Task 1: `settingsStatus` Pinia store

The shared save-status the panel header reads. `saving`/`saved`/`failed`/`reset`; `saved` auto-fades to `idle` after 2 s; `error` is sticky until the next `saving`/`saved`.

**Files:**
- Create: `src/stores/settingsStatus.ts`
- Test: `tests/settings-status-store.test.ts`

**Interfaces:**
- Produces: `useSettingsStatusStore()` → store with `state: "idle" | "saving" | "saved" | "error"`, `error: string | null`, and actions `saving()`, `saved()`, `failed(message: string)`, `reset()`. Exported type `SaveState`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/settings-status-store.test.ts
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useSettingsStatusStore } from "../src/stores/settingsStatus";

describe("settingsStatus store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("starts idle", () => {
    expect(useSettingsStatusStore().state).toBe("idle");
  });

  it("saving() sets saving and clears a prior error", () => {
    const s = useSettingsStatusStore();
    s.failed("boom");
    s.saving();
    expect(s.state).toBe("saving");
    expect(s.error).toBeNull();
  });

  it("saved() shows saved then fades to idle after 2s", () => {
    const s = useSettingsStatusStore();
    s.saving();
    s.saved();
    expect(s.state).toBe("saved");
    vi.advanceTimersByTime(1999);
    expect(s.state).toBe("saved");
    vi.advanceTimersByTime(1);
    expect(s.state).toBe("idle");
  });

  it("a new saving() cancels a pending saved fade", () => {
    const s = useSettingsStatusStore();
    s.saved();
    vi.advanceTimersByTime(1000);
    s.saving();
    vi.advanceTimersByTime(2000);
    expect(s.state).toBe("saving"); // the old fade timer was cancelled
  });

  it("failed() holds the error state (no auto-fade) with the message", () => {
    const s = useSettingsStatusStore();
    s.failed("disk full");
    vi.advanceTimersByTime(5000);
    expect(s.state).toBe("error");
    expect(s.error).toBe("disk full");
  });

  it("reset() returns to idle and clears the error", () => {
    const s = useSettingsStatusStore();
    s.failed("boom");
    s.reset();
    expect(s.state).toBe("idle");
    expect(s.error).toBeNull();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/settings-status-store.test.ts`
Expected: FAIL — cannot resolve `../src/stores/settingsStatus`.

- [ ] **Step 3: Write the store**

```ts
// src/stores/settingsStatus.ts
import { defineStore } from "pinia";

export type SaveState = "idle" | "saving" | "saved" | "error";

// How long the "Saved" acknowledgement lingers before fading to idle.
const SAVED_LINGER_MS = 2000;

// Module-scoped (not reactive state) so storing a timer handle never trips
// Pinia's reactivity — the same module-constant idiom the settings store uses.
let fadeTimer: ReturnType<typeof setTimeout> | null = null;
function clearFade() {
  if (fadeTimer !== null) {
    clearTimeout(fadeTimer);
    fadeTimer = null;
  }
}

// The panel header's transient save indicator, shared across every
// auto-saving settings field so one indicator covers the whole view.
export const useSettingsStatusStore = defineStore("settingsStatus", {
  state: () => ({
    state: "idle" as SaveState,
    error: null as string | null,
  }),
  actions: {
    saving() {
      clearFade();
      this.state = "saving";
      this.error = null;
    },
    saved() {
      clearFade();
      this.state = "saved";
      this.error = null;
      fadeTimer = setTimeout(() => {
        // Only fade if nothing newer superseded us.
        if (this.state === "saved") this.state = "idle";
        fadeTimer = null;
      }, SAVED_LINGER_MS);
    },
    // Sticky until the next saving()/saved()/reset() — a failure the user isn't
    // looking at must not silently disappear.
    failed(message: string) {
      clearFade();
      this.state = "error";
      this.error = message;
    },
    reset() {
      clearFade();
      this.state = "idle";
      this.error = null;
    },
  },
});
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/settings-status-store.test.ts`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src/stores/settingsStatus.ts tests/settings-status-store.test.ts
git commit -m "feat(ui): add shared settingsStatus store for the save indicator"
```

---

### Task 2: `useAutosave` composable

Debounced/immediate save with in-flight serialization, status reporting, an inline `error` ref, and an unmount flush.

**Files:**
- Create: `src/composables/useAutosave.ts`
- Test: `tests/use-autosave.test.ts`

**Interfaces:**
- Consumes: `useSettingsStatusStore` (Task 1).
- Produces: `useAutosave(save: () => Promise<void>, opts?: { label?: string }) → { schedule(): void; flush(): void; saveNow(): void; error: Ref<string | null> }`.
  - `schedule()` — debounced 600 ms (typed input).
  - `flush()` — run now **only if** a debounced save is pending (bind to `@focusout`).
  - `saveNow()` — run immediately (toggles/selects).
  - A `schedule`/`flush`/`saveNow` while a save is running coalesces into exactly one trailing run.
  - On failure: sets `error`, calls `status.failed(msg)`, and `logWarning("<label> autosave failed: <msg>")`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/use-autosave.test.ts
import { flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import { logWarning } from "../src/logging";
import { useAutosave } from "../src/composables/useAutosave";
import { useSettingsStatusStore } from "../src/stores/settingsStatus";

beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
  (logWarning as ReturnType<typeof vi.fn>).mockClear();
});
afterEach(() => {
  vi.useRealTimers();
});

// useAutosave calls onBeforeUnmount, which warns without an active component;
// these unit tests exercise it outside a component, which is supported (the
// composable no-ops the lifecycle hook when there's no current instance).
describe("useAutosave", () => {
  it("schedule() debounces: one save after 600ms of quiet", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    const a = useAutosave(save);
    a.schedule();
    a.schedule();
    a.schedule();
    expect(save).not.toHaveBeenCalled();
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("saveNow() saves immediately", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    const a = useAutosave(save);
    a.saveNow();
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("flush() runs a pending debounced save, and is a no-op when none pending", async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    const a = useAutosave(save);
    a.flush(); // nothing scheduled → no-op
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(0);
    a.schedule();
    a.flush(); // pending → runs now, no need to wait 600ms
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("reports saving then saved to the status store on success", async () => {
    const status = useSettingsStatusStore();
    const a = useAutosave(vi.fn().mockResolvedValue(undefined));
    a.saveNow();
    expect(status.state).toBe("saving");
    await flushPromises();
    expect(status.state).toBe("saved");
  });

  it("reports the error, sets error ref, and logs on failure", async () => {
    const status = useSettingsStatusStore();
    const a = useAutosave(vi.fn().mockRejectedValue("disk full"), { label: "docs" });
    a.saveNow();
    await flushPromises();
    expect(status.state).toBe("error");
    expect(a.error.value).toBe("disk full");
    expect(logWarning).toHaveBeenCalledWith(expect.stringContaining("docs autosave failed"));
  });

  it("coalesces a save requested mid-flight into exactly one trailing run", async () => {
    let resolveFirst!: () => void;
    const save = vi
      .fn()
      .mockImplementationOnce(() => new Promise<void>((r) => (resolveFirst = r)))
      .mockResolvedValue(undefined);
    const a = useAutosave(save);
    a.saveNow(); // first run starts, pending on resolveFirst
    a.saveNow(); // mid-flight #1
    a.saveNow(); // mid-flight #2 — both collapse into one trailing run
    expect(save).toHaveBeenCalledTimes(1);
    resolveFirst();
    await flushPromises();
    expect(save).toHaveBeenCalledTimes(2); // one trailing run, not three
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/use-autosave.test.ts`
Expected: FAIL — cannot resolve `../src/composables/useAutosave`.

- [ ] **Step 3: Write the composable**

```ts
// src/composables/useAutosave.ts
import { getCurrentInstance, onBeforeUnmount, ref } from "vue";

import { logWarning } from "../logging";
import { useSettingsStatusStore } from "../stores/settingsStatus";

// Debounce window for typed fields; a blur/flush or a toggle bypasses it.
const DEBOUNCE_MS = 600;

/**
 * Wraps an async save fn with the mechanics every auto-saving settings field
 * needs, so no card re-implements them:
 * - `schedule()` debounces typed input (rapid keystrokes collapse to one save);
 * - `flush()` runs a *pending* debounced save now (bind to @focusout — a blur
 *   with nothing scheduled is a no-op, so an unchanged field doesn't re-save);
 * - `saveNow()` runs immediately (toggles/selects);
 * - in-flight serialization: a trigger while a save is running does NOT start a
 *   second concurrent write — it coalesces into ONE trailing run that re-reads
 *   the latest values (the McpSettings saving-guard lesson, generalized);
 * - status reporting to the shared settingsStatus store + an inline `error` ref;
 * - a flush on beforeUnmount so leaving the settings view never drops a queued
 *   write.
 *
 * `save` builds the payload from live refs and invokes; it must reject on
 * failure. `label` names the component in the warning log line.
 */
export function useAutosave(save: () => Promise<void>, opts: { label?: string } = {}) {
  const status = useSettingsStatusStore();
  const error = ref<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let running = false; // an invoke is awaiting
  let pending = false; // a save was requested mid-flight → run once more

  function clearTimer() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  async function run() {
    clearTimer();
    if (running) {
      // Coalesce: don't start a second concurrent write; mark a trailing run.
      pending = true;
      return;
    }
    running = true;
    status.saving();
    error.value = null;
    try {
      await save();
      status.saved();
    } catch (e) {
      const message = String(e);
      error.value = message;
      status.failed(message);
      logWarning(`${opts.label ?? "settings"} autosave failed: ${message}`);
    } finally {
      running = false;
      if (pending) {
        // A trailing run for the edit(s) made mid-flight, with latest values.
        pending = false;
        void run();
      }
    }
  }

  function schedule() {
    clearTimer();
    timer = setTimeout(() => void run(), DEBOUNCE_MS);
  }
  function flush() {
    if (timer !== null) void run(); // run() clears the timer
  }
  function saveNow() {
    void run();
  }

  // A pending debounced save must not die with the component when the settings
  // view navigates away (ActionPanel v-if-unmounts the settings component).
  if (getCurrentInstance()) {
    onBeforeUnmount(() => {
      if (timer !== null) void run();
    });
  }

  return { schedule, flush, saveNow, error };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/use-autosave.test.ts`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add src/composables/useAutosave.ts tests/use-autosave.test.ts
git commit -m "feat(ui): add useAutosave composable (debounce, serialize, status)"
```

---

### Task 3: `TabGroup.vue` reusable tab container

Accessible tabs; every panel mounted, only the active shown (`v-show`); resets to the first tab on mount; arrow-key navigation.

**Files:**
- Create: `src/components/TabGroup.vue`
- Test: `tests/tab-group.test.ts`

**Interfaces:**
- Produces: component with props `tabs: { id: string; label: string }[]` and optional `initial?: string`; one named slot per tab id. Renders `[data-testid="tab-<id>"]` buttons (`role="tab"`) and `[data-testid="panel-<id>"]` panels (`role="tabpanel"`), the active button `aria-selected="true"`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/tab-group.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { h } from "vue";

import TabGroup from "../src/components/TabGroup.vue";

const TABS = [
  { id: "one", label: "One" },
  { id: "two", label: "Two" },
  { id: "three", label: "Three" },
];

function mountGroup(initial?: string) {
  return mount(TabGroup, {
    props: { tabs: TABS, ...(initial ? { initial } : {}) },
    slots: {
      one: () => h("p", { "data-testid": "c-one" }, "one body"),
      two: () => h("p", { "data-testid": "c-two" }, "two body"),
      three: () => h("p", { "data-testid": "c-three" }, "three body"),
    },
    attachTo: document.body,
  });
}

describe("TabGroup", () => {
  it("starts on the first tab and marks it selected", () => {
    const wrapper = mountGroup();
    expect(wrapper.get('[data-testid="tab-one"]').attributes("aria-selected")).toBe("true");
    expect(wrapper.get('[data-testid="tab-two"]').attributes("aria-selected")).toBe("false");
    wrapper.unmount();
  });

  it("honors the initial prop", () => {
    const wrapper = mountGroup("two");
    expect(wrapper.get('[data-testid="tab-two"]').attributes("aria-selected")).toBe("true");
    wrapper.unmount();
  });

  it("mounts every panel but shows only the active one", () => {
    const wrapper = mountGroup();
    // all slot bodies are in the DOM (eager mount)
    expect(wrapper.find('[data-testid="c-one"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="c-two"]').exists()).toBe(true);
    // inactive panels are hidden via v-show (display:none)
    expect(wrapper.get('[data-testid="panel-two"]').isVisible()).toBe(false);
    expect(wrapper.get('[data-testid="panel-one"]').isVisible()).toBe(true);
    wrapper.unmount();
  });

  it("switches the shown panel on tab click", async () => {
    const wrapper = mountGroup();
    await wrapper.get('[data-testid="tab-two"]').trigger("click");
    expect(wrapper.get('[data-testid="panel-two"]').isVisible()).toBe(true);
    expect(wrapper.get('[data-testid="panel-one"]').isVisible()).toBe(false);
    expect(wrapper.get('[data-testid="tab-two"]').attributes("aria-selected")).toBe("true");
    wrapper.unmount();
  });

  it("moves between tabs with arrow keys and wraps", async () => {
    const wrapper = mountGroup();
    await wrapper.get('[data-testid="tab-one"]').trigger("keydown", { key: "ArrowRight" });
    expect(wrapper.get('[data-testid="tab-two"]').attributes("aria-selected")).toBe("true");
    await wrapper.get('[data-testid="tab-two"]').trigger("keydown", { key: "ArrowLeft" });
    expect(wrapper.get('[data-testid="tab-one"]').attributes("aria-selected")).toBe("true");
    await wrapper.get('[data-testid="tab-one"]').trigger("keydown", { key: "ArrowLeft" });
    expect(wrapper.get('[data-testid="tab-three"]').attributes("aria-selected")).toBe("true"); // wrapped
    wrapper.unmount();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/tab-group.test.ts`
Expected: FAIL — cannot resolve `../src/components/TabGroup.vue`.

- [ ] **Step 3: Write the component**

```vue
<!-- src/components/TabGroup.vue -->
<script setup lang="ts">
import { nextTick, ref } from "vue";

// A reusable tab container. Every panel is mounted and only the active one is
// shown (v-show), so each tab keeps its own self-contained load, a pending
// debounced save survives a tab switch (no unmount), and the content is in the
// DOM for tests to read. Slots are named by tab id.
const props = defineProps<{
  tabs: { id: string; label: string }[];
  initial?: string;
}>();

const active = ref(props.initial ?? props.tabs[0]?.id ?? "");

function select(id: string) {
  active.value = id;
}

// Roving-tabindex arrow-key navigation over the tab bar; wraps at both ends.
async function onKeydown(event: KeyboardEvent, index: number) {
  const n = props.tabs.length;
  let target = index;
  if (event.key === "ArrowRight" || event.key === "ArrowDown") target = (index + 1) % n;
  else if (event.key === "ArrowLeft" || event.key === "ArrowUp") target = (index - 1 + n) % n;
  else if (event.key === "Home") target = 0;
  else if (event.key === "End") target = n - 1;
  else return;
  event.preventDefault();
  active.value = props.tabs[target].id;
  await nextTick();
  // Move focus to the newly selected tab so keyboard nav keeps flowing.
  const bar = (event.currentTarget as HTMLElement).parentElement;
  bar?.querySelectorAll<HTMLElement>('[role="tab"]')[target]?.focus();
}
</script>

<template>
  <div>
    <div
      role="tablist"
      class="mb-3 flex gap-1 border-b border-white/10"
    >
      <button
        v-for="(t, i) in tabs"
        :id="`tab-${t.id}`"
        :key="t.id"
        type="button"
        role="tab"
        :data-testid="`tab-${t.id}`"
        :aria-selected="active === t.id"
        :aria-controls="`panel-${t.id}`"
        :tabindex="active === t.id ? 0 : -1"
        class="-mb-px cursor-pointer border-b-2 px-2 py-1 text-xs font-semibold transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :class="
          active === t.id
            ? 'border-violet-400 text-slate-100'
            : 'border-transparent text-slate-400 hover:text-slate-200'
        "
        @click="select(t.id)"
        @keydown="onKeydown($event, i)"
      >
        {{ t.label }}
      </button>
    </div>
    <div
      v-for="t in tabs"
      v-show="active === t.id"
      :id="`panel-${t.id}`"
      :key="t.id"
      role="tabpanel"
      :data-testid="`panel-${t.id}`"
      :aria-labelledby="`tab-${t.id}`"
    >
      <slot :name="t.id" />
    </div>
  </div>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/tab-group.test.ts`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/TabGroup.vue tests/tab-group.test.ts
git commit -m "feat(ui): add reusable TabGroup tab container"
```

---

### Task 4: Panel header save indicator

`ActionPanel` shows a transient `Saving…` / `Saved ✓` / `⚠ Couldn't save` beside the title while in a settings view, and resets the status on view change.

**Files:**
- Modify: `src/components/ActionPanel.vue` (script: add store + computeds + view watcher; template: header block ~lines 136-140)
- Test: `tests/action-panel.test.ts` (append a describe block)

**Interfaces:**
- Consumes: `useSettingsStatusStore` (Task 1), `store.view`.
- Produces: `[data-testid="save-status"]` span, present only when `view ∈ {settings, captureSettings}` and `state !== "idle"`.

- [ ] **Step 1: Write the failing test** (append to `tests/action-panel.test.ts`)

```ts
// --- append inside tests/action-panel.test.ts ---
import { useSettingsStatusStore } from "../src/stores/settingsStatus";

describe("ActionPanel save indicator", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("shows the transient save status only in a settings view", async () => {
    const store = useVaultsStore();
    store.loaded = true;
    store.openSettings(); // view = "settings"
    const status = useSettingsStatusStore();
    status.saving();
    const wrapper = mount(ActionPanel, { global: { stubs: { BuddySettings: true } } });
    expect(wrapper.get('[data-testid="save-status"]').text()).toContain("Saving");
    status.saved();
    await flushPromises();
    expect(wrapper.get('[data-testid="save-status"]').text()).toContain("Saved");
  });

  it("holds the error label when a save fails", () => {
    const store = useVaultsStore();
    store.loaded = true;
    store.openSettings();
    useSettingsStatusStore().failed("disk full");
    const wrapper = mount(ActionPanel, { global: { stubs: { BuddySettings: true } } });
    expect(wrapper.get('[data-testid="save-status"]').text()).toContain("Couldn't save");
  });

  it("hides the indicator on the vault list view", () => {
    const store = useVaultsStore();
    store.loaded = true;
    store.vaults = sampleVaults;
    useSettingsStatusStore().saving();
    const wrapper = mount(ActionPanel);
    expect(wrapper.find('[data-testid="save-status"]').exists()).toBe(false);
  });

  it("resets the status when the view changes", async () => {
    const store = useVaultsStore();
    store.loaded = true;
    store.openSettings();
    const status = useSettingsStatusStore();
    status.failed("boom");
    const wrapper = mount(ActionPanel, { global: { stubs: { BuddySettings: true } } });
    store.showList();
    await flushPromises();
    expect(status.state).toBe("idle");
  });
});
```

Note: `flushPromises` is already imported at the top of the file; `useSettingsStatusStore`, `afterEach`, `clearMocks` may need adding to existing imports.

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/action-panel.test.ts`
Expected: FAIL — no `[data-testid="save-status"]`.

- [ ] **Step 3: Add the script logic** to `src/components/ActionPanel.vue`

After the existing `const { view } = storeToRefs(store);` add:

```ts
import { useSettingsStatusStore } from "../stores/settingsStatus";

const saveStatus = useSettingsStatusStore();
const isSettingsView = computed(
  () => view.value === "settings" || view.value === "captureSettings",
);
const saveStatusLabel = computed(() => {
  if (saveStatus.state === "saving") return "Saving…";
  if (saveStatus.state === "saved") return "Saved ✓";
  if (saveStatus.state === "error") return "⚠ Couldn't save";
  return "";
});
// A stale saving/saved/error must not linger when navigating between views.
watch(
  () => store.view,
  () => saveStatus.reset(),
);
```

(`computed` and `watch` are already imported at the top of the file.)

- [ ] **Step 4: Add the indicator to the header template**

Replace the header title block:

```html
    <div class="mb-2 flex items-center justify-between">
      <h1 class="text-sm font-bold text-slate-100">
        {{ title }}
      </h1>
```

with:

```html
    <div class="mb-2 flex items-center justify-between gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <h1 class="truncate text-sm font-bold text-slate-100">
          {{ title }}
        </h1>
        <span
          v-if="isSettingsView && saveStatus.state !== 'idle'"
          data-testid="save-status"
          role="status"
          aria-live="polite"
          class="shrink-0 text-xs"
          :class="{
            'text-slate-400': saveStatus.state === 'saving',
            'text-emerald-300': saveStatus.state === 'saved',
            'text-red-300': saveStatus.state === 'error',
          }"
        >{{ saveStatusLabel }}</span>
      </div>
```

Then add `shrink-0` to the existing icon-group wrapper so the title truncates instead of the icons: change `<div class="flex items-center gap-2">` (the icon row, ~line 140) to `<div class="flex shrink-0 items-center gap-2">`. Leave the closing `</div>` structure intact (the new inner `<div>` is balanced by the block above; verify the header still closes with exactly the two `</div>`s it had — one for the icon row, one for the header row).

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/action-panel.test.ts`
Expected: PASS (existing tests still green + 4 new).

- [ ] **Step 6: Commit**

```bash
git add src/components/ActionPanel.vue tests/action-panel.test.ts
git commit -m "feat(ui): show a transient save-status indicator in the panel header"
```

---

### Task 5: Buddy settings tab groups

Wrap `BuddySettings`'s sections in `TabGroup`: **Buddy** (Character + Behavior) · **System** (autostart + Updates + Diagnostics) · **Integrations** (MCP + Document import). Script logic (autostart load, previews) is unchanged — only the template is regrouped.

**Files:**
- Modify: `src/components/BuddySettings.vue` (template only; import `TabGroup`)
- Test: `tests/buddy-settings.test.ts` (add tab-structure tests; existing tests keep passing because `v-show` leaves all content in the DOM)

**Interfaces:**
- Consumes: `TabGroup` (Task 3).

- [ ] **Step 1: Write the failing test** (append to `tests/buddy-settings.test.ts`)

```ts
it("groups settings into Buddy / System / Integrations tabs", () => {
  const wrapper = mount(BuddySettings);
  for (const id of ["buddy", "system", "integrations"]) {
    expect(wrapper.find(`[data-testid="tab-${id}"]`).exists()).toBe(true);
  }
  // Character grid lives under the (default) Buddy tab, visible on mount.
  expect(wrapper.get('[data-testid="panel-buddy"]').isVisible()).toBe(true);
});

it("puts the autostart control under the System tab and MCP under Integrations", () => {
  const wrapper = mount(BuddySettings);
  expect(
    wrapper.get('[data-testid="panel-system"]').find('[data-testid="autostart-toggle"]').exists(),
  ).toBe(true);
  expect(
    wrapper.get('[data-testid="panel-integrations"]').find('[data-testid="mcp-enabled"]').exists(),
  ).toBe(false); // McpSettings renders nothing until get_mcp_config resolves; presence checked by its own test
  expect(wrapper.get('[data-testid="panel-integrations"]').exists()).toBe(true);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/buddy-settings.test.ts`
Expected: FAIL — no `[data-testid="tab-buddy"]`.

- [ ] **Step 3: Regroup the template**

Add the import in `<script setup>`:

```ts
import TabGroup from "./TabGroup.vue";
```

Replace the template's root `<div class="flex flex-col gap-3"> … </div>` (the whole block containing the Character `<section>`, Behavior `<section>`, System `<section>`, and the `<UpdateSettings /> <DiagnosticsSettings /> <McpSettings /> <DocumentImportSettings />` line) with:

```html
  <TabGroup
    :tabs="[
      { id: 'buddy', label: 'Buddy' },
      { id: 'system', label: 'System' },
      { id: 'integrations', label: 'Integrations' },
    ]"
  >
    <template #buddy>
      <div class="flex flex-col gap-3">
        <!-- (Character section — unchanged, moved here verbatim) -->
        <!-- (Behavior section — unchanged, moved here verbatim) -->
      </div>
    </template>
    <template #system>
      <div class="flex flex-col gap-3">
        <!-- (System/autostart section — unchanged, moved here verbatim) -->
        <UpdateSettings />
        <DiagnosticsSettings />
      </div>
    </template>
    <template #integrations>
      <div class="flex flex-col gap-3">
        <McpSettings />
        <DocumentImportSettings />
      </div>
    </template>
  </TabGroup>
```

Move the three existing `<section>` blocks (Buddy character, Behavior, System) into the matching `<template>` slots **byte-for-byte** — do not restyle them. The `<script setup>` block (autostart refs, `onMounted`, `toggleAutostart`, `previewId`, `messageDuration`) is unchanged: `onMounted`'s `get_autostart` still runs on component mount, and the System panel (mounted via `v-show`) binds the loaded value.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/buddy-settings.test.ts`
Expected: PASS (all existing tests + 2 new). If any existing test fails because it asserted a single flat container, adjust that assertion to look inside the relevant `[data-testid="panel-*"]`.

- [ ] **Step 5: Commit**

```bash
git add src/components/BuddySettings.vue tests/buddy-settings.test.ts
git commit -m "feat(ui): group Buddy settings into Buddy/System/Integrations tabs"
```

---

### Task 6: `DocumentsConfigTab.vue` (auto-saved Documents tab)

Extract the Documents group into a self-contained tab component that loads `get_documents_config`, auto-saves via `set_documents_config`, and shows an inline load error (no editable fields) on read failure.

**Files:**
- Create: `src/components/DocumentsConfigTab.vue`
- Test: `tests/documents-config-tab.test.ts`

**Interfaces:**
- Consumes: `useAutosave` (Task 2), `VaultFolderSetting.vue` (existing), `DocumentsConfig` (types).
- Produces: component with prop `vaultId: string`. Emits IPC `set_documents_config { id, documentsFolder: string|null, documentDateFolders: boolean }`. Testids reused: `documents-folder-input`, `document-date-folders-toggle`, `documents-folder-error`; new `documents-load-error`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/documents-config-tab.test.ts
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import DocumentsConfigTab from "../src/components/DocumentsConfigTab.vue";

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
});
afterEach(() => {
  active?.unmount();
  active = null;
  vi.useRealTimers();
  clearMocks();
  document.body.innerHTML = "";
});

function mountTab(
  opts: { documentsFolder?: string | null; documentDateFolders?: boolean; onGet?: () => unknown; onSet?: (a: unknown) => unknown } = {},
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_documents_config")
      return opts.onGet
        ? opts.onGet()
        : { documentsFolder: opts.documentsFolder ?? null, documentDateFolders: opts.documentDateFolders ?? true };
    if (cmd === "set_documents_config") return opts.onSet?.(args) ?? null;
  });
  active = mount(DocumentsConfigTab, { props: { vaultId: "v1" }, attachTo: document.body });
  return { wrapper: active, calls };
}

describe("DocumentsConfigTab", () => {
  it("loads the folder and toggle from disk", async () => {
    const { wrapper } = mountTab({ documentsFolder: "Docs", documentDateFolders: false });
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="documents-folder-input"]').element.value).toBe("Docs");
    expect(wrapper.get<HTMLInputElement>('[data-testid="document-date-folders-toggle"]').element.checked).toBe(false);
  });

  it("does not save on mount", async () => {
    const { calls } = mountTab({ documentsFolder: "Docs" });
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_documents_config")).toBe(false);
  });

  it("debounces a folder edit and saves both fields after 600ms", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs", documentDateFolders: false });
    await flushPromises();
    await wrapper.get('[data-testid="documents-folder-input"]').setValue("Imported");
    expect(calls.some((c) => c.cmd === "set_documents_config")).toBe(false); // not yet
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Imported",
      documentDateFolders: false,
    });
  });

  it("saves the toggle immediately (no debounce)", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs", documentDateFolders: true });
    await flushPromises();
    await wrapper.get('[data-testid="document-date-folders-toggle"]').setValue(false);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Docs",
      documentDateFolders: false,
    });
  });

  it("flushes a pending folder save on blur", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs" });
    await flushPromises();
    const input = wrapper.get('[data-testid="documents-folder-input"]');
    await input.setValue("Imported");
    await input.trigger("blur"); // focusout bubbles to the tab container
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_documents_config")).toBe(true);
  });

  it("empties the folder to null on save", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs" });
    await flushPromises();
    await wrapper.get('[data-testid="documents-folder-input"]').setValue("");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toMatchObject({
      documentsFolder: null,
    });
  });

  it("shows a save error inline and keeps the value", async () => {
    const { wrapper } = mountTab({
      documentsFolder: "Docs",
      onSet: () => {
        throw "Configured documents folder must stay inside the vault";
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="document-date-folders-toggle"]').setValue(false);
    await flushPromises();
    expect(wrapper.get('[data-testid="documents-folder-error"]').text()).toContain("inside the vault");
  });

  it("shows a load error and no editable fields when the read fails", async () => {
    const { wrapper } = mountTab({
      onGet: () => {
        throw "config unreadable";
      },
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="documents-load-error"]').text()).toContain("config unreadable");
    expect(wrapper.find('[data-testid="documents-folder-input"]').exists()).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/documents-config-tab.test.ts`
Expected: FAIL — cannot resolve `../src/components/DocumentsConfigTab.vue`.

- [ ] **Step 3: Write the component**

```vue
<!-- src/components/DocumentsConfigTab.vue -->
<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { DocumentsConfig } from "../types";
import VaultFolderSetting from "./VaultFolderSetting.vue";

// The Documents tab of Vault settings. Self-contained: loads its own config,
// auto-saves both fields (folder + date-folders toggle) through the one
// set_documents_config command. A failed read shows an inline error and NO
// editable fields, so a seeded default can never be auto-saved over a value we
// failed to read.
const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const loadError = ref<string | null>(null);
const documentsFolder = ref("");
const documentDateFolders = ref(true);

const autosave = useAutosave(
  async () => {
    await invoke("set_documents_config", {
      id: props.vaultId,
      documentsFolder: documentsFolder.value.trim() || null,
      documentDateFolders: documentDateFolders.value,
    });
  },
  { label: "documents settings" },
);

onMounted(async () => {
  try {
    const cfg = await invoke<DocumentsConfig>("get_documents_config", { id: props.vaultId });
    documentsFolder.value = cfg.documentsFolder ?? "";
    documentDateFolders.value = cfg.documentDateFolders;
  } catch (e) {
    loadError.value = String(e);
    logWarning(`get_documents_config failed (vault ${props.vaultId}): ${String(e)}`);
  } finally {
    loading.value = false;
  }
});

// Typed folder edits debounce; the toggle saves immediately. onMounted assigns
// the refs directly (not via these handlers), so neither fires on load.
function onFolderInput(value: string) {
  documentsFolder.value = value;
  autosave.schedule();
}
function onToggle(event: Event) {
  documentDateFolders.value = (event.target as HTMLInputElement).checked;
  autosave.saveNow();
}
</script>

<template>
  <!-- focusout flushes a pending debounced folder save when focus leaves. -->
  <div
    class="flex flex-col gap-3"
    @focusout="autosave.flush()"
  >
    <p
      v-if="loading"
      class="text-xs text-slate-400"
    >
      Loading…
    </p>
    <p
      v-else-if="loadError"
      data-testid="documents-load-error"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ loadError }}
    </p>
    <template v-else>
      <VaultFolderSetting
        :model-value="documentsFolder"
        heading="Documents folder"
        label="Documents folder"
        placeholder="Documents"
        input-id="documents-folder"
        input-testid="documents-folder-input"
        error-testid="documents-folder-error"
        :error="autosave.error.value"
        @update:model-value="onFolderInput"
      />
      <div class="flex items-center justify-between rounded-xl border border-white/10 bg-white/5 p-2">
        <label
          for="document-date-folders"
          class="text-sm text-slate-200"
        >
          Organize into year/month folders
          <span class="block text-xs text-slate-500">Off = one flat folder</span>
        </label>
        <input
          id="document-date-folders"
          data-testid="document-date-folders-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="documentDateFolders"
          @change="onToggle"
        >
      </div>
    </template>
  </div>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/documents-config-tab.test.ts`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/DocumentsConfigTab.vue tests/documents-config-tab.test.ts
git commit -m "feat(ui): add auto-saved Documents settings tab"
```

---

### Task 7: `TaskListSettings.vue` → auto-save

Replace the Save button + `saveState`/`editGen` machinery with `useAutosave`: the default-list pick and each reorder move save immediately.

**Files:**
- Modify: `src/components/TaskListSettings.vue`
- Test: `tests/task-list-settings.test.ts` (rewrite the save-flow tests)

**Interfaces:**
- Consumes: `useAutosave` (Task 2). Still invokes `set_task_lists_config { id, defaultList: string|null, listOrder: string[] }`.
- Produces: same load behavior; removes `[data-testid="task-lists-save"]`; keeps `[data-testid="task-lists-error"]`, `[data-testid="default-list"]`, `[data-testid="list-order-*"]`.

- [ ] **Step 1: Rewrite the failing tests** — replace the bodies of the save-related `it(...)` blocks in `tests/task-list-settings.test.ts` (keep "loads the vault's lists…" and "offers a hint…" unchanged; the mount helper still returns `{ wrapper, calls }`):

```ts
  it("saves the new order immediately after a reorder (no Save button)", async () => {
    const { wrapper, calls } = mountSettings();
    await flushPromises();
    expect(wrapper.find('[data-testid="task-lists-save"]').exists()).toBe(false);
    await wrapper.get('[data-testid="list-order-up-2"]').trigger("click"); // Waiting up one
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_lists_config")?.args).toEqual({
      id: "v1",
      defaultList: "Inbox",
      listOrder: ["Next", "Waiting", "Inbox"],
    });
  });

  it("saves immediately when the default list changes", async () => {
    const { wrapper, calls } = mountSettings();
    await flushPromises();
    await wrapper.get('[data-testid="default-list"]').trigger("click");
    await flushPromises();
    (document.body.querySelector('[data-testid="default-list-option-Waiting"]') as HTMLElement).click();
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_lists_config")?.args).toMatchObject({
      defaultList: "Waiting",
    });
  });

  it("clearing the default sends null", async () => {
    const { wrapper, calls } = mountSettings();
    await flushPromises();
    await wrapper.get('[data-testid="default-list"]').trigger("click");
    await flushPromises();
    (document.body.querySelector('[data-testid="default-list-option-"]') as HTMLElement).click();
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_task_lists_config")?.args).toMatchObject({
      defaultList: null,
    });
  });

  it("shows a field-level error when the save fails", async () => {
    const { wrapper } = mountSettings({
      set_task_lists_config: () => {
        throw new Error("List path must stay inside the tasks folder");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="list-order-up-2"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="task-lists-error"]').text()).toContain("inside the tasks folder");
  });
```

Delete the three obsolete Save-button/"Saved"-acknowledgement tests ("moves a list up and saves the full settings object", "clears the Saved acknowledgement…", "does not show Saved when the form is edited while the save is in flight…") — the shared header status and `useAutosave`'s coalescing replace that per-card machinery (covered by Tasks 1, 2, 4). Add `setActivePinia(createPinia())` to a `beforeEach` and import `createPinia, setActivePinia` from `pinia` (TaskListSettings now uses `useAutosave` → the settingsStatus store).

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/task-list-settings.test.ts`
Expected: FAIL — `task-lists-save` still exists / no autosave yet.

- [ ] **Step 3: Rewrite the component script**

Replace the `<script setup>` of `src/components/TaskListSettings.vue` with:

```ts
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { TasksConfig } from "../types";
import { orderLists } from "../utils/taskSections";
import TaskListPicker from "./TaskListPicker.vue";

// The per-vault lists settings object (defaultList + listOrder), rendered
// inside the Vault settings Tasks tab. Self-contained (own load) and
// auto-saved: a default-list pick or a reorder saves immediately through
// set_task_lists_config. Folders on disk stay the source of truth for which
// lists exist; this card only edits preferences about them.
const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const defaultList = ref("");
const order = ref<string[]>([]);

const autosave = useAutosave(
  async () => {
    await invoke("set_task_lists_config", {
      id: props.vaultId,
      defaultList: defaultList.value || null,
      listOrder: order.value,
    });
  },
  { label: "task lists" },
);

onMounted(async () => {
  try {
    const [cfg, lists] = await Promise.all([
      invoke<TasksConfig>("get_tasks_config", { id: props.vaultId }),
      invoke<string[]>("list_task_lists", { id: props.vaultId }),
    ]);
    defaultList.value = cfg?.defaultList ?? "";
    order.value = orderLists(
      Array.isArray(lists) ? lists : [],
      Array.isArray(cfg?.listOrder) ? cfg.listOrder : [],
    );
  } catch (e) {
    logWarning(`task list settings load failed: ${String(e)}`);
  } finally {
    loading.value = false;
  }
});

// The picker and the reorder buttons fire only on user action (onMounted
// assigns the refs directly), so saveNow() here never fires on load.
function onDefaultChange(value: string) {
  defaultList.value = value;
  autosave.saveNow();
}
function move(index: number, delta: -1 | 1) {
  const target = index + delta;
  if (target < 0 || target >= order.value.length) return;
  const next = [...order.value];
  [next[index], next[target]] = [next[target], next[index]];
  order.value = next;
  autosave.saveNow();
}
```

- [ ] **Step 4: Rewrite the component template**

In `src/components/TaskListSettings.vue`, (a) bind the picker to the change handler:

```html
        <TaskListPicker
          :model-value="defaultList"
          :lists="order"
          :allow-create="false"
          aria-label="Default list for new tasks"
          data-testid="default-list"
          @update:model-value="onDefaultChange"
        />
```

(b) Delete the whole save-button block:

```html
        <div class="mt-2 flex items-center gap-2">
          <button ... data-testid="task-lists-save" ...>...</button>
          <span v-if="saveState === 'saved'" ...>Saved</span>
        </div>
```

(c) Replace the error line's condition to read `autosave.error.value`:

```html
        <p
          v-if="autosave.error.value"
          data-testid="task-lists-error"
          class="mt-1 text-xs text-red-300"
        >
          {{ autosave.error.value }}
        </p>
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/task-list-settings.test.ts`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/TaskListSettings.vue tests/task-list-settings.test.ts
git commit -m "refactor(ui): auto-save the Task lists settings card"
```

---

### Task 8: `TasksConfigTab.vue` (auto-saved Tasks tab)

Extract the Tasks group: the tasks-folder text field (auto-saved via `set_tasks_config`) plus the embedded `TaskListSettings` card, remounting the card when the persisted folder changes.

**Files:**
- Create: `src/components/TasksConfigTab.vue`
- Test: `tests/tasks-config-tab.test.ts`

**Interfaces:**
- Consumes: `useAutosave` (Task 2), `VaultFolderSetting`, `TaskListSettings` (Task 7), `TasksConfig` (types).
- Produces: prop `vaultId: string`. Invokes `set_tasks_config { id, tasksFolder: string|null }`. Testids: `tasks-folder-input`, `tasks-folder-error`, new `tasks-load-error`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/tasks-config-tab.test.ts
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import TasksConfigTab from "../src/components/TasksConfigTab.vue";

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
});
afterEach(() => {
  active?.unmount();
  active = null;
  vi.useRealTimers();
  clearMocks();
  document.body.innerHTML = "";
});

function mountTab(
  opts: { tasksFolder?: string | null; onGet?: () => unknown; onSet?: (a: unknown) => unknown; onListLists?: () => unknown } = {},
) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_tasks_config")
      return opts.onGet ? opts.onGet() : { tasksFolder: opts.tasksFolder ?? null, defaultList: null, listOrder: [] };
    if (cmd === "list_task_lists") return opts.onListLists?.() ?? [];
    if (cmd === "set_tasks_config") return opts.onSet?.(args) ?? null;
    if (cmd === "set_task_lists_config") return null;
  });
  active = mount(TasksConfigTab, { props: { vaultId: "v1" }, attachTo: document.body });
  return { wrapper: active, calls };
}

describe("TasksConfigTab", () => {
  it("loads the tasks folder from disk", async () => {
    const { wrapper } = mountTab({ tasksFolder: "Inbox/Tasks" });
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="tasks-folder-input"]').element.value).toBe("Inbox/Tasks");
  });

  it("does not save on mount", async () => {
    const { calls } = mountTab({ tasksFolder: "Inbox/Tasks" });
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_tasks_config")).toBe(false);
  });

  it("debounces a folder edit and trims on save", async () => {
    const { wrapper, calls } = mountTab({ tasksFolder: "Tasks" });
    await flushPromises();
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("  Work/Tasks  ");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")?.args).toEqual({ id: "v1", tasksFolder: "Work/Tasks" });
  });

  it("empties to null on save", async () => {
    const { wrapper, calls } = mountTab({ tasksFolder: "Tasks" });
    await flushPromises();
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")?.args).toEqual({ id: "v1", tasksFolder: null });
  });

  it("remounts the lists card only when the persisted folder changes", async () => {
    let lists = ["OldList", "OldToo"];
    const { wrapper, calls } = mountTab({ tasksFolder: "Tasks", onListLists: () => lists });
    await flushPromises();
    const cardLoads = () => calls.filter((c) => c.cmd === "list_task_lists").length;
    const before = cardLoads();
    expect(before).toBeGreaterThan(0);
    lists = ["NewList", "NewToo"];
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Other/Tasks");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(cardLoads()).toBe(before + 1); // remounted → re-read the lists
    // A second save with the folder unchanged does not remount.
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Other/Tasks");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(cardLoads()).toBe(before + 1);
  });

  it("shows a save error inline", async () => {
    const { wrapper } = mountTab({
      tasksFolder: "Tasks",
      onSet: () => {
        throw "Configured tasks folder must stay inside the vault";
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("../x");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(wrapper.get('[data-testid="tasks-folder-error"]').text()).toContain("inside the vault");
  });

  it("shows a load error (no folder input) but still renders the lists card when the read fails", async () => {
    const { wrapper } = mountTab({
      onGet: () => {
        throw "config unreadable";
      },
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="tasks-load-error"]').text()).toContain("config unreadable");
    expect(wrapper.find('[data-testid="tasks-folder-input"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("Task lists"); // TaskListSettings still mounted
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/tasks-config-tab.test.ts`
Expected: FAIL — cannot resolve `../src/components/TasksConfigTab.vue`.

- [ ] **Step 3: Write the component**

```vue
<!-- src/components/TasksConfigTab.vue -->
<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { TasksConfig } from "../types";
import TaskListSettings from "./TaskListSettings.vue";
import VaultFolderSetting from "./VaultFolderSetting.vue";

// The Tasks tab of Vault settings: the per-vault tasks folder (auto-saved via
// set_tasks_config) plus the self-contained TaskListSettings card. A failed
// folder read shows an inline error and no folder input (so a seed can't be
// saved over an unread value), but the lists card — which loads independently
// — still renders.
const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const loadError = ref<string | null>(null);
const tasksFolder = ref("");
// Last value known persisted (null = tasks root / none). A save that changes
// it remounts the lists card so its lists reload against the new root — else a
// default/order save from the stale card would target the old root.
const savedFolder = ref<string | null>(null);
const listsNonce = ref(0);

const autosave = useAutosave(
  async () => {
    const value = tasksFolder.value.trim() || null;
    await invoke("set_tasks_config", { id: props.vaultId, tasksFolder: value });
    if (value !== savedFolder.value) {
      savedFolder.value = value;
      listsNonce.value += 1;
    }
  },
  { label: "tasks folder" },
);

onMounted(async () => {
  try {
    const cfg = await invoke<TasksConfig>("get_tasks_config", { id: props.vaultId });
    tasksFolder.value = cfg.tasksFolder ?? "";
    savedFolder.value = cfg.tasksFolder ?? null;
  } catch (e) {
    loadError.value = String(e);
    logWarning(`get_tasks_config failed (vault ${props.vaultId}): ${String(e)}`);
  } finally {
    loading.value = false;
  }
});

function onFolderInput(value: string) {
  tasksFolder.value = value;
  autosave.schedule();
}
</script>

<template>
  <div
    class="flex flex-col gap-3"
    @focusout="autosave.flush()"
  >
    <p
      v-if="loading"
      class="text-xs text-slate-400"
    >
      Loading…
    </p>
    <template v-else>
      <p
        v-if="loadError"
        data-testid="tasks-load-error"
        class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
      >
        {{ loadError }}
      </p>
      <VaultFolderSetting
        v-else
        :model-value="tasksFolder"
        heading="Tasks folder"
        label="Tasks folder"
        placeholder="Tasks"
        input-id="tasks-folder"
        input-testid="tasks-folder-input"
        error-testid="tasks-folder-error"
        :error="autosave.error.value"
        @update:model-value="onFolderInput"
      />
      <!-- Self-contained (own load/save); remounts on a persisted folder change. -->
      <TaskListSettings
        :key="listsNonce"
        :vault-id="vaultId"
      />
    </template>
  </div>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/tasks-config-tab.test.ts`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/TasksConfigTab.vue tests/tasks-config-tab.test.ts
git commit -m "feat(ui): add auto-saved Tasks settings tab"
```

---

### Task 9: `RecordingConfigTab.vue` (auto-saved Recording tab)

Extract the Recording group: load `get_capture_config` + `list_audio_devices`, host the controlled `RecordingSettings`, and auto-save the whole `set_capture_config` struct — debouncing folder edits, saving toggles/selects immediately.

**Files:**
- Create: `src/components/RecordingConfigTab.vue`
- Test: `tests/recording-config-tab.test.ts`

**Interfaces:**
- Consumes: `useAutosave` (Task 2), `RecordingSettings.vue` (existing controlled component, `RecordingSettingsValue` bundle shape), `CaptureConfig`/`AudioDevices` (types).
- Produces: prop `vaultId: string`. Invokes `set_capture_config { id, cfg: {...} }` (`mode` preserved as loaded). Testids reused from `RecordingSettings`: `meeting-folder-input`, `voice-note-folder-input`, `bitrate-select`, `input-device-select`, `note-toggle`, `recording-date-folders-toggle`, `folder-error`; new `recording-load-error`, `recording-form-error`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/recording-config-tab.test.ts
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import RecordingConfigTab from "../src/components/RecordingConfigTab.vue";

const config = {
  mode: "meeting",
  meetingFolder: "Meetings",
  voiceNoteFolder: "Voice Notes",
  bitrateKbps: 160,
  createNote: true,
  inputDevice: "USB Mic",
  outputDevice: null,
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: null as string | null,
  transcriptTimestamps: true,
  followUpTemplate: true,
  recordingDateFolders: true,
};
const devices = {
  inputs: [{ name: "USB Mic", isDefault: false }],
  outputs: [{ name: "Speakers", isDefault: true }],
};

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
});
afterEach(() => {
  active?.unmount();
  active = null;
  vi.useRealTimers();
  clearMocks();
  document.body.innerHTML = "";
});

function mountTab(opts: { config?: Partial<typeof config>; onSet?: (a: unknown) => unknown; onGet?: () => unknown } = {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config") return opts.onGet ? opts.onGet() : { ...config, ...opts.config };
    if (cmd === "list_audio_devices") return devices;
    if (cmd === "set_capture_config") return opts.onSet?.(args) ?? null;
  });
  active = mount(RecordingConfigTab, { props: { vaultId: "v1" }, attachTo: document.body });
  return { wrapper: active, calls };
}

const pick = async (wrapper: ReturnType<typeof mount>, testid: string, value: string | number) => {
  await wrapper.get(`[data-testid="${testid}"]`).trigger("click");
  (document.body.querySelector(`[data-testid="${testid}-option-${value}"]`) as HTMLElement).click();
  await flushPromises();
};

describe("RecordingConfigTab", () => {
  it("loads the config into the form", async () => {
    const { wrapper } = mountTab();
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="meeting-folder-input"]').element.value).toBe("Meetings");
  });

  it("does not save on mount", async () => {
    const { calls } = mountTab();
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);
  });

  it("debounces a folder edit and saves the whole struct with mode preserved", async () => {
    const { wrapper, calls } = mountTab();
    await flushPromises();
    await wrapper.get('[data-testid="meeting-folder-input"]').setValue("Inbox/Audio");
    expect(calls.some((c) => c.cmd === "set_capture_config")).toBe(false);
    vi.advanceTimersByTime(600);
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config");
    expect(set?.args).toMatchObject({
      id: "v1",
      cfg: { mode: "meeting", meetingFolder: "Inbox/Audio", voiceNoteFolder: "Voice Notes", recordingDateFolders: true },
    });
  });

  it("saves a toggle immediately (no debounce)", async () => {
    const { wrapper, calls } = mountTab();
    await flushPromises();
    await wrapper.get('[data-testid="recording-date-folders-toggle"]').setValue(false);
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config") as { args: { cfg: { recordingDateFolders: boolean } } };
    expect(set.args.cfg.recordingDateFolders).toBe(false);
  });

  it("saves a select change immediately", async () => {
    const { calls, wrapper } = mountTab();
    await flushPromises();
    await pick(wrapper, "bitrate-select", 192);
    const set = calls.find((c) => c.cmd === "set_capture_config") as { args: { cfg: { bitrateKbps: number } } };
    expect(set.args.cfg.bitrateKbps).toBe(192);
  });

  it("routes a folder rejection to the inline folder error", async () => {
    const { wrapper } = mountTab({
      onSet: () => {
        throw 'Configured recording folder must stay inside the vault: "../x"';
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="meeting-folder-input"]').setValue("../x");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(wrapper.get('[data-testid="folder-error"]').text()).toContain("must stay inside the vault");
  });

  it("routes a non-folder failure to a form error", async () => {
    const { wrapper } = mountTab({
      onSet: () => {
        throw "Could not save capture settings: disk full";
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="recording-date-folders-toggle"]').setValue(false);
    await flushPromises();
    expect(wrapper.get('[data-testid="recording-form-error"]').text()).toContain("disk full");
  });

  it("shows a load error and no form when the read fails", async () => {
    const { wrapper } = mountTab({
      onGet: () => {
        throw "config unreadable";
      },
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="recording-load-error"]').text()).toContain("config unreadable");
    expect(wrapper.find('[data-testid="meeting-folder-input"]').exists()).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/recording-config-tab.test.ts`
Expected: FAIL — cannot resolve `../src/components/RecordingConfigTab.vue`.

- [ ] **Step 3: Write the component**

```vue
<!-- src/components/RecordingConfigTab.vue -->
<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { useAutosave } from "../composables/useAutosave";
import { logWarning } from "../logging";
import type { AudioDevices, CaptureConfig } from "../types";
import RecordingSettings from "./RecordingSettings.vue";

interface RecordingValue {
  meetingFolder: string;
  voiceNoteFolder: string;
  bitrateKbps: number;
  createNote: boolean;
  followUpTemplate: boolean;
  inputDevice: string;
  outputDevice: string;
  transcribe: boolean;
  transcriptionModel: string;
  transcriptionLanguage: string;
  transcriptTimestamps: boolean;
  recordingDateFolders: boolean;
}

// The Recording tab of Vault settings. Owns the capture-config + devices load,
// hosts the controlled RecordingSettings, and auto-saves the whole
// set_capture_config struct. Folder text debounces; every other control
// (toggles/selects) saves immediately. `mode` is a pass-through — the UI can't
// edit it, but the loaded value is sent back unchanged.
const props = defineProps<{ vaultId: string }>();

const loading = ref(true);
const loadError = ref<string | null>(null);
const mode = ref<"meeting" | "voice-note">("meeting");
const devices = ref<AudioDevices>({ inputs: [], outputs: [] });
const rec = ref<RecordingValue>({
  meetingFolder: "",
  voiceNoteFolder: "",
  bitrateKbps: 128,
  createNote: true,
  followUpTemplate: true,
  inputDevice: "",
  outputDevice: "",
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: "",
  transcriptTimestamps: true,
  recordingDateFolders: true,
});

const autosave = useAutosave(
  async () => {
    const r = rec.value;
    await invoke("set_capture_config", {
      id: props.vaultId,
      cfg: {
        mode: mode.value,
        meetingFolder: r.meetingFolder.trim() || null,
        voiceNoteFolder: r.voiceNoteFolder.trim() || null,
        bitrateKbps: r.bitrateKbps,
        createNote: r.createNote,
        followUpTemplate: r.followUpTemplate,
        inputDevice: r.inputDevice || null,
        outputDevice: r.outputDevice || null,
        transcribe: r.transcribe,
        transcriptionModel: r.transcriptionModel,
        transcriptionLanguage: r.transcriptionLanguage.trim() || null,
        transcriptTimestamps: r.transcriptTimestamps,
        recordingDateFolders: r.recordingDateFolders,
      },
    });
  },
  { label: "capture settings" },
);

// Folders are the only free-text fields → debounce; everything else is a
// toggle/select → save immediately. RecordingSettings emits the whole bundle
// on any change and only on user interaction (never on the load assignment
// below), so this handler is a safe single trigger point.
function onUpdate(next: RecordingValue) {
  const cur = rec.value;
  const onlyFolders =
    (next.meetingFolder !== cur.meetingFolder || next.voiceNoteFolder !== cur.voiceNoteFolder) &&
    next.bitrateKbps === cur.bitrateKbps &&
    next.createNote === cur.createNote &&
    next.followUpTemplate === cur.followUpTemplate &&
    next.inputDevice === cur.inputDevice &&
    next.outputDevice === cur.outputDevice &&
    next.transcribe === cur.transcribe &&
    next.transcriptionModel === cur.transcriptionModel &&
    next.transcriptionLanguage === cur.transcriptionLanguage &&
    next.transcriptTimestamps === cur.transcriptTimestamps &&
    next.recordingDateFolders === cur.recordingDateFolders;
  rec.value = next;
  if (onlyFolders) autosave.schedule();
  else autosave.saveNow();
}

// Split the one autosave error into the inline folder line vs a form-level
// line, preserving the pre-autosave UX.
const folderError = computed(() =>
  autosave.error.value && autosave.error.value.toLowerCase().includes("folder") ? autosave.error.value : null,
);
const formError = computed(() =>
  autosave.error.value && !autosave.error.value.toLowerCase().includes("folder") ? autosave.error.value : null,
);

onMounted(async () => {
  try {
    const [cfg, devs] = await Promise.all([
      invoke<CaptureConfig>("get_capture_config", { id: props.vaultId }),
      invoke<AudioDevices>("list_audio_devices"),
    ]);
    mode.value = cfg.mode;
    rec.value = {
      meetingFolder: cfg.meetingFolder ?? "",
      voiceNoteFolder: cfg.voiceNoteFolder ?? "",
      bitrateKbps: cfg.bitrateKbps,
      createNote: cfg.createNote,
      followUpTemplate: cfg.followUpTemplate,
      inputDevice: cfg.inputDevice ?? "",
      outputDevice: cfg.outputDevice ?? "",
      transcribe: cfg.transcribe,
      transcriptionModel: cfg.transcriptionModel,
      transcriptionLanguage: cfg.transcriptionLanguage ?? "",
      transcriptTimestamps: cfg.transcriptTimestamps,
      recordingDateFolders: cfg.recordingDateFolders,
    };
    devices.value = devs;
  } catch (e) {
    loadError.value = String(e);
    logWarning(`get_capture_config failed (vault ${props.vaultId}): ${String(e)}`);
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <div
    class="flex flex-col gap-3"
    @focusout="autosave.flush()"
  >
    <p
      v-if="loading"
      class="text-xs text-slate-400"
    >
      Loading…
    </p>
    <p
      v-else-if="loadError"
      data-testid="recording-load-error"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ loadError }}
    </p>
    <template v-else>
      <RecordingSettings
        :model-value="rec"
        :devices="devices"
        :folder-error="folderError"
        @update:model-value="onUpdate"
      />
      <p
        v-if="formError"
        data-testid="recording-form-error"
        class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
      >
        {{ formError }}
      </p>
    </template>
  </div>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/recording-config-tab.test.ts`
Expected: PASS (8 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/RecordingConfigTab.vue tests/recording-config-tab.test.ts
git commit -m "feat(ui): add auto-saved Recording settings tab"
```

---

### Task 10: Assemble the `CaptureSettings` shell + delete `useOptionalFolderField`

Replace `CaptureSettings.vue` with a thin `TabGroup` shell over the three tab components, rewrite `capture-settings.test.ts` as a shell test, and delete the now-unused `useOptionalFolderField` composable.

**Files:**
- Modify (replace): `src/components/CaptureSettings.vue`
- Delete: `src/composables/useOptionalFolderField.ts`
- Rewrite: `tests/capture-settings.test.ts`
- Verify no other importers: `src/composables/useOptionalFolderField.ts`, old CaptureSettings testids.

**Interfaces:**
- Consumes: `TabGroup` (Task 3), `RecordingConfigTab` (Task 9), `TasksConfigTab` (Task 8), `DocumentsConfigTab` (Task 6).

- [ ] **Step 1: Confirm nothing else imports the composable or the old testids**

Run:
```bash
rg -n "useOptionalFolderField" src tests
rg -n "save-button|save-error" src tests
```
Expected: `useOptionalFolderField` only in `src/composables/useOptionalFolderField.ts` + `src/components/CaptureSettings.vue`; `save-button`/`save-error` only in `src/components/CaptureSettings.vue` + `tests/capture-settings.test.ts` (both replaced below). If anything else references them, stop and reconcile before continuing.

- [ ] **Step 2: Write the failing shell test** — replace the entire contents of `tests/capture-settings.test.ts` with:

```ts
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logBreadcrumb: vi.fn(), logWarning: vi.fn() }));

import CaptureSettings from "../src/components/CaptureSettings.vue";

const config = {
  mode: "meeting",
  meetingFolder: "Meetings",
  voiceNoteFolder: "Voice Notes",
  bitrateKbps: 160,
  createNote: true,
  inputDevice: "USB Mic",
  outputDevice: null,
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: null as string | null,
  transcriptTimestamps: true,
  followUpTemplate: true,
  recordingDateFolders: true,
};
const devices = { inputs: [{ name: "USB Mic", isDefault: false }], outputs: [{ name: "Speakers", isDefault: true }] };

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
});
afterEach(() => {
  active?.unmount();
  active = null;
  clearMocks();
  document.body.innerHTML = "";
});

function mountShell() {
  mockIPC((cmd) => {
    if (cmd === "get_capture_config") return config;
    if (cmd === "list_audio_devices") return devices;
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [] };
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_documents_config") return { documentsFolder: null, documentDateFolders: true };
  });
  active = mount(CaptureSettings, { props: { vaultId: "v1" }, attachTo: document.body });
  return active;
}

describe("CaptureSettings shell", () => {
  it("renders Recording / Tasks / Documents tabs", async () => {
    const wrapper = mountShell();
    await flushPromises();
    for (const id of ["recording", "tasks", "documents"]) {
      expect(wrapper.find(`[data-testid="tab-${id}"]`).exists()).toBe(true);
    }
  });

  it("shows the Recording tab by default with its form loaded", async () => {
    const wrapper = mountShell();
    await flushPromises();
    expect(wrapper.get('[data-testid="panel-recording"]').isVisible()).toBe(true);
    expect(wrapper.get<HTMLInputElement>('[data-testid="meeting-folder-input"]').element.value).toBe("Meetings");
  });

  it("reveals the Documents tab content on click", async () => {
    const wrapper = mountShell();
    await flushPromises();
    await wrapper.get('[data-testid="tab-documents"]').trigger("click");
    expect(wrapper.get('[data-testid="panel-documents"]').isVisible()).toBe(true);
    expect(wrapper.find('[data-testid="documents-folder-input"]').exists()).toBe(true);
  });

  it("no longer renders a Save button", async () => {
    const wrapper = mountShell();
    await flushPromises();
    expect(wrapper.find('[data-testid="save-button"]').exists()).toBe(false);
  });
});
```

(Behavioral coverage of each tab's load/save now lives in the per-tab test files from Tasks 6–9; this shell test only proves assembly.)

- [ ] **Step 3: Run test to verify it fails**

Run: `npx vitest run tests/capture-settings.test.ts`
Expected: FAIL — `CaptureSettings` still renders the old monolithic form (no `tab-recording`).

- [ ] **Step 4: Replace `CaptureSettings.vue`**

```vue
<!-- src/components/CaptureSettings.vue -->
<script setup lang="ts">
import DocumentsConfigTab from "./DocumentsConfigTab.vue";
import RecordingConfigTab from "./RecordingConfigTab.vue";
import TabGroup from "./TabGroup.vue";
import TasksConfigTab from "./TasksConfigTab.vue";

// Vault settings shell: three auto-saving tabs, each self-contained (own load +
// autosave). No Save button — edits persist on blur/debounce (folders) or
// immediately (toggles/selects); the panel header shows the transient status.
defineProps<{ vaultId: string }>();

const TABS = [
  { id: "recording", label: "Recording" },
  { id: "tasks", label: "Tasks" },
  { id: "documents", label: "Documents" },
];
</script>

<template>
  <TabGroup :tabs="TABS">
    <template #recording>
      <RecordingConfigTab :vault-id="vaultId" />
    </template>
    <template #tasks>
      <TasksConfigTab :vault-id="vaultId" />
    </template>
    <template #documents>
      <DocumentsConfigTab :vault-id="vaultId" />
    </template>
  </TabGroup>
</template>
```

- [ ] **Step 5: Delete the unused composable**

```bash
git rm src/composables/useOptionalFolderField.ts
```

- [ ] **Step 6: Run the affected tests + typecheck**

Run:
```bash
npx vitest run tests/capture-settings.test.ts tests/tasks-config-tab.test.ts tests/documents-config-tab.test.ts tests/recording-config-tab.test.ts
npm run build
```
Expected: all PASS; `vue-tsc` typecheck clean (no dangling `useOptionalFolderField` import).

- [ ] **Step 7: Commit**

```bash
git add src/components/CaptureSettings.vue tests/capture-settings.test.ts
git commit -m "refactor(ui): make Vault settings an auto-saving 3-tab shell"
```

---

### Task 11: Full quality gate + baselines

Run the whole frontend gate, update the LOC baseline if a file moved above/below its cap, and re-floor coverage if it rose.

**Files:**
- Possibly modify: `scripts/loc-baseline.json`, `vite.config.ts` (coverage thresholds) — only if the gate reports a shrink to record.

- [ ] **Step 1: Run the full Vitest suite**

Run: `npm test`
Expected: PASS. Fix any cross-file fallout (e.g. a `panel-root`/`buddy-root` test asserting old settings structure). Do not weaken assertions to pass — correct them to the new structure.

- [ ] **Step 2: Lint**

Run: `npm run lint`
Expected: clean. Resolve any `import/order`, unused-import (deleted composable), or complexity warnings.

- [ ] **Step 3: LOC guard**

Run: `npm run check:loc`
Expected: PASS. `CaptureSettings.vue` shrank well under the cap; the new tab files are all small. If the guard reports a recordable shrink, run `npm run check:loc -- --update` and stage `scripts/loc-baseline.json`.

- [ ] **Step 4: Quality ratchet**

Run: `npm run check:quality`
Expected: PASS (run with no `coverage/` dir present — this is why it precedes coverage). If it reports an improvement to record, run the documented `--update` and stage `scripts/quality-baseline.json`.

- [ ] **Step 5: Coverage floors**

Run: `npm run test:coverage`
Expected: PASS at or above statements 93 / branches 90 / functions 90 / lines 95. If coverage rose, raise the floors in `vite.config.ts` to the new numbers (rise-only ratchet) in this commit.

- [ ] **Step 6: Commit any baseline updates**

```bash
git add -A
git commit -m "chore(ui): re-floor coverage/LOC baselines after settings autosave+tabs"
```

(If no baselines changed, skip this commit.)

- [ ] **Step 7: Push and open the PR**

```bash
git push -u origin claude/settings-autosave-tabs-ni7qt5
```
Then open a ready-for-review PR against `main` titled `feat(ui): auto-save settings + tab groups`, body summarizing: the two settings views now auto-save (blur+debounce for text, immediate for toggles) with a header status indicator, and both are split into 3 tabs; `CaptureSettings` split into `RecordingConfigTab`/`TasksConfigTab`/`DocumentsConfigTab`; `useOptionalFolderField` deleted as the load/edited race-guards collapsed. Reference the spec path.

---

## Self-Review

**Spec coverage:**
- Auto-save trigger (blur + debounce, toggles immediate) → Tasks 2, 6, 8, 9. ✓
- Header transient status → Tasks 1, 4. ✓
- Buddy tabs (Buddy/System/Integrations) → Task 5. ✓
- Vault tabs (Recording/Tasks/Documents) + split → Tasks 6–10. ✓
- Detailed field errors inline; header compact error → Task 4 (header), Tasks 6/8/9 (inline). ✓
- Reset-to-first-tab → Task 3 (TabGroup). ✓
- Failed load → no editable fields → Tasks 6, 8, 9. ✓
- Collapse race-guards + delete `useOptionalFolderField` → Task 10. ✓
- In-flight serialization → Task 2. ✓
- TDD + baselines → every task + Task 11. ✓
- No IPC surface change → Global Constraints (only pre-existing commands invoked). ✓

**Placeholder scan:** No TBD/TODO; every code and test step is complete. The one "moved verbatim" instruction (Task 5) refers to relocating existing, in-repo markup, not new code to invent.

**Type consistency:** `useAutosave` returns `{ schedule, flush, saveNow, error }` — consumed identically in Tasks 6/7/8/9. `settingsStatus` actions `saving`/`saved`/`failed`/`reset` — used consistently in Tasks 2/4. `RecordingValue` (Task 9) matches `RecordingSettings`'s `RecordingSettingsValue` shape (12 fields). Command payloads (`set_capture_config { id, cfg }`, `set_tasks_config { id, tasksFolder }`, `set_documents_config { id, documentsFolder, documentDateFolders }`, `set_task_lists_config { id, defaultList, listOrder }`) match the existing IPC contract verified against `tests/capture-settings.test.ts` and `AGENTS.md`.
