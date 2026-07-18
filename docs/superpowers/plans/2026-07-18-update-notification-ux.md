# Update-notification UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route the buddy's update announcement to a dedicated, focused update view and make the announcement bubble genuinely clickable (with an interactive affordance) instead of landing on the buried Buddy-settings Updates card.

**Architecture:** A new `update` panel view (`UpdateView.vue`) reuses the existing `updates` Pinia store. The startup check and the failed-install reopen arm `pendingView="update"`. The bubble gains an optional per-message `action`: the update announcement passes `"openUpdate"`, `SpeechBubble` renders a clickable affordance, and a click invokes a new idempotent Rust `open_panel` command whose `panel-shown` refresh consumes the armed pending view.

**Tech Stack:** Vue 3 + Pinia + Tailwind 4 frontend (Vitest + happy-dom + `@vue/test-utils` + `mockIPC`); Rust Tauri v2 shell (window commands, verified by the Linux compile gate).

## Global Constraints

- **TDD, always:** write the failing test first, watch it fail, then implement. Regression tests name the failure mode in a comment.
- **Node 22.** Run a single test file with `npx vitest run tests/<file>.test.ts`.
- **Commits:** Conventional Commits with existing scopes (`feat(ui)`, `fix(updates)`, `fix(shell)`, `docs(...)`). Imperative subject; body explains the *why*. End every commit body with the two trailers this session requires:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` and `Claude-Session: https://claude.ai/code/session_01VM2UiiDJMxWmQnzgCwh8xN`. Author must be `Claude <noreply@anthropic.com>` (already set via `git config`).
- **Cross-window rule:** the bubble and panel are separate webviews with separate Pinia stores — cross-window routing goes through Rust, never a shared store.
- **Vault-sacred / minimal-deps:** release notes render as **plain preformatted text** — never `v-html`, no markdown-renderer dependency.
- **Bubble reveal chokepoint:** `open_panel` wraps the existing `commands::show_panel` (idempotent: safe on an already-open panel; always emits `panel-shown`). Never use `toggle_panel` for the bubble click — it would hide an already-open panel.
- **Keep `AGENTS.md` in sync** when adding a command (IPC table + count) or changing the events contract.
- **Rust window commands** aren't unit-tested (they need a live window); they are verified by `cd src-tauri && cargo fmt --check` and the Linux compile gate `npm run setup:linux` (once) then `npx tauri build --no-bundle`, with CI's `windows-app` as the behavior gate.

---

### Task 1: `update` view + `openUpdate()` in the vaults store

**Files:**
- Modify: `src/stores/vaults.ts` (view union ~17-27; `pendingView` ~53; `requestView` ~168-176; `requestViewOnNextOpen` ~181-184; add `openUpdate` near `openSettings` ~222-224)
- Test: `tests/vaults-store.test.ts`

**Interfaces:**
- Produces: `store.view` can be `"update"`; `store.openUpdate(): void`; `requestView("update")` and `requestViewOnNextOpen("update")` accepted; `pendingView` may be `"update"`. `back()` from `update` falls through to the vault list (no new case).

- [ ] **Step 1: Write the failing tests**

Add to `tests/vaults-store.test.ts` (near the other view tests):

```ts
it("openUpdate switches to the update view, back returns to the list", () => {
  const store = useVaultsStore();
  store.openUpdate();
  expect(store.view).toBe("update");
  store.back();
  expect(store.view).toBe("list");
});

it("requestView('update') survives the next-open refresh, then reverts to list", async () => {
  // the failed-install reopen requests the focused update view before
  // reopening the panel; the panel-shown refresh honors it once.
  mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
  const store = useVaultsStore();
  store.requestView("update");
  expect(store.view).toBe("update"); // reflected immediately
  await store.refresh();
  expect(store.view).toBe("update"); // honored once
  await store.refresh();
  expect(store.view).toBe("list"); // one-shot
});

it("requestViewOnNextOpen('update') arms the next open without flipping the live view", async () => {
  // the startup update check arms the NEXT open; an already-open panel must
  // not be yanked mid-task.
  mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
  const store = useVaultsStore();
  store.requestViewOnNextOpen("update");
  expect(store.view).toBe("list"); // live view untouched
  await store.refresh();
  expect(store.view).toBe("update"); // consumed once
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/vaults-store.test.ts`
Expected: FAIL — `openUpdate` is not a function / type `"update"` not assignable.

- [ ] **Step 3: Implement**

In `src/stores/vaults.ts`:

Extend the `view` union — change the last member line:
```ts
      | "importPicker"
      | "documentImport"
      | "update",
```

Extend `pendingView`:
```ts
    pendingView: null as "list" | "settings" | "captureSettings" | "update" | null,
```

Widen `requestView`'s parameter:
```ts
    requestView(
      view: "list" | "settings" | "captureSettings" | "update",
      captureVaultId: string | null = null,
    ) {
```

Widen `requestViewOnNextOpen`'s parameter:
```ts
    requestViewOnNextOpen(view: "list" | "settings" | "captureSettings" | "update") {
```

Add the action right after `openSettings`:
```ts
    openUpdate() {
      this.view = "update";
    },
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run tests/vaults-store.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/stores/vaults.ts tests/vaults-store.test.ts
git commit  # feat(ui): add a dedicated update panel view to the vaults store
```

---

### Task 2: `UpdateView.vue` — the focused "what's new" view

