# Preset Panel Sizing + Task-Management Usability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the panel three preset sizes (S/M/L) with a larger default, applied through the existing flicker-safe show sequence, and redesign the tasks view so the reclaimed height goes to visible tasks instead of stacked chrome.

**Architecture:** A new app-global `panel` config section (`core`) resolves to logical window dimensions; the shell sizes the *hidden* panel from that config on the open path (never a visible resize) and exposes get/set IPC; the frontend adds a Buddy-settings size control and declutters the tasks view. Behavior-preserving for task actions — the existing task suite is the guard.

**Tech Stack:** Rust (Tauri v2, `core` pure crate), Vue 3.5 `<script setup>`, Tailwind 4 (tokens/primitives already in place), Vitest + `@vue/test-utils`.

## Global Constraints

- **`resizable` stays `false`** — presets only; never make the panel OS-resizable.
- **Never resize a *visible* panel.** Sizing happens only while the panel is hidden, inside the existing *size/position-while-hidden → show* sequence (the WebView2 stale-frame-flash invariant). A size change re-applies by re-running that show path, not by `set_size` on a shown window.
- **Config discipline:** the new `panel` section is parsed per-field defensively (unknown/missing size → `comfortable`) and **round-tripped by `serialize_config`** — mirror the existing `mcp` section exactly (regression-tested "never drop a section on save").
- **Preset dims (logical px):** `compact` = 400×460, `comfortable` = 448×580 (**default**), `large` = 560×720.
- **Behavior-preserving tasks:** every task action (toggle/edit/archive/drag-reorder/list-move) and the grouping/sort/filter *logic* stay intact; every `data-testid` is preserved. The ONE intentional UI-behavior change is the filter becoming toggle-activated (Task 6), with its visibility test deliberately updated — no other task test may change.
- **Shell compile-gate on Linux:** `npm run setup:linux` once, then `npx tauri build --no-bundle`; `cargo fmt --check` + clippy `-D warnings`. TDD; Conventional Commits (`feat(shell)`, `feat(ui)`, `style(core)`, …).

---

## Task 1: `core` — panel config + size→dims

**Files:**
- Create: `src-tauri/core/src/panel_config.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod panel_config;`)
- Modify: `src-tauri/core/src/capture_config.rs` (`AppConfig` field + parse + serialize)
- Test: inline `#[cfg(test)]` in `panel_config.rs` + a round-trip test in `capture_config.rs`'s test module (mirror the `mcp` round-trip test)

**Interfaces:**
- Produces: `panel_config::{PanelSize, PanelConfig, panel_entry}`; `PanelSize::{from_str, as_str, dims}`; `AppConfig.panel: PanelConfig`.

- [ ] **Step 1: Write the failing test** (`src-tauri/core/src/panel_config.rs`, at the bottom)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_defaults_unknown_to_comfortable() {
        assert_eq!(PanelSize::from_str("compact"), PanelSize::Compact);
        assert_eq!(PanelSize::from_str("large"), PanelSize::Large);
        assert_eq!(PanelSize::from_str("comfortable"), PanelSize::Comfortable);
        assert_eq!(PanelSize::from_str("nonsense"), PanelSize::Comfortable);
        assert_eq!(PanelSize::default(), PanelSize::Comfortable);
    }

    #[test]
    fn dims_match_the_presets() {
        assert_eq!(PanelSize::Compact.dims(), (400.0, 460.0));
        assert_eq!(PanelSize::Comfortable.dims(), (448.0, 580.0));
        assert_eq!(PanelSize::Large.dims(), (560.0, 720.0));
    }

    #[test]
    fn panel_entry_reads_size_defensively() {
        assert_eq!(panel_entry(&serde_json::json!({"size": "large"})).size, PanelSize::Large);
        // missing / wrong-type → default
        assert_eq!(panel_entry(&serde_json::json!({})).size, PanelSize::Comfortable);
        assert_eq!(panel_entry(&serde_json::json!({"size": 5})).size, PanelSize::Comfortable);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd src-tauri/core && cargo test panel_config 2>&1 | tail -20`
Expected: FAIL — `panel_config` module doesn't exist.

- [ ] **Step 3: Write the module**

```rust
// src-tauri/core/src/panel_config.rs
//! App-global panel window preset size, stored as a top-level `panel` section
//! beside `vaults`/`mcp` in config.json. Pure size→dims mapping (no Tauri
//! types) so the shell reads it on the flicker-safe panel-open path.

/// The three panel presets. `Comfortable` is the default (and the
/// tauri.conf.json default), so an absent/malformed config lands there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelSize {
    Compact,
    #[default]
    Comfortable,
    Large,
}

