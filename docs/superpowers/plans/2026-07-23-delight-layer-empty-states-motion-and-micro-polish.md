# Delight Layer (Empty States, Motion & Micro-Polish) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a visible, additive polish layer on top of the merged design system — a shared `EmptyState` primitive rolled out to the panel's bare-`<p>` empty states, a reduced-motion-safe cross-view fade, a shared `Spinner`, and light press micro-interactions in the interactive primitives.

**Architecture:** Purely presentational/frontend. Two new SFC primitives under `src/components/ui/` (extending the existing design system), a small CSS/`<Transition>` motion layer, and a one-line press treatment folded into the existing interactive primitives. No `src-tauri`, config, IPC, or store changes. Behavior-preserving: empty-state message text stays byte-identical (so the existing Vitest suite is the regression net); animations change how things appear, never what renders.

**Tech Stack:** Vue 3.5 (`<script setup lang="ts">`), Tailwind CSS 4 (CSS-based config; `@theme` tokens already exist), Vitest 4 + `@vue/test-utils` 2 + happy-dom.

## Global Constraints

- **No `src-tauri`/config/IPC/store changes.** Frontend-only; the `frontend` CI job is the gate.
- **Behavior-preserving.** Empty-state message text and every existing `data-testid`/`aria-label` stay byte-identical. The empty-state `<p>`s carry **no `data-testid`** today (verified) — do not add one. Existing view tests must pass **unchanged**; a test that must change means behavior changed — treat it as a defect, not a test edit.
- **`.text()` preservation rule.** When swapping a `<p>message</p>` for `<EmptyState title="message">`, keep the title string **verbatim** and **do not add hint text** during the rollout (an aria-hidden icon adds no text, so `.text()` stays identical; a hint would change it and can break a `toBe`/`toContain` assertion).
- **Tokens only** in new components (`text-fg`/`text-fg-muted`/`text-micro`, `rounded-control`, …) per the AGENTS.md UI-primitives contract; consume primitives at their declared prop names.
- **Reduced motion** is already enforced globally in `src/style.css` (`@media (prefers-reduced-motion: reduce)` zeroes animation/transition durations) — new motion must rely on that, not re-implement it.
- **Additive only.** No view added/removed; no navigation, banner stack, window sizing, or window-system invariant touched.
- **TDD + Conventional Commits** (`feat(ui)`/`refactor(ui)`/`style(ui)`/`docs`). Keep lint at **0 new warnings** (add `undefined` defaults for optional props; keep `:class` on its own line).

---

## Task 1: `EmptyState` primitive

**Files:**
- Create: `src/components/ui/EmptyState.vue`
- Test: `tests/empty-state.test.ts`

**Interfaces:**
- Produces: `EmptyState` — props `{ title: string; hint?: string }`; slots `icon` (optional, rendered aria-hidden) and `action` (optional, e.g. an `AppButton`). Renders a centered column: optional icon, the `title` (in `text-fg-muted`, same brightness as the old `<p>`), optional `hint` (`text-micro text-fg-subtle`), optional action.

- [ ] **Step 1: Write the failing test**

```ts
// tests/empty-state.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import EmptyState from "../src/components/ui/EmptyState.vue";

describe("EmptyState", () => {
  it("renders the title verbatim and nothing else by default", () => {
    const w = mount(EmptyState, { props: { title: "No tasks yet." } });
    // .text() must equal the title exactly, so a `<p>`→EmptyState swap keeps
    // existing `.text()` assertions green.
    expect(w.text()).toBe("No tasks yet.");
    expect(w.get("[data-testid='empty-state']").exists()).toBe(true);
  });

  it("renders an aria-hidden icon slot without adding text", () => {
    const w = mount(EmptyState, {
      props: { title: "No recordings yet." },
      slots: { icon: "<svg data-testid='ic'/>" },
    });
    expect(w.find("[data-testid='ic']").exists()).toBe(true);
    expect(w.get("[data-testid='empty-state-icon']").attributes("aria-hidden")).toBe("true");
    expect(w.text()).toBe("No recordings yet."); // icon contributes no text
  });

  it("renders the optional hint and action slot", () => {
    const w = mount(EmptyState, {
      props: { title: "Obsidian not found", hint: "Install it first." },
      slots: { action: "<button>Retry</button>" },
    });
    expect(w.text()).toContain("Obsidian not found");
    expect(w.text()).toContain("Install it first.");
    expect(w.find("button").exists()).toBe(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/empty-state.test.ts`
