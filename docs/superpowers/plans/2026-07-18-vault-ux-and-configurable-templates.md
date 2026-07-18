# Vault UX polish + per-vault configurable templates — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an import-picker filter, favorite vaults (dedicated top
section), flip date-folder defaults off, move the Task-ID settings card
second, and give Tasks/Documents/Recording-Notes per-vault additive
templates (extra frontmatter + body content).

**Architecture:** All note/task/document rendering stays in the pure `core`
crate. Templates are **additive**: the app always emits managed identity
frontmatter + structural body (embeds, pandoc content); a per-vault
*extra-frontmatter* string and *body-template* string, with `{{placeholder}}`
substitution, are composed around it via one shared `core::template` helper.
Each template pair mirrors an existing per-vault field's exact plumbing so
the config-merge (GAP-60) discipline is preserved. Favorites are frontend
`localStorage`.

**Tech Stack:** Rust (Tauri v2 shell + pure `core` crate, `cargo test`),
Vue 3 + Pinia + TS (Vitest + happy-dom + `mockIPC`).

## Global Constraints

- Rendering logic lives in the pure `core` crate (testable on Linux). No new Tauri deps.
- Never-clobber/atomic writers are unchanged: `write_note_collision_safe`, `write_note_atomic`, `write_atomic_replacing`.
- **Empty templates reproduce byte-identical current output** — existing renderer tests must keep passing unchanged.
- Managed/identity frontmatter (`type:`…) + embeds (`![[…]]`, transcript, pandoc `{{content}}`) are always emitted and never user-removable.
- Extra frontmatter: substituted, then sanitized — a `---`/`...` line can never break the fence; a line whose key collides with a managed/reserved key is dropped.
- Per-vault config: per-field defensive parse; `Option<String>` blank→`None` (the `transcription_vocabulary` pattern); serialize omit-when-`None`; every new field added to the `config_merge.rs` preserve-lists it isn't owned by (GAP-60).
- Free-text config fields go in each frontend surface's `TEXT_KEYS` debounce set.
- DTO field names are camelCase across Rust↔TS.
- Quality gates before each commit: `cd src-tauri && cargo fmt --check` and (for touched crates) `cargo clippy --all-targets -- -D warnings` + `cargo test`; `npm test` for frontend. Conventional Commits (`feat(ui)`, `feat(core)`, `fix(shell)`, `docs`…).
- Keep `AGENTS.md` current (IPC table, config sections, domain invariants) when the surface changes.

---

## File structure

**Phase 1 — small UX/defaults**
- Modify: `src-tauri/core/src/vault_config.rs` — flip date-folder defaults (3 spots) + test.
- Modify: `src/components/TasksConfigTab.vue` — reorder cards.
- Modify: `src/components/ImportVaultPicker.vue` — local filter.
- Create: `src/utils/favoriteVaults.ts` — localStorage favorites util.
- Create: `tests/favorite-vaults.test.ts`.
- Modify: `src/stores/vaults.ts` — `favorites` set + `toggleFavorite`.
- Modify: `src/components/VaultList.vue` — Favorites group + star toggle + open dot.
- Modify: `src/components/ActionPanel.vue` — pass `favorites` prop down; favorites-first order feeding the picker.
- Modify/extend tests: `tests/import-vault-picker.test.ts`, `tests/vault-list.test.ts`.

**Phase 2 — templates core**
- Create: `src-tauri/core/src/template.rs` — `substitute` + `sanitize_extra_frontmatter`.
- Modify: `src-tauri/core/src/lib.rs` — `pub mod template;`.
- Modify: `src-tauri/core/src/vault_config.rs` — 6 new `Option<String>` fields (struct/Default/parse/serialize) + tests.
- Modify: `src-tauri/core/src/config_merge.rs` — preserve the 4 non-capture template fields; extend `merge_documents_owned` with the 2 document template fields + tests.

**Phase 3 — templates per type (renderer, then wiring/frontend)**
- Modify: `src-tauri/core/src/capture_note.rs` — `NoteMeta` fields + `render_note` composition + tests.
- Modify: `src-tauri/capture/src/session.rs`, `src-tauri/src/capture_commands.rs`, `src-tauri/capture/src/recovery.rs`, `src-tauri/core/src/recordings.rs` — thread note template.
- Modify: `src-tauri/src/capture_config_commands.rs` — `CaptureConfigDto` note fields.
- Modify: `src/components/RecordingSettings.vue`, `src/components/RecordingConfigTab.vue`, `src/components/RecordMode.vue`, `src/types.ts` — note template UI + passthrough.
- Modify: `src-tauri/core/src/tasks/disk.rs` — `render_task`/`create_task` task template + tests.
- Modify: `src-tauri/core/src/services/tasks/mod.rs` — thread task template through `add_task`.
- Modify: `src-tauri/src/task_commands.rs` — `TasksConfigDto` fields + new `set_task_template_config`.
- Modify: `src-tauri/src/lib.rs` — register `set_task_template_config`.
- Create: `src/components/TaskTemplateSettings.vue`; modify `src/components/TasksConfigTab.vue`, `src/types.ts`.
- Modify: `src-tauri/core/src/document_import.rs` — `render_frontmatter`/`publish` document template + tests.
- Modify: `src-tauri/src/document_commands.rs` — `DocumentsConfigDto` fields + `set_documents_config` params; thread template into convert.
- Modify: `src/components/DocumentsConfigTab.vue`, `src/types.ts`.

**Phase 4 — docs**
- Modify: `AGENTS.md` (IPC table + config sections + template invariants); `docs/Gaps.md` if a new gap surfaces.

---