**Files:**
- Create: `src/components/UpdateView.vue`
- Create: `tests/update-view.test.ts`

**Interfaces:**
- Consumes: `useUpdatesStore()` — `phase`, `available` (`{version, currentVersion?, date?, body?}`), `currentVersion`, `error`, `installUpdate()`, `loadVersion()`.
- Produces: `<UpdateView />` — renders version + release notes + Install & restart; a friendly empty state otherwise. `data-testid`s: `release-notes`, `install-update`, `update-error`.

- [ ] **Step 1: Write the failing tests**

Create `tests/update-view.test.ts`:

```ts
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("0.1.0"),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn() }));

import UpdateView from "../src/components/UpdateView.vue";
import { useUpdatesStore } from "../src/stores/updates";

function primeAvailable(overrides: Record<string, unknown> = {}) {
  const store = useUpdatesStore();
  store.phase = "available";
  store.currentVersion = "0.1.0";
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  store.available = {
    version: "0.2.0",
    currentVersion: "0.1.0",
    date: "2026-07-18",
    body: "- Faster startup\n- Bug fixes",
    ...overrides,
  } as any;
  return store;
}

describe("UpdateView", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => vi.clearAllMocks());

  it("shows the available version and its release notes", () => {
    primeAvailable();
    const wrapper = mount(UpdateView);
    expect(wrapper.text()).toContain("0.2.0");
    expect(wrapper.get('[data-testid="release-notes"]').text()).toContain(
      "Faster startup",
    );
  });

  it("falls back gracefully when a release has no notes", () => {
    primeAvailable({ body: "" });
    const wrapper = mount(UpdateView);
    expect(wrapper.find('[data-testid="release-notes"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("No release notes provided.");
  });

  it("installs via the store on the Install button", async () => {
    const store = primeAvailable();
    const spy = vi.spyOn(store, "installUpdate").mockResolvedValue();
    const wrapper = mount(UpdateView);
    await wrapper.get('[data-testid="install-update"]').trigger("click");
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("shows a spinner and disables the button while installing", () => {
    const store = primeAvailable();
    store.phase = "installing";
    const wrapper = mount(UpdateView);
    const btn = wrapper.get('[data-testid="install-update"]');
    expect(btn.text()).toContain("Installing…");
    expect(btn.attributes("disabled")).toBeDefined();
  });

  it("surfaces an install error while keeping the retry button", () => {
    const store = primeAvailable();
    store.phase = "error";
    store.error = "signature mismatch";
    const wrapper = mount(UpdateView);
    expect(wrapper.get('[data-testid="update-error"]').text()).toContain(
      "signature mismatch",
    );
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(true);
  });

  it("shows a friendly empty state when no update is available", () => {
    const store = useUpdatesStore();
    store.phase = "idle";
    store.available = null;
    const wrapper = mount(UpdateView);
    expect(wrapper.text()).toContain("No update is available right now.");
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(false);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/update-view.test.ts`
Expected: FAIL — cannot resolve `../src/components/UpdateView.vue`.

- [ ] **Step 3: Implement**

Create `src/components/UpdateView.vue`:

```vue
<script setup lang="ts">
import { computed, onMounted } from "vue";

import { useUpdatesStore } from "../stores/updates";

const updates = useUpdatesStore();

// The view is reached when an update is available (the announcement's
// landing spot, and the settings "View update →" link) or when an install
// just failed but kept `available` for retry — the same gate the settings
// card uses today.
const showUpdate = computed(
  () =>
    updates.phase === "available" ||
    updates.phase === "installing" ||
    (updates.phase === "error" && updates.available !== null),
);

// Release notes come from the signed release feed; render as PLAIN text
// (never v-html, no markdown dependency) — honest and injection-proof.
const releaseNotes = computed(() => updates.available?.body?.trim() ?? "");

onMounted(() => {
  void updates.loadVersion();
});
</script>

<template>
  <div
    v-if="showUpdate && updates.available"
    class="flex flex-col gap-3"
  >
    <div>
      <p class="text-sm font-semibold text-slate-100">
        Version {{ updates.available.version }} is available
      </p>
      <p
        v-if="updates.currentVersion"
        class="text-xs text-slate-400"
      >
        You're on v{{ updates.currentVersion }}<span
          v-if="updates.available.date"
        > · released {{ updates.available.date }}</span>
      </p>
    </div>

    <section>
      <h2
        class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-400"
      >
        What's new
      </h2>
      <pre
        v-if="releaseNotes"
        data-testid="release-notes"
        class="max-h-48 overflow-y-auto whitespace-pre-wrap rounded-xl border border-white/10 bg-white/5 p-2 font-sans text-xs leading-relaxed text-slate-200"
      >{{ releaseNotes }}</pre>
      <p
        v-else
        class="rounded-xl border border-white/10 bg-white/5 p-2 text-xs text-slate-400"
      >
        No release notes provided.
      </p>
    </section>

    <button
      type="button"
      class="cursor-pointer rounded-lg border border-violet-400 bg-violet-500/20 px-3 py-1.5 text-sm text-slate-100 transition-colors hover:bg-violet-500/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
      :disabled="updates.phase === 'installing'"
      data-testid="install-update"
      @click="updates.installUpdate()"
    >
      <span
        v-if="updates.phase === 'installing'"
        class="flex items-center justify-center gap-1.5"
      >
        <span
          class="h-3 w-3 animate-spin rounded-full border-2 border-white/30 border-t-white"
          role="status"
          aria-label="Installing update…"
        />
        Installing…
      </span>
      <span v-else>Install &amp; restart</span>
    </button>

    <p
      v-if="updates.phase === 'error'"
      data-testid="update-error"
      class="text-xs text-red-300"
    >
      {{ updates.error }}
    </p>
  </div>

  <p
    v-else
    class="text-xs text-slate-400"
  >
    No update is available right now.
  </p>
</template>
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run tests/update-view.test.ts`
Expected: PASS (all 6).