Expected: FAIL (cannot resolve `../src/components/ui/EmptyState.vue`).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/EmptyState.vue -->
<script setup lang="ts">
// Centered empty/degraded-state block: an optional icon, a title (the exact
// former `<p>` message), an optional hint, and an optional action slot.
// Replaces the lonely gray one-liners with a consistent, on-brand treatment.
defineProps<{ title: string; hint?: string }>();
</script>

<template>
  <div
    data-testid="empty-state"
    class="flex flex-col items-center gap-2 px-4 py-8 text-center"
  >
    <span
      v-if="$slots.icon"
      data-testid="empty-state-icon"
      class="text-fg-subtle"
      aria-hidden="true"
    >
      <slot name="icon" />
    </span>
    <p class="text-xs text-fg-muted">{{ title }}</p>
    <p
      v-if="hint"
      class="text-micro text-fg-subtle"
    >{{ hint }}</p>
    <slot name="action" />
  </div>
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/empty-state.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/components/ui/EmptyState.vue tests/empty-state.test.ts
git commit -m "feat(ui): add EmptyState primitive"
```

---

## Task 2: Roll `EmptyState` into ActionPanel, Tasks, Recordings

**Files:**
- Modify: `src/components/ActionPanel.vue` (the two `<p>` states at ~376-388)
- Modify: `src/components/Tasks.vue` (the two `<p>` states at ~401-412)
- Modify: `src/components/Recordings.vue` (the `<p>` at ~146-151)
- Test (gate, unchanged): `tests/action-panel.test.ts`, `tests/tasks.test.ts`, `tests/recordings.test.ts`

**Interfaces:**
- Consumes: `EmptyState` (Task 1); `AppIcon` (existing, `src/components/AppIcon.vue`).

- [ ] **Step 1: Green baseline**

Run: `npx vitest run tests/action-panel.test.ts tests/tasks.test.ts tests/recordings.test.ts`
Expected: PASS (record counts; they must not change).

- [ ] **Step 2: ActionPanel — import + swap the two states**

Add to the `<script setup>` imports: `import EmptyState from "./ui/EmptyState.vue";` and (for the icon) confirm `AppIcon` is imported (it is).

Replace the two `<p class="text-xs text-slate-400">` blocks (the *No vaults match* and *Obsidian not found* states) — keep the exact `v-else-if` conditions and the exact text. The filter message interpolates `filter`, so it must be a **bound** `:title` (a static attribute can't contain `{{ }}`):

```vue
<EmptyState
  v-else-if="store.vaults.length > 0"
  :title="`No vaults match &quot;${filter}&quot;.`"
/>
<EmptyState
  v-else-if="store.loaded"
  title="Obsidian not found — no vaults discovered. Is Obsidian installed and has it been opened at least once?"
>
  <template #icon>
    <AppIcon :size="28">
      <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7l-2-2H5a2 2 0 0 0-2 2Z" />
    </AppIcon>
  </template>
</EmptyState>
```

(The bound title for the filter case reproduces `No vaults match "<filter>".` exactly — verify `.text()` in `tests/action-panel.test.ts` still matches. The *Obsidian not found* title is the full original string verbatim.)

- [ ] **Step 3: Tasks — swap the two states**

Import `EmptyState`. Replace the `<p>` at ~401-405 (empty) and ~407-412 (filter-empty), keeping the exact `v-else-if` conditions and text. The filter-empty one has an interpolated message — reproduce it with a bound title:

```vue
<EmptyState
  v-else-if="tasks.length === 0 && buckets.length === 0"
  title="No tasks yet."
>
  <template #icon>
    <AppIcon :size="28">
      <path d="M9 11l3 3 8-8" />
      <path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h9" />
    </AppIcon>
  </template>
