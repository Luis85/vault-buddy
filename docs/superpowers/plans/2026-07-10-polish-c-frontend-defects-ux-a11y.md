# Polish Sub-pass C — Frontend Defects, UX, Accessibility, Copy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the frontend defect/UX/a11y cluster (GAP-26 through GAP-31, the selected GAP-32 trio, GAP-33) plus the aggregation review's deferred debt, per the umbrella spec § Sub-pass C.

**Architecture:** Component-local fixes with component tests (Vitest + happy-dom + mockIPC); two store-level races closed by re-checking state after awaits; one Rust copy/DRY chokepoint (the shared vault lookup). No new stores, no new IPC commands, no view-tree changes.

**Tech Stack:** Vue 3 + Pinia + Vitest; one Rust task in the shell crate.

## Global Constraints

- **Branch:** `claude/task-management-vertical-slice-ikeuly`. Never push elsewhere; never amend/rebase existing commits.
- **Bookkeeping:** each task's Gaps.md entry tombstoned **in the same commit**, GAP-40 format. GAP-32 is a PARTIAL fix by spec ("the user-visible slice"): do NOT strike its severity — instead annotate the three fixed bullets inline with `(FIXED 2026-07-10 — <one line>)` and leave the entry and its other bullets open.
- **TDD:** failing test first for every behavioral fix; regression tests name the GAP in a comment. Frontend tests must mirror the target file's existing idiom (mockIPC, store setup) — read the neighboring tests before writing.
- **Copy rule (spec C8):** user-facing errors never embed absolute local paths (paths go to the log line); the vault-lookup failure copy is the user-worded "Vault not found — was it removed from Obsidian?" form (the shared `services::find_vault` variant appends `(id: …)` — that is the sanctioned form, keep it).
- **Gates at every task boundary:** `npx vitest run <touched test files>`, `npm run build` (vue-tsc), `npm run lint`, `npm run check:loc` (Tasks.vue is allowlisted at 760 and capture.ts at 602 — growth needs `--update` in the same commit **+ a justification line in the commit body**); Rust task additionally: `cd src-tauri && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib && npx tauri build --no-bundle` (repo root).
- **Commits:** Conventional Commits (`fix(ui)`, `fix(updates)`, `fix(shell)`), one per task, ending with the two trailers:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU`
- **Open PR thread:** the Codex taskCounts finding (PR #46, Tasks.vue:272) is closed by Task 6 — the CONTROLLER replies and resolves it after that task's review; the implementer must not comment on GitHub.

---

### Task 1: SelectMenu — Escape stays local; the full listbox keyboard story (C1 · GAP-27 + C7 · GAP-33)

**Files:**
- Modify: `src/components/SelectMenu.vue`
- Modify: `src/components/Search.vue:261` (`aria-expanded`) — two attributes only
- Modify: `docs/Gaps.md` (GAP-27 tombstone + GAP-33 tombstone)
- Test: `tests/select-menu.test.ts`, `tests/search.test.ts`

**Interfaces:**
- Consumes: existing `SelectMenu` props (`modelValue/options/id/ariaLabel/dataTestid/wide`) — unchanged.
- Produces: option elements get `:id="optionId(i)"` where `optionId(i)` = `` `${listboxId}-opt-${i}` `` and `listboxId` = `props.id ?? props.dataTestid ?? "select-menu"`; the `<ul>` gets `:aria-activedescendant="activeIndex >= 0 ? optionId(activeIndex) : undefined"`. No API change for consumers.

- [ ] **Step 1: Write the failing tests**

In `tests/select-menu.test.ts` (mirror its existing mount/open idiom — it already opens the menu and asserts on options):

```ts
it("Escape closes the menu without reaching window listeners (GAP-27)", async () => {
  // Dismissing a dropdown used to bubble Escape to window, where PanelRoot
  // closes the whole panel.
  const windowSpy = vi.fn();
  window.addEventListener("keydown", windowSpy);
  try {
    // ...open the menu per the file's idiom...
    await popup.trigger("keydown", { key: "Escape" });
    expect(/* menu closed per the file's idiom */).toBe(true);
    expect(windowSpy).not.toHaveBeenCalled();
  } finally {
    window.removeEventListener("keydown", windowSpy);
  }
});

it("moves aria-activedescendant with the keyboard highlight (GAP-33)", async () => {
  // ...open the menu...
  const ul = /* the listbox */;
  await ul.trigger("keydown", { key: "ArrowDown" });
  const active = ul.attributes("aria-activedescendant");
  expect(active).toBeTruthy();
  expect(document.getElementById(active!)?.textContent).toContain(/* second option label */);
});