- [ ] **Step 5: Commit**

```bash
git add src/components/UpdateView.vue tests/update-view.test.ts
git commit  # feat(ui): add UpdateView — focused "what's new" update screen
```

---

### Task 3: Render `UpdateView` in `ActionPanel`

**Files:**
- Modify: `src/components/ActionPanel.vue` (imports ~9-24; `VIEW_TITLES` ~53-63; view blocks ~367-376)
- Test: `tests/action-panel.test.ts`

**Interfaces:**
- Consumes: `store.view === "update"` (Task 1); `<UpdateView />` (Task 2).
- Produces: the panel renders `UpdateView` with the "Update" title and the shared ← back button when `view === "update"`.

- [ ] **Step 1: Write the failing test**

Add to `tests/action-panel.test.ts` (import `UpdateView` at the top: `import UpdateView from "../src/components/UpdateView.vue";`):

```ts
it("renders the dedicated update view and a back button", () => {
  const store = useVaultsStore();
  store.vaults = sampleVaults;
  store.loaded = true;
  store.openUpdate();
  // stub UpdateView so the panel test needn't mock the updater plugins
  const wrapper = mount(ActionPanel, { global: { stubs: { UpdateView: true } } });
  expect(wrapper.text()).toContain("Update"); // the view title
  expect(wrapper.findComponent(UpdateView).exists()).toBe(true);
  expect(wrapper.find('[data-testid="back-button"]').exists()).toBe(true);
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/action-panel.test.ts`
Expected: FAIL — `UpdateView` not found in the rendered tree.

- [ ] **Step 3: Implement**

In `src/components/ActionPanel.vue`:

Add the import beside the other component imports (keep alphabetical grouping near `Transcriptions`):
```ts
import UpdateView from "./UpdateView.vue";
```

Add the title to `VIEW_TITLES`:
```ts
  documentImport: "Document import",
  update: "Update",
};
```

Add a render block right after the `documentImport` block (before the final `v-else` vault-list block):
```html
    <div
      v-else-if="view === 'update'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <UpdateView />
    </div>
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run tests/action-panel.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/ActionPanel.vue tests/action-panel.test.ts
git commit  # feat(ui): route the update view through ActionPanel
```

---

### Task 4: `UpdateSettings` links to the view instead of installing inline

**Files:**
- Modify: `src/components/UpdateSettings.vue` (script ~1-22; the `showInstall` block ~52-79)
- Create: `tests/update-settings.test.ts`

**Interfaces:**
- Consumes: `store.openUpdate()` (Task 1).
- Produces: when an update is available the settings card shows a `data-testid="view-update"` button that calls `openUpdate()`; the `data-testid="check-updates"` control and the startup toggle stay. No inline `install-update` button remains in settings.

- [ ] **Step 1: Write the failing tests**

Create `tests/update-settings.test.ts`:

```ts
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("0.1.0"),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn() }));

import UpdateSettings from "../src/components/UpdateSettings.vue";
import { useUpdatesStore } from "../src/stores/updates";
import { useVaultsStore } from "../src/stores/vaults";

describe("UpdateSettings", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => vi.clearAllMocks());

  it("keeps the manual check-for-updates control", () => {
    const wrapper = mount(UpdateSettings);
    expect(wrapper.find('[data-testid="check-updates"]').exists()).toBe(true);
  });

  it("links an available update to the dedicated view instead of installing inline", async () => {
    const updates = useUpdatesStore();
    updates.phase = "available";
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    updates.available = { version: "0.2.0" } as any;
    const vaults = useVaultsStore();
    const wrapper = mount(UpdateSettings);
    // the inline install button moved to UpdateView — settings only links now
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(false);
    await wrapper.get('[data-testid="view-update"]').trigger("click");
    expect(vaults.view).toBe("update");
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/update-settings.test.ts`
Expected: FAIL — no `view-update` button; `install-update` still present.

- [ ] **Step 3: Implement**

In `src/components/UpdateSettings.vue` script, add the vaults store:
```ts
import { useSettingsStore } from "../stores/settings";
import { useUpdatesStore } from "../stores/updates";
import { useVaultsStore } from "../stores/vaults";

const updates = useUpdatesStore();
const settings = useSettingsStore();
const vaults = useVaultsStore();
```

Replace the entire available/install `<div v-else-if="showInstall" ...>` block (the one containing the `install-update` button) with:
```html
      <div
        v-else-if="showInstall"
        class="mt-1.5 flex items-center justify-between gap-2"
      >
        <span class="text-xs text-slate-300">
          Version {{ updates.available?.version }} is available
        </span>
        <button
          type="button"
          class="cursor-pointer rounded-lg border border-violet-400 bg-violet-500/20 px-2 py-0.5 text-xs text-slate-100 transition-colors hover:bg-violet-500/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          data-testid="view-update"
          @click="vaults.openUpdate()"
        >
          View update →
        </button>
      </div>
```