</EmptyState>
<EmptyState
  v-else-if="buckets.length === 0"
  :title="`No tasks match${tagFilter ? ` #${tagFilter}` : ''}${showFilter && filter ? ` &quot;${filter}&quot;` : ''}.`"
/>
```

(`AppIcon` must be imported in `Tasks.vue` if not already — check the import block and add `import AppIcon from "./AppIcon.vue";` if missing. Verify the filter-empty bound title renders the SAME string the old `<p>` produced — `tests/tasks.test.ts` asserts on these.)

- [ ] **Step 4: Recordings — swap the empty state**

Import `EmptyState` + ensure `AppIcon`. Replace the `<p>` at ~146-150:

```vue
<EmptyState
  v-else-if="recordings.length === 0"
  title="No recordings yet."
>
  <template #icon>
    <AppIcon :size="28">
      <rect x="9" y="3" width="6" height="11" rx="3" />
      <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
    </AppIcon>
  </template>
</EmptyState>
```

- [ ] **Step 5: Run the gate + build**

Run: `npx vitest run tests/action-panel.test.ts tests/tasks.test.ts tests/recordings.test.ts && npm run build`
Expected: PASS with the SAME counts as Step 1; build clean. If any assertion fails on the interpolated titles, adjust the bound-title expression until `.text()` matches the original byte-for-byte — do NOT change the test.

- [ ] **Step 6: Commit**

```bash
git add src/components/ActionPanel.vue src/components/Tasks.vue src/components/Recordings.vue
git commit -m "feat(ui): friendly EmptyState for vault/task/recording empty states"
```

---

## Task 3: Roll `EmptyState` into Transcriptions, ImportVaultPicker, Search

**Files:**
- Modify: `src/components/Transcriptions.vue` (the `<p>` at ~106-111)
- Modify: `src/components/ImportVaultPicker.vue` (the `<p>` at ~240-245)
- Modify: `src/components/Search.vue` (the two states at ~331 and ~369)
- Test (gate, unchanged): `tests/search.test.ts` (+ any transcriptions/import tests present)

**Interfaces:**
- Consumes: `EmptyState`, `AppIcon`.

- [ ] **Step 1: Green baseline**

Run: `npx vitest run tests/search.test.ts` (and `npx vitest run tests/transcriptions.test.ts tests/import-vault-picker.test.ts` if those files exist — check `ls tests/ | grep -E 'transcription|import'`).
Expected: PASS (record counts).

- [ ] **Step 2: Transcriptions — swap the empty state**

Import `EmptyState` + `AppIcon`. Replace the `<p>` at ~106-110 (`v-if="isEmpty"`, text `No transcriptions yet.`) with an `EmptyState title="No transcriptions yet."` carrying a document/text `AppIcon` (`:size="28"`, e.g. paths `M4 4h16v16H4Z` + `M8 9h8M8 13h5`), preserving the `v-if="isEmpty"` condition.

- [ ] **Step 3: ImportVaultPicker — swap the empty state**

Import `EmptyState` + `AppIcon`. Replace the `<p>` at ~240-244 (`v-else-if="viewState === 'empty'"`, text `No vaults found.`) with `EmptyState title="No vaults found."` + the same vault/folder `AppIcon` used in ActionPanel's Obsidian-not-found state, preserving the `v-else-if` condition.

- [ ] **Step 4: Search — swap the two no-results states**

Import `EmptyState` + `AppIcon`. Replace the two states, preserving their exact conditions and the exact (interpolated) text:
- ~331 `No matches for "{{ resultsQuery }}".` → `<EmptyState :title="\`No matches for &quot;${resultsQuery}&quot;.\`">` with a magnifier `AppIcon` (`:size="28"`, paths `<circle cx="11" cy="11" r="8" />` + `<path d="m21 21-4.35-4.35" />`).
- ~369 `Nothing matches this filter.` → `<EmptyState title="Nothing matches this filter." />` (title-only; it's a transient filter state).

Read the surrounding markup first to keep each swap inside its existing conditional wrapper. `tests/search.test.ts` asserts the "No matches"/summary text — verify `.text()` still matches after the swap.

- [ ] **Step 5: Run the gate + build**

Run: `npx vitest run tests/search.test.ts && npm run build` (plus the transcriptions/import test files if they exist).
Expected: PASS with the SAME counts as Step 1; build clean.

- [ ] **Step 6: Commit**

```bash
git add src/components/Transcriptions.vue src/components/ImportVaultPicker.vue src/components/Search.vue
git commit -m "feat(ui): friendly EmptyState for transcription/import/search empty states"
```

---

## Task 4: `Spinner` primitive + opportunistic swaps

**Files:**
- Create: `src/components/ui/Spinner.vue`
- Test: `tests/spinner.test.ts`
- Modify: `src/components/VaultList.vue` and `src/components/Recordings.vue` **only where the swap is a clean 1:1** (same size/markup)
- Test (gate): `tests/vault-list.test.ts`, `tests/recordings.test.ts`

**Interfaces:**
- Produces: `Spinner` — props `{ size?: "sm" | "md"; label?: string }`. Renders the spinning ring (`animate-spin rounded-full border-2 border-white/30 border-t-white`), `role="status"`, `aria-label` from `label` (default "Loading…"). `sm` = `h-4 w-4`, `md` = `h-5 w-5`.

- [ ] **Step 1: Write the failing test**

```ts
// tests/spinner.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Spinner from "../src/components/ui/Spinner.vue";

