# Pandoc-status cache + intake-button icon — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop the knowledge-intake menu from re-probing Pandoc on every open once it's found, and replace the intake button's microphone icon with a plus-in-a-rounded-square.

**Architecture:** A new panel-scoped Pinia store (`usePandocStore`) caches the app-global Pandoc status; the two intake surfaces (`RecordMode`, `ImportVaultPicker`) probe through it (skip when already installed), and the settings card (`DocumentImportSettings`) keeps its own probe but writes results through to the store. Frontend-only — no Rust/IPC/config changes.

**Tech Stack:** Vue 3 + Pinia (Options-API `defineStore`), Vitest (happy-dom + `mockIPC`).

**Spec:** `docs/superpowers/specs/2026-07-16-pandoc-status-cache-and-intake-icon-design.md`

## Global Constraints

- **Cache a positive result only.** `ensureDetected()` skips the probe when `status?.installed` is true; when the status is null / not-installed it probes again on the next call (a fresh install is still picked up). Explicit settings Recheck / path-override changes refresh via write-through.
- **Degrade, never throw upward.** A failed/absent `detect_pandoc` leaves `status: null` (treated as "not installed") and is logged via `logWarning` — the same fallback the components used before.
- **No behavior change to the settings card.** `DocumentImportSettings` keeps its own `detect()` / `detectTicket` / Recheck / path-override / seed logic; it only *adds* a `pandocStore.markDetected(s)` write-through.
- **Icon: plus-in-a-rounded-square**, 16×16, `stroke="currentColor"`, `aria-hidden="true"`; the button's `aria-label`/`title` ("Capture knowledge"), disabled state, and click handler are unchanged.
- **Conventional Commits**; end each message with the two trailers this repo requires (`Co-Authored-By:` and `Claude-Session:`).
- Do not put the model identifier `claude-opus-4-8` in any committed artifact.

---

### Task 1: The `usePandocStore` cache

**Files:**
- Create: `src/stores/pandoc.ts`
- Test: `tests/pandoc-store.test.ts`

**Interfaces:**
- Produces: `usePandocStore()` with state `{ status: PandocStatus | null; checking: boolean }` and actions `ensureDetected(): Promise<void>` and `markDetected(status: PandocStatus): void`. Consumed by Tasks 2 and 3.

- [ ] **Step 1: Write the failing test.** Create `tests/pandoc-store.test.ts`:

```ts
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { usePandocStore } from "../src/stores/pandoc";

const NOT_INSTALLED = {
  installed: false, version: null, path: null, sandboxSupported: false, configuredPath: null,
};
const installed = () => ({
  installed: true, version: "pandoc 3.1.9", path: "pandoc", sandboxSupported: true, configuredPath: null,
});

describe("usePandocStore", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("probes once and caches when Pandoc is installed", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") { calls += 1; return installed(); }
    });
    const store = usePandocStore();
    await store.ensureDetected();
    expect(store.status?.installed).toBe(true);
    // Found → a second ensureDetected must NOT re-probe.
    await store.ensureDetected();
    expect(calls).toBe(1);
  });

  it("re-probes while Pandoc is not installed", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") { calls += 1; return NOT_INSTALLED; }
    });
    const store = usePandocStore();
    await store.ensureDetected();
    await store.ensureDetected();
    expect(calls).toBe(2); // not cached — a fresh install can still be picked up
  });

  it("degrades to null and does not throw when the probe fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") throw new Error("io error");
    });
    const store = usePandocStore();
    await store.ensureDetected();
    expect(store.status).toBeNull();
    expect(store.checking).toBe(false);
  });

  it("markDetected caches a status without probing", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") { calls += 1; return installed(); }
    });
    const store = usePandocStore();
    store.markDetected(installed());
    expect(store.status?.installed).toBe(true);
    // The written-through status counts as "found", so ensureDetected skips.
    await store.ensureDetected();
    expect(calls).toBe(0);
  });
});
```

- [ ] **Step 2: Run the test to verify it fails.**