(Leave the `showInstall` computed, the "You're up to date." line, the `phase === 'error'` line, and the startup toggle unchanged.)

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run tests/update-settings.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/UpdateSettings.vue tests/update-settings.test.ts
git commit  # feat(ui): settings links to the update view instead of installing inline
```

---

### Task 5: Point the notification + failed-install reopen at the update view

**Files:**
- Modify: `src/composables/useStartupUpdateCheck.ts:39-40` (+ its doc comment ~24-26)
- Modify: `src/stores/updates.ts:108-117` (the failed-install reopen + comment)
- Modify: `src/buddyMessages.ts:53-57` (comment only — copy unchanged)
- Test: `tests/startup-update-check.test.ts`, `tests/updates-store.test.ts`

**Interfaces:**
- Consumes: `requestViewOnNextOpen("update")` / `requestView("update")` (Task 1); `announce(text, action?)` — the `action` arg is added in Task 6, but this task already passes `"openUpdate"` (the frontend `announce` currently ignores a second arg, so it is safe to land first; Task 6 makes it live).

- [ ] **Step 1: Update the failing tests**

In `tests/startup-update-check.test.ts`, change the "found" test (rename its title too) so it expects the update view and the announce action:
```ts
  it("asks via a clickable bubble + next-open update view when an update is found", async () => {
    mocks.check.mockResolvedValue({ version: "0.9.0" });
    mount(Host);
    expect(mocks.check).not.toHaveBeenCalled(); // waits out the settle delay
    await vi.advanceTimersByTimeAsync(STARTUP_CHECK_DELAY_MS);
    expect(mocks.check).toHaveBeenCalledTimes(1);
    // the bubble carries the openUpdate action so it is clickable
    expect(mocks.announce).toHaveBeenCalledWith(
      expect.stringContaining("0.9.0"),
      "openUpdate",
    );
    // the ask lands on the dedicated update view at the NEXT panel open
    expect(useVaultsStore().pendingView).toBe("update");
  });
