# Preset panel sizing + task-management usability — design

Date: 2026-07-23
Status: accepted (user request: "improving the usability heavily … add a
toggle to adjust the panel's size to make the task management more
user-friendly … the default size could grow a bit more". Chosen: **preset
sizes (S/M/L), not free drag** — flicker-safe; the usability work **anchored on
the tasks view**; the broader home-view declutter deferred)

## Problem

The panel is a separate always-on-top window fixed at **400×420,
`resizable: false`**, deliberately *sized-while-hidden → positioned → shown* and
**never resized while visible** — that discipline is what avoids the WebView2
stale-frame flash the three-window split was built to prevent (see AGENTS.md,
"The window system"). Two usability costs follow:

1. **The panel can't grow.** 400×420 is cramped, and there's no way to give a
   dense view more room.
2. **The tasks view spends most of its height on chrome.** Above the task list
   it stacks, top to bottom: a progress bar (`Tasks.vue:327-337`), a filter
   input (`339-347`), an active-tag-filter chip (`349-364`), the add-composer
   (`366-377`), and the grouping/sort toolbar (`TaskViewControls`, `379-390`) —
   ~5 rows of controls. In a 420px window that leaves only a handful of tasks
   visible before scrolling, which is the opposite of "task management is
   friendly."

The design system is fully in place (tokens + primitives, GAP-66 merged), so
this increment is layout + one window mechanism, not new visual vocabulary.

## Goals & scope decisions

- **Preset panel sizes (S/M/L)** with a **larger default**, applied through the
  existing flicker-safe show sequence — never resizing a visible surface.
- **Tasks-view usability redesign**: collapse the control stack so the panel's
  height (especially the reclaimed room) goes to *visible tasks*, not chrome.
- **Behavior-preserving for tasks**: every task action (toggle/edit/archive/
  drag-reorder/list-move), the grouping/sort/filter logic, and all
  `data-testid`s stay intact — this is a *space/layout* redesign, guarded by the
  existing task test suite.
- **`resizable` stays `false`** — presets give control without the OS
  edge-drag stale-frame-flash risk.

### Non-goals (this increment)

- Home-view banner-stack consolidation and the five-per-row vault actions
  (deferred follow-up — noted in GAP-66/Gaps, not built here).
- OS drag-to-resize; a `maxSize`/free-form window.
- Light mode / theming; any change to task *logic* or the task file format.

## Design

### 1. Preset panel sizing

**Sizes (height-biased — height is where tasks need room; final values tuned in
the plan, all clamped into the monitor work area by the existing
`place_beside`):**

| Preset | Approx. dims |
| --- | --- |
| `compact` (S) | 400 × 460 |
| `comfortable` (M, **new default**) | 448 × 580 |
| `large` (L) | 560 × 720 |

- **Config.** A new app-global section in `%APPDATA%\vault-buddy\config.json`:
  `"panel": { "size": "compact" | "comfortable" | "large" }`, parsed per-field
  defensively (unknown/missing → `comfortable`), round-tripped by
  `serialize_config` (the regression-tested "don't drop a section on save"
  discipline every other section already follows). The size→dims mapping is a
  pure `core` function (`panel_size::dims(size) -> (f64, f64)`), unit-tested on
  Linux.
- **Flicker-safe apply.** The panel already runs *set-position-while-hidden →
  show*. Sizing slots into the same sequence: on the panel-open path
  (`commands::position_panel` / `show_panel`), read the configured size and
  `panel.set_size(LogicalSize)` **while the panel is hidden**, then run the
  existing `place_beside_buddy` placement (which already takes `w,h`, so it
  positions the new size correctly) and show. A visible WebView2 surface is
  never resized.