it("Home and End jump to the first and last option (GAP-33)", async () => {
  // ...open...
  await ul.trigger("keydown", { key: "End" });
  expect(ul.attributes("aria-activedescendant")).toMatch(/-opt-(N-1)$/); // use the real index
  await ul.trigger("keydown", { key: "Home" });
  expect(ul.attributes("aria-activedescendant")).toMatch(/-opt-0$/);
});
```

(Fill the `...` from the file's existing helpers — the assertions shown are the contract. Note: the popup teleports to `document.body`; the existing tests already handle that.)

In `tests/search.test.ts` add:

```ts
it("aria-expanded reflects whether hits are visible (GAP-33)", async () => {
  // Static aria-expanded="true" claimed an always-open popup even for the
  // empty/recents states.
  // ...mount with no results per the file's idiom...
  expect(input.attributes("aria-expanded")).toBe("false");
  expect(input.attributes("aria-autocomplete")).toBe("list");
  // ...run a query that yields hits...
  expect(input.attributes("aria-expanded")).toBe("true");
});
```

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run tests/select-menu.test.ts tests/search.test.ts`
Expected: the Escape test FAILS (event bubbles to window), the activedescendant/Home/End tests FAIL (attribute absent, keys unhandled), the Search test FAILS (static `"true"`).

- [ ] **Step 3: Implement SelectMenu**

In `SelectMenu.vue`'s script, add after `popupStyle`:

```ts
// Option ids for aria-activedescendant — the listbox has focus, so AT needs
// an id trail to the highlighted option (GAP-33).
const listboxId = computed(() => props.id ?? props.dataTestid ?? "select-menu");
const optionId = (i: number) => `${listboxId.value}-opt-${i}`;

function setActive(i: number) {
  activeIndex.value = i;
  // A 13-item list scrolls at 220px — keep the highlight on-screen.
  void nextTick(() => {
    document.getElementById(optionId(i))?.scrollIntoView({ block: "nearest" });
  });
}
```

Replace `onPopupKeydown` with:

```ts
function onPopupKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.preventDefault();
    // GAP-27: without stopPropagation the Escape bubbles to window, where
    // PanelRoot closes the whole panel — dismissing a dropdown must only
    // dismiss the dropdown.
    e.stopPropagation();
    closeMenu();
  } else if (e.key === "ArrowDown") {
    e.preventDefault();
    setActive(Math.min(props.options.length - 1, activeIndex.value + 1));
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    setActive(Math.max(0, activeIndex.value - 1));
  } else if (e.key === "Home") {
    e.preventDefault();
    setActive(0);
  } else if (e.key === "End") {
    e.preventDefault();
    setActive(props.options.length - 1);
  } else if (e.key === "Enter") {
    e.preventDefault();
    const o = props.options[activeIndex.value];
    if (o) select(o.value);
  }
}
```

In the template: `<ul ... :aria-activedescendant="activeIndex >= 0 ? optionId(activeIndex) : undefined">`, and on the `<li>`: `:id="optionId(i)"` plus change `@pointermove="activeIndex = i"` to `@pointermove="activeIndex = i"` (unchanged — pointer moves must NOT scrollIntoView, that would fight the pointer; only keyboard moves scroll, which is why `setActive` is called only from the keyboard branches).

- [ ] **Step 4: Implement Search.vue**