```

In `tests/updates-store.test.ts`, change the failed-install test (title + the two `"settings"` occurrences) to target the update view:
```ts
  it("reopens the panel on the update view when the install fails", async () => {
    const vaults = useVaultsStore();
    const download = vi.fn().mockResolvedValue(undefined);
    const install = vi.fn().mockImplementation(async () => {
      // whatever the view state was when the process was about to exit,
      // the reopened panel must land on the update view
      vaults.view = "list";
      throw "install broke";
    });
    mocks.check.mockResolvedValue({ version: "0.2.0", download, install });
    vaults.view = "update"; // installs start from the update view
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();
    expect(store.phase).toBe("error");
    expect(store.error).toContain("install broke");
    expect(store.available).not.toBeNull(); // retry stays possible
    // close_panel hid the panel window before the install threw — toggle_panel
    // re-shows it, on the update view where the error/retry button live
    expect(mocks.invoke).toHaveBeenCalledWith("toggle_panel");
    expect(vaults.view).toBe("update");
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/startup-update-check.test.ts tests/updates-store.test.ts`
Expected: FAIL — `pendingView` is `"settings"`; announce called with one arg; `vaults.view` is `"settings"`.

- [ ] **Step 3: Implement**

In `src/composables/useStartupUpdateCheck.ts`, update the announce call and arming (and the doc comment's "settings view" → "update view"):
```ts
      void updates.checkForUpdatesQuietly().then(() => {
        if (updates.phase !== "available") return;
        // Announce with the openUpdate action so the bubble is clickable, and
        // arm the next panel open to land on the dedicated update view.
        announce(updateAvailableMessage(updates.available?.version ?? ""), "openUpdate");
        vaults.requestViewOnNextOpen("update");
      });
```

In `src/stores/updates.ts`, in the install-failure `catch`, change the reopen target and its comment:
```ts
        // The install threw, so the process is still alive: reopen the panel
        // on the dedicated update view so the error and retry button are
        // visible. `close_panel`/`prepare_update_install` hid the panel window,
        // so `toggle_panel` reliably re-shows it. Use `requestView` (not
        // `openUpdate`): reopening fires the panel-shown refresh, which resets
        // to the vault list unless a view was explicitly requested. `available`
        // is kept for retry.
        vaults.requestView("update");
```

In `src/buddyMessages.ts`, update the `updateAvailableMessage` doc comment (leave the returned strings unchanged so `tests/buddy-messages.test.ts` still passes):
```ts
/**
 * The startup check found an update — the buddy asks; the bubble is
 * clickable and clicking it opens the dedicated update view where Install &
 * restart is the answer. Generic fallback so a blank version never renders a
 * dangling "Update v is ready".
 */
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run tests/startup-update-check.test.ts tests/updates-store.test.ts tests/buddy-messages.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/composables/useStartupUpdateCheck.ts src/stores/updates.ts src/buddyMessages.ts tests/startup-update-check.test.ts tests/updates-store.test.ts
git commit  # feat(ui): land the update announcement on the dedicated update view
```

---

### Task 6: `announce` carries an optional per-message action (frontend + Rust)

**Files:**
- Modify: `src/announce.ts`
- Modify: `src-tauri/src/commands.rs:426-437` (the `announce` command)
- Create: `tests/announce.test.ts`

**Interfaces:**
- Produces: `announce(text: string, action?: string): void` — invokes `announce` with `{ text }` (no action) or `{ text, action }`. Rust `announce(app, text, action: Option<String>)` emits `bubble-message` `{ text, action }`.

- [ ] **Step 1: Write the failing tests**

Create `tests/announce.test.ts`:

```ts
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { announce } from "../src/announce";
import { useSettingsStore } from "../src/stores/settings";

describe("announce", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
  });
  afterEach(() => clearMocks());

  it("forwards an action so the bubble becomes clickable", () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    useSettingsStore();
    announce("Update ready", "openUpdate");
    expect(calls).toEqual([
      { cmd: "announce", args: { text: "Update ready", action: "openUpdate" } },
    ]);
  });

  it("omits the action key when there is none (unchanged for every other caller)", () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    useSettingsStore();
    announce("Opening Personal ✨");
    expect(calls).toEqual([
      { cmd: "announce", args: { text: "Opening Personal ✨" } },
    ]);
  });

  it("stays silent when Buddy messages are off", () => {
    localStorage.setItem("vault-buddy.messages", "off");
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
    });
    useSettingsStore();
    announce("Update ready", "openUpdate");
    expect(calls).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/announce.test.ts`
Expected: FAIL — the first test sees `{ text: "Update ready" }` (no `action` key).

- [ ] **Step 3: Implement (frontend)**

Replace the body of `src/announce.ts`'s `announce` (keep the file's doc comment, extending it to mention the action):
```ts
export function announce(text: string, action?: string): void {
  if (!useSettingsStore().buddyMessagesEnabled) return;
  // Include `action` only when present so every existing caller's payload
  // stays exactly `{ text }`. A present action (e.g. "openUpdate") reaches
  // BubbleRoot via `bubble-message` and makes the bubble clickable.
  void invoke("announce", action ? { text, action } : { text }).catch(() => {});
}
```

- [ ] **Step 4: Run to verify pass (frontend)**

Run: `npx vitest run tests/announce.test.ts`
Expected: PASS.

- [ ] **Step 5: Implement (Rust)**

In `src-tauri/src/commands.rs`, change the `announce` command signature + emit (and extend its doc comment to note the optional action drives the bubble's click affordance):
```rust
#[tauri::command]
pub fn announce(app: tauri::AppHandle, text: String, action: Option<String>) {
    use tauri::Emitter;
    // Same placement/reveal path as the launch greeting. A suppressed show
    // (buddy hidden to tray) also skips the text emit: delivering it would
    // start BubbleRoot's dismiss timer inside a window that never appeared.
    if !show_bubble(&app) {
        return;
    }
    // Deliver the text (and the optional click action, which makes the bubble
    // clickable in BubbleRoot); BubbleRoot renders it and (re)starts its
    // dismiss timer.
    let _ = app.emit("bubble-message", serde_json::json!({ "text": text, "action": action }));
}
```

- [ ] **Step 6: Verify Rust compiles + is formatted**

Run: `cd src-tauri && cargo fmt --check`
Expected: no diff.
Run (compile gate; `npm run setup:linux` once first if not already installed): `npx tauri build --no-bundle`
Expected: builds successfully.

- [ ] **Step 7: Commit**

```bash
git add src/announce.ts src-tauri/src/commands.rs tests/announce.test.ts
git commit  # feat(shell): announce carries an optional bubble click action
```

---

### Task 7: `SpeechBubble` renders a clickable affordance

**Files:**
- Modify: `src/components/SpeechBubble.vue`
- Test: `tests/speech-bubble.test.ts`

**Interfaces:**
- Produces: `<SpeechBubble>` accepts `clickable?: boolean`; emits `click` only when `clickable`. When clickable it carries the `clickable` CSS class (pointer + persistent violet ring + hover lift) and is keyboard-activatable.

- [ ] **Step 1: Write the failing tests**

Add to `tests/speech-bubble.test.ts`:

```ts
it("is inert by default — no interactive class, no click emit", async () => {
  const wrapper = mount(SpeechBubble, {
    props: { text: "Hi", side: "right", valign: "middle" },
  });
  const bubble = wrapper.get('[data-testid="speech-bubble"]');
  expect(bubble.classes()).not.toContain("clickable");
  await bubble.trigger("click");
  expect(wrapper.emitted("click")).toBeUndefined();
});