## Task 1: Date-folder defaults → OFF (core)

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs:133-134` (Default), `:323-330` (parse), `:398-403` (serialize), `:787-819` (test)

**Interfaces:**
- Produces: `VaultCaptureConfig::default().recording_date_folders == false` and `.document_date_folders == false`.

- [ ] **Step 1: Update the round-trip test to expect the flipped default**

In `src-tauri/core/src/vault_config.rs`, replace the test named
`date_folder_toggles_default_true_and_round_trip` (around line 787) so it
asserts the new default and inverted serialize behavior:

```rust
#[test]
fn date_folder_toggles_default_false_and_round_trip() {
    let d = VaultCaptureConfig::default();
    assert!(!d.recording_date_folders, "recordings default to flat");
    assert!(!d.document_date_folders, "documents default to flat");

    // Serialize omits when false (the default), writes when true.
    let jf = serde_json::Value::Object(serialize_vault_entry(&d)).to_string();
    assert!(!jf.contains("recordingDateFolders"), "omit at default: {jf}");
    assert!(!jf.contains("documentDateFolders"), "omit at default: {jf}");

    let on = VaultCaptureConfig {
        recording_date_folders: true,
        document_date_folders: true,
        ..VaultCaptureConfig::default()
    };
    let jt = serde_json::Value::Object(serialize_vault_entry(&on)).to_string();
    assert!(jt.contains("\"recordingDateFolders\":true"), "{jt}");
    assert!(jt.contains("\"documentDateFolders\":true"), "{jt}");

    // Absent keys parse back to the new default (false).
    let parsed = vault_entry(&serde_json::json!({}));
    assert!(!parsed.recording_date_folders);
    assert!(!parsed.document_date_folders);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri/core && cargo test date_folder_toggles_default_false -- --nocapture`
Expected: FAIL (default is still `true`; assertions and serialize expectations don't hold).

- [ ] **Step 3: Flip the three coupled spots**

`vault_config.rs` Default impl (~133-134):
```rust
            recording_date_folders: false,
            document_date_folders: false,
```
`vault_entry` parse (~323-330): change both `.unwrap_or(true)` to `.unwrap_or(false)` for `recordingDateFolders` and `documentDateFolders`.
`serialize_vault_entry` (~398-403): invert both guards to write-when-true:
```rust
    if v.recording_date_folders {
        entry.insert("recordingDateFolders".to_string(), json!(true));
    }
    if v.document_date_folders {
        entry.insert("documentDateFolders".to_string(), json!(true));
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri/core && cargo test date_folder && cargo test --lib`
Expected: PASS (new test passes; the existing `config_round_trips_through_serialize_and_parse` and `config_merge` tests still pass — they set values explicitly).

- [ ] **Step 5: Flip the cosmetic frontend seed defaults**

These are overwritten on mount but should not flash the wrong state:
- `src/components/DocumentsConfigTab.vue:19` → `const documentDateFolders = ref(false);`
- `src/components/RecordingConfigTab.vue:43` → `recordingDateFolders: false,`
- `src/components/RecordMode.vue:63` → `recordingDateFolders: false,`
- `tests/documents-config-tab.test.ts:39` → `documentDateFolders: opts.documentDateFolders ?? false`

- [ ] **Step 6: Run gates + commit**

Run: `cd src-tauri && cargo fmt --check && cd core && cargo clippy --all-targets -- -D warnings` then `npm test`
```bash
git add src-tauri/core/src/vault_config.rs src/components/DocumentsConfigTab.vue src/components/RecordingConfigTab.vue src/components/RecordMode.vue tests/documents-config-tab.test.ts
git commit -m "feat(core): default recording/document date folders to off"
```

---

## Task 2: Task-ID card second (frontend)

**Files:**
- Modify: `src/components/TasksConfigTab.vue` (template order, ~120-160)

**Interfaces:** none (pure template reorder).

- [ ] **Step 1: Move the `<TaskIdSettings>` block above `<TaskListSettings>`**

In `src/components/TasksConfigTab.vue`, relocate the entire `<TaskIdSettings ... />`
element (currently after `<TaskListSettings>`/its `v-else` `<p>`) to sit
immediately AFTER the `<VaultFolderSetting … heading="Tasks folder" … />`
block and BEFORE `<TaskListSettings>`. Preserve adjacency:
- `<VaultFolderSetting>` keeps its `v-else` paired with the loadError `<p v-if>`.
- `<TaskListSettings v-if="!pendingFolderChange">` keeps its `v-else` `<p>` adjacent.
Do not change `<script>` (imports already present). Resulting order:
Tasks folder → Task IDs → Task lists.

- [ ] **Step 2: Verify tests still pass (they locate by `data-testid`, not order)**

Run: `npx vitest run tests/tasks-config-tab.test.ts`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/TasksConfigTab.vue
git commit -m "feat(ui): move Task IDs card to second in the Tasks settings tab"
```

---

## Task 3: Import picker vault filter (frontend)

**Files:**
- Modify: `src/components/ImportVaultPicker.vue`
- Test: `tests/import-vault-picker.test.ts`

**Interfaces:**
- Consumes: `store.vaults` (the shared list; favorites-first ordering from Task 5 applies once merged).
- Produces: a filtered `<li v-for>` list in the vault-first/drop list view.

- [ ] **Step 1: Write the failing test**

Add to `tests/import-vault-picker.test.ts` (a picker mounted with >5 vaults;
reuse the file's existing mount helper / store seeding):

```ts
it("filters the vault list by name once above the threshold", async () => {
  const wrapper = await mountPickerWithVaults([
    "Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta",
  ]); // 6 vaults → filter shown
  const input = wrapper.get('[data-testid="import-picker-filter"]');
  await input.setValue("gam");
  const rows = wrapper.findAll('[data-testid="import-picker-vault"]');
  expect(rows).toHaveLength(1);
  expect(rows[0].text()).toContain("Gamma");
});

it("hides the filter when 5 or fewer vaults", async () => {
  const wrapper = await mountPickerWithVaults(["A", "B", "C"]);
  expect(wrapper.find('[data-testid="import-picker-filter"]').exists()).toBe(false);
});
```

If `mountPickerWithVaults` doesn't exist, add a small helper in the test file
that seeds `useVaultsStore().vaults` with `{id, name, path: name, open:false}`
entries and mounts `ImportVaultPicker` (mirroring the existing mount setup).

- [ ] **Step 2: Run to verify it fails**

Run: `npx vitest run tests/import-vault-picker.test.ts`
Expected: FAIL (no `import-picker-filter` element).

- [ ] **Step 3: Add the filter to `ImportVaultPicker.vue`**

In `<script setup>` add (mirroring `ActionPanel.vue`):
```ts
const filter = ref("");
const FILTER_THRESHOLD = 5;
const showFilter = computed(() => store.vaults.length > FILTER_THRESHOLD);
const filteredVaults = computed(() => {
  const query = filter.value.trim().toLowerCase();
  if (!query) return store.vaults;
  return store.vaults.filter(
    (v) => v.name.toLowerCase().includes(query) || v.path.toLowerCase().includes(query),
  );
});
function onFilterEscape(event: KeyboardEvent) {
  if (event.isComposing) return; // GAP-31: IME Escape must not clear
  if (filter.value) {
    filter.value = "";
    event.stopPropagation();
  }
}
// Reset the query whenever the panel is re-shown, so a stale filter can't
// strand the picker (the ActionPanel precedent).
watch(() => store.shownNonce, () => (filter.value = ""));
```
Add `ref, computed, watch` to the `vue` import if missing.

In the template, immediately above the `<ul … viewState === 'list'>`:
```vue
<input
  v-if="showFilter"
  v-model="filter"
  type="search"
  placeholder="Filter vaults…"
  aria-label="Filter vaults"
  data-testid="import-picker-filter"
  class="mb-2 w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-white/20 focus:outline-none"
  @keydown.escape="onFilterEscape"
>
```
Change the list `v-for="vault in store.vaults"` → `v-for="vault in filteredVaults"`.

- [ ] **Step 4: Run to verify it passes**

Run: `npx vitest run tests/import-vault-picker.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/ImportVaultPicker.vue tests/import-vault-picker.test.ts
git commit -m "feat(ui): filter the import-document vault picker like the main list"
```

---

## Task 4: Favorite vaults — util + store

**Files:**
- Create: `src/utils/favoriteVaults.ts`
- Test: `tests/favorite-vaults.test.ts`
- Modify: `src/stores/vaults.ts`

**Interfaces:**
- Produces: `loadFavorites(): string[]`, `isFavorite(id, set): boolean`, `toggleFavorite(id): string[]` (persists, returns the new list) in `favoriteVaults.ts`; store gains reactive `favorites: Set<string>` and action `toggleFavorite(id: string): void`.

- [ ] **Step 1: Write the failing util test**

`tests/favorite-vaults.test.ts`:
```ts
import { beforeEach, describe, expect, it } from "vitest";
import { loadFavorites, toggleFavorite } from "../src/utils/favoriteVaults";

describe("favoriteVaults", () => {
  beforeEach(() => localStorage.clear());

  it("starts empty and toggles on/off, persisting", () => {
    expect(loadFavorites()).toEqual([]);
    expect(toggleFavorite("v1")).toEqual(["v1"]);
    expect(loadFavorites()).toEqual(["v1"]);
    expect(toggleFavorite("v1")).toEqual([]);
    expect(loadFavorites()).toEqual([]);
  });

  it("degrades to empty on corrupt storage", () => {
    localStorage.setItem("vault-buddy:favorite-vaults", "{not json");
    expect(loadFavorites()).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `npx vitest run tests/favorite-vaults.test.ts`
Expected: FAIL (module not found).

- [ ] **Step 3: Implement `src/utils/favoriteVaults.ts`** (modeled on `recentSearches.ts`)

```ts
import { logWarning } from "../logging";

const KEY = "vault-buddy:favorite-vaults";

export function loadFavorites(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((v): v is string => typeof v === "string");
  } catch (err) {
    logWarning("favoriteVaults: failed to load", err);
    return [];
  }
}

function save(ids: string[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(ids));
  } catch (err) {
    logWarning("favoriteVaults: failed to save", err);
  }
}

/** Toggle a vault's favorite state; returns the new list and persists it. */
export function toggleFavorite(id: string): string[] {
  const ids = loadFavorites();
  const next = ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
  save(next);
  return next;
}
```
(Confirm the `logWarning` import path matches `recentSearches.ts`; adjust if the helper name differs.)

- [ ] **Step 4: Run to verify it passes**

Run: `npx vitest run tests/favorite-vaults.test.ts`
Expected: PASS.

- [ ] **Step 5: Wire the store**

In `src/stores/vaults.ts`, add to `state`:
```ts
    favorites: new Set<string>(loadFavorites()),
```
(import `loadFavorites, toggleFavorite` at top). Add an action:
```ts
    toggleFavorite(id: string) {
      const next = toggleFavorite(id);
      this.favorites = new Set(next);
    },
```
Rename the imported util if it clashes with the action name, e.g.
`import { loadFavorites, toggleFavorite as persistFavorite } from "../utils/favoriteVaults";`
and call `persistFavorite(id)` inside the action.

- [ ] **Step 6: Run the store's tests + commit**

Run: `npx vitest run tests/favorite-vaults.test.ts tests/vaults-store.test.ts` (if the latter exists; otherwise `npm test`)
Expected: PASS.
```bash
git add src/utils/favoriteVaults.ts tests/favorite-vaults.test.ts src/stores/vaults.ts
git commit -m "feat(ui): persist favorite vaults in localStorage via the vaults store"
```

---

## Task 5: Favorites — VaultList group + star toggle + picker ordering

**Files:**
- Modify: `src/components/VaultList.vue` (groups computed + row star + open dot)
- Modify: `src/components/ActionPanel.vue` (pass `:favorites` prop; keep `filtered` feeding VaultList)
- Test: `tests/vault-list.test.ts`

**Interfaces:**
- Consumes: `store.favorites: Set<string>`, `store.toggleFavorite(id)`.
- Produces: a `★ Favorites` group rendered above `Open now`/`Other vaults`; a per-row star button `data-testid="vault-favorite-<id>"`.

- [ ] **Step 1: Write the failing test**

In `tests/vault-list.test.ts`, add (adapt to the file's mount helper + how it
injects the store; VaultList reads favorites from the store, so seed
`useVaultsStore().favorites`):

```ts
it("pins favorites into a Favorites group above the others", async () => {
  const wrapper = mountList({
    vaults: [
      { id: "a", name: "Apple", path: "Apple", open: true },
      { id: "b", name: "Box", path: "Box", open: false },
      { id: "c", name: "Cat", path: "Cat", open: false },
    ],
    favorites: ["c"],
  });
  const headers = wrapper.findAll("h3").map((h) => h.text());
  expect(headers[0]).toContain("Favorites");
  // The favorite appears once, in the Favorites group.
  const favSection = wrapper.get('[data-section="favorites"]');
  expect(favSection.text()).toContain("Cat");
  expect(wrapper.findAll('[data-section="favorites"] li')).toHaveLength(1);
});

it("toggles a favorite via the row star", async () => {
  const store = useVaultsStore();
  const spy = vi.spyOn(store, "toggleFavorite");
  const wrapper = mountList({ vaults: [{ id: "a", name: "Apple", path: "Apple", open: false }], favorites: [] });
  await wrapper.get('[data-testid="vault-favorite-a"]').trigger("click");
  expect(spy).toHaveBeenCalledWith("a");
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `npx vitest run tests/vault-list.test.ts`
Expected: FAIL (no Favorites header / no star button).

- [ ] **Step 3: Update the `groups` computed in `VaultList.vue`**

Read `store.favorites` (import `useVaultsStore` if the component doesn't
already; VaultList currently receives `vaults` as a prop — keep that, and read
favorites from the store). Replace the `groups` computed:

```ts
const store = useVaultsStore();
const isFav = (id: string) => store.favorites.has(id);

// Favorites are pinned above Open now / Other vaults. A favorite appears once
// (in Favorites), regardless of open state; its row shows an "open" dot when
// it is currently open in Obsidian. Alphabetical order (from discovery) is
// preserved within each group.
const groups = computed(() => {
  const favs = props.vaults.filter((v) => isFav(v.id));
  const rest = props.vaults.filter((v) => !isFav(v.id));
  const open = rest.filter((v) => v.open);
  const other = rest.filter((v) => !v.open);
  const out: { key: string; section: string; label: string | null; vaults: Vault[] }[] = [];
  if (favs.length) out.push({ key: "fav", section: "favorites", label: "Favorites", vaults: favs });
  if (open.length) out.push({ key: "open", section: "open", label: "Open now", vaults: open });
  if (other.length) {
    // With favorites or open present, the remainder gets an "Other vaults"
    // header; with nothing pinned/open it stays a flat, header-less list.
    const flat = !favs.length && !open.length;
    out.push({ key: "rest", section: "other", label: flat ? null : "Other vaults", vaults: other });
  }
  return out;
});
```
Import `Vault` type if not already. In the template's group `v-for`, add
`:data-section="group.section"` to the group wrapper and gate the header
`v-if="group.label"`.

- [ ] **Step 4: Add the star toggle + open dot to the row**

In the per-row button cluster add a star button:
```vue
<button
  type="button"
  :data-testid="`vault-favorite-${vault.id}`"
  :aria-pressed="store.favorites.has(vault.id)"
  :aria-label="store.favorites.has(vault.id) ? 'Unfavorite' : 'Favorite'"
  class="shrink-0 rounded p-1 text-slate-400 hover:text-amber-300"
  @click.stop="store.toggleFavorite(vault.id)"
>
  <span aria-hidden="true">{{ store.favorites.has(vault.id) ? "★" : "☆" }}</span>
</button>
```
For the open dot, in the Favorites group only, render a small dot when
`group.section === 'favorites' && vault.open` beside the vault name (a
`<span class="…" title="Open in Obsidian">●</span>`), so the "open" signal
isn't lost for a favorited-and-open vault.

- [ ] **Step 5: Pass favorites so the picker orders them first**

Favorites live in the store, so `ImportVaultPicker` (Task 3) already reads the
same store. To make the picker's flat list favorites-first, sort
`filteredVaults` there with favorites ahead:
```ts
const store = useVaultsStore();
// in filteredVaults, before returning, stable-sort favorites first:
const ordered = (list: Vault[]) =>
  [...list].sort((a, b) => Number(store.favorites.has(b.id)) - Number(store.favorites.has(a.id)));
```
Apply `ordered(...)` to both the unfiltered and filtered return of
`filteredVaults`. (Alphabetical order is already the store's order; the sort
is stable so it only lifts favorites.)

- [ ] **Step 6: Run tests to verify pass**

Run: `npx vitest run tests/vault-list.test.ts tests/import-vault-picker.test.ts`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/components/VaultList.vue src/components/ImportVaultPicker.vue src/components/ActionPanel.vue tests/vault-list.test.ts
git commit -m "feat(ui): pin favorite vaults into a top section with a row star"
```

---

## Task 6: `core::template` helper (substitute + sanitize)

**Files:**
- Create: `src-tauri/core/src/template.rs`
- Modify: `src-tauri/core/src/lib.rs` (`pub mod template;`)

**Interfaces:**
- Produces:
  - `template::substitute(template: &str, vars: &[(&str, &str)]) -> String`
  - `template::sanitize_extra_frontmatter(text: &str, reserved: &[&str]) -> String`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/core/src/template.rs`:
```rust
//! Additive-template primitives shared by the note/task/document renderers.
//! `substitute` fills `{{token}}` placeholders; `sanitize_extra_frontmatter`
//! makes a user's extra-frontmatter text safe to inject before a closing
//! `---` — it can never break the fence or redefine a managed key.

/// Replace every `{{key}}` (whitespace inside the braces tolerated) with its
/// value from `vars`. An unknown key renders empty (the available keys are
/// documented in the UI). Unclosed `{{` is emitted literally. UTF-8 safe.
pub fn substitute(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let key = after[..end].trim();
            if let Some((_, val)) = vars.iter().find(|(k, _)| *k == key) {
                out.push_str(val);
            }
            rest = &after[end + 2..];
        } else {
            out.push_str("{{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Return the lines of `text` safe to inject into a frontmatter block:
/// - a `---`/`...` line (a fence) is dropped, and so is any indented block
///   under it — user frontmatter can never break out of the block;
/// - a top-level line whose key (before the first `:`) is in `reserved`
///   (case-insensitive) is dropped along with its indented continuation
///   lines, so a managed key can't be redefined;
/// - blank lines are dropped.
/// Everything else is kept verbatim, newline-terminated.
pub fn sanitize_extra_frontmatter(text: &str, reserved: &[&str]) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            skipping = false;
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if !skipping {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        if trimmed == "---" || trimmed == "..." {
            skipping = true;
            continue;
        }
        let key = line.split(':').next().unwrap_or("").trim();
        if reserved.iter().any(|r| r.eq_ignore_ascii_case(key)) {
            skipping = true;
            continue;
        }
        skipping = false;
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_fills_known_and_empties_unknown() {
        let vars = [("title", "Buy milk"), ("date", "2026-07-18")];
        assert_eq!(substitute("# {{title}} ({{date}})", &vars), "# Buy milk (2026-07-18)");
        assert_eq!(substitute("{{ title }}", &vars), "Buy milk"); // whitespace tolerated
        assert_eq!(substitute("x {{nope}} y", &vars), "x  y"); // unknown → empty
    }

    #[test]
    fn substitute_is_utf8_safe_and_tolerates_unclosed() {
        assert_eq!(substitute("café {{x}}", &[]), "café ");
        assert_eq!(substitute("a {{ open", &[]), "a {{ open");
    }

    #[test]
    fn sanitize_drops_fences_and_reserved_keys() {
        let text = "project: Alpha\ntype: Evil\n---\nowner: me\ntags:\n  - x\n  - y\nnote: keep";
        let reserved = ["type", "tags"];
        let out = sanitize_extra_frontmatter(text, &reserved);
        assert!(out.contains("project: Alpha"));
        assert!(out.contains("note: keep"));
        assert!(!out.contains("type: Evil"), "reserved key dropped: {out}");
        assert!(!out.contains("---"), "fence dropped: {out}");
        assert!(!out.contains("- x"), "reserved block items dropped: {out}");
        // `owner: me` sits after a fence line → the fence starts a skip block,
        // but a following TOP-LEVEL key resets it and is kept.
        assert!(out.contains("owner: me"), "{out}");
    }

    #[test]
    fn sanitize_empty_in_empty_out() {
        assert_eq!(sanitize_extra_frontmatter("", &["type"]), "");
        assert_eq!(sanitize_extra_frontmatter("\n\n", &["type"]), "");
    }
}
```

- [ ] **Step 2: Register the module + run to verify it fails then passes**

Add `pub mod template;` to `src-tauri/core/src/lib.rs` (with the other `pub mod` lines).
Run: `cd src-tauri/core && cargo test template::`
Expected: PASS (the module + tests compile and pass).

- [ ] **Step 3: Gates + commit**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
```bash
git add src-tauri/core/src/template.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): add template substitute + extra-frontmatter sanitizer"
```

---

## Task 7: Config — 6 template fields + parse/serialize + merge

**Files:**
- Modify: `src-tauri/core/src/vault_config.rs` (struct/Default/vault_entry/serialize_vault_entry + tests)
- Modify: `src-tauri/core/src/config_merge.rs` (preserve 4 fields; extend `merge_documents_owned` + tests)

**Interfaces:**
- Produces on `VaultCaptureConfig`: `note_extra_frontmatter`, `note_body_template`, `task_extra_frontmatter`, `task_body_template`, `document_extra_frontmatter`, `document_body_template` — all `Option<String>`, default `None`.
- `merge_documents_owned(existing, documents_folder, document_date_folders, document_extract_images, document_extra_frontmatter, document_body_template)`.

- [ ] **Step 1: Write the failing round-trip test**

Add to `vault_config.rs` tests:
```rust
#[test]
fn template_fields_default_none_and_round_trip() {
    let d = VaultCaptureConfig::default();
    assert_eq!(d.note_body_template, None);
    assert_eq!(d.task_extra_frontmatter, None);
    assert_eq!(d.document_body_template, None);
    // Omitted at default (keeps config.json minimal).
    let j = serde_json::Value::Object(serialize_vault_entry(&d)).to_string();
    assert!(!j.contains("noteBodyTemplate"), "{j}");

    let set = VaultCaptureConfig {
        note_extra_frontmatter: Some("attendees:".into()),
        note_body_template: Some("## Notes\n{{type}}".into()),
        task_extra_frontmatter: Some("project: Alpha".into()),
        task_body_template: Some("- [ ] {{title}}".into()),
        document_extra_frontmatter: Some("area: Legal".into()),
        document_body_template: Some("> imported\n\n{{content}}".into()),
        ..VaultCaptureConfig::default()
    };
    let entry = serde_json::Value::Object(serialize_vault_entry(&set));
    let back = vault_entry(&entry);
    assert_eq!(back.note_body_template.as_deref(), Some("## Notes\n{{type}}"));
    assert_eq!(back.task_extra_frontmatter.as_deref(), Some("project: Alpha"));
    assert_eq!(back.document_body_template.as_deref(), Some("> imported\n\n{{content}}"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri/core && cargo test template_fields_default_none`
Expected: FAIL (fields don't exist).

- [ ] **Step 3: Add the fields to the struct + Default**

In `VaultCaptureConfig` (after `document_extract_images`):
```rust
    /// Additive per-vault templates. Extra-frontmatter is injected after the
    /// managed identity keys (reserved keys dropped, fence-safe); body-template
    /// composes the body with `{{placeholders}}`. None → today's exact output.
    pub note_extra_frontmatter: Option<String>,
    pub note_body_template: Option<String>,
    pub task_extra_frontmatter: Option<String>,
    pub task_body_template: Option<String>,
    pub document_extra_frontmatter: Option<String>,
    pub document_body_template: Option<String>,
```
In `impl Default` (after `document_extract_images: true,`):
```rust
            note_extra_frontmatter: None,
            note_body_template: None,
            task_extra_frontmatter: None,
            task_body_template: None,
            document_extra_frontmatter: None,
            document_body_template: None,
```

- [ ] **Step 4: Add per-field defensive parse (blank→None) in `vault_entry`**

After the `document_extract_images` parse block, add (repeat the
`transcription_vocabulary` pattern for each):
```rust
        note_extra_frontmatter: template_field(entry, "noteExtraFrontmatter"),
        note_body_template: template_field(entry, "noteBodyTemplate"),
        task_extra_frontmatter: template_field(entry, "taskExtraFrontmatter"),
        task_body_template: template_field(entry, "taskBodyTemplate"),
        document_extra_frontmatter: template_field(entry, "documentExtraFrontmatter"),
        document_body_template: template_field(entry, "documentBodyTemplate"),
```
And add a small helper near `parse_string_list`:
```rust
/// Read an optional free-text field: trimmed, blank → None (the
/// `transcriptionVocabulary` treatment) — but preserve interior whitespace
/// (templates are multi-line, so only the ends are trimmed).
fn template_field(entry: &serde_json::Value, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}
```

- [ ] **Step 5: Add omit-when-None serialize in `serialize_vault_entry`**

After the `documentExtractImages` block:
```rust
    if let Some(t) = &v.note_extra_frontmatter {
        entry.insert("noteExtraFrontmatter".to_string(), json!(t));
    }
    if let Some(t) = &v.note_body_template {
        entry.insert("noteBodyTemplate".to_string(), json!(t));
    }
    if let Some(t) = &v.task_extra_frontmatter {
        entry.insert("taskExtraFrontmatter".to_string(), json!(t));
    }
    if let Some(t) = &v.task_body_template {
        entry.insert("taskBodyTemplate".to_string(), json!(t));
    }
    if let Some(t) = &v.document_extra_frontmatter {
        entry.insert("documentExtraFrontmatter".to_string(), json!(t));
    }
    if let Some(t) = &v.document_body_template {
        entry.insert("documentBodyTemplate".to_string(), json!(t));
    }
```

- [ ] **Step 6: Run the config test to verify it passes**

Run: `cd src-tauri/core && cargo test template_fields_default_none && cargo test --lib`
Expected: PASS. (The `config_round_trips_through_serialize_and_parse` test that inserts a `::default()` vault still round-trips.)

- [ ] **Step 7: Update `config_merge.rs` — write a failing preserve test first**

Extend the existing `merge_capture_owned_writes_owned_and_preserves_the_rest`
test: set the 4 non-capture template fields on `existing` and assert they
survive:
```rust
        // on `existing`, before merge:
        task_extra_frontmatter: Some("project: A".into()),
        task_body_template: Some("- [ ] {{title}}".into()),
        document_extra_frontmatter: Some("area: X".into()),
        document_body_template: Some("{{content}}".into()),
        // after merge, assert preserved:
        assert_eq!(merged.task_body_template.as_deref(), Some("- [ ] {{title}}"));
        assert_eq!(merged.document_extra_frontmatter.as_deref(), Some("area: X"));
```
And extend `merge_documents_owned_writes_owned_and_preserves_the_rest` to pass
the two new args and assert they land:
```rust
        let merged = merge_documents_owned(
            &existing, Some("Docs".into()), false, false,
            Some("area: Legal".into()), Some("{{content}}".into()),
        );
        assert_eq!(merged.document_body_template.as_deref(), Some("{{content}}"));
```

Run: `cd src-tauri/core && cargo test merge_` → FAIL (signature + preserve mismatch).

- [ ] **Step 8: Implement the merge changes**

`merge_capture_owned` — add to the explicit preserve list (the 4 non-capture
template fields; note fields stay in `..incoming` because capture owns them):
```rust
        task_extra_frontmatter: existing.task_extra_frontmatter.clone(),
        task_body_template: existing.task_body_template.clone(),
        document_extra_frontmatter: existing.document_extra_frontmatter.clone(),
        document_body_template: existing.document_body_template.clone(),
```
`merge_documents_owned` — extend the signature + body:
```rust
pub fn merge_documents_owned(
    existing: &VaultCaptureConfig,
    documents_folder: Option<String>,
    document_date_folders: bool,
    document_extract_images: bool,
    document_extra_frontmatter: Option<String>,
    document_body_template: Option<String>,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        documents_folder,
        document_date_folders,
        document_extract_images,
        document_extra_frontmatter,
        document_body_template,
        ..existing.clone()
    }
}
```
Update the doc comment's owned-field list accordingly.

- [ ] **Step 9: Run + gates + commit**

Run: `cd src-tauri/core && cargo test --lib && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
Expected: PASS. (The shell caller `set_documents_config` won't compile yet — it's updated in Task 12; if running workspace-wide clippy, expect that one call-site error until then. Commit the core crate now.)
```bash
git add src-tauri/core/src/vault_config.rs src-tauri/core/src/config_merge.rs
git commit -m "feat(core): add per-vault template config fields + merge preservation"
```

---

## Task 8: Note renderer — additive templates (core)

**Files:**
- Modify: `src-tauri/core/src/capture_note.rs` (`NoteMeta` + `render_note` + tests)

**Interfaces:**
- Consumes: `template::substitute`, `template::sanitize_extra_frontmatter`.
- Produces: `NoteMeta` gains `extra_frontmatter: Option<String>` and `body_template: Option<String>`; `render_note` composition changes.

- [ ] **Step 1: Write failing tests (defaults byte-identical; templates apply)**

Add to `capture_note.rs` tests (and extend `fn meta()` to set the two new
fields to `None`):
```rust
#[test]
fn note_default_output_is_byte_identical_with_empty_templates() {
    // A note with follow-up + transcript, no templates, must equal the exact
    // legacy string (regression guard for the additive refactor).
    let mut m = meta();
    m.follow_up = true;
    m.transcribe = true;
    let note = render_note(&m, "R.mp3");
    let expected = "---\nrecorded: \"2026-07-04T14:05:00+02:00\"\nduration: \"1:02:03\"\nvault: \"Work\"\ntype: \"Meeting\"\ninputs:\n  - \"Headset Mic\"\n  - \"Speakers (loopback)\"\ncreated-by: Vault Buddy\n---\n\n![[R.mp3]]\n\n## Follow-up\n\n### Action items\n\n- [ ] \n\n### Decisions\n\n### Notes\n\n## Transcript\n\n![[R.transcript]]\n";
    assert_eq!(note, expected);
}

#[test]
fn note_extra_frontmatter_injected_and_reserved_dropped() {
    let mut m = meta();
    m.extra_frontmatter = Some("attendees: 3\ntype: HIJACK".into());
    let note = render_note(&m, "R.mp3");
    assert!(note.contains("attendees: 3"));
    assert!(!note.contains("type: HIJACK"), "reserved key dropped: {note}");
    // Managed type survives and the fence isn't broken.
    assert!(note.contains("type: \"Meeting\""));
}

#[test]
fn note_body_template_replaces_the_scaffold_between_the_embeds() {
    let mut m = meta();
    m.follow_up = true; // would normally add the scaffold
    m.transcribe = true;
    m.body_template = Some("## Summary\n{{type}} in {{vault}}".into());
    let note = render_note(&m, "R.mp3");
    assert!(note.contains("## Summary\nMeeting in Work"));
    assert!(!note.contains("## Follow-up"), "template replaces scaffold: {note}");
    // Embeds still bracket the body.
    let audio = note.find("![[R.mp3]]").unwrap();
    let body = note.find("## Summary").unwrap();
    let tr = note.find("## Transcript").unwrap();
    assert!(audio < body && body < tr, "{note}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri/core && cargo test note_ -- --nocapture`
Expected: FAIL (new `NoteMeta` fields don't exist; body-template behavior absent).

- [ ] **Step 3: Add the fields + rewrite `render_note` composition**

Add to `NoteMeta` (after `follow_up`):
```rust
    /// Additive template content (per-vault). None → today's exact output.
    pub extra_frontmatter: Option<String>,
    pub body_template: Option<String>,
```
Rewrite the tail of `render_note` (from the `created-by` line onward):
```rust
    out.push_str("created-by: Vault Buddy\n");
    // Extra frontmatter: substituted then sanitized (reserved keys dropped,
    // fence-safe) so a user field can never break the block or shadow a
    // managed key. Injected right before the closing fence.
    let date = meta.recorded_at.split(['T', ' ']).next().unwrap_or("");
    let vars = [
        ("recordedAt", meta.recorded_at.as_str()),
        ("duration", duration.as_str()),
        ("vault", meta.vault_name.as_str()),
        ("type", meta.recording_type.as_str()),
        ("date", date),
    ];
    if let Some(extra) = &meta.extra_frontmatter {
        const NOTE_RESERVED: &[&str] =
            &["recorded", "duration", "paused", "vault", "type", "inputs", "event", "created-by"];
        out.push_str(&crate::template::sanitize_extra_frontmatter(
            &crate::template::substitute(extra, &vars),
            NOTE_RESERVED,
        ));
    }
    out.push_str("---\n\n");
    out.push_str(&format!("![[{mp3_file_name}]]\n"));
    // Body: a non-empty body template replaces the scaffold; otherwise the
    // legacy follow-up scaffold renders when opted in.
    match meta.body_template.as_deref().map(str::trim) {
        Some(body) if !body.is_empty() => {
            out.push('\n');
            let rendered = crate::template::substitute(body, &vars);
            out.push_str(&rendered);
            if !rendered.ends_with('\n') {
                out.push('\n');
            }
        }
        _ if meta.follow_up => {
            out.push_str(
                "\n## Follow-up\n\n### Action items\n\n- [ ] \n\n### Decisions\n\n### Notes\n",
            );
        }
        _ => {}
    }
    if meta.transcribe {
        let stem = mp3_file_name.strip_suffix(".mp3").unwrap_or(mp3_file_name);
        out.push_str(&format!("\n## Transcript\n\n![[{stem}.transcript]]\n"));
    }
    out
```
Add `let duration = format_duration(meta.duration_secs);` near the top and use
it both for the `duration:` line and the `vars` array (so the format string
uses the local binding).

- [ ] **Step 4: Fix the `meta()` test helper + run**

In the `meta()` helper add `extra_frontmatter: None, body_template: None,`.
Run: `cd src-tauri/core && cargo test --lib`
Expected: PASS (new tests + all existing note tests, including the byte-identical one).

- [ ] **Step 5: Gates + commit**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
```bash
git add src-tauri/core/src/capture_note.rs
git commit -m "feat(core): additive templates for the recording companion note"
```

---

## Task 9: Note template wiring (shell threading + IPC + frontend)

**Files:**
- Modify: `src-tauri/capture/src/session.rs` (`SessionParams` fields + `NoteMeta` construction + the internal default), `src-tauri/src/capture_commands.rs` (build from cfg), `src-tauri/capture/src/recovery.rs`, `src-tauri/core/src/recordings.rs` (None at their NoteMeta sites)
- Modify: `src-tauri/src/capture_config_commands.rs` (`CaptureConfigDto` + get/set)
- Modify: `src/components/RecordingSettings.vue`, `src/components/RecordingConfigTab.vue`, `src/components/RecordMode.vue`, `src/types.ts`
- Test: `tests/recording-settings.test.ts` (or the config-tab test)

**Interfaces:**
- Consumes: `VaultCaptureConfig.note_extra_frontmatter/.note_body_template`.
- Produces: `CaptureConfigDto.note_extra_frontmatter/.note_body_template`; the note template reaches `render_note` on capture.

- [ ] **Step 1: Thread the fields through the capture crate**

`capture/src/session.rs`: add to `SessionParams` (near `follow_up`):
```rust
    pub note_extra_frontmatter: Option<String>,
    pub note_body_template: Option<String>,
```
In the `NoteMeta { … }` construction (~line 499) add:
```rust
                extra_frontmatter: params.note_extra_frontmatter.clone(),
                body_template: params.note_body_template.clone(),
```
At the internal `SessionParams`/`NoteMeta` default site (~line 579, the
recovery/test default) set both to `None`.

`capture/src/recovery.rs` (~line 135) and `core/src/recordings.rs` (~line 113):
wherever a `NoteMeta` is built with `follow_up: false`, add
`extra_frontmatter: None, body_template: None,` (recovery/list paths never use
templates).

- [ ] **Step 2: Build `SessionParams` from the vault config in the shell**

`src-tauri/src/capture_commands.rs` (~line 348, beside `follow_up: cfg.follow_up_template,`):
```rust
                note_extra_frontmatter: cfg.note_extra_frontmatter.clone(),
                note_body_template: cfg.note_body_template.clone(),
```

- [ ] **Step 3: Extend `CaptureConfigDto` (get + set)**

`src-tauri/src/capture_config_commands.rs`:
- Struct (after `follow_up_template`): `pub note_extra_frontmatter: Option<String>,` and `pub note_body_template: Option<String>,`
- `from_config` (after `follow_up_template: v.follow_up_template,`): `note_extra_frontmatter: v.note_extra_frontmatter.clone(), note_body_template: v.note_body_template.clone(),`
- In `set_capture_config`'s `incoming` (after `follow_up_template: cfg.follow_up_template,`), with blank→None like `transcription_vocabulary`:
```rust
        note_extra_frontmatter: cfg.note_extra_frontmatter.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
        note_body_template: cfg.note_body_template.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
```
(These are capture-owned, so they flow via `incoming` — no `merge_capture_owned` change.)

- [ ] **Step 4: Compile the Rust workspace (compile gate)**

Run: `cd src-tauri && cargo test -p vault_buddy_core -p vault_buddy_capture --lib` and `cargo clippy -p vault-buddy --all-targets -- -D warnings` (shell). If GUI libs are needed, run `npm run setup:linux` once then `npx tauri build --no-bundle`.
Expected: builds; capture/core tests pass.

- [ ] **Step 5: Frontend types + form models + UI**

`src/types.ts`: add `noteExtraFrontmatter?: string | null; noteBodyTemplate?: string | null;` to the `CaptureConfig` interface and to `RecordingSettingsValue`.

`src/components/RecordingConfigTab.vue`: in the on-mount DTO→form map add
`noteExtraFrontmatter: cfg.noteExtraFrontmatter ?? "", noteBodyTemplate: cfg.noteBodyTemplate ?? "",`; in the autosave form→DTO map add
`noteExtraFrontmatter: r.noteExtraFrontmatter, noteBodyTemplate: r.noteBodyTemplate,`; add both keys to `TEXT_KEYS` (debounced).

`src/components/RecordingSettings.vue`: under the Companion note section (shown
when `createNote`), add two `<textarea>`s bound via `patch()` computed proxies
(mirror the `followUpTemplate` proxy), `data-testid="note-extra-frontmatter"`
and `data-testid="note-body-template"`, with helper text: "Placeholders:
`{{date}}`, `{{recordedAt}}`, `{{duration}}`, `{{vault}}`, `{{type}}`. Identity
fields and the audio/transcript embeds are always added."

`src/components/RecordMode.vue`: add `noteExtraFrontmatter`/`noteBodyTemplate`
to the `rec` form model (load + save passthrough) and to its `TEXT_KEYS`, WITHOUT
rendering editors — a quick-record save must round-trip them so it can't wipe a
note template (the `followUpTemplate` precedent in this file).

- [ ] **Step 6: Frontend test + run**

Add a test to `tests/recording-config-tab.test.ts` (or `recording-settings.test.ts`):
mounting with a `noteBodyTemplate` from `get_capture_config` shows it in the
textarea, and editing it calls `set_capture_config` with the new value
(debounced — advance timers as the existing `transcriptionVocabulary` test does).
Run: `npx vitest run tests/recording-config-tab.test.ts tests/record-mode.test.ts`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/capture/src/session.rs src-tauri/capture/src/recovery.rs src-tauri/core/src/recordings.rs src-tauri/src/capture_commands.rs src-tauri/src/capture_config_commands.rs src/components/RecordingSettings.vue src/components/RecordingConfigTab.vue src/components/RecordMode.vue src/types.ts tests/
git commit -m "feat: configurable recording-note template (frontmatter + body)"
```

---

## Task 10: Task renderer — additive templates (core)

**Files:**
- Modify: `src-tauri/core/src/tasks/disk.rs` (`render_task` + `create_task` signatures + tests)

**Interfaces:**
- Produces: `render_task(title, created, due, priority, tags, task_id, extra_frontmatter: Option<&str>, body_template: Option<&str>)` and `create_task(root, title, today, due, priority, tags, task_id, extra_frontmatter, body_template)`.

- [ ] **Step 1: Write failing tests**

Add to `tasks/disk.rs` tests:
```rust
#[test]
fn task_default_output_is_byte_identical_with_no_template() {
    let out = render_task("Buy milk", "2026-07-08", None, None, &[], None, None, None);
    assert_eq!(out, "---\ntype: Task\nstatus: new\ntitle: \"Buy milk\"\ncreated: 2026-07-08\n---\n\n");
}

#[test]
fn task_extra_frontmatter_and_body_apply_and_reserved_dropped() {
    let out = render_task(
        "Buy milk", "2026-07-08", None, None, &[], None,
        Some("project: Alpha\nstatus: HIJACK"), Some("- [ ] {{title}} by {{date}}"),
    );
    assert!(out.contains("project: Alpha"));
    assert!(!out.contains("status: HIJACK"), "reserved dropped: {out}");
    assert!(out.contains("status: new"), "managed status intact");
    // Body after the fence, placeholders filled.
    assert!(out.ends_with("- [ ] Buy milk by 2026-07-08\n"), "{out}");
    // Still a valid task (closed fence + type: Task).
    assert!(out.contains("---\ntype: Task\n"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri/core && cargo test task_ -- --nocapture`
Expected: FAIL (arity mismatch).

- [ ] **Step 3: Extend `render_task` + `create_task`**

`render_task` — add two params and compose (reserved set includes the id
property when present):
```rust
pub fn render_task(
    title: &str,
    created: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    task_id: Option<(&str, &str)>,
    extra_frontmatter: Option<&str>,
    body_template: Option<&str>,
) -> String {
    let mut extra = String::new();
    if let Some((prop, id)) = task_id {
        extra.push_str(&format!("{prop}: {id}\n"));
    }
    if let Some(d) = due {
        extra.push_str(&format!("due: {d}\n"));
    }
    if let Some(p) = priority {
        extra.push_str(&format!("priority: {p}\n"));
    }
    if !tags.is_empty() {
        extra.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    // User extra frontmatter: substituted, then sanitized against the reserved
    // task keys (+ the id property) so the surgical field writer is never
    // confused. Injected before the closing fence.
    if let Some(ef) = extra_frontmatter {
        let vars = [
            ("title", title),
            ("date", created),
            ("due", due.unwrap_or("")),
            ("priority", priority.unwrap_or("")),
        ];
        let mut reserved: Vec<&str> =
            vec!["type", "status", "title", "created", "due", "priority", "tags", "tag", "order"];
        if let Some((prop, _)) = task_id {
            reserved.push(prop);
        }
        extra.push_str(&crate::template::sanitize_extra_frontmatter(
            &crate::template::substitute(ef, &vars),
            &reserved,
        ));
    }
    let body = match body_template.map(str::trim) {
        Some(b) if !b.is_empty() => {
            let vars = [
                ("title", title),
                ("date", created),
                ("due", due.unwrap_or("")),
                ("priority", priority.unwrap_or("")),
            ];
            let rendered = crate::template::substitute(b, &vars);
            if rendered.ends_with('\n') { rendered } else { format!("{rendered}\n") }
        }
        _ => String::new(),
    };
    format!(
        "---\ntype: Task\nstatus: new\ntitle: {}\ncreated: {created}\n{extra}---\n\n{body}",
        yaml_quote(title)
    )
}
```
`create_task` — add the two params and forward them:
```rust
pub fn create_task(
    root: &Path,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    task_id: Option<(&str, &str)>,
    extra_frontmatter: Option<&str>,
    body_template: Option<&str>,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    crate::capture_note::write_note_collision_safe(
        &target,
        &render_task(title, today, due, priority, tags, task_id, extra_frontmatter, body_template),
    )
}
```

- [ ] **Step 4: Update existing `render_task`/`create_task` callers in this crate's tests**

Any existing test in `tasks/disk.rs` calling `render_task(...)`/`create_task(...)`
gets `, None, None` appended. Run: `cd src-tauri/core && cargo test --lib`
Expected: PASS.

- [ ] **Step 5: Gates + commit**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
```bash
git add src-tauri/core/src/tasks/disk.rs
git commit -m "feat(core): additive template (frontmatter + body) for task documents"
```

---

## Task 11: Task template wiring (service + IPC + frontend)

**Files:**
- Modify: `src-tauri/core/src/services/tasks/mod.rs` (`add_task` threads `cfg` templates into `create_task`)
- Modify: `src-tauri/src/task_commands.rs` (`TasksConfigDto` fields + new `set_task_template_config`)
- Modify: `src-tauri/src/lib.rs` (register command)
- Create: `src/components/TaskTemplateSettings.vue`; modify `src/components/TasksConfigTab.vue`, `src/types.ts`
- Test: `tests/tasks-config-tab.test.ts`

**Interfaces:**
- Produces: `set_task_template_config(id, extra_frontmatter, body_template)` command; `TasksConfigDto` gains `task_extra_frontmatter`/`task_body_template`.

- [ ] **Step 1: Thread templates through `add_task`**

In `services/tasks/mod.rs::add_task`, at the `create_task(...)` call (~line 216),
pass the vault config's task templates (the `cfg` binding is already in scope):
```rust
    let path = tasks::create_task(
        &target_root, title, today, due, priority, tags, task_id,
        cfg.task_extra_frontmatter.as_deref(),
        cfg.task_body_template.as_deref(),
    )
    .map_err(|e| format!("Could not create task: {e}"))?;
```
(If `services` has its own `create_task` unit tests or an MCP-facing caller,
append `None, None` there or thread the cfg fields similarly.)

- [ ] **Step 2: Extend `TasksConfigDto` + `get_tasks_config`**

`src-tauri/src/task_commands.rs`:
- Add to `TasksConfigDto`: `pub task_extra_frontmatter: Option<String>,` and `pub task_body_template: Option<String>,`
- In `get_tasks_config`, populate them: `task_extra_frontmatter: cfg.task_extra_frontmatter.clone(), task_body_template: cfg.task_body_template.clone(),` (clone before `cfg` is moved into the struct, or reorder field moves).

- [ ] **Step 3: Add the `set_task_template_config` command**

Append to `task_commands.rs` (mirror `set_task_lists_config`'s read-modify-write):
```rust
/// Persist the vault's per-vault task template (extra frontmatter + body).
/// Independent field-save (the set_task_id_config pattern): a template save
/// can't block the folder/lists/id saves and vice versa. Blank→None. ASYNC —
/// fsync'd config write.
#[tauri::command]
pub async fn set_task_template_config(
    lock: tauri::State<'_, ConfigWriteLock>,
    id: String,
    extra_frontmatter: Option<String>,
    body_template: Option<String>,
) -> Result<(), String> {
    crate::commands::find_vault(&id)?;
    let clean = |s: Option<String>| s.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let _guard = lock_ignoring_poison(&lock.0);
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.task_extra_frontmatter = clean(extra_frontmatter);
    value.task_body_template = clean(body_template);
    capture_config::update_vault_config(&id, value)
}
```

- [ ] **Step 4: Register the command**

In `src-tauri/src/lib.rs` `generate_handler!`, add `task_commands::set_task_template_config` beside the other task commands.

- [ ] **Step 5: Compile gate**

Run: `cd src-tauri && cargo test -p vault_buddy_core --lib` then shell clippy/build as in Task 9 Step 4.
Expected: builds; core tests pass.

- [ ] **Step 6: Frontend — presentational card + tab wiring**

`src/types.ts`: add `taskExtraFrontmatter?: string | null; taskBodyTemplate?: string | null;` to the tasks-config type used by `TasksConfigTab`.

Create `src/components/TaskTemplateSettings.vue` — presentational (props
`extraFrontmatter`, `bodyTemplate`; emits `update:extraFrontmatter`,
`update:bodyTemplate`, `blur`), mirroring `TaskIdSettings.vue`'s shape: an
`<h2>` "Task template", two `<textarea>`s (`data-testid="task-extra-frontmatter"`,
`data-testid="task-body-template"`), helper text listing `{{title}}`,
`{{date}}`, `{{due}}`, `{{priority}}` and noting identity fields are always added.

`src/components/TasksConfigTab.vue`: import + render `<TaskTemplateSettings>`
as the LAST card (after Task lists); load `task_extra_frontmatter`/`task_body_template`
from `get_tasks_config`, and on change/blur call
`invoke("set_task_template_config", { id, extraFrontmatter, bodyTemplate })`
via the tab's existing autosave/debounce mechanism.

- [ ] **Step 7: Frontend test + run**

Add to `tests/tasks-config-tab.test.ts`: `get_tasks_config` returning a
`taskBodyTemplate` renders it in the textarea; editing calls
`set_task_template_config`. Run: `npx vitest run tests/tasks-config-tab.test.ts`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/core/src/services/tasks/mod.rs src-tauri/src/task_commands.rs src-tauri/src/lib.rs src/components/TaskTemplateSettings.vue src/components/TasksConfigTab.vue src/types.ts tests/tasks-config-tab.test.ts
git commit -m "feat: configurable task-document template (frontmatter + body)"
```

---

## Task 12: Document renderer — additive templates (core + wiring)

**Files:**
- Modify: `src-tauri/core/src/document_import.rs` (`DocMeta` + `render_frontmatter` + `publish`/`publish_inner` body handling + tests)
- Modify: `src-tauri/src/document_commands.rs` (`DocumentsConfigDto` + `set_documents_config` args + convert call site)
- Modify: `src/components/DocumentsConfigTab.vue`, `src/types.ts`
- Test: `tests/documents-config-tab.test.ts`

**Interfaces:**
- Produces: `render_frontmatter` injects sanitized extra frontmatter; `publish` wraps the pandoc body via `{{content}}`; `DocumentsConfigDto` + `set_documents_config` gain the two document template fields.

- [ ] **Step 1: Write failing core tests**

Add to `document_import.rs` tests (create a staged note + call `publish`, or
test a new pure helper — prefer a pure `assemble(frontmatter, body_template,
content)` helper so the composition is unit-testable without disk):
```rust
#[test]
fn document_frontmatter_default_is_byte_identical() {
    let meta = DocMeta { source_path: "/x/a.docx".into(), imported: "2026-07-10".into(), format: DocFormat::Docx };
    let fm = render_frontmatter(&meta, None);
    assert_eq!(fm, "---\ntype: Document\ntags: [vault-buddy-import]\nsource: \"/x/a.docx\"\nimported: \"2026-07-10\"\nformat: \"docx\"\ncreated-by: Vault Buddy\n---\n\n");
}

#[test]
fn document_body_template_wraps_content_via_placeholder() {
    // assemble_body inserts the pandoc content at {{content}}, appends if absent.
    assert_eq!(assemble_body(Some("> imported\n\n{{content}}"), "BODY"), "> imported\n\nBODY");
    assert_eq!(assemble_body(Some("> note"), "BODY"), "> note\nBODY"); // appended
    assert_eq!(assemble_body(None, "BODY"), "BODY"); // default = content only
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri/core && cargo test document_ -- --nocapture`
Expected: FAIL (arity/helper missing).

- [ ] **Step 3: Implement extra-frontmatter injection + a `content` body wrapper**

`render_frontmatter` — add an `extra_frontmatter: Option<&str>` param and inject
before the closing fence (reserved = the document managed keys):
```rust
pub fn render_frontmatter(meta: &DocMeta, extra_frontmatter: Option<&str>) -> String {
    let mut fm = format!(
        "---\ntype: Document\ntags: [vault-buddy-import]\nsource: {}\nimported: {}\nformat: {}\ncreated-by: Vault Buddy\n",
        yaml_quote(&meta.source_path),
        yaml_quote(&meta.imported),
        yaml_quote(meta.format.label()),
    );
    if let Some(ef) = extra_frontmatter {
        const DOC_RESERVED: &[&str] = &["type", "tags", "source", "imported", "format", "created-by"];
        let vars = [
            ("source", meta.source_path.as_str()),
            ("format", meta.format.label()),
            ("date", meta.imported.as_str()),
        ];
        fm.push_str(&crate::template::sanitize_extra_frontmatter(
            &crate::template::substitute(ef, &vars),
            DOC_RESERVED,
        ));
    }
    fm.push_str("---\n\n");
    fm
}

/// Compose the note body: substitute the pandoc `content` into a body
/// template (appended if the template omits `{{content}}`, so the converted
/// text is never dropped). None → content only (today's output).
pub fn assemble_body(body_template: Option<&str>, content: &str) -> String {
    match body_template.map(str::trim) {
        Some(t) if !t.is_empty() => {
            if t.contains("{{content}}") {
                crate::template::substitute(t, &[("content", content)])
            } else {
                let rendered = crate::template::substitute(t, &[("content", content)]);
                format!("{}\n{content}", rendered.trim_end_matches('\n'))
            }
        }
        _ => content.to_string(),
    }
}
```
Update `publish`/`publish_inner` to take `body_template: Option<&str>` and use
`assemble_body(body_template, &body)` instead of using the raw `body` directly:
```rust
    let body = std::fs::read_to_string(&staged_note)?;
    let full = format!("{frontmatter}{}", assemble_body(body_template, &body));
```
Thread `body_template` through `publish` → `publish_inner`.

- [ ] **Step 4: Run core tests to verify pass**

Run: `cd src-tauri/core && cargo test --lib`
Expected: PASS (including the byte-identical frontmatter test).

- [ ] **Step 5: Update the shell convert call site + config command**

`src-tauri/src/document_commands.rs`:
- Convert call site (~238-245): read the vault's document template from config
  (the fn already resolves the vault + config for `document_date_folders`; read
  `cfg.document_extra_frontmatter`/`cfg.document_body_template` from the same
  `vault_config` load) and pass them:
```rust
    let frontmatter = document_import::render_frontmatter(&meta, cfg.document_extra_frontmatter.as_deref());
    let note = document_import::publish(&plan, &dir, &frontmatter, cfg.document_body_template.as_deref())
        .map_err(|e| format!("Could not save the imported note: {e}"))?;
```
  (Ensure a `let cfg = capture_config::vault_config(&capture_config::load_config(), &id);` binding exists in that function; if the function already reads config for the date-folder toggle, reuse it.)
- `DocumentsConfigDto`: add `pub document_extra_frontmatter: Option<String>,` and `pub document_body_template: Option<String>,`; populate in `get_documents_config`.
- `set_documents_config`: add params `document_extra_frontmatter: Option<String>` and `document_body_template: Option<String>`, clean blank→None, and pass them into the now-6-arg `merge_documents_owned(&existing, folder, document_date_folders, document_extract_images, ef, body)`.

- [ ] **Step 6: Compile gate**

Run: `cd src-tauri && cargo test -p vault_buddy_core --lib` + shell clippy/build (Task 9 Step 4).
Expected: builds; core tests pass.

- [ ] **Step 7: Frontend**

`src/types.ts`: add `documentExtraFrontmatter?: string | null; documentBodyTemplate?: string | null;` to the documents-config type.

`src/components/DocumentsConfigTab.vue`: load the two fields from
`get_documents_config`; render two `<textarea>`s
(`data-testid="document-extra-frontmatter"`, `data-testid="document-body-template"`)
with helper text listing `{{date}}`, `{{source}}`, `{{format}}`, `{{name}}`,
`{{content}}` (and noting `{{content}}` is where the converted document goes,
identity fields always added); include both fields in the `set_documents_config`
invoke and the tab's `TEXT_KEYS`/debounce (the invoke gains the two new args).

- [ ] **Step 8: Frontend test + run**

Add to `tests/documents-config-tab.test.ts`: loads `documentBodyTemplate`, and
editing calls `set_documents_config` with the two new args. Update the mount
helper/mocked `get_documents_config`/`set_documents_config` to include the new
fields/args. Run: `npx vitest run tests/documents-config-tab.test.ts`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/core/src/document_import.rs src-tauri/src/document_commands.rs src/components/DocumentsConfigTab.vue src/types.ts tests/documents-config-tab.test.ts
git commit -m "feat: configurable imported-document template (frontmatter + body)"
```

---

## Task 13: Full-workspace verification

**Files:** none (verification only).

- [ ] **Step 1: Rust workspace gates**

Run:
```bash
cd src-tauri && cargo fmt --check
cargo test -p vault_buddy_core -p vault_buddy_capture --lib
npm run setup:linux   # once, if not already done
cd .. && npm run build && npx tauri build --no-bundle
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib
```
Expected: all pass (shell compile gate green).

- [ ] **Step 2: Frontend gates**

Run: `npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage`
Expected: pass. If the LOC or quality baseline improved, re-run with `--update` and stage the baseline. If a coverage floor trips because a new file needs a test, add the missing test (do not lower the floor).

- [ ] **Step 3: Commit any baseline updates**

```bash
git add scripts/loc-baseline.json scripts/quality-baseline.json vite.config.ts
git commit -m "chore: update LOC/quality baselines for template + favorites work"
```
(Only if a baseline actually changed.)

---

## Task 14: Documentation

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/Gaps.md` (only if a new gap surfaced)

- [ ] **Step 1: Update `AGENTS.md`**

- IPC table (`task_commands.rs` row): add `set_task_template_config` *(async — independent field-save; blank→None)*. Bump the command count in the surrounding prose if it states a total.
- The capture/tasks/document domain sections: note the additive template (extra frontmatter + body, `{{placeholders}}`, managed keys/embeds always emitted, reserved-key sanitize + fence-safety) and that empty templates reproduce today's output. Note the date-folder defaults are now OFF.
- Config section: list the six new per-vault `noteExtraFrontmatter`/`noteBodyTemplate`/`taskExtraFrontmatter`/`taskBodyTemplate`/`documentExtraFrontmatter`/`documentBodyTemplate` fields and the `config_merge` preservation.
- Frontend state / vault domain: mention favorite vaults (localStorage, dedicated top section) and the import-picker filter.

- [ ] **Step 2: Commit**

```bash
git add AGENTS.md docs/Gaps.md
git commit -m "docs: document templates, favorites, and date-folder default change"
```

---

## Self-review notes (addressed in this plan)

- **Spec coverage:** #1 import filter → Task 3; #2 favorites → Tasks 4–5; #3 date defaults → Task 1; #4 task-id second → Task 2; #5 templates → Tasks 6–12 (helper, config, three renderers + wiring); docs → Task 14.
- **Byte-identical guard:** Tasks 8/10/12 each assert the empty-template default equals the current exact output, protecting existing behavior.
- **GAP-60:** Task 7 preserves the 4 non-capture template fields in `merge_capture_owned` and extends `merge_documents_owned`; note fields are capture-owned (ride `set_capture_config`, round-tripped by `RecordMode`).
- **Type consistency:** `substitute`/`sanitize_extra_frontmatter` signatures are used identically across renderers; `render_task`/`create_task` arity is updated at every caller (Task 10 Step 4 + Task 11 Step 1); `merge_documents_owned`'s new arity is updated at its sole caller (Task 12 Step 5) and its test (Task 7 Step 7).
- **Reserved-key sets:** note `[recorded,duration,paused,vault,type,inputs,event,created-by]`; task `[type,status,title,created,due,priority,tags,tag,order]` + id property; document `[type,tags,source,imported,format,created-by]`.