Run: `npx vitest run tests/pandoc-store.test.ts`
Expected: FAIL — `Cannot find module '../src/stores/pandoc'` (the store doesn't exist yet).

- [ ] **Step 3: Create the store.** Write `src/stores/pandoc.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";

import { logWarning } from "../logging";
import type { PandocStatus } from "../types";

// App-global Pandoc detection, cached for the panel session so the intake
// surfaces (RecordMode, ImportVaultPicker) don't re-spawn `pandoc --version`
// every time their view opens. Shared via the panel's single Pinia instance.
export const usePandocStore = defineStore("pandoc", {
  state: () => ({
    // Last resolved status; null before the first probe or after a failed one
    // (consumers treat null as "not installed").
    status: null as PandocStatus | null,
    // True only while a probe runs with no cached status yet — drives the
    // intake surfaces' "Checking Pandoc…" gate.
    checking: false,
  }),
  actions: {
    // Called on mount by the intake surfaces. Once Pandoc is known installed it
    // returns without probing (the "found → don't re-check" behavior); when the
    // status is unknown/not-installed it probes once and caches, so a freshly
    // installed Pandoc is still picked up on the next open. No concurrent-probe
    // dedup: the two consumers never mount at the same time.
    async ensureDetected(): Promise<void> {
      if (this.status?.installed) return;
      this.checking = true;
      try {
        this.status = await invoke<PandocStatus>("detect_pandoc");
      } catch (e) {
        // Degrade to "not installed" (null) — the fallback the components used
        // before this cache existed.
        logWarning(`pandoc store: detect_pandoc failed: ${String(e)}`);
      } finally {
        this.checking = false;
      }
    },
    // Write-through from the settings card's own probe, so a settings-side
    // Recheck / path-override fix refreshes the cache the intake menu reads.
    markDetected(status: PandocStatus) {
      this.status = status;
    },
  },
});
```

- [ ] **Step 4: Run the test to verify it passes.**

Run: `npx vitest run tests/pandoc-store.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit.**

```bash
git add src/stores/pandoc.ts tests/pandoc-store.test.ts
git commit  # subject: feat(ui): add usePandocStore to cache Pandoc detection
```

---

### Task 2: Intake surfaces use the store (RecordMode + ImportVaultPicker)

**Files:**
- Modify: `src/components/RecordMode.vue`
- Modify: `src/components/ImportVaultPicker.vue`
- Test: `tests/record-mode.test.ts`, `tests/import-vault-picker.test.ts`, and the RecordMode half of `tests/documentImport.test.ts`

**Interfaces:**
- Consumes: `usePandocStore()` (Task 1) — `status`, `checking`, `ensureDetected()`.

- [ ] **Step 1: Add the caching assertion (the new behavior) to `tests/record-mode.test.ts`.** Add this test inside the `describe("RecordMode", …)` block:

```ts
  it("does not re-probe Pandoc once it has been found (cached across opens)", async () => {
    let detectCalls = 0;
    mockIPC((cmd) => {
      if (cmd === "get_capture_config") return { mode: "meeting" };
      if (cmd === "list_recordings") return [];
      if (cmd === "detect_pandoc") {
        detectCalls += 1;
        return { installed: true, version: "pandoc 3.1.9", path: "pandoc", sandboxSupported: true, configuredPath: null };
      }
    });
    // First open probes.
    const first = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();
    expect(detectCalls).toBe(1);
    first.unmount();
    // Second open reuses the cached "installed" result — no new probe.
    const second = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();
    expect(detectCalls).toBe(1);
    second.unmount();
  });
```

- [ ] **Step 2: Run it to verify it fails.**

Run: `npx vitest run tests/record-mode.test.ts -t "does not re-probe"`
Expected: FAIL — `expect(detectCalls).toBe(1)` on the second mount currently reads 2 (RecordMode probes on every mount today).

- [ ] **Step 3: Wire `RecordMode.vue` to the store.** Make these edits:

3a. Add the store import next to the other store imports (after the `useVaultsStore` import line):

```ts
import { usePandocStore } from "../stores/pandoc";
```

3b. Remove `PandocStatus` from the types import (it's no longer referenced here). Change:

```ts
import type { CaptureConfig, PandocStatus, Recording } from "../types";
```
to:
```ts
import type { CaptureConfig, Recording } from "../types";
```

3c. Add the store instance next to the other store setups (after `const notifications = useNotificationsStore();`):

```ts
const pandocStore = usePandocStore();
```

3d. Delete the local `pandoc` ref and `checking` ref (the two `const pandoc = ref…` / `const checking = ref(true);` blocks with their comments). Replace the whole block with this single comment + nothing else (the store now owns both):

```ts
// App-global Pandoc status is owned by the shared pandoc store: the intake
// menu reuses a cached "installed" result instead of re-spawning
// `pandoc --version` on every open. `pandocStore.status` null = "not installed"
// (Import routes to the setup screen); `pandocStore.checking` gates the
// pre-probe window so a blocked click can't route to Settings before the probe
// settles (Codex review).
```

3e. In `importStatus`, replace the two `pandoc.value` reads with `pandocStore.status`:

```ts
const importStatus = computed(() => {
  if (!pandocStore.status?.installed) {
    return { blocked: true, hint: "Install Pandoc to import documents" };
  }
  if (!pandocStore.status.sandboxSupported) {
    return { blocked: true, hint: "Update Pandoc (2.15+ needed)" };
  }
  return { blocked: false, hint: "Convert a Word, ODT, or RTF file into a note" };
});
```

3f. Delete the entire `async function detectPandoc() { … }` block.

3g. In `onMounted`, replace `void detectPandoc();` with `void pandocStore.ensureDetected();`.

3h. In `onImportClick`, replace `if (checking.value) return;` with `if (pandocStore.checking) return;`.

3i. In the template's Import Document button, replace `:disabled="importing || checking"` with `:disabled="importing || pandocStore.checking"`, and in the hint expression replace the `checking` ternary test with `pandocStore.checking`:

```html
        :disabled="importing || pandocStore.checking"
```
```html
        <span class="block text-xs text-slate-400">{{
          pandocStore.checking
            ? "Checking Pandoc…"
            : importing
              ? "Converting… this can take a few seconds"
              : importStatus.hint
        }}</span>
```

- [ ] **Step 4: Wire `ImportVaultPicker.vue` to the store.** Make the analogous edits:

4a. Add `import { usePandocStore } from "../stores/pandoc";` with the other imports, and remove `PandocStatus` from the `../types` import if it becomes unused there.

4b. Delete the local `const pandoc = ref<PandocStatus | null>(null);` and `const checking = ref(true);` lines (keep `const busyVaultId = ref<string | null>(null);`), and add `const pandocStore = usePandocStore();` near the top of the setup.

4c. In `gate`, replace both `pandoc.value` reads with `pandocStore.status`:

```ts
const gate = computed(() => {
  if (!pandocStore.status?.installed) {
    return {
      blocked: true,
      hint: "Pandoc isn't installed — install it to import documents.",
    };
  }
  if (!pandocStore.status.sandboxSupported) {
    return {
      blocked: true,
      hint: "Your Pandoc is too old for safe import (need 2.15+).",
    };
  }
  return { blocked: false, hint: "" };
});
```

4d. In `viewState`, replace `if (checking.value) return "checking";` with `if (pandocStore.checking) return "checking";`.

4e. Delete the entire `async function detectPandoc() { … }` block and, in `onMounted`, replace `void detectPandoc();` with `void pandocStore.ensureDetected();`.

- [ ] **Step 5: Run the affected suites to verify they pass.**

Run: `npx vitest run tests/record-mode.test.ts tests/import-vault-picker.test.ts tests/documentImport.test.ts`
Expected: PASS. `record-mode`/`import-vault-picker` already call `setActivePinia(createPinia())` per test, and the `documentImport.ts` RecordMode `describe` does too, so the store is fresh each test. The new "does not re-probe" test now passes; every existing behavior assertion (button gating, "Checking Pandoc…", blocked-click routing) is unchanged.
(If `tests/documentImport.test.ts`'s DocumentImportSettings `describe` fails with a Pinia error, that's Task 3's `setActivePinia` — do Task 3 before re-running the whole file.)

- [ ] **Step 6: Typecheck + lint.**

Run: `npm run build && npm run lint`
Expected: `vue-tsc` clean (confirms the removed `PandocStatus`/`ref` imports left nothing dangling), ESLint clean.

- [ ] **Step 7: Commit.**

```bash
git add src/components/RecordMode.vue src/components/ImportVaultPicker.vue tests/record-mode.test.ts
git commit  # subject: feat(ui): cache Pandoc detection in the intake menu and vault picker
```

---

### Task 3: Settings card writes through to the cache

**Files:**
- Modify: `src/components/DocumentImportSettings.vue`
- Test: `tests/documentImport.test.ts`

**Interfaces:**
- Consumes: `usePandocStore().markDetected(status)` (Task 1).

- [ ] **Step 1: Add `setActivePinia` to the DocumentImportSettings suite.** In `tests/documentImport.test.ts`, the `describe("DocumentImportSettings", …)` block's `beforeEach` currently resets the mocks but does not set up Pinia. Add the Pinia import and the reset. At the top of the file the imports already include neither `createPinia` nor `setActivePinia` for this describe — add them to the existing pinia import line (the RecordMode describe already imports them). Ensure this import is present:

```ts
import { createPinia, setActivePinia } from "pinia";
```

Then change the DocumentImportSettings `beforeEach` from:

```ts
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.open.mockReset();
    vi.mocked(logWarning).mockClear();
  });