describe("Spinner", () => {
  it("renders a spinning status ring with a default label", () => {
    const w = mount(Spinner);
    const el = w.get("[data-testid='spinner']");
    expect(el.classes()).toEqual(expect.arrayContaining(["animate-spin", "rounded-full"]));
    expect(el.attributes("role")).toBe("status");
    expect(el.attributes("aria-label")).toBe("Loading…");
  });

  it("honors a custom label and size", () => {
    const w = mount(Spinner, { props: { label: "Opening vault…", size: "md" } });
    const el = w.get("[data-testid='spinner']");
    expect(el.attributes("aria-label")).toBe("Opening vault…");
    expect(el.classes()).toContain("h-5");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/spinner.test.ts`
Expected: FAIL (cannot resolve the SFC).

- [ ] **Step 3: Write the primitive**

```vue
<!-- src/components/ui/Spinner.vue -->
<script setup lang="ts">
// The shared spinning ring, dedup'ing the hand-rolled `animate-spin` copies.
withDefaults(defineProps<{ size?: "sm" | "md"; label?: string }>(), {
  size: "sm",
  label: "Loading…",
});
</script>

<template>
  <span
    data-testid="spinner"
    role="status"
    :aria-label="label"
    class="inline-block shrink-0 animate-spin rounded-full border-2 border-white/30 border-t-white"
    :class="size === 'md' ? 'h-5 w-5' : 'h-4 w-4'"
  />
</template>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/spinner.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Opportunistic 1:1 swaps (read first)**

Open `VaultList.vue` and `Recordings.vue`. Find any `<span class="… h-4 w-4 … animate-spin rounded-full border-2 border-white/30 border-t-white …" role="status" aria-label="…">`. Where the markup matches Spinner's output 1:1 (an `h-4 w-4` ring with `role="status"` + an aria-label), replace it with `<Spinner :label="…" />`, preserving the exact `aria-label`. **Skip** any spinner that carries extra layout classes (`block`, `mr-1`, positioning) or a different size — leave those for GAP-66 rather than reshaping markup here. If neither file has a clean 1:1 site, make no change to it (the primitive still lands for future use — note this in the report).

- [ ] **Step 6: Run the gate + build**

Run: `npx vitest run tests/spinner.test.ts tests/vault-list.test.ts tests/recordings.test.ts && npm run build`
Expected: PASS; build clean. (VaultList's busy spinners carry an aria-label the tests assert — confirm those labels are preserved by the swap.)

- [ ] **Step 7: Commit**

```bash
git add src/components/ui/Spinner.vue tests/spinner.test.ts src/components/VaultList.vue src/components/Recordings.vue
git commit -m "feat(ui): add Spinner primitive; adopt at clean 1:1 sites"
```

---

## Task 5: Cross-view fade (guarded, separable)

**Files:**
- Modify: `src/components/ActionPanel.vue` (wrap the view-body `v-if/v-else-if` chain, ~290-389)
- Modify: `src/style.css` (add the `.view-*` transition classes)
- Test (gate, unchanged): `tests/action-panel.test.ts`, `tests/panel-root.test.ts`

**Interfaces:** none new.

- [ ] **Step 1: Green baseline**

Run: `npx vitest run tests/action-panel.test.ts tests/panel-root.test.ts`
Expected: PASS (record counts).

- [ ] **Step 2: Add the transition CSS**

Append to `src/style.css`:

```css
/* Panel view crossfade. Opacity-only (no transform → no layout thrash in the
   fixed panel). The prefers-reduced-motion rule above zeroes this out. */
.view-enter-active,
.view-leave-active {
  transition: opacity 120ms ease;
}
.view-enter-from,
.view-leave-to {
  opacity: 0;
}
```

- [ ] **Step 3: Wrap the view chain and key each branch**

In `ActionPanel.vue`, wrap the entire `v-if="view === 'settings'"` … `v-else` (list) chain of `<div class="panel-scroll …">` blocks in a single `<Transition name="view" mode="out-in">`, and add a static `:key` to each branch div so Vue treats each view as a distinct element (required for `mode="out-in"` to animate the swap). Keys, one per branch: `settings`, `captureSettings`, `recordings`, `recordMode`, `transcriptions`, `tasks`, `search`, `importPicker`, `documentImport`, `update`, and `list` (the final `v-else`). Example (first and last branch shown; apply the same `:key` addition to every branch, changing nothing else):

```vue
<Transition name="view" mode="out-in">
  <div
    v-if="view === 'settings'"
    key="settings"
    class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
  >
    <BuddySettings />
  </div>
  <!-- … every other branch unchanged except for its added key … -->
  <div
    v-else
    key="list"
    class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
  >
    <!-- VaultList / EmptyState branches unchanged -->
  </div>
</Transition>
```

`<NotificationHost />` stays OUTSIDE the `<Transition>` (it must not transition). The banners above the chain (RecordingBar, TranscriptionSummary, ImportProgress, RenamePrompt, error, filter) are already outside it — leave them.

- [ ] **Step 4: Run the gate + build**

Run: `npx vitest run tests/action-panel.test.ts tests/panel-root.test.ts && npm run build`
Expected: PASS with the SAME counts as Step 1; build clean.

**Guard (separable task):** if any `action-panel`/`panel-root` test needs its assertions changed to pass, the wrapper altered behavior (focus landing on `panel-shown`, the `/`-to-search focus jump, or a `panel-scroll` container) — **revert this task entirely** and report it. The fade is a nice-to-have; the empty states (Tasks 2-3) are the increment's core and must not be risked for it. Do NOT edit a test to accommodate the wrapper.

- [ ] **Step 5: Commit**

```bash
git add src/components/ActionPanel.vue src/style.css
git commit -m "feat(ui): reduced-motion-safe crossfade between panel views"
```

---

## Task 6: Press micro-interaction (light, tunable, last)

**Files:**
- Modify: `src/components/ui/IconButton.vue`, `src/components/ui/AppButton.vue`, `src/components/ui/Chip.vue`
- Test (gate): `tests/icon-button.test.ts`, `tests/app-button.test.ts`, `tests/chip.test.ts`

**Interfaces:** none new (additive class only).

- [ ] **Step 1: Green baseline**

Run: `npx vitest run tests/icon-button.test.ts tests/app-button.test.ts tests/chip.test.ts`
Expected: PASS (record counts).

- [ ] **Step 2: Add the press treatment**

In each of the three primitives' root button class list, add `active:scale-95` and change the existing `transition-colors` to `transition` (so the transform animates too). For `Chip`, apply it only to the `interactive` (`<button>`) branch, not the static `<span>`. Example — `IconButton.vue`'s root `class="…"` gains `active:scale-95` and its `transition-colors` becomes `transition`. Keep everything else identical. (Scale is a transform — it does not affect layout; the global reduced-motion rule snaps it instantly, which is fine for a press.)

- [ ] **Step 3: Run the gate + build**

Run: `npx vitest run tests/icon-button.test.ts tests/app-button.test.ts tests/chip.test.ts && npm run build`
Expected: PASS with the SAME counts (the tests assert specific classes via `toContain`, so an added class is safe); build clean.

- [ ] **Step 4: Commit**

```bash
git add src/components/ui/IconButton.vue src/components/ui/AppButton.vue src/components/ui/Chip.vue
git commit -m "style(ui): subtle active:scale press feedback on interactive primitives"
```

*(This task is deliberately last and self-contained: if a look on Windows shows the scale reads too springy for the always-on-top shell, revert just this commit — nothing else depends on it. `active:opacity-80` is the fallback if scale is unwanted.)*

---

## Task 7: Docs, baselines, and the full gate

**Files:**
- Modify: `AGENTS.md` (add `EmptyState` + `Spinner` to the "UI primitives & design tokens" list)
- Modify: `scripts/loc-baseline.json` / `scripts/quality-baseline.json` (via `--update`) and `vite.config.ts` (only if a coverage floor rose)

- [ ] **Step 1: Document the two new primitives**

In AGENTS.md's "UI primitives & design tokens" subsection, add to the primitive list: `EmptyState` (centered icon + title + optional hint/action, for empty/degraded states) and `Spinner` (the shared `animate-spin` ring). One clause each, matching the existing entries' style.

- [ ] **Step 2: Run the full frontend gate**

Run: `rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage`
Expected: `lint` 0 errors and **no new warnings** (1 pre-existing `main.ts` warning is fine); the LOC/quality gates may report improvement (dedup).

- [ ] **Step 3: Bank any improved baselines**

Run (only the ones the gate reported as improved):

```bash
node scripts/check-loc.mjs --update
node scripts/check-quality.mjs --update
```

Then re-run `rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage` and confirm all pass. If a `vite.config.ts` coverage floor rose, bump it to the new floor.

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md scripts/loc-baseline.json scripts/quality-baseline.json vite.config.ts
git commit -m "docs(ui): document EmptyState + Spinner; update baselines"
```

---

## Self-Review

**1. Spec coverage:**
- §Design.1 EmptyState primitive → Task 1; rollout → Tasks 2-3 (all six sites: ActionPanel ×2, Tasks ×2, Recordings, Transcriptions, ImportVaultPicker, Search ×2). ✔
- §Design.2 cross-view fade → Task 5 (guarded/separable, matching the spec's guard). ✔
- §Design.3 micro-interactions → Task 6 (separable/last/tunable, matching the spec). ✔
- §Design.4 Spinner → Task 4 (opportunistic 1:1 only). ✔
- Testing (per-primitive + regression net) → each primitive task + the baseline/gate steps. ✔
- Docs + baselines → Task 7. ✔

**2. Placeholder scan:** Tasks 3/4 say "read first, apply the pattern to the site you find" for the Search/spinner swaps rather than reproducing unread surrounding markup — deliberate, with the exact conditions/text/icons given and the existing tests as the gate, not a "TBD". Every primitive task has complete code. No `TODO`/"handle edge cases".

**3. Type consistency:** `EmptyState{title, hint}` + `icon`/`action` slots and `Spinner{size, label}` are used consistently across Tasks 1-4. The `.text()`-preservation rule (verbatim title, no hint during rollout) is stated in Global Constraints and applied at every swap. The interpolated-title expressions (ActionPanel filter, Tasks filter, Search query) are the one risk — every rollout step says to verify `.text()` matches the original byte-for-byte and to fix the expression (never the test) if it doesn't.

**Risk carried into execution:** the interpolated bound-title expressions must reproduce the old message strings exactly (quotes included). If an implementer can't get `.text()` byte-identical for a filter/query-empty state, the fallback is to leave that one state as its original `<p>` (token-swapped `text-slate-400`→`text-fg-muted`) and note it — the primary (genuinely-empty) states are the core win and are plain static strings with no interpolation risk.