Line ~261: `aria-expanded="true"` becomes `:aria-expanded="visibleHits.length > 0 ? 'true' : 'false'"` and add `aria-autocomplete="list"` beside it. (Check the actual computed's name — the file calls its visible-row list `visibleHits`; if it differs, bind to that.)

- [ ] **Step 5: Run to verify GREEN + component suites**

Run: `npx vitest run tests/select-menu.test.ts tests/search.test.ts && npm run build && npm run lint`
Expected: PASS, including all pre-existing cases (the pointer path and existing 4 SelectMenu tests unchanged).

- [ ] **Step 6: Tombstones + commit**

GAP-27 tombstone: `### GAP-27 · ~~Medium~~ FIXED 2026-07-10 · Escape in an open dropdown also closes the whole panel` + one line (stopPropagation in the popup's Escape branch; regression test pins window listeners uncalled). GAP-33 tombstone: `### GAP-33 · ~~Low~~ FIXED 2026-07-10 · Accessibility gaps in the two listbox surfaces` + two lines (SelectMenu: option ids, aria-activedescendant, keyboard scrollIntoView, Home/End, keyboard-path tests; Search: aria-expanded bound to visible hits + aria-autocomplete).

```bash
git add src/components/SelectMenu.vue src/components/Search.vue tests/select-menu.test.ts tests/search.test.ts docs/Gaps.md
git commit -m "fix(ui): keep dropdown Escape local and finish the listbox a11y story" -m "GAP-27: SelectMenu's Escape bubbled to window where PanelRoot closes the panel — dismissing the bitrate/model dropdown hid everything. GAP-33: the keyboard highlight was visual-only (no option ids, no aria-activedescendant, off-screen at 220px) and Search claimed an always-open popup; both listboxes now carry the full story, pinned by keyboard-path tests." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 2: Quiet update check can't stomp a manual check or install (C2 · GAP-28)

**Files:**
- Modify: `src/stores/updates.ts` (`checkForUpdatesQuietly`)
- Modify: `docs/Gaps.md` (GAP-28 tombstone)
- Test: `tests/updates-store.test.ts`

- [ ] **Step 1: Write the failing test** (mirror the file's existing quiet-check mocks — it stubs `check()` from the updater plugin):

```ts
it("quiet check discards a result that raced a manual check (GAP-28)", async () => {
  // A slow quiet check resolving after the user manually checked/installed
  // used to flip phase back to `available` mid-install.
  let resolveCheck!: (u: unknown) => void;
  checkMock.mockReturnValueOnce(new Promise((r) => (resolveCheck = r)));
  const store = useUpdatesStore();
  const quiet = store.checkForUpdatesQuietly();
  store.phase = "installing"; // a manual flow started while the quiet check hung
  resolveCheck({ version: "9.9.9", download: vi.fn(), install: vi.fn() });
  await quiet;
  expect(store.phase).toBe("installing");
  expect(store.available).toBeNull();
});
```

(Adapt mock names to the file; the contract: phase and `available` untouched when phase left `idle` during the await.)

- [ ] **Step 2: RED**

Run: `npx vitest run tests/updates-store.test.ts` — the new test FAILS (phase flips to `available`).

- [ ] **Step 3: Implement** — in `checkForUpdatesQuietly`, after the await:

```ts
      const update = await check();
      // GAP-28: the idle guard above ran BEFORE the await — a manual check
      // or install that started while this hung must not be stomped by a
      // stale quiet result.
      if (this.phase !== "idle") return;
      if (update) {
```

- [ ] **Step 4: GREEN** — `npx vitest run tests/updates-store.test.ts && npm run build && npm run lint`

- [ ] **Step 5: Tombstone GAP-28 + commit**

```bash
git add src/stores/updates.ts tests/updates-store.test.ts docs/Gaps.md
git commit -m "fix(updates): re-check idle after the quiet check's await" -m "GAP-28: the phase guard ran only before await check(); a slow quiet check resolving after a manual check/install flipped phase back to available mid-install — landing between download() and install() would run install() on a fresh, never-downloaded Update. The result is now discarded unless the store is still idle." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 3: Panel reopen keeps a fresh rename prompt (C3 · GAP-29)

**Files:**
- Modify: `src/stores/capture.ts` (stamp `lastSavedAtMs`; new `dismissRenameIfStale()`)
- Modify: `src/components/ActionPanel.vue` (`shownNonce` watcher calls the stale-only dismiss)
- Modify: `docs/Gaps.md` (GAP-29 tombstone)
- Test: `tests/capture-store.test.ts` and/or `tests/action-panel.test.ts` (whichever hosts the shownNonce cases — check both)

**Design:** the store already self-expires the prompt via `armRenameExpiry`'s 30 s timer (`RENAME_PROMPT_MS`); the watcher's unconditional `dismissRename()` is what kills a FRESH prompt on the first open after a tray-stop save. Stamp the save time and make the watcher dismiss only genuinely stale prompts (belt for throttled timers in hidden webviews).

- [ ] **Step 1: Write the failing test**

```ts
it("panel reopen keeps a rename prompt younger than the rename window (GAP-29)", () => {
  // A tray-stopped recording arms lastSaved in the hidden panel's store;
  // the shownNonce watcher used to dismiss it before it ever rendered.
  const store = useCaptureStore();
  store.lastSaved = { mp3: "/v/2026-07-10 1200 Meeting.mp3", note: null };
  store.lastSavedAtMs = Date.now() - 5_000; // 5 s old — fresh
  store.dismissRenameIfStale();
  expect(store.lastSaved).not.toBeNull();
  store.lastSavedAtMs = Date.now() - 31_000; // past RENAME_PROMPT_MS — stale
  store.dismissRenameIfStale();
  expect(store.lastSaved).toBeNull();
});
```

- [ ] **Step 2: RED** — `npx vitest run tests/capture-store.test.ts` (compile/type failure on the missing member is the RED).

- [ ] **Step 3: Implement**

`capture.ts` state: add `lastSavedAtMs: null as number | null,` next to `lastSaved`. In the `capture:saved` handler (where `this.lastSaved = {...}` is set, ~line 267): add `this.lastSavedAtMs = Date.now();`. In `dismissRename()`: also `this.lastSavedAtMs = null;`. New action next to it:

```ts
    /** Dismiss the rename prompt only when it is genuinely stale (older than
     * the rename window) — the panel-reopen reset must not kill a fresh
     * prompt armed while the panel was closed (GAP-29). The 30 s timer is
     * the primary expiry; this is the reopen-time belt for throttled timers
     * in a hidden webview. */
    dismissRenameIfStale() {
      if (!this.lastSaved) return;
      if (this.lastSavedAtMs != null && Date.now() - this.lastSavedAtMs < RENAME_PROMPT_MS) return;
      this.dismissRename();
    },
```

`ActionPanel.vue` shownNonce watcher: `capture.dismissRename()` becomes `capture.dismissRenameIfStale()`; update the comment block above it (it currently claims the watcher clears "a lingering post-save rename prompt" unconditionally — say it now clears only stale ones and why).

- [ ] **Step 4: GREEN** — `npx vitest run tests/capture-store.test.ts tests/action-panel.test.ts && npm run build && npm run lint`. If an existing ActionPanel test pins the OLD unconditional dismiss, update it to the new contract and say so in the report (that test was pinning the bug).

- [ ] **Step 5: Tombstone GAP-29 + commit** (fix(ui) scope; LOC: capture.ts is allowlisted at 602 — `--update` + body line if grown).

```bash
git add src/stores/capture.ts src/components/ActionPanel.vue tests/capture-store.test.ts tests/action-panel.test.ts docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(ui): panel reopen dismisses only stale rename prompts" -m "GAP-29: the shownNonce watcher killed the rename prompt unconditionally, so a recording stopped from the tray (panel closed) could never be named — the 30 s window only worked with the panel already open. The store stamps lastSavedAtMs and the watcher now uses dismissRenameIfStale, keeping prompts younger than RENAME_PROMPT_MS." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 4: RecordMode's transcription toggle never persists a default-seeded config (C4 · GAP-30)

**Files:**
- Modify: `src/components/RecordMode.vue` (`loadConfig`)
- Modify: `docs/Gaps.md` (GAP-30 tombstone)
- Test: `tests/record-mode.test.ts`

- [ ] **Step 1: Write the failing test** (mirror the file's mount + mockIPC idiom):

```ts
it("does not persist after a failed config load (GAP-30)", async () => {
  // loadConfig's finally set loaded=true even on failure, so one
  // transcription toggle persisted the default-seeded config — wiping the
  // vault's real recordingFolder/bitrate/devices on disk.
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "get_capture_config") throw new Error("read failed");
    if (cmd === "list_recordings") return [];
    if (cmd === "capture_status") return { recording: false /* ...per file idiom */ };
    return undefined;
  });
  // ...mount RecordMode per the file's idiom, await settle...
  // ...flip the transcription toggle per the file's idiom...
  expect(calls).not.toContain("set_capture_config");
});
```

- [ ] **Step 2: RED** — `npx vitest run tests/record-mode.test.ts` (the toggle persists → `set_capture_config` present).

- [ ] **Step 3: Implement** — in `loadConfig`, move the `loaded` flip out of `finally` into the success path, and log the failure (no-swallowed-error rule):

```ts
async function loadConfig() {
  // A config read failure must never block recording — config keeps the
  // defaults above, so the toggles stay usable for THIS session. But a
  // failed read must never unlock persistence: one toggle would rewrite
  // the vault's real settings with the default-seeded object (GAP-30 —
  // the tasksFolderLoaded gate pattern in CaptureSettings).
  try {
    config.value = await invoke<CaptureConfig>("get_capture_config", { id: props.vaultId });
    loaded.value = true;
  } catch (e) {
    logWarning(`get_capture_config failed (vault ${props.vaultId}): ${String(e)}`);
  }
}
```

Check the `transcription` setter's comment ("Never persist against the default-seeded config") still reads true — it does, and now the gate actually enforces it on failure too.

- [ ] **Step 4: GREEN** — `npx vitest run tests/record-mode.test.ts && npm run build && npm run lint`. If an existing test pinned the old save-after-failure behavior, update it to the new contract and say so.

- [ ] **Step 5: Tombstone GAP-30 + commit**

```bash
git add src/components/RecordMode.vue tests/record-mode.test.ts docs/Gaps.md
git commit -m "fix(ui): gate RecordMode persistence on a successful config load" -m "GAP-30: loadConfig's finally set loaded=true even when the read failed, so the transcription setter persisted the default-seeded config (folder null, bitrate 128, devices null) over the user's real settings. loaded now flips only on success — the CaptureSettings tasksFolderLoaded pattern — and the failure is logged." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 5: IME-composition guards on the add-task Enter and the filter Escape (C5 · GAP-31)