```
to:
```ts
  beforeEach(() => {
    setActivePinia(createPinia());
    mocks.invoke.mockReset();
    mocks.open.mockReset();
    vi.mocked(logWarning).mockClear();
  });
```

- [ ] **Step 2: Run the suite to confirm it still passes (guards the refactor).**

Run: `npx vitest run tests/documentImport.test.ts`
Expected: PASS — adding `setActivePinia` is inert until the component uses a store; this step just proves the harness change is clean before wiring the store in.

- [ ] **Step 3: Add the write-through in `DocumentImportSettings.vue`.**

3a. Add the store import with the other imports:

```ts
import { usePandocStore } from "../stores/pandoc";
```

3b. Instantiate it in setup (after the existing refs, e.g. below `const saving = ref(false);`):

```ts
const pandocStore = usePandocStore();
```

3c. In `detect()`, inside the existing ticket guard on success, add the write-through so a settings-side probe refreshes the shared cache. Change:

```ts
    const s = await invoke<PandocStatus>("detect_pandoc");
    if (ticket === detectTicket) status.value = s;
```
to:
```ts
    const s = await invoke<PandocStatus>("detect_pandoc");
    if (ticket === detectTicket) {
      status.value = s;
      // Keep the shared intake-menu cache fresh after a settings-side probe
      // (Recheck / path-override re-detect), so the record chooser sees the fix.
      pandocStore.markDetected(s);
    }