it("shows an interactive affordance and emits click when clickable", async () => {
  const wrapper = mount(SpeechBubble, {
    props: { text: "Update ready", side: "right", valign: "middle", clickable: true },
  });
  const bubble = wrapper.get('[data-testid="speech-bubble"]');
  expect(bubble.classes()).toContain("clickable");
  await bubble.trigger("click");
  expect(wrapper.emitted("click")).toHaveLength(1);
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/speech-bubble.test.ts`
Expected: FAIL — no `clickable` class; no `click` emitted.

- [ ] **Step 3: Implement**

Replace the `<script setup>` of `src/components/SpeechBubble.vue`:
```ts
<script setup lang="ts">
const props = defineProps<{
  text: string;
  // The tail points back at the buddy: `side` puts it on the buddy-facing
  // face; `valign` sets its vertical position — `middle` (level with the
  // buddy) is the common case, `top`/`bottom` only when a screen edge pushes
  // the bubble above or below the buddy's center.
  side: "left" | "right";
  valign: "top" | "middle" | "bottom";
  // When set, this bubble carries a click action (e.g. an update
  // announcement): it renders an interactive affordance and emits `click`.
  clickable?: boolean;
}>();
const emit = defineEmits<{ (e: "click"): void }>();
// Only an actionable bubble reacts — a plain greeting/ack must stay inert.
function activate() {
  if (props.clickable) emit("click");
}
</script>
```

Replace the template's root `<div>` (keep the tail styling untouched):
```html
<template>
  <div
    data-testid="speech-bubble"
    class="bubble"
    :class="[`side-${side}`, `valign-${valign}`, { clickable }]"
    role="status"
    aria-live="polite"
    :tabindex="clickable ? 0 : undefined"
    :title="clickable ? 'Open' : undefined"
    @click="activate"
    @keydown.enter.prevent="activate"
    @keydown.space.prevent="activate"
  >
    {{ text }}
  </div>
</template>
```

Add to the `<style scoped>` block (after the existing `.bubble` rules):
```css
/* An actionable bubble reads as interactive. The bubble auto-dismisses in a
   few seconds, so a hover-only hint could be missed — carry a PERSISTENT
   violet ring (the app accent, layered onto the existing shadow) at rest,
   with a pointer cursor and a hover lift on top. */
.bubble.clickable {
  cursor: pointer;
  box-shadow:
    0 4px 14px rgba(0, 0, 0, 0.22),
    0 0 0 1.5px rgba(139, 92, 246, 0.55);
  transition: transform 120ms ease, box-shadow 120ms ease;
}
.bubble.clickable:hover {
  transform: translateY(-1px);
  box-shadow:
    0 8px 20px rgba(0, 0, 0, 0.3),
    0 0 0 1.5px rgba(139, 92, 246, 0.9);
}
.bubble.clickable:focus-visible {
  outline: none;
  box-shadow:
    0 8px 20px rgba(0, 0, 0, 0.3),
    0 0 0 2px rgba(139, 92, 246, 1);
}
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run tests/speech-bubble.test.ts`
Expected: PASS (all 5).

- [ ] **Step 5: Commit**

```bash
git add src/components/SpeechBubble.vue tests/speech-bubble.test.ts
git commit  # feat(ui): SpeechBubble renders a clickable affordance
```

---

### Task 8: `useBuddyBubble` tracks a per-message action

**Files:**
- Modify: `src/composables/useBuddyBubble.ts`
- Test: `tests/use-buddy-bubble.test.ts`

**Interfaces:**
- Produces: the composable now returns `action: Ref<string | null>`; `show(message, durationMs, action?)` sets it (default `null`); `dismiss()` clears it. Latest-wins, same as `text`.

- [ ] **Step 1: Write the failing tests**

Add to `tests/use-buddy-bubble.test.ts`:

```ts
it("tracks a per-message action and clears it latest-wins", () => {
  const wrapper = mount(Host);
  expect(wrapper.vm.action).toBeNull(); // the greeting carries no action
  wrapper.vm.show("Update ready", BUBBLE_MS.normal.ack, "openUpdate");
  expect(wrapper.vm.action).toBe("openUpdate");
  wrapper.vm.show("Transcript ready! ✨", BUBBLE_MS.normal.ack); // no action
  expect(wrapper.vm.action).toBeNull();
});

it("dismiss clears the action", () => {
  const wrapper = mount(Host);
  wrapper.vm.show("Update ready", BUBBLE_MS.normal.ack, "openUpdate");
  wrapper.vm.dismiss();
  expect(wrapper.vm.action).toBeNull();
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/use-buddy-bubble.test.ts`
Expected: FAIL — `wrapper.vm.action` is undefined; `show` takes no third arg.

- [ ] **Step 3: Implement**

In `src/composables/useBuddyBubble.ts`, update the return type, add the `action` ref, and thread it through `show`/`dismiss`:
```ts
export function useBuddyBubble(): {
  visible: Ref<boolean>;
  text: Ref<string>;
  action: Ref<string | null>;
  show: (message: string, durationMs: number, action?: string | null) => void;
  dismiss: () => void;
} {
  const settings = useSettingsStore();
  const visible = ref(false);
  const text = ref("");
  // The current message's click action, or null for a plain greeting/ack.
  const action = ref<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  function dismiss() {
    clearTimeout(timer);
    timer = undefined;
    visible.value = false;
    action.value = null;
  }

  function show(message: string, durationMs: number, act: string | null = null) {
    text.value = message;
    action.value = act;
    visible.value = true;
    clearTimeout(timer);
    timer = setTimeout(dismiss, durationMs);
  }

  onMounted(() =>
    show(greetingFor(new Date()), BUBBLE_MS[settings.messageDuration].greeting),
  );
  onUnmounted(() => clearTimeout(timer));

  return { visible, text, action, show, dismiss };
}
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run tests/use-buddy-bubble.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/composables/useBuddyBubble.ts tests/use-buddy-bubble.test.ts
git commit  # feat(ui): useBuddyBubble tracks a per-message click action
```

---

### Task 9: Rust `open_panel` command (idempotent panel reveal)

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `open_panel` next to `close_panel` ~199-205)
- Modify: `src-tauri/src/lib.rs:412` (register in `generate_handler`, after `commands::close_panel`)

**Interfaces:**
- Produces: IPC command `open_panel` — idempotently shows the panel (wraps `commands::show_panel`), emitting `panel-shown`. Invoked by BubbleRoot in Task 10.

- [ ] **Step 1: Implement the command**

In `src-tauri/src/commands.rs`, add right after the `close_panel` command:
```rust
/// Show the panel window (idempotent). The clickable bubble calls this on a
/// click that carries an action: unlike `toggle_panel` it never HIDES an
/// already-open panel, so a bubble click always REVEALS the panel — which
/// then runs its `panel-shown` refresh and consumes the armed pending view
/// (for the update announcement, the dedicated update view). Sync, so it
/// runs on the main thread where window show/focus are valid.
#[tauri::command]
pub fn open_panel(app: tauri::AppHandle) {
    show_panel(&app);
}
```

- [ ] **Step 2: Register it**

In `src-tauri/src/lib.rs`, add to `generate_handler!` right after `commands::close_panel,`:
```rust
            commands::open_panel,
```

- [ ] **Step 3: Verify Rust compiles + is formatted**

Run: `cd src-tauri && cargo fmt --check`
Expected: no diff.
Run: `npx tauri build --no-bundle`
Expected: builds successfully.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit  # feat(shell): add open_panel — idempotent panel reveal for the bubble
```

---

### Task 10: `BubbleRoot` wires the action → clickable bubble → `open_panel`

**Files:**
- Modify: `src/roots/BubbleRoot.vue` (destructure ~15; `bubble-message` listener ~68-71; template ~102-106)
- Test: `tests/bubble-root.test.ts`

**Interfaces:**
- Consumes: `useBuddyBubble().action` (Task 8); `SpeechBubble` `clickable` + `@click` (Task 7); `open_panel` (Task 9); `bubble-message` payload `{ text, action? }` (Task 6).

- [ ] **Step 1: Write the failing tests**

Add to `tests/bubble-root.test.ts`:

```ts
it("makes an update announcement clickable and reveals the panel on click", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
  });
  const wrapper = mount(BubbleRoot);
  await flushPromises();
  // Rust's announce emitted the openUpdate action alongside the text.
  listeners["bubble-message"]?.({
    payload: { text: "Update v0.9.0 is ready — click me! ⬆️", action: "openUpdate" },
  });
  await flushPromises();
  const bubble = wrapper.get('[data-testid="speech-bubble"]');
  expect(bubble.classes()).toContain("clickable");
  await bubble.trigger("click");
  await flushPromises();
  expect(calls).toContain("open_panel"); // routed the action to Rust
  expect(calls).toContain("close_bubble"); // dismiss closed the window
});

it("leaves an action-less acknowledgement inert", async () => {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
  });
  const wrapper = mount(BubbleRoot);
  await flushPromises();
  listeners["bubble-message"]?.({ payload: { text: "Opening Personal ✨" } });
  await flushPromises();
  const bubble = wrapper.get('[data-testid="speech-bubble"]');
  expect(bubble.classes()).not.toContain("clickable");
  await bubble.trigger("click");
  await flushPromises();
  expect(calls).not.toContain("open_panel");
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run tests/bubble-root.test.ts`
Expected: FAIL — bubble has no `clickable` class; `open_panel` never invoked.

- [ ] **Step 3: Implement**

In `src/roots/BubbleRoot.vue`, destructure `action`:
```ts
const { visible, text, action, show, dismiss } = useBuddyBubble();
```

Extend the `bubble-message` listener to pass the action (and widen its payload type):
```ts
    unlistenMessage = await listen<{ text: string; action?: string | null }>(
      "bubble-message",
      (event) =>
        show(
          event.payload.text,
          BUBBLE_MS[settings.messageDuration].ack,
          event.payload.action ?? null,
        ),
    );
```

Add a click handler in the `<script setup>` (near the top, after the composable destructure):
```ts
// A bubble that carries an action is clickable: route the action to Rust,
// then dismiss. Best-effort like every other bubble command. `open_panel`
// idempotently reveals the panel (safe whether it is open or closed); its
// panel-shown refresh consumes the armed pending view — for the update
// announcement, the dedicated update view.
function onBubbleClick() {
  if (action.value === "openUpdate") {
    void invoke("open_panel").catch(() => {});
  }
  dismiss();
}
```

Wire `SpeechBubble` in the template:
```html
    <SpeechBubble
      :text="text"
      :side="side"
      :valign="valign"
      :clickable="!!action"
      @click="onBubbleClick"
    />
```

- [ ] **Step 4: Run to verify pass**

Run: `npx vitest run tests/bubble-root.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/roots/BubbleRoot.vue tests/bubble-root.test.ts
git commit  # feat(ui): clickable update bubble reveals the panel on the update view
```

---

### Task 11: Docs — `AGENTS.md` + `docs/Gaps.md`

**Files:**
- Modify: `AGENTS.md` (IPC table intro + `commands.rs` row; Events table `bubble-message` row; Updater-flow section; Frontend-state view union)
- Modify: `docs/Gaps.md` (the `UpdateSettings.vue` coverage bullet)

- [ ] **Step 1: Update the IPC table**

In `AGENTS.md`, change `All 68 commands` → `All 69 commands`.
In the `commands.rs` row, insert `open_panel` after `close_panel` and annotate `announce`:
- `` `close_panel`, `open_panel` (idempotent show — the clickable bubble's reveal), `close_bubble`, ``
- `` `announce` (optional `action` → a clickable bubble), ``

- [ ] **Step 2: Update the Events table**

Change the `bubble-message` row to:
```
| `bubble-message` | `{text, action?}` — the bubble's text plus an optional `action` (`openUpdate`) that makes it clickable | BubbleRoot (routes the action → `open_panel`) |
```

- [ ] **Step 3: Update the Updater-flow section**

Change the heading to reference the view:
`## Updater flow (`src/stores/updates.ts`, `UpdateView.vue`, `UpdateSettings.vue`)`

Change `reopens on the settings view via `toggle_panel`` → `reopens on the dedicated update view (`requestView("update")`) via `toggle_panel``.
Change `requestViewOnNextOpen("settings")` → `requestViewOnNextOpen("update")` in that section's prose.

Append this paragraph to the section:
```
The available update lands on a dedicated `update` panel view
(`UpdateView.vue`) — the new version, the release notes (`Update.body`,
rendered as plain text with a graceful fallback), and Install & restart with
the same installing/error/retry states — instead of the buried Buddy-settings
Updates card, which now shows a "View update →" link to it. The announcement
bubble is itself clickable: the startup check calls `announce(msg,
"openUpdate")`, so `bubble-message` carries the action, `SpeechBubble` renders
an interactive affordance (persistent violet ring + pointer + hover lift), and
a click invokes `open_panel` (idempotent show → the `panel-shown` refresh
consumes the armed `pendingView="update"`). Buddy messages off / buddy hidden
to tray still arm `pendingView`, so a manual open lands on the update view too.
```

- [ ] **Step 4: Update the Frontend-state view union**

Find the `view: list | settings | captureSettings | ... | importPicker | documentImport` enumeration in the Frontend-state section and append `| update` so it reads `... | importPicker | documentImport | update`.

- [ ] **Step 5: Update the Gaps coverage note**

In `docs/Gaps.md`, find the bullet beginning ``- `UpdateSettings.vue` is tested only indirectly`` and update it to note the new direct coverage: `tests/update-settings.test.ts` covers the manual-check control and the "View update →" link, and `tests/update-view.test.ts` covers the release-notes view. (Remove the "only indirectly" claim.)

- [ ] **Step 6: Commit**

```bash
git add AGENTS.md docs/Gaps.md
git commit  # docs: update AGENTS.md + Gaps for the update view and clickable bubble
```

---

### Task 12: Full verification & baselines

**Files:** none (verification only; baseline files only if a gate legitimately requires it).

- [ ] **Step 1: Full frontend suite**

Run: `npm test`
Expected: all tests pass (including the new files).

- [ ] **Step 2: Lint + typecheck + build**

Run: `npm run lint`
Expected: no errors.
Run: `npm run build`
Expected: `vue-tsc` typecheck + production build succeed.

- [ ] **Step 3: LOC + quality baselines**

Run: `npm run check:loc`
Run: `npm run check:quality`
Expected: pass. These are shrink-only baselines; the new `UpdateView.vue` and test files add lines. If a gate flags a legitimate increase, re-run the specific gate with `--update` and commit the regenerated baseline (`scripts/loc-baseline.json` / `scripts/quality-baseline.json`) in this task's commit, with the justification recorded in the PR body. Do NOT loosen a baseline to hide a real regression.

- [ ] **Step 4: Coverage**

Run: `npm run test:coverage`
Expected: coverage floors in `vite.config.ts` still met (run this LAST — `check:quality` must run with no `coverage/` dir present).

- [ ] **Step 5: Rust gate**

Run: `cd src-tauri && cargo fmt --check`
Expected: no diff.
Run: `npx tauri build --no-bundle`
Expected: builds successfully (final confirmation the shell compiles with `open_panel` + the `announce` signature).

- [ ] **Step 6: Commit any baseline updates**

```bash
# only if Step 3 required a baseline regen
git add scripts/loc-baseline.json scripts/quality-baseline.json
git commit  # chore: update LOC/quality baselines for the update view
```

---

## Self-Review

**Spec coverage:**
- Dedicated update view → Tasks 1-3 (`update` view, `UpdateView.vue`, ActionPanel wiring). ✓
- Rich "what's new" (release notes, version, install, error/retry, empty state) → Task 2. ✓
- Keep settings section, link to view → Task 4. ✓
- Route notification + failed install to the view → Task 5. ✓
- Clickable bubble carrying an action → Tasks 6 (announce), 7 (SpeechBubble), 8 (useBuddyBubble), 10 (BubbleRoot). ✓
- Persistent + hover interactive treatment → Task 7 CSS. ✓
- `open_panel` idempotent reveal (not `toggle_panel`) → Task 9. ✓
- Docs (IPC/events/updater/view-union/Gaps) → Task 11. ✓
- Verification + baselines → Task 12. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. ✓

**Type consistency:** `openUpdate()`, `requestView("update")`, `pendingView` including `"update"` (Task 1) are used consistently in Tasks 3-5; `action` ref + `show(…, action?)` (Task 8) match BubbleRoot's destructure/usage (Task 10); `clickable` prop + `click` emit (Task 7) match BubbleRoot's `:clickable`/`@click` (Task 10); `announce(text, action?)` (Task 6) matches the Task-5 call site and the Rust `Option<String>` param; `open_panel` (Task 9) matches BubbleRoot's `invoke("open_panel")` (Task 10). ✓