impl PanelSize {
    pub fn from_str(s: &str) -> PanelSize {
        match s {
            "compact" => PanelSize::Compact,
            "large" => PanelSize::Large,
            _ => PanelSize::Comfortable, // "comfortable" + any unknown value
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            PanelSize::Compact => "compact",
            PanelSize::Comfortable => "comfortable",
            PanelSize::Large => "large",
        }
    }
    /// Logical (width, height) for this preset. Height-biased — tasks need
    /// vertical room. `place_beside` clamps into the work area, so `large` is
    /// safe on small screens.
    pub fn dims(self) -> (f64, f64) {
        match self {
            PanelSize::Compact => (400.0, 460.0),
            PanelSize::Comfortable => (448.0, 580.0),
            PanelSize::Large => (560.0, 720.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PanelConfig {
    pub size: PanelSize,
}

/// Parse a `panel` config entry defensively — a missing or non-string `size`
/// degrades to the default.
pub fn panel_entry(entry: &serde_json::Value) -> PanelConfig {
    let size = entry
        .get("size")
        .and_then(|v| v.as_str())
        .map(PanelSize::from_str)
        .unwrap_or_default();
    PanelConfig { size }
}
```

Then add `pub mod panel_config;` to `src-tauri/core/src/lib.rs` (beside the other `pub mod` lines).

- [ ] **Step 4: Wire into `AppConfig`** (`src-tauri/core/src/capture_config.rs`)

Add the field to the struct at line 30:
```rust
pub struct AppConfig {
    pub vaults: HashMap<String, VaultCaptureConfig>,
    pub mcp: McpConfig,
    pub document_import: DocumentImportConfig,
    pub panel: crate::panel_config::PanelConfig,
}
```
In `parse_config`, after the `document_import` section is parsed and assigned (near `cfg.document_import = di;`, ~line 217), add:
```rust
    if let Some(p) = root.get("panel") {
        cfg.panel = crate::panel_config::panel_entry(p);
    }
```
(Use the same `root`/section accessor the `mcp`/`document_import` branches use — read those two branches first and mirror their exact shape.)
In `serialize_config`, after the `document_import` block (~line 133-140), add:
```rust
    if cfg.panel != crate::panel_config::PanelConfig::default() {
        let mut panel = serde_json::Map::new();
        panel.insert("size".to_string(), json!(cfg.panel.size.as_str()));
        map.insert("panel".to_string(), json!(panel));
    }
```
(Match the exact map/`json!` idiom the `mcp` block uses — read `serialize_config` lines 122-140 first and mirror it.)

- [ ] **Step 5: Add the round-trip test** (`capture_config.rs` test module — mirror `mcp_config_round_trips_through_serialize`)

```rust
    #[test]
    fn panel_config_round_trips_and_defaults() {
        use crate::panel_config::{PanelConfig, PanelSize};
        // default is omitted from the serialized output (like mcp/document_import)
        let mut cfg = parse_config("{}");
        assert_eq!(cfg.panel, PanelConfig::default());
        assert!(!serialize_config(&cfg).contains("\"panel\""));
        // a non-default size round-trips
        cfg.panel.size = PanelSize::Large;
        let round = parse_config(&serialize_config(&cfg));
        assert_eq!(round.panel.size, PanelSize::Large);
        // malformed degrades to default without dropping other sections
        assert_eq!(parse_config(r#"{ "panel": { "size": true } }"#).panel, PanelConfig::default());
    }
```

- [ ] **Step 6: Run tests + fmt/clippy**

Run: `cd src-tauri/core && cargo test panel && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: PASS; clippy clean; fmt clean.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/core/src/panel_config.rs src-tauri/core/src/lib.rs src-tauri/core/src/capture_config.rs
git commit -m "feat(core): panel preset size config + size→dims"
```

---

## Task 2: Shell — flicker-safe sizing on the panel-open path + IPC

**Files:**
- Modify: `src-tauri/src/commands.rs` (`position_panel` ~368; add `get_panel_config` + `set_panel_size`)
- Modify: `src-tauri/src/lib.rs` (register the two commands in `generate_handler` ~406, beside `mcp_commands::get_mcp_config`)
- Test: shell unit tests run in `linux-app` CI; the compile-gate is the local check

**Interfaces:**
- Consumes: `panel_config::PanelSize`, `capture_config::{load_config, config_path, serialize_config}`, `ConfigWriteLock` (`capture_commands`).
- Produces: IPC `get_panel_config() -> {size}`, `set_panel_size(size: String)`.

- [ ] **Step 1: Size the hidden panel in `position_panel`**

Edit `position_panel` (commands.rs ~368) to size the hidden panel from config *before* placing it — `place_beside_buddy` reads the target's `outer_size()`, so the new size positions correctly:

```rust
pub(crate) fn position_panel(app: &tauri::AppHandle) {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{Side, VMode};
    let Some(panel) = app.get_webview_window("panel") else {
        return;
    };
    // Flicker-safe: the panel is HIDDEN here (positioned-while-hidden → shown),
    // so sizing it from config now never resizes a visible surface. Placement
    // below reads the panel's (just-updated) outer size, so the chosen preset
    // is positioned correctly.
    let (w, h) = vault_buddy_core::capture_config::load_config().panel.size.dims();
    if let Err(e) = panel.set_size(tauri::LogicalSize::new(w, h)) {
        log::warn!("position_panel: set_size failed: {e}");
    }
    if let Some((pos, _anchor)) =
        place_beside_buddy(app, &panel, SidePref::Fixed(Side::Right), VMode::Edge, 0.0)
    {
        if let Err(e) = panel.set_position(pos) {
            log::warn!("position_panel: set_position failed: {e}");
        }
    }
}
```

(If a follow-up shows `outer_size()` lagging the `set_size` on some setup, pass `(w,h)` explicitly into a `place_beside_buddy` variant instead of reading `outer_size()` — note it, don't pre-build it. `set_size` is synchronous in Tauri, so read-back is expected to be correct.)

- [ ] **Step 2: Add the two commands** (commands.rs, near the other panel commands)

```rust
/// The panel's configured preset size, for the settings control.
#[tauri::command]
pub fn get_panel_config() -> serde_json::Value {
    let size = vault_buddy_core::capture_config::load_config().panel.size;
    serde_json::json!({ "size": size.as_str() })
}

/// Persist the panel preset size. Applies on the next panel open (position_panel
/// sizes the hidden panel) — the frontend re-shows to reflect it immediately.
/// Read-modify-write under ConfigWriteLock, mirroring set_mcp_config's write.
#[tauri::command]
pub fn set_panel_size(
    size: String,
    lock: tauri::State<'_, crate::capture_commands::ConfigWriteLock>,
) -> Result<(), String> {
    use vault_buddy_core::capture_config;
    let new_size = vault_buddy_core::panel_config::PanelSize::from_str(&size);
    let _guard = lock.0.lock().map_err(|_| "config lock poisoned".to_string())?;
    let mut cfg = capture_config::load_config();
    cfg.panel.size = new_size;
    let path = capture_config::config_path().ok_or("Cannot resolve the config directory")?;
    // Mirror set_mcp_config's atomic write of serialize_config(&cfg) to `path`.
    crate::capture_commands::write_config_atomically(&path, &capture_config::serialize_config(&cfg))
        .map_err(|e| format!("Couldn't save panel size: {e}"))?;
    Ok(())
}
```

(Read `mcp_commands::set_mcp_config` first for the EXACT config-write helper name/signature the codebase uses — replace `write_config_atomically` with whatever `set_mcp_config`/`set_documents_config` call to write `serialize_config` to disk. Do not invent a new writer.)

- [ ] **Step 3: Register in `generate_handler`** (`src-tauri/src/lib.rs` ~462, beside `mcp_commands::get_mcp_config`)

```rust
            commands::get_panel_config,
            commands::set_panel_size,
```

- [ ] **Step 4: Compile-gate + fmt/clippy**

Run: `cd src-tauri && cargo fmt --check && npx tauri build --no-bundle 2>&1 | tail -20`
Expected: builds clean (shell compiles with the new commands + the `position_panel` edit); fmt clean. (If `setup:linux` hasn't run in this env, run `npm run setup:linux` once first.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): size the hidden panel from config; panel-size IPC"
```

---

## Task 3: Bigger default in `tauri.conf.json`

**Files:**
- Modify: `src-tauri/tauri.conf.json` (panel window `width`/`height`, lines ~30-31)

- [ ] **Step 1: Change the panel default to the `comfortable` dims**

Set the panel window's `"width": 400` → `"width": 448` and `"height": 420` → `"height": 580`, so the first open (before any config exists) is already the new default. Leave `"resizable": false`.

- [ ] **Step 2: Verify config validity**

Run: `cd src-tauri && npx tauri build --no-bundle 2>&1 | tail -5` (or `node -e "JSON.parse(require('fs').readFileSync('src-tauri/tauri.conf.json'))"` for a fast JSON check)
Expected: valid config; builds clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "feat(shell): grow the default panel size to comfortable (448x580)"
```

---

## Task 4: Frontend — Panel size settings control

**Files:**
- Modify: `src/components/BuddySettings.vue` (add a "Panel size" section)
- Create: `src/components/PanelSizeSetting.vue` (presentational S/M/L segmented control) + `tests/panel-size-setting.test.ts`
- Modify: the panel view flow to reflect a change immediately (via the existing `vaults` store `requestView` + `close_panel`/`open_panel`)

**Interfaces:**
- Consumes IPC: `get_panel_config` → `{size}`, `set_panel_size({size})`; `close_panel`, `open_panel`; the `vaults` store's `requestView`.
- Produces: `PanelSizeSetting` — props `{ modelValue: "compact"|"comfortable"|"large" }`, emits `update:modelValue`.

- [ ] **Step 1: Write the failing test** (`tests/panel-size-setting.test.ts`)

```ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import PanelSizeSetting from "../src/components/PanelSizeSetting.vue";

describe("PanelSizeSetting", () => {
  it("renders the three presets and marks the selected one", () => {
    const w = mount(PanelSizeSetting, { props: { modelValue: "comfortable" } });
    const btns = w.findAll("button");
    expect(btns.map((b) => b.text())).toEqual(["Compact", "Comfortable", "Large"]);
    expect(w.get('[data-testid="panel-size-comfortable"]').attributes("aria-pressed")).toBe("true");
    expect(w.get('[data-testid="panel-size-large"]').attributes("aria-pressed")).toBe("false");
  });

  it("emits the chosen size", async () => {
    const w = mount(PanelSizeSetting, { props: { modelValue: "comfortable" } });
    await w.get('[data-testid="panel-size-large"]').trigger("click");
    expect(w.emitted("update:modelValue")).toEqual([["large"]]);
  });
});
```

- [ ] **Step 2: Run it to see it fail**

Run: `npx vitest run tests/panel-size-setting.test.ts`
Expected: FAIL (SFC missing).

- [ ] **Step 3: Write the control** (a segmented control, matching the existing bespoke segmented pattern used by the tasks grouping toggle — `border-focus`/`bg-accent/20` active, tokens throughout)

```vue
<!-- src/components/PanelSizeSetting.vue -->
<script setup lang="ts">
defineProps<{ modelValue: "compact" | "comfortable" | "large" }>();
defineEmits<{ (e: "update:modelValue", v: "compact" | "comfortable" | "large"): void }>();
const OPTIONS = [
  { value: "compact", label: "Compact" },
  { value: "comfortable", label: "Comfortable" },
  { value: "large", label: "Large" },
] as const;
</script>

<template>
  <div
    class="flex gap-0.5"
    role="radiogroup"
    aria-label="Panel size"
  >
    <button
      v-for="o in OPTIONS"
      :key="o.value"
      type="button"
      role="radio"
      :data-testid="`panel-size-${o.value}`"
      :aria-pressed="modelValue === o.value"
      :aria-checked="modelValue === o.value"
      class="cursor-pointer rounded-control border px-2 py-1 text-xs transition focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="
        modelValue === o.value
          ? 'border-focus bg-accent/20 text-fg'
          : 'border-white/10 bg-white/5 text-fg-muted hover:bg-white/10'
      "
      @click="$emit('update:modelValue', o.value)"
    >
      {{ o.label }}
    </button>
  </div>
</template>
```

- [ ] **Step 4: Run the test**

Run: `npx vitest run tests/panel-size-setting.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 5: Wire it into `BuddySettings.vue`**

Read `BuddySettings.vue` first to match its section idiom (the `<h2>` header + a settings row). Add a "Panel size" section that: loads the current size via `invoke("get_panel_config")` on mount; renders `<PanelSizeSetting v-model="size" />`; on change, `await invoke("set_panel_size", { size })`, then reflect it immediately by keeping the user on settings across a re-show — `store.requestView("settings"); await invoke("close_panel"); await invoke("open_panel")`. (Read how another settings control invokes IPC + how `requestView` is called elsewhere, and mirror it; `open_panel`'s reopen runs `position_panel`, which sizes the hidden panel to the new preset.) Add a `text-fg-subtle` hint line: "Resizes the panel; task lists get more room in larger sizes."

- [ ] **Step 6: Gate**

Run: `npx vitest run tests/panel-size-setting.test.ts tests/buddy-settings.test.ts && npm run build`
Expected: PASS; build clean. (If `buddy-settings.test.ts` asserts a fixed set of sections, extend it for the new one — that's a deliberate addition, not a regression.)

- [ ] **Step 7: Commit**

```bash
git add src/components/PanelSizeSetting.vue tests/panel-size-setting.test.ts src/components/BuddySettings.vue
git commit -m "feat(ui): panel size control in Buddy settings"
```

---

## Task 5: Frontend — tasks-view declutter (slim chrome, list fills the room)

**Files:**
- Modify: `src/components/Tasks.vue` (the header region ~327-390: progress bar, gaps, list container)
- Test (gate): `tests/tasks.test.ts` + the task-* test files must stay green **unchanged**

**Interfaces:** none new. Presentational only.

- [ ] **Step 1: Green baseline**

Run: `npx vitest run tests/tasks.test.ts tests/task-sections.test.ts tests/task-reorder.test.ts`
Expected: PASS (record counts; they must not change in this task).

- [ ] **Step 2: Slim the progress indicator + tighten the header**

In `Tasks.vue`, the progress block (~327-337) is a full row with a bar + a `{{ done }} / {{ total }}` numeric. Make it a **thin 2px bar** with the count folded into a compact inline label (or a `title`), so it stops eating a full row. Reduce the vertical gaps in the header stack (the container's `gap-*`) so the composer/toolbar sit tighter. Keep the exact `progress.done`/`progress.total` values reachable (a test may assert the numbers) — render them in a `title`/`aria-label` or a `text-micro` inline span rather than a large row.

- [ ] **Step 3: Make the list fill the panel height**

Ensure the task-list container is `flex-1 min-h-0 overflow-y-auto` within the tasks view's flex column, so the reclaimed height + the larger panel become visible rows (not whitespace). Confirm the outer `ActionPanel` view wrapper already provides the flex column (it uses `panel-scroll min-h-0 flex-1`); the tasks view should let its list grow rather than the whole view scrolling as one block, so the toolbar/composer stay pinned and only the list scrolls. (If the current structure scrolls the whole view, restructure into a pinned header + scrolling list — presentational, no logic change.)

- [ ] **Step 4: Gate**

Run: `npx vitest run tests/tasks.test.ts tests/task-sections.test.ts tests/task-reorder.test.ts && npm run build`
Expected: PASS with the SAME counts as Step 1; build clean. If a test asserting the progress `done/total` text fails, keep the numbers reachable via the compact label/`title` rather than changing the test.

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue
git commit -m "feat(ui): slim the tasks-view chrome so the list fills the panel"
```

---

## Task 6: Frontend — filter as a toolbar toggle

**Files:**
- Modify: `src/components/TaskViewControls.vue` (add a filter-toggle button to the toolbar) and `src/components/Tasks.vue` (drive filter visibility from the toggle instead of the `>5` auto-show)
- Test: `tests/tasks.test.ts` — the ONE deliberate visibility-test update

**Interfaces:** `TaskViewControls` gains a `filterActive: boolean` prop + `toggleFilter` emit (or a `v-model:filterOpen`), consumed by `Tasks.vue`.

- [ ] **Step 1: Baseline + find the filter-visibility test**

Run: `npx vitest run tests/tasks.test.ts` (record count). Read the test(s) asserting the filter input appears above 5 tasks (`data-testid="task-filter"`) — those are the ones this task intentionally changes.

- [ ] **Step 2: Add the toggle**

Add a magnifier `IconButton` to `TaskViewControls`'s right group (beside sort), `data-testid="task-filter-toggle"`, `aria-label="Filter tasks"`, `aria-pressed` bound to a `filterActive` prop, emitting `toggleFilter`. In `Tasks.vue`, replace the `showFilter` (`tasks.length > 5`) gate on the filter `<input>` (~339-347) with a `filterOpen` ref toggled by the button; the filtering LOGIC (`filter` model, the `filteredTasks` computed) is unchanged. Keep the filter auto-open when a `tagFilter` is active (so behavior there is unchanged). Focus the input when the toggle opens it.

- [ ] **Step 3: Update the ONE visibility test**

Change the filter test from "input is present when >5 tasks" to "the toggle is present, and clicking it reveals `task-filter`, and filtering still narrows the list." Do NOT weaken any other assertion; the filtering-behavior assertions (that typing narrows results) stay.

- [ ] **Step 4: Gate**

Run: `npx vitest run tests/tasks.test.ts && npm run build`
Expected: PASS (the count may shift by the reworked filter test only); build clean. Any OTHER task test needing a change means a real behavior slip — stop and reconsider.

- [ ] **Step 5: Commit**

```bash
git add src/components/TaskViewControls.vue src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): filter tasks via a toolbar toggle instead of an always-on row"
```

---

## Task 7: Docs + baselines

**Files:**
- Modify: `AGENTS.md` (the IPC-surface table → add `get_panel_config`/`set_panel_size` under `commands.rs`; the "Where state lives on disk" table → note the `panel` config section; a line on the panel presets in the window-system section)
- Modify: `scripts/loc-baseline.json` / `scripts/quality-baseline.json` (via `--update`) + `vite.config.ts` (only if a coverage floor rose)

- [ ] **Step 1: Update AGENTS.md**

Add the two commands to the `commands.rs` row of the IPC table; add `panel` to the config-section list in "Where state lives on disk"; add one sentence to "The window system" noting the three presets (S/M/L) applied flicker-safe on the hidden panel, `resizable` still false.

- [ ] **Step 2: Full gate + bank baselines**

Run: `rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage`
Then, for any metric the gate reports improved: `node scripts/check-loc.mjs --update` / `node scripts/check-quality.mjs --update`; re-run the gate and confirm green. Also `cd src-tauri && cargo fmt --check && cd core && cargo test`.

- [ ] **Step 3: Commit**

```bash
git add AGENTS.md scripts/loc-baseline.json scripts/quality-baseline.json vite.config.ts
git commit -m "docs: document panel-size IPC + config; update baselines"
```

---

## Self-Review

**1. Spec coverage:**
- §1 preset sizing (config + dims) → Task 1; flicker-safe apply on the hidden panel → Task 2; bigger default → Task 3; settings control → Task 4. ✔
- §2 tasks redesign: slim chrome + list-fills-room → Task 5; filter-as-toggle → Task 6. ✔
- Architecture (core/shell/frontend split) → Tasks 1–4; testing → each task's gate; docs/baselines → Task 7. ✔

**2. Placeholder scan:** Tasks 2 & 4 say "read `set_mcp_config`/`BuddySettings` first and mirror the exact writer/idiom" rather than reproducing an unread helper — deliberate, because the config-write helper name and the settings-section idiom must match the codebase exactly; every panel-specific bit (the `position_panel` edit, the command bodies, the SFC) has complete code. No `TODO`/"handle edge cases".

**3. Type consistency:** `PanelSize::{from_str,as_str,dims}` and `PanelConfig{size}` (Task 1) are used verbatim in Task 2's shell code; the size strings `"compact"|"comfortable"|"large"` are identical across the Rust `as_str`/`from_str`, the `get/set_panel_size` IPC, and the frontend `PanelSizeSetting` prop union (Task 4). The `comfortable` dims (448×580) match between `PanelSize::dims` (Task 1) and the `tauri.conf.json` default (Task 3).

**Risks carried into execution:** (a) the `set_size`-then-read-`outer_size` sequence in `position_panel` assumes synchronous sizing — noted with the explicit-dims fallback if it lags; (b) Task 4's immediate re-show (`close_panel`/`open_panel` with `requestView("settings")`) depends on the reopen running `position_panel` — verify the open path calls it; if not, the size still applies on the next manual open. (c) Task 6 is the one intentional behavior change; if the user prefers the filter stay always-on, Task 6 is independently revertible without affecting Tasks 1–5.