```

- [ ] **Step 4: Run the suite to verify it passes.**

Run: `npx vitest run tests/documentImport.test.ts`
Expected: PASS — all existing detection / Recheck / savePath / browse assertions still hold; the component's own logic is unchanged and `markDetected` just writes to the store.

- [ ] **Step 5: Typecheck + lint, then commit.**

```bash
npm run build && npm run lint
git add src/components/DocumentImportSettings.vue tests/documentImport.test.ts
git commit  # subject: feat(ui): refresh the Pandoc cache from the settings card
```

---

### Task 4: Swap the intake button icon

**Files:**
- Modify: `src/components/VaultList.vue`
- Test: `tests/vault-list.test.ts` (assert only — no change expected)

**Interfaces:** none.

- [ ] **Step 1: Replace the microphone SVG.** In `VaultList.vue`, the "Capture knowledge" button (`title="Capture knowledge"`) contains an inline `<svg>` with a `<rect x="9" y="2" …>` and `<path d="M5 10v1a7 7 0 0 0 14 0v-1M12 18v4" />` (a microphone). Replace that entire `<svg>…</svg>` with a plus-in-a-rounded-square:

```html
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <rect
                x="3"
                y="3"
                width="18"
                height="18"
                rx="4"
              />
              <path d="M12 8v8M8 12h8" />
            </svg>