**Files:**
- Modify: `src/components/Tasks.vue` (`@keydown.enter="add"` at ~line 464 → guarded handler)
- Modify: `src/components/ActionPanel.vue` (`onFilterEscape`)
- Modify: `docs/Gaps.md` (GAP-31 tombstone)
- Test: `tests/tasks.test.ts`, `tests/action-panel.test.ts`

- [ ] **Step 1: Write the failing tests**

`tests/tasks.test.ts` (mirror the add-task test idiom):

```ts
it("ignores Enter while composing an IME candidate (GAP-31)", async () => {
  // Committing a CJK candidate with Enter used to immediately create a task
  // document from the half-composed title — a sanctioned vault write.
  // ...mount with >0 vaults, type a title per the file's idiom...
  await titleInput.trigger("keydown.enter", { isComposing: true });
  expect(/* add_task NOT invoked — assert per the file's mockIPC recording */).toBe(true);
  await titleInput.trigger("keydown.enter", { isComposing: false });
  // ...the normal add proceeds (existing tests already pin this half)...
});
```

`tests/action-panel.test.ts`:

```ts
it("filter Escape ignores IME composition (GAP-31)", async () => {
  // ...mount with >5 vaults so the filter shows, type a query...
  await filterInput.trigger("keydown.esc", { isComposing: true });
  expect((filterInput.element as HTMLInputElement).value).not.toBe("");
});
```