- **Changing the size** re-applies through that same path: write the pref, then
  (if the panel is open) hide → size → reposition → show — one clean re-show, no
  in-place visible resize. All window ops stay on the main thread (sync command;
  the small config write is fsync'd like every other settings save).
- **Default.** `tauri.conf.json`'s panel `width`/`height` change to the
  `comfortable` dims so the very first open (before any pref exists) is already
  the new default; the config pref overrides it thereafter.
- **Control.** A **"Panel size" segmented control (S/M/L)** in Buddy settings,
  persisted, applying immediately. (A one-click header affordance is an easy
  add-on if wanted; deferred unless requested — settings is the "adjust it if
  needed" home.)

### 2. Tasks-view usability redesign

The principle: **less chrome, more tasks.** The five stacked control rows
collapse to ~two, and the reclaimed height (plus the bigger panel) becomes
visible task rows.

- **One toolbar row.** Merge the grouping segmented control (Lists/Dates/Tags)
  and the sort control + direction into a single tidy toolbar row, with the
  **filter as a toggle** (a magnifier that reveals the filter input on demand)
  rather than an always-present input row. The active-tag-filter chip inlines
  into this row when present.
- **Slim progress.** Fold the done/total progress into a thin inline indicator
  in/under the toolbar, not a full row with a large numeric.
- **Calm composer.** The quick-add stays one clean row (title + options toggle +
  add); due/priority/tags/list expand inline, and may default open in the
  `large` size where there's room.
- **Roomier list.** With the wider/taller panel, `TaskRow` (chips/due/actions)
  and section headers get breathing space and better scannability; many more
  rows are visible before scrolling — the core payoff.
- **`TaskViewControls` / `TaskComposer` / `Tasks.vue`** are the touched files;
  all presentational, reusing the primitives. Existing `data-testid`s, emits,
  and the grouping/sort/filter/composer logic are unchanged; a test that must
  change its assertions signals a behavior slip, not a layout change.

## Architecture

- **`core`** (Linux-testable): `panel_config` parse/serialize (defensive,
  round-tripped) + `panel_size::dims`. No Tauri types.
- **Shell (`src-tauri/src`)**: `commands.rs` gains the size-read + `set_size`
  in the panel-open path; new IPC `get_panel_config` (sync) + `set_panel_size`
  (writes config under `ConfigWriteLock`, re-applies to the window on the main
  thread). Registered in `lib.rs`'s `generate_handler` (keep the IPC-surface
  table in AGENTS.md in sync). Compile-gated on Linux (`npx tauri build
  --no-bundle`).
- **Frontend**: a `settings`-store-backed **Panel size** control in
  `BuddySettings`; the tasks redesign in `Tasks.vue` + `TaskViewControls.vue` +
  `TaskComposer.vue`.

## Error handling

Window sizing is best-effort (a failed `set_size`/`set_position` logs a warning
and leaves the panel where/what it was — the existing placement posture); a
malformed `panel` config section degrades to `comfortable`. No new user-facing
error path. The tasks redesign adds no runtime paths.

## Testing

- **Rust:** unit tests for `panel_size::dims` (each preset → expected dims) and
  the defensive `panel` config parse/serialize round-trip; the shell
  compile-gate on Linux.
- **Frontend:** Vitest for the Panel-size settings control (renders presets,
  emits the choice, calls the IPC) and the redesigned tasks layout; the
  **existing task test suite must stay green unchanged** (behavior preserved).
- **Windows** remains the one place the flicker-safe re-show and the actual
  preset dimensions are eyeballed — called out for the reviewer, not gating.

## Quality gates & docs

- `npm run lint && npm run check:loc && npm run check:quality &&
  npm run test:coverage`; `cargo fmt`/clippy/tests for `core` + the shell.
  Update the AGENTS.md IPC-surface table (new commands) and the "Where state
  lives on disk" table (the `panel` config section).

## Rollout / compatibility

- Additive: a new optional config section (absent → `comfortable`, which is
  also the new tauri.conf default), no migration. Existing users get the
  slightly larger default on next launch and can pick S/L.
- Suggested phasing for the plan: (1) `core` panel-config + `panel_size::dims`
  + tests; (2) shell IPC + the flicker-safe sizing in the open path; (3) the
  bigger `tauri.conf.json` default; (4) the Buddy-settings size control; (5) the
  tasks-view toolbar/composer/list redesign; (6) docs + baselines. Sizing (1–4)
  and the tasks redesign (5) are independently shippable — a natural PR split if
  the branch merges early again.