```

- [ ] **Step 2: Confirm the button tests still pass.**

Run: `npx vitest run tests/vault-list.test.ts`
Expected: PASS — the suite keys the button on `[aria-label^="Capture knowledge in"]` and `title === "Capture knowledge"`, both unchanged; the icon is `aria-hidden` (presentational), so nothing asserts on the SVG paths.

- [ ] **Step 3: Typecheck + build, then commit.**

```bash
npm run build
git add src/components/VaultList.vue
git commit  # subject: feat(ui): replace the intake button's microphone icon with a plus
```

---

### Task 5: Docs + full verification + PR

**Files:**
- Modify: `AGENTS.md` (document-import Frontend note)

**Interfaces:** none.

- [ ] **Step 1: Note the cache in AGENTS.md.** In `AGENTS.md`, find the document-import domain's Frontend paragraph that mentions `DocumentImportSettings` probing Pandoc on mount (search: `probes Pandoc on mount`). Add a sentence that the intake surfaces now share a cache, e.g. append to that paragraph:

```markdown
The app-global Pandoc status is cached in a small `usePandocStore` (panel
webview): the intake surfaces (`RecordMode`, `ImportVaultPicker`) probe through
`ensureDetected()` and reuse a found result instead of re-spawning
`pandoc --version` on every open, while `DocumentImportSettings` keeps its own
on-mount probe/Recheck and writes each result through (`markDetected`) so a
settings-side fix stays reflected in the intake menu.
```

If the exact anchor differs, place the sentence in the document-import Frontend bullet near the `DocumentImportSettings` mention.

- [ ] **Step 2: Full frontend verification.**

Run: `npm run lint && npm run build && npm test`
Expected: ESLint clean, `vue-tsc` + build succeed, full Vitest suite passes (including the new `pandoc-store` tests and the updated component suites).

- [ ] **Step 3: Commit the docs.**

```bash
git add AGENTS.md
git commit  # subject: docs: note the shared Pandoc-status cache
```

- [ ] **Step 4: Push and open a PR.**

```bash
git push -u origin claude/document-intake-image-text-config-lhrmsk
```

Then open a PR (ready for review) against `main`, mirroring `.github/pull_request_template.md` (Summary / Test plan / Related), and subscribe to its activity.

---

## Self-Review

**Spec coverage:**
- Shared `usePandocStore` (status/checking/ensureDetected/markDetected), cache-positive policy → Task 1. ✓
- Intake surfaces reuse the cache; re-probe when not installed → Task 2 (RecordMode + ImportVaultPicker) + its new caching test. ✓
- Settings card keeps its own probe, writes through → Task 3. ✓
- Icon swap (plus-in-rounded-square), label/title unchanged → Task 4. ✓
- Tests: new store suite, existing suites pass, `setActivePinia` added to the DocumentImportSettings describe, VaultList label assertion kept → Tasks 1–4. ✓
- Docs → Task 5. ✓
- Degrade-on-failure, no Rust/IPC/config change → Global Constraints, Task 1. ✓

**Placeholder scan:** No TBD/TODO; every code step shows the exact code. The one "if the anchor differs" note (Task 5 Step 1) is a doc-placement hint, not a code gap.

**Type consistency:** `usePandocStore` / `status` / `checking` / `ensureDetected()` / `markDetected(status)` are used identically in Tasks 1–3. `PandocStatus` is imported by the store (defined) and dropped from `RecordMode`/`ImportVaultPicker` where it becomes unused (Task 2 Steps 3b/4a). The store's `status` shape matches the existing `PandocStatus` type the components already used.