- [ ] **Step 2: RED** — `npx vitest run tests/tasks.test.ts tests/action-panel.test.ts`.

- [ ] **Step 3: Implement**

`Tasks.vue`: template `@keydown.enter="add"` → `@keydown.enter="onTitleEnter"`; script, next to `add()`:

```ts
function onTitleEnter(e: KeyboardEvent) {
  // GAP-31: committing an IME candidate fires Enter with isComposing=true —
  // that must select the candidate, never create a task document (a vault
  // write) from the half-composed title. The Search view's handlers are the
  // precedent.
  if (e.isComposing) return;
  void add();
}
```

`ActionPanel.vue` `onFilterEscape`: first line `if (event.isComposing) return;` with a one-line `// GAP-31` comment.

- [ ] **Step 4: GREEN** — `npx vitest run tests/tasks.test.ts tests/action-panel.test.ts && npm run build && npm run lint`. LOC: Tasks.vue allowlisted at 760 — `--update` + body line if grown.

- [ ] **Step 5: Tombstone GAP-31 + commit**

```bash
git add src/components/Tasks.vue src/components/ActionPanel.vue tests/tasks.test.ts tests/action-panel.test.ts docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(ui): ignore IME composition on the add-task Enter and filter Escape" -m "GAP-31: Search guards its keys with event.isComposing but Tasks/the panel filter did not — a CJK user committing a candidate with Enter immediately created a task document from the half-composed title." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 6: Fresh badges, faithful reverts, honest toast TTLs (C6 · GAP-32 selected trio)

**Files:**
- Modify: `src/stores/vaults.ts` (new `refreshTaskCount(id)`; `back()` reloads counts when leaving tasks)
- Modify: `src/components/Tasks.vue` (call the refresh on mutation success; toggle revert restores the ORIGINAL status; archive re-insert re-sorts)
- Modify: `src/stores/notifications.ts` (dedupe restarts the TTL; dismiss clears the timer)
- Modify: `docs/Gaps.md` (GAP-32: annotate the three fixed bullets inline, entry stays open)
- Test: `tests/vaults-store.test.ts`, `tests/tasks.test.ts`, `tests/notifications-store.test.ts`

**Interfaces:**
- Produces: `vaults` store action `async refreshTaskCount(id: string): Promise<void>` — one `count_open_tasks` call, replaces only that vault's entry, logs-and-keeps-previous on failure (do NOT zero a badge because one refresh failed — unlike the full `loadTaskCounts`, this runs mid-session).

- [ ] **Step 1: Write the failing tests**

`tests/notifications-store.test.ts`:

```ts
it("dedupe-reuse restarts the TTL (GAP-32)", () => {
  // A re-raise at t=3.9s used to vanish at t=4.0s and read as flicker.
  vi.useFakeTimers();
  const store = useNotificationsStore();
  const id = store.success("saved");
  vi.advanceTimersByTime(3_900);
  expect(store.notify("success", "saved")).toBe(id); // dedupe-reuse
  vi.advanceTimersByTime(3_000); // 6.9s after first, 3.0s after reuse
  expect(store.items.some((i) => i.id === id)).toBe(true);
  vi.advanceTimersByTime(1_100);
  expect(store.items.some((i) => i.id === id)).toBe(false);
  vi.useRealTimers();
});
```

`tests/tasks.test.ts`:

```ts
it("failed toggle restores the ORIGINAL status, not a forged one (GAP-32)", async () => {
  // Revert used to hardcode status "new": a failed toggle on an
  // in-progress task silently relabeled it.
  // ...mount with a task whose status is "in-progress" (done=false), make
  //    set_task_status reject per the file's idiom...
  // ...click the checkbox, settle...
  expect(task.status).toBe("in-progress");
  expect(task.done).toBe(false);
});

it("refreshes the vault's task count after a successful mutation (GAP-32)", async () => {
  // Badges only reloaded on panel-shown — stale after add/toggle/archive
  // until reopen (Codex PR #46 finding).
  // ...mount, successful toggle per the file's idiom...
  expect(/* count_open_tasks invoked for the row's vaultId after the write */).toBe(true);
});
```

`tests/vaults-store.test.ts`:

```ts
it("refreshTaskCount updates one vault and keeps the previous count on failure (GAP-32)", async () => {
  // ...seed store.taskCounts = { a: 2, b: 5 }; mock count_open_tasks → 3 for "a"...
  await store.refreshTaskCount("a");
  expect(store.taskCounts.a).toBe(3);
  // ...mock a rejection for "b"...
  await store.refreshTaskCount("b");
  expect(store.taskCounts.b).toBe(5); // kept, not zeroed; failure logged
});
```

- [ ] **Step 2: RED** — `npx vitest run tests/notifications-store.test.ts tests/tasks.test.ts tests/vaults-store.test.ts`.

- [ ] **Step 3: Implement**

`notifications.ts` — timers become cancellable and dedupe re-arms:

```ts
const timers = new Map<number, ReturnType<typeof setTimeout>>();

// inside notify():
      const last = this.items[this.items.length - 1];
      const ttlOf = (k: NotifyKind) => (opts?.ttlMs !== undefined ? opts.ttlMs : DEFAULT_TTL[k]);
      if (last && last.kind === kind && last.message === message) {
        // GAP-32: reusing the newest identical toast must also restart its
        // TTL — a re-raise moments before expiry otherwise reads as flicker.
        const ttlMs = ttlOf(kind);
        const t = timers.get(last.id);
        if (t) clearTimeout(t);
        if (ttlMs != null) timers.set(last.id, setTimeout(() => this.dismiss(last.id), ttlMs));
        return last.id;
      }
      const id = ++seq;
      this.items.push({ id, kind, message });
      if (this.items.length > MAX_ITEMS) this.items.splice(0, this.items.length - MAX_ITEMS);
      const ttlMs = ttlOf(kind);
      if (ttlMs != null) timers.set(id, setTimeout(() => this.dismiss(id), ttlMs));
      return id;

// dismiss():
    dismiss(id: number) {
      const t = timers.get(id);
      if (t) clearTimeout(t);
      timers.delete(id);
      this.items = this.items.filter((i) => i.id !== id);
    },
// clear(): also clear all timers.
```

`vaults.ts` — after `loadTaskCounts`:

```ts
    /** Refresh ONE vault's open-task badge after a mutation (GAP-32 / Codex
     * PR #46): panel-shown is too late for a badge the user is looking at.
     * On failure keep the previous count — zeroing a badge because one
     * mid-session refresh failed would misreport a vault that has tasks. */
    async refreshTaskCount(id: string) {
      try {
        const count = await invoke<number>("count_open_tasks", { id });
        this.taskCounts = { ...this.taskCounts, [id]: count };
      } catch (e) {
        logWarning(`count_open_tasks refresh failed for vault ${id}: ${String(e)}`);
      }
    },
```

and in `back()`'s tasks branch: `} else if (this.view === "tasks") { void this.loadTaskCounts(); return this.showList(); }` (full reload when leaving the tasks view — covers bulk edits and the aggregate view's null vaultId).

`Tasks.vue`:
- `toggle()`: capture `const prevStatus = task.status;` before the flip; revert becomes `task.done = prevStatus === "done"; task.status = prevStatus; sortInPlace();`. On SUCCESS append `void vaultsStore.refreshTaskCount(task.vaultId);` (the component already imports the vaults store for `allVaults` — check the actual local name).
- `archive()`: failure path becomes `tasks.value.push(removed); sortInPlace();` (recompute placement at revert time — the captured index is stale after a concurrent add). On SUCCESS append the same `refreshTaskCount(task.vaultId)`.
- `add()`: after the successful unshift/sort, `void vaultsStore.refreshTaskCount(targetVault);`.

- [ ] **Step 4: GREEN** — `npx vitest run tests/notifications-store.test.ts tests/tasks.test.ts tests/vaults-store.test.ts && npm run build && npm run lint`. LOC (`Tasks.vue`, `vaults.ts` unlisted/allowlisted — check) `--update` + body line if tripped.

- [ ] **Step 5: GAP-32 partial annotation + commit**

In GAP-32, annotate exactly three bullets: the `taskCounts` bullet, the failed-toggle bullet, and the notifications bullet each gain `(FIXED 2026-07-10 — <what changed, one line>)`. The entry heading and remaining bullets stay untouched.

```bash
git add src/stores/vaults.ts src/stores/notifications.ts src/components/Tasks.vue tests/vaults-store.test.ts tests/notifications-store.test.ts tests/tasks.test.ts docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(ui): live task badges, faithful toggle reverts, honest toast TTLs" -m "GAP-32 (selected trio, incl. the Codex PR #46 badge finding): taskCounts now refreshes per-vault on add/toggle/archive success and fully on leaving the tasks view; a failed toggle restores the task's ORIGINAL status instead of forging \"new\" and the archive revert re-sorts instead of trusting a stale index; a deduped toast restarts its TTL and dismissed toasts cancel their timers." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

(Controller replies to + resolves the Codex thread after this task's review — not the implementer.)

---

### Task 7: One shared vault lookup; no absolute paths in user-facing errors (C8 · GAP-26)

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (`set_capture_config` ~line 210, `start_capture_blocking` ~line 329)
- Modify: `src-tauri/src/task_commands.rs` (~lines 34, 64 — the two `discover_vaults().find(...)` lookups)
- Modify: `docs/Gaps.md` (GAP-26 tombstone)
- Test: existing suites (this is a DRY/copy refactor; `commands::find_vault` → `services::find_vault` is already tested in core — state that instead of inventing tests)

**Design:** `crate::commands::find_vault(id)` already wraps `services::find_vault` (user-worded copy + `(id: …)`). The four shell sites that hand-roll `discovery::discover_vaults().into_iter().find(|v| v.id == id)` switch to it. Then sweep user-facing error strings that embed absolute paths: `grep -n '{}", vault.path\|Vault folder not found' src-tauri/src/*.rs` — each becomes a user-worded message with the path moved to a `log::warn!` line (e.g. `Err("Vault folder not found — was it moved or deleted?".to_string())` after `log::warn!("start_capture: vault folder missing: {}", vault.path)`). Do NOT touch `services::find_vault` itself (MCP contract) or read-only degrade paths that never surface to the user.

- [ ] **Step 1: Make the four lookup sites delegate**

Each `let vault = discovery::discover_vaults().into_iter().find(|v| v.id == id).ok_or(...)?;` becomes `let vault = crate::commands::find_vault(&id)?;` (adjust the borrow to the local's type). Remove now-unused `discovery` imports ONLY if nothing else in the file uses them (grep first — capture_commands.rs still uses discovery in `run_recovery`/`open_recording_note`/`rename_capture`).

- [ ] **Step 2: Sweep the path-embedding errors**

For every user-facing `Err(format!(...))` that interpolates `vault.path` or another absolute path in the four command files, split into `log::warn!` (with the path) + a path-free user message. List each site + before/after in your report. Known sites from the gap: the vault-folder-missing error in `start_capture_blocking` and `add_task`'s equivalent in task_commands.rs; verify with the grep rather than trusting these two to be exhaustive. Leave `Err` strings that only reach logs untouched.

- [ ] **Step 3: Gates**

`cd src-tauri && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib`, `npx tauri build --no-bundle` (repo root), `npm test` (frontend tests that assert on error toasts must still pass — if one asserted on a path-bearing message, update it to the new copy and say so). `npm run check:loc` (--update + body line if tripped).

- [ ] **Step 4: Tombstone GAP-26 + commit**

```bash
git add src-tauri/src/capture_commands.rs src-tauri/src/task_commands.rs docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(shell): one shared vault lookup; keep absolute paths out of user-facing errors" -m "GAP-26: the discover_vaults().find(..) lookup was duplicated across the shell with two error styles, and several user-facing errors embedded absolute local paths. The four hand-rolled sites now delegate to commands::find_vault (the services user-worded copy), and path detail moved to the log lines." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 8: The deferred-debt trio (C9)

**Files:**
- Modify: `src/components/Tasks.vue` (~line 85, the `busy` set comment)
- Modify: `tests/tasks.test.ts` (~line 847, the teleport test cleanup)
- Modify: `src/stores/vaults.ts:5` (import comma-space)

- [ ] **Step 1: Apply all three**

(a) Above `const busy = ref(new Set<string>());`, extend the comment block's last line with: `// Keyed by path alone: task paths are unique across vaults in practice (two vaults would have to contain the same absolute file), and the aggregation spec documents that assumption — this comment is its code-side anchor.` — adjust wording to flow with the existing comment.

(b) In the teleport/attachTo test near tests/tasks.test.ts:847: wrap its unmount/body-reset tail in `try { ...asserts... } finally { wrapper.unmount(); document.body.innerHTML = ""; }` (match the file's actual cleanup lines — the goal: cleanup runs even when an assert throws, so a second attachTo test can't inherit a dirty body).

(c) `vaults.ts:5`: add the missing space after the comma in the import list.

- [ ] **Step 2: Gates + commit**

`npx vitest run tests/tasks.test.ts && npm run lint && npm run build`

```bash
git add src/components/Tasks.vue tests/tasks.test.ts src/stores/vaults.ts
git commit -m "chore(ui): aggregation review's deferred debt — comment, test cleanup, import space" -m "The cross-vault path-uniqueness assumption behind the busy set is now stated in code; the teleport test's cleanup is exception-safe; the vaults import gets its comma space. All three were recorded by the task-aggregation final review for the next touch." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 9: Sub-pass close-out — full gate run

- [ ] **Step 1: Full gate run, CI order** (same command list as sub-pass B's Task 5: lint → check:loc → check:quality → test:coverage → cargo fmt --check → workspace clippy → all crate suites → shell lib tests → deny → tauri build --no-bundle; machete/llvm-cov are CI-only, confirm the rust-core job is green on the pushed head instead of claiming a local run).
- [ ] **Step 2: Ledger** — GAP-26/27/28/29/30/31/33 tombstones + GAP-32's three inline annotations present; one commit per task.
- [ ] **Step 3: Push** (`git push -u origin claude/task-management-vertical-slice-ikeuly`; PR #46 exists — no new PR). Then the controller dispatches the final whole-sub-pass review (most capable model) and, after it approves, replies to + resolves the Codex taskCounts thread.

---

## Self-review record

- **Spec coverage:** C1→T1, C2→T2, C3→T3, C4→T4, C5→T5, C6(a/b/c)→T6, C7→T1, C8→T7, C9→T8; close-out→T9. The spec's C6 wording "on back()" is implemented as the full-reload branch in `back()`; per-mutation refresh covers the in-view badge.
- **Placeholder scan:** test skeletons deliberately defer mount/mock boilerplate to each file's existing idiom (named per test) while spelling out every contract assertion — consistent with how B's briefs handled the same and with the mirror-the-idiom global constraint. No TBDs.
- **Type consistency:** `refreshTaskCount(id: string)` matches its three Tasks.vue call sites and the vaults-store test; `dismissRenameIfStale()` matches the ActionPanel call and capture-store test; `lastSavedAtMs: number | null` used consistently; `optionId(i)`/`listboxId` consistent between script and template.
- **Known scope guard:** GAP-32's unfixed bullets (ticketing, init re-entry, back() dead branches, progress typing) stay catalogued — T6's tombstone step annotates only the three fixed bullets.
