# Buddy / Panel Window Split — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the buddy its own fixed 88×88 window that only ever moves (never resizes), with the panel and greeting bubble as separate windows positioned beside it, so the panel-open flicker becomes structurally impossible.

**Architecture:** Three Tauri windows (`main`=buddy, `panel`, `bubble`), all transparent/undecorated/always-on-top/non-resizable. Rust owns placement (a pure core-crate function), panel/bubble visibility, and a focus watcher for click-away close. The frontend is one Vite bundle whose `main.ts` mounts a different root component per window label; the three roots coordinate the few shared bits over Tauri capture events and shared-origin `localStorage`.

**Tech Stack:** Rust (Tauri v2.11, `windows-sys`), Vue 3 + Pinia + Tailwind 4, Vitest + happy-dom + `@vue/test-utils`.

## Global Constraints

- **Node 22**; install with `npm ci`. Full Vitest suite: `npm test`; single file: `npx vitest run tests/<file>.test.ts`.
- **The shell crate (`src-tauri/src/*.rs`) does not compile on Linux** (no webkit2gtk). For shell tasks there is no local build/test: mirror existing patterns exactly, run `cd src-tauri && cargo fmt --check` as the only local gate, and rely on CI's `windows-app` job as the compile gate.
- **Core crate** builds/tests locally: `cd src-tauri/core && cargo test` and `cargo clippy --all-targets -- -D warnings`.
- **Rust logic that doesn't need Tauri types goes in `core`** (per AGENTS.md).
- **The app never writes into a vault.** This increment touches no vault-writing path.
- **Invoke the tauri CLI only as `npx tauri <cmd>`**, never via npm script indirection.
- **Commits:** Conventional Commits with existing scopes (`feat(core)`, `feat(shell)`, `fix(shell)`, `feat(ui)`, `refactor(ui)`, `docs`). Imperative subject; body explains the *why*.
- **Comments** explain constraints the code can't show (race windows, platform quirks, ordering) — match the existing heavy-on-invariants density.
- **Windows keep these flags:** `transparent: true`, `decorations: false`, `alwaysOnTop: true`, `resizable: false`, `skipTaskbar: true`, `shadow: false`. The buddy window keeps label `main` (preserves the persisted window-state key and single-instance/tray wiring).
- **The drag-crash fix stays intact:** the buddy still drags via `start_buddy_drag`; window-state saves and window getters run on the MAIN thread only; the metronome (`window_upkeep_tick`) still checkpoints the buddy position and re-asserts always-on-top.

---

## File Structure

**Core (Rust, Linux-testable):**
- Create `src-tauri/core/src/companion_placement.rs` — pure `panel_position(...)`. Register in `core/src/lib.rs`.

**Shell (Rust, Windows-only compile):**
- Modify `src-tauri/tauri.conf.json` — add `panel` + `bubble` windows (hidden).
- Modify `src-tauri/src/commands.rs` — add `toggle_panel`, `close_panel`, `position_panel` helper; remove `set_panel_offset`, `set_window_geometry`, `PanelOffset`, and `prepare_update_install`'s offset restore.
- Modify `src-tauri/src/lib.rs` — register new commands, drop removed ones; add the focus watcher; show the bubble on launch; drop `PanelOffset` state; simplify the `CloseRequested`/upkeep paths that referenced the offset.
- Modify `src-tauri/src/tray.rs` — `restore_home_position` removed; `finish_quit`/`hide_buddy` simplified; hide panel+bubble when hiding the buddy.

**Frontend (Vue/TS):**
- Modify `src/main.ts` — mount `BuddyRoot` / `PanelRoot` / `BubbleRoot` by window label.
- Create `src/roots/BuddyRoot.vue`, `src/roots/PanelRoot.vue`, `src/roots/BubbleRoot.vue`.
- Modify `src/stores/settings.ts` — add a `storage`-event sync action.
- Modify `src/stores/vaults.ts` — remove `panelOpen`/`togglePanel`; add `refresh()`.
- Delete `src/App.vue`, `src/composables/useCompanionWindow.ts`, `src/composables/companionPlacement.ts` and their tests.
- `src/components/CompanionCharacter.vue`, `ActionPanel.vue`, `SpeechBubble.vue`, `useGreeting.ts` are reused by the roots.

**Docs:**
- Modify `AGENTS.md` — rewrite the window-geometry section and IPC surface.

---

## Phase 1 — Core placement

### Task 1: `panel_position` in the core crate

**Files:**
- Create: `src-tauri/core/src/companion_placement.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod companion_placement;`)
- Test: in `companion_placement.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces:
  - `vault_buddy_core::companion_placement::Rect { x: i32, y: i32, w: i32, h: i32 }` (physical px, `Clone+Copy+Debug+PartialEq+Eq`)
  - `vault_buddy_core::companion_placement::Point { x: i32, y: i32 }` (same derives)
  - `vault_buddy_core::companion_placement::panel_position(buddy: Rect, work_area: Option<Rect>, panel_w: i32, panel_h: i32) -> Point`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/core/src/companion_placement.rs`:

```rust
//! Where the panel/bubble window goes relative to the buddy window. Pure and
//! unit-tested here; the shell calls it when it positions the (hidden) panel
//! window before showing it. All coordinates are physical pixels.

/// A rectangle in physical pixels (top-left origin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// A point in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Top-left for the panel window, given the buddy rect, the monitor work area,
/// and the panel size.
///
/// Prefers RIGHT of the buddy and TOP-aligned with it; near the right edge it
/// flips LEFT, near the bottom edge it BOTTOM-aligns so the panel unfolds
/// upward. The result is clamped to the work area. With no work area (unknown
/// monitor) it degrades to right + top-aligned, unclamped — the same fallback
/// the old single-window code used.
pub fn panel_position(buddy: Rect, work_area: Option<Rect>, panel_w: i32, panel_h: i32) -> Point {
    let Some(area) = work_area else {
        return Point {
            x: buddy.x + buddy.w,
            y: buddy.y,
        };
    };
    // Horizontal: to the right of the buddy unless that overflows the right
    // edge, in which case flip to the left of the buddy.
    let right_x = buddy.x + buddy.w;
    let x = if right_x + panel_w <= area.x + area.w {
        right_x
    } else {
        buddy.x - panel_w
    };
    // Vertical: top-aligned with the buddy unless that overflows the bottom
    // edge, in which case bottom-align (panel unfolds upward).
    let y = if buddy.y + panel_h <= area.y + area.h {
        buddy.y
    } else {
        buddy.y + buddy.h - panel_h
    };
    // Clamp fully on-screen. A panel larger than the work area (or a buddy in
    // a corner) still lands inside; max is floored to the min so clamp never
    // sees max < min.
    let max_x = (area.x + area.w - panel_w).max(area.x);
    let max_y = (area.y + area.h - panel_h).max(area.y);
    Point {
        x: x.clamp(area.x, max_x),
        y: y.clamp(area.y, max_y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
    };
    const PANEL_W: i32 = 360;
    const PANEL_H: i32 = 340;
    const BUDDY: i32 = 88;

    fn buddy_at(x: i32, y: i32) -> Rect {
        Rect {
            x,
            y,
            w: BUDDY,
            h: BUDDY,
        }
    }

    #[test]
    fn opens_right_and_top_aligned_with_room() {
        let p = panel_position(buddy_at(100, 100), Some(AREA), PANEL_W, PANEL_H);
        assert_eq!(p, Point { x: 100 + BUDDY, y: 100 });
    }

    #[test]
    fn flips_left_near_the_right_edge() {
        // buddy hugging the right edge: right of it would overflow → open left
        let p = panel_position(buddy_at(1900, 100), Some(AREA), PANEL_W, PANEL_H);
        assert_eq!(p.x, 1900 - PANEL_W);
        assert_eq!(p.y, 100);
    }

    #[test]
    fn bottom_aligns_near_the_bottom_edge() {
        // buddy near the bottom: top-aligned would overflow → panel bottom
        // meets the buddy bottom (unfolds upward)
        let p = panel_position(buddy_at(100, 1000), Some(AREA), PANEL_W, PANEL_H);
        assert_eq!(p.x, 100 + BUDDY);
        assert_eq!(p.y, 1000 + BUDDY - PANEL_H);
    }

    #[test]
    fn handles_the_bottom_right_corner() {
        let p = panel_position(buddy_at(1900, 1000), Some(AREA), PANEL_W, PANEL_H);
        assert_eq!(p.x, 1900 - PANEL_W);
        assert_eq!(p.y, 1000 + BUDDY - PANEL_H);
    }

    #[test]
    fn no_monitor_falls_back_to_right_top() {
        let p = panel_position(buddy_at(100, 100), None, PANEL_W, PANEL_H);
        assert_eq!(p, Point { x: 188, y: 100 });
    }

    #[test]
    fn clamps_a_panel_larger_than_the_work_area() {
        let small = Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 200,
        };
        let p = panel_position(buddy_at(10, 10), Some(small), PANEL_W, PANEL_H);
        // max is floored to the area origin — never panics, lands at origin
        assert_eq!(p, Point { x: 0, y: 0 });
    }

    #[test]
    fn respects_a_non_zero_work_area_origin() {
        let area = Rect {
            x: -1920,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let p = panel_position(buddy_at(-1800, 100), Some(area), PANEL_W, PANEL_H);
        assert_eq!(p, Point { x: -1800 + BUDDY, y: 100 });
    }
}
```

- [ ] **Step 2: Wire the module in**

Edit `src-tauri/core/src/lib.rs` — add alphabetically among the `pub mod` lines (after `pub mod checkpoint;`, before `pub mod crash;`):

```rust
pub mod companion_placement;
```

- [ ] **Step 3: Run the tests (fail first, then pass)**

Run: `cd src-tauri/core && cargo test companion_placement`
Expected before Step 1: FAIL (module not found). After Steps 1–2: **PASS** (7 tests).

- [ ] **Step 4: Lint**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/companion_placement.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): panel window placement relative to the buddy"
```

---

## Phase 2 — Shell: windows, commands, focus

> Shell tasks have no local build. After each, run `cd src-tauri && cargo fmt --check` and commit; CI's `windows-app` job is the compile gate.

### Task 2: Declare the panel and bubble windows

**Files:**
- Modify: `src-tauri/tauri.conf.json` (`app.windows`)

- [ ] **Step 1: Add the two hidden windows**

In `src-tauri/tauri.conf.json`, replace the single-element `app.windows` array with three entries (buddy unchanged, plus `panel` and `bubble`, both `visible: false`):

```json
"windows": [
  {
    "label": "main",
    "title": "Vault Buddy",
    "width": 88,
    "height": 88,
    "transparent": true,
    "decorations": false,
    "alwaysOnTop": true,
    "resizable": false,
    "skipTaskbar": true,
    "shadow": false
  },
  {
    "label": "panel",
    "title": "Vault Buddy Panel",
    "width": 360,
    "height": 340,
    "visible": false,
    "transparent": true,
    "decorations": false,
    "alwaysOnTop": true,
    "resizable": false,
    "skipTaskbar": true,
    "shadow": false,
    "focus": false
  },
  {
    "label": "bubble",
    "title": "Vault Buddy Greeting",
    "width": 260,
    "height": 150,
    "visible": false,
    "transparent": true,
    "decorations": false,
    "alwaysOnTop": true,
    "resizable": false,
    "skipTaskbar": true,
    "shadow": false,
    "focus": false
  }
]
```

All three load the app's `index.html` by default; `main.ts` (Task 7) renders the right root per label. Sizes are starting points — tune during the manual Windows check.

- [ ] **Step 2: Capabilities — grant the new windows the same core permissions**

In `src-tauri/capabilities/default.json`, change `"windows": ["main"]` to `"windows": ["main", "panel", "bubble"]` so all three inherit `core:default`, `log:default`, and the position getters the placement/close paths use. (The offset/geometry window permissions are removed in Task 6.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat(shell): declare hidden panel and bubble windows"
```

### Task 3: `toggle_panel` / `close_panel` commands

**Files:**
- Modify: `src-tauri/src/commands.rs` (add commands + `position_panel`)
- Modify: `src-tauri/src/lib.rs` (register in `invoke_handler`)

**Interfaces:**
- Consumes: `vault_buddy_core::companion_placement::{Rect, panel_position}` (Task 1)
- Produces: commands `toggle_panel(app)`, `close_panel(app)`; helper `position_panel(&AppHandle)`

- [ ] **Step 1: Add the commands to `commands.rs`**

Add near the other window commands:

```rust
/// Show/hide the panel window. A sync command, so it runs on the main thread
/// (where window show/hide and the placement getters are valid). Positioned
/// while still hidden, then shown — the panel window never resizes and is
/// only moved, so there is no WebView2 stale-frame flash. Opening hides the
/// greeting bubble.
#[tauri::command]
pub fn toggle_panel(app: tauri::AppHandle) {
    use tauri::Manager;
    let Some(panel) = app.get_webview_window("panel") else {
        log::warn!("toggle_panel: no panel window");
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
        return;
    }
    position_panel(&app);
    if let Err(e) = panel.show() {
        log::warn!("toggle_panel: show failed: {e}");
    }
    let _ = panel.set_focus();
    if let Some(bubble) = app.get_webview_window("bubble") {
        let _ = bubble.hide();
    }
}

/// Hide the panel window. Idempotent; called by Escape, drag start, a launched
/// vault action, and the updater.
#[tauri::command]
pub fn close_panel(app: tauri::AppHandle) {
    use tauri::Manager;
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
}

/// Move the (hidden) panel window beside the buddy, respecting screen edges.
/// Best-effort: any missing window/monitor info leaves the panel where it was.
pub(crate) fn position_panel(app: &tauri::AppHandle) {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{panel_position, Rect};
    let (Some(buddy), Some(panel)) = (
        app.get_webview_window("main"),
        app.get_webview_window("panel"),
    ) else {
        return;
    };
    let (Ok(bpos), Ok(bsize), Ok(psize)) =
        (buddy.outer_position(), buddy.outer_size(), panel.outer_size())
    else {
        return;
    };
    let buddy_rect = Rect {
        x: bpos.x,
        y: bpos.y,
        w: bsize.width as i32,
        h: bsize.height as i32,
    };
    // Monitor bounds as the work area. Tauri exposes monitor size/position;
    // the taskbar overlap is harmless here because a bottom-edge buddy bottom-
    // aligns the panel to the buddy (already above the taskbar).
    let work = buddy.current_monitor().ok().flatten().map(|m| {
        let p = m.position();
        let s = m.size();
        Rect {
            x: p.x,
            y: p.y,
            w: s.width as i32,
            h: s.height as i32,
        }
    });
    let point = panel_position(buddy_rect, work, psize.width as i32, psize.height as i32);
    if let Err(e) = panel.set_position(tauri::PhysicalPosition::new(point.x, point.y)) {
        log::warn!("position_panel: set_position failed: {e}");
    }
}
```

- [ ] **Step 2: Register the commands**

In `src-tauri/src/lib.rs` `invoke_handler`, add `commands::toggle_panel,` and `commands::close_panel,` and remove `commands::set_panel_offset,` and `commands::set_window_geometry,` (those are deleted in Task 6; if Task 6 runs later, leave them until then — but do not register the removed ones once deleted).

- [ ] **Step 3: fmt + commit**

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): toggle_panel/close_panel position the panel window"
```

### Task 4: Focus watcher — click-away closes the panel

**Files:**
- Modify: `src-tauri/src/lib.rs` (`on_window_event` + a debounced both-unfocused check)

- [ ] **Step 1: Add the focus watcher**

In `src-tauri/src/lib.rs`, extend the `on_window_event` match with a `Focused` arm that, when a window blurs, re-checks on the next main-thread turn whether *either* companion window (`main`/`panel`) is focused; if neither is, hide the panel. Add near the top-level statics:

```rust
/// Debounce the both-unfocused check: clicking from the panel to the buddy (or
/// back) briefly leaves neither focused before the other gains focus. Only
/// hide the panel once a full turn confirms focus really left the app.
fn schedule_focus_out_check(app: &tauri::AppHandle) {
    use tauri::Manager;
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        let focused = |label: &str| {
            app.get_webview_window(label)
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false)
        };
        if !focused("main") && !focused("panel") {
            if let Some(panel) = app.get_webview_window("panel") {
                if panel.is_visible().unwrap_or(false) {
                    let _ = panel.hide();
                }
            }
        }
    });
}
```

Then in the `on_window_event` match (which already handles `Moved` and `CloseRequested`), add:

```rust
tauri::WindowEvent::Focused(false) => schedule_focus_out_check(window.app_handle()),
```

Clicking the buddy while the panel is open keeps the app focused (buddy gains focus), so this does *not* hide it; the subsequent `toggle_panel` explicitly hides it — no reopen race.

- [ ] **Step 2: fmt + commit**

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/src/lib.rs
git commit -m "feat(shell): hide the panel when focus leaves the app"
```

### Task 5: Show the greeting bubble on launch

**Files:**
- Modify: `src-tauri/src/lib.rs` (`setup`: position + show the bubble)

- [ ] **Step 1: Show the bubble after setup**

In `src-tauri/src/lib.rs` `setup`, after the tray/recovery wiring, position and show the bubble beside the buddy (reusing the placement helper against the bubble window). Add a small helper in `commands.rs`:

```rust
/// Position + show the greeting bubble beside the buddy on launch. Best-effort.
pub(crate) fn show_bubble(app: &tauri::AppHandle) {
    use tauri::Manager;
    use vault_buddy_core::companion_placement::{panel_position, Rect};
    let (Some(buddy), Some(bubble)) = (
        app.get_webview_window("main"),
        app.get_webview_window("bubble"),
    ) else {
        return;
    };
    let (Ok(bpos), Ok(bsize), Ok(size)) =
        (buddy.outer_position(), buddy.outer_size(), bubble.outer_size())
    else {
        return;
    };
    let buddy_rect = Rect {
        x: bpos.x,
        y: bpos.y,
        w: bsize.width as i32,
        h: bsize.height as i32,
    };
    let work = buddy.current_monitor().ok().flatten().map(|m| {
        let p = m.position();
        let s = m.size();
        Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 }
    });
    let point = panel_position(buddy_rect, work, size.width as i32, size.height as i32);
    let _ = bubble.set_position(tauri::PhysicalPosition::new(point.x, point.y));
    let _ = bubble.show();
}
```

And call `commands::show_bubble(app.handle());` in `setup` (after `tray::create_tray`). The bubble window's own root (Task 10) auto-dismisses on a timer and calls `close_bubble` (add a trivial `close_bubble` command mirroring `close_panel` but hiding `"bubble"`; register it). `toggle_panel` already hides the bubble when the panel opens.

- [ ] **Step 2: fmt + commit**

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): greet with a separate bubble window on launch"
```

### Task 6: Remove the offset/shift machinery

**Files:**
- Modify: `src-tauri/src/commands.rs` (delete `PanelOffset`, `set_panel_offset`, `set_window_geometry`; simplify `prepare_update_install`)
- Modify: `src-tauri/src/lib.rs` (drop `PanelOffset` state + its use in `window_upkeep_tick`/`CloseRequested`; `prepare_update_install` calls `close_panel` instead of offset restore)
- Modify: `src-tauri/src/tray.rs` (delete `restore_home_position`; simplify `finish_quit`; `hide_buddy` also hides panel+bubble)
- Modify: `src-tauri/Cargo.toml` (drop the `Win32_UI_WindowsAndMessaging`/`Win32_Foundation` `windows-sys` features added for `set_window_geometry`; keep `Win32_UI_Input_KeyboardAndMouse` for `start_buddy_drag`)
- Modify: `src-tauri/capabilities/default.json` (remove `core:window:allow-set-size`, `allow-set-position` if no longer used by the frontend — the panel is positioned from Rust; keep `allow-outer-position`, `allow-current-monitor`, `allow-scale-factor` only if some frontend path still needs them, otherwise remove)

- [ ] **Step 1: Delete the offset state and commands**

In `commands.rs` remove the entire `PanelOffset` struct + impl, `set_panel_offset`, and `set_window_geometry` (and its `primary_button_down`-adjacent helpers stay — only the geometry function goes). In `prepare_update_install`, replace the `restore_home_position` + geometry save with:

```rust
#[tauri::command]
pub fn prepare_update_install(app: tauri::AppHandle) {
    use tauri_plugin_window_state::{AppHandleExt, StateFlags};
    // The buddy window never shifts, so there is no home position to restore —
    // just make sure the panel is closed and persist the buddy position.
    close_panel(app.clone());
    if let Err(e) = app.save_window_state(StateFlags::POSITION) {
        log::error!("update install: saving window state failed: {e}");
    }
    log::info!("clean shutdown (update install)");
    crate::diagnostics::mark_clean_shutdown();
}
```

- [ ] **Step 2: Drop `PanelOffset` from `lib.rs`**

Remove `.manage(commands::PanelOffset::default())`. In `window_upkeep_tick`, delete the `PanelOffset`-zero guard (the buddy position is always the home position now — always checkpoint it). In the `CloseRequested` else-branch, remove the `tray::restore_home_position(app)` call (keep the clean-shutdown stamp). Remove `commands::set_panel_offset`/`commands::set_window_geometry` from `invoke_handler`.

- [ ] **Step 3: Simplify `tray.rs`**

Delete `restore_home_position`. In `finish_quit`, drop the `restore_home_position(&app2)` call (keep the main-thread marshaled `save_window_state` + destroy + exit). Make `hide_buddy` also hide the panel and bubble:

```rust
pub fn hide_buddy(app: &AppHandle) {
    if crate::capture_commands::is_recording(app) {
        log::info!("hide ignored: recording in progress");
        return;
    }
    for label in ["panel", "bubble", "main"] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
}
```

- [ ] **Step 4: Trim Cargo features + capabilities**

`src-tauri/Cargo.toml`: set the windows-sys features back to just the drag guard:

```toml
windows-sys = { version = "0.61", features = ["Win32_UI_Input_KeyboardAndMouse"] }
```

Run `cargo update -w --offline` to sync `Cargo.lock`. In `capabilities/default.json`, remove `core:window:allow-set-size` and `core:window:allow-set-position` (the frontend no longer sets geometry). Keep `core:window:allow-start-dragging`? No — drags go through `start_buddy_drag`; leave it removed as today. Keep the position/monitor/scale getters only if a frontend path still calls them (after Task 12 the frontend does not — remove them then).

- [ ] **Step 5: fmt + commit**

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/src src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/capabilities/default.json
git commit -m "refactor(shell): remove the offset/shift geometry machinery"
```

---

## Phase 3 — Frontend: three roots

### Task 7: Mount a root per window label

**Files:**
- Modify: `src/main.ts`
- Create: `src/roots/BuddyRoot.vue`, `src/roots/PanelRoot.vue`, `src/roots/BubbleRoot.vue` (stubs here; filled in Tasks 8–10)
- Test: `tests/main-root.test.ts`

**Interfaces:**
- Produces: `rootFor(label: string)` — returns the component to mount for a window label.

- [ ] **Step 1: Write the failing test**

Create `tests/main-root.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { rootFor } from "../src/main";
import BuddyRoot from "../src/roots/BuddyRoot.vue";
import PanelRoot from "../src/roots/PanelRoot.vue";
import BubbleRoot from "../src/roots/BubbleRoot.vue";

describe("rootFor", () => {
  it("maps window labels to root components", () => {
    expect(rootFor("main")).toBe(BuddyRoot);
    expect(rootFor("panel")).toBe(PanelRoot);
    expect(rootFor("bubble")).toBe(BubbleRoot);
  });
  it("defaults an unknown label to the buddy", () => {
    expect(rootFor("whatever")).toBe(BuddyRoot);
  });
});
```

- [ ] **Step 2: Create stub roots**

Create each of `src/roots/BuddyRoot.vue`, `PanelRoot.vue`, `BubbleRoot.vue` with a minimal SFC so the import resolves:

```vue
<script setup lang="ts"></script>
<template>
  <div />
</template>
```

- [ ] **Step 3: Rewrite `main.ts` to branch on label**

```ts
import { createApp, type Component } from "vue";
import { createPinia } from "pinia";
import { getCurrentWindow } from "@tauri-apps/api/window";
import BuddyRoot from "./roots/BuddyRoot.vue";
import PanelRoot from "./roots/PanelRoot.vue";
import BubbleRoot from "./roots/BubbleRoot.vue";
import { initLogging, logError } from "./logging";
import "./style.css";

/** Which root component a given window label renders. */
export function rootFor(label: string): Component {
  if (label === "panel") return PanelRoot;
  if (label === "bubble") return BubbleRoot;
  return BuddyRoot; // "main" and any unexpected label
}

initLogging();

let label = "main";
try {
  label = getCurrentWindow().label;
} catch {
  // not under Tauri (dev/tests) — default to the buddy root
}

const app = createApp(rootFor(label));
app.config.errorHandler = (err, _instance, info) => {
  console.error(err);
  logError(`vue error (${info}): ${String(err)}`);
};
app.use(createPinia()).mount("#app");
```

- [ ] **Step 4: Run tests (fail → pass)**

Run: `npx vitest run tests/main-root.test.ts`
Expected before Steps 2–3: FAIL (imports missing). After: **PASS**.

- [ ] **Step 5: Commit**

```bash
git add src/main.ts src/roots tests/main-root.test.ts
git commit -m "feat(ui): mount a root component per window label"
```

### Task 8: BuddyRoot

**Files:**
- Modify: `src/roots/BuddyRoot.vue`
- Test: `tests/buddy-root.test.ts`

**Interfaces:**
- Consumes: `CompanionCharacter` (existing), `useSettingsStore`, `useCaptureStore`, commands `toggle_panel`/`start_buddy_drag`/`close_panel`/`show_buddy_menu`.

- [ ] **Step 1: Write the failing test**

Create `tests/buddy-root.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import BuddyRoot from "../src/roots/BuddyRoot.vue";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));

const calls: string[] = [];

describe("BuddyRoot", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    calls.length = 0;
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "start_buddy_drag") return true;
    });
  });
  afterEach(() => clearMocks());

  it("toggles the panel when the buddy is clicked", async () => {
    const wrapper = mount(BuddyRoot);
    await wrapper.find("button.buddy").trigger("click");
    expect(calls).toContain("toggle_panel");
  });

  it("closes the panel when a drag starts", async () => {
    const wrapper = mount(BuddyRoot);
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { buttons: 1, screenX: 90, screenY: 90 });
    await Promise.resolve();
    expect(calls).toContain("start_buddy_drag");
    expect(calls).toContain("close_panel");
  });
});
```

- [ ] **Step 2: Implement `BuddyRoot.vue`**

Reuse `CompanionCharacter` unchanged; BuddyRoot wires its events to commands (no App-level focus/geometry logic remains).

```vue
<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import CompanionCharacter from "../components/CompanionCharacter.vue";
import { useSettingsStore } from "../stores/settings";
import { useCaptureStore } from "../stores/capture";

const settings = useSettingsStore();
const capture = useCaptureStore();

function invokeQuiet(cmd: string, args?: Record<string, unknown>) {
  void invoke(cmd, args).catch(() => {
    // not under Tauri (tests) / best-effort window command
  });
}

function onToggle() {
  invokeQuiet("toggle_panel");
}
function onDragStart() {
  // a drag repositions the buddy — get the panel out of the way
  invokeQuiet("close_panel");
}

let unlistenAnimation: (() => void) | undefined;
let unlistenDragging: (() => void) | undefined;

onMounted(async () => {
  void capture.init();
  try {
    unlistenAnimation = await listen("buddy-toggle-animation", () =>
      settings.toggleAnimations(),
    );
    unlistenDragging = await listen("buddy-toggle-dragging", () =>
      settings.toggleDragging(),
    );
  } catch {
    // not under Tauri (tests)
  }
});
onUnmounted(() => {
  unlistenAnimation?.();
  unlistenDragging?.();
});
</script>

<template>
  <div class="flex h-screen w-screen items-start justify-start p-2">
    <CompanionCharacter
      :working="false"
      :animated="settings.animationsEnabled"
      :character="settings.character"
      :draggable="settings.draggingEnabled"
      :facing="settings.facing"
      :recording="capture.status === 'recording' || capture.status === 'saving'"
      :paused="capture.paused"
      @toggle="onToggle"
      @drag-start="onDragStart"
    />
  </div>
</template>
```

`CompanionCharacter` already emits `drag-start` and invokes `start_buddy_drag` itself; BuddyRoot's `onDragStart` adds the `close_panel`. The old `drag-cancelled` handling is gone (no blur suppression to retract).

- [ ] **Step 3: Run tests (fail → pass); Step 4: Commit**

Run: `npx vitest run tests/buddy-root.test.ts` → PASS.

```bash
git add src/roots/BuddyRoot.vue tests/buddy-root.test.ts
git commit -m "feat(ui): BuddyRoot renders the buddy and forwards input to commands"
```

### Task 9: PanelRoot

**Files:**
- Modify: `src/roots/PanelRoot.vue`
- Modify: `src/stores/vaults.ts` (drop `panelOpen`/`togglePanel`, add `refresh()`; `runAction` calls `close_panel`)
- Test: `tests/panel-root.test.ts`

**Interfaces:**
- Consumes: `ActionPanel` (existing), `useVaultsStore`, command `close_panel`.
- Produces: `useVaultsStore().refresh()` (re-runs discovery), replacing `togglePanel`.

- [ ] **Step 1: Update the vaults store**

In `src/stores/vaults.ts`: remove `panelOpen` from state and delete `togglePanel`. Rename the open-time refresh into `refresh()`:

```ts
async refresh() {
  this.showList();
  await this.loadVaults();
},
```

In `runAction`, replace `this.panelOpen = false;` with a `close_panel` invoke:

```ts
await invoke(command, { id: vaultId });
await invoke("close_panel").catch(() => {});
```

- [ ] **Step 2: Write the failing test**

Create `tests/panel-root.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import PanelRoot from "../src/roots/PanelRoot.vue";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

const calls: string[] = [];

describe("PanelRoot", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    calls.length = 0;
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_vaults") return [];
    });
  });
  afterEach(() => clearMocks());

  it("refreshes vaults on mount", async () => {
    mount(PanelRoot);
    await Promise.resolve();
    expect(calls).toContain("list_vaults");
  });

  it("closes the panel on Escape", async () => {
    mount(PanelRoot);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await Promise.resolve();
    expect(calls).toContain("close_panel");
  });
});
```

- [ ] **Step 3: Implement `PanelRoot.vue`**

```vue
<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import ActionPanel from "../components/ActionPanel.vue";
import { useVaultsStore } from "../stores/vaults";

const store = useVaultsStore();

function closePanel() {
  void invoke("close_panel").catch(() => {});
}
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") closePanel();
}
// Clicks on the transparent gutter around the panel card read as "clicked
// away" — close, like the old expanded-window gutter did.
function onGutterClick(event: MouseEvent) {
  if (event.target === event.currentTarget) closePanel();
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
  // Re-run discovery every time the window becomes visible so a user who just
  // launched Obsidian sees a fresh list. The window is shown on open; a
  // Focused(true) is the reliable "became visible" signal here.
  void store.refresh();
});
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div class="h-screen w-screen p-2" @click="onGutterClick">
    <ActionPanel />
  </div>
</template>
```

Note: refreshing on *every* show (not just mount) — add a `focus` listener in a follow-up if the window is reused across opens; on first cut, PanelRoot mounts once and refresh-on-mount plus refresh when the store is re-shown is sufficient. (The panel window persists; wire a `getCurrentWindow().onFocusChanged` → `store.refresh()` here to refresh on each open.)

- [ ] **Step 4: Run tests (fail → pass); Step 5: Commit**

Run: `npx vitest run tests/panel-root.test.ts tests/vaults-store.test.ts` → PASS (update `vaults-store.test.ts` to drop `togglePanel`/`panelOpen` assertions and cover `refresh()`).

```bash
git add src/roots/PanelRoot.vue src/stores/vaults.ts tests/panel-root.test.ts tests/vaults-store.test.ts
git commit -m "feat(ui): PanelRoot hosts the action panel with click-away close"
```

### Task 10: BubbleRoot

**Files:**
- Modify: `src/roots/BubbleRoot.vue`
- Test: `tests/bubble-root.test.ts`

**Interfaces:**
- Consumes: `SpeechBubble` (existing), `useGreeting` (existing), command `close_bubble`.

- [ ] **Step 1: Write the failing test**

Create `tests/bubble-root.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import BubbleRoot from "../src/roots/BubbleRoot.vue";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

describe("BubbleRoot", () => {
  beforeEach(() => {
    mockIPC(() => {});
  });
  afterEach(() => clearMocks());

  it("renders the greeting text", async () => {
    const wrapper = mount(BubbleRoot);
    await Promise.resolve();
    expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(true);
  });
});
```

- [ ] **Step 2: Implement `BubbleRoot.vue`**

```vue
<script setup lang="ts">
import { watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import SpeechBubble from "../components/SpeechBubble.vue";
import { useGreeting } from "../composables/useGreeting";

// The bubble window is shown by Rust on launch; useGreeting drives the text
// and the auto-dismiss timer. When it dismisses, hide the window.
const { bubbleVisible, bubbleText } = useGreeting();

watch(bubbleVisible, (visible) => {
  if (!visible) void invoke("close_bubble").catch(() => {});
});
</script>

<template>
  <div class="flex h-screen w-screen items-center p-2">
    <SpeechBubble :text="bubbleText" side="right" valign="down" />
  </div>
</template>
```

If `useGreeting` couples to panel state, adapt it to a standalone timer here (it currently exposes `bubbleVisible`/`bubbleText`/`dismiss`). The `side`/`valign` props only affect the bubble's tail direction; fixed `right`/`down` is fine for the first cut.

- [ ] **Step 3: Run test (fail → pass); Step 4: Commit**

```bash
git add src/roots/BubbleRoot.vue tests/bubble-root.test.ts
git commit -m "feat(ui): BubbleRoot renders the greeting in its own window"
```

### Task 11: Cross-window settings sync

**Files:**
- Modify: `src/stores/settings.ts` (add `syncFromStorage()` + a `storage` listener helper)
- Modify: `src/roots/BuddyRoot.vue` (install the listener)
- Test: `tests/settings-store.test.ts` (add a case)

**Interfaces:**
- Produces: `useSettingsStore().syncFromStorage()` — re-reads all settings from `localStorage`.

- [ ] **Step 1: Write the failing test**

Add to `tests/settings-store.test.ts`:

```ts
it("re-reads settings when localStorage changes in another window", () => {
  const store = useSettingsStore();
  expect(store.animationsEnabled).toBe(true);
  localStorage.setItem("vault-buddy.animations", "off");
  store.syncFromStorage();
  expect(store.animationsEnabled).toBe(false);
});
```

- [ ] **Step 2: Add `syncFromStorage`**

In `src/stores/settings.ts`, add an action that re-reads the same keys the state initializer uses:

```ts
syncFromStorage() {
  this.animationsEnabled = localStorage.getItem(ANIMATIONS_KEY) !== "off";
  this.draggingEnabled = localStorage.getItem(DRAGGING_KEY) !== "off";
  this.facing = localStorage.getItem(FACING_KEY) === "left" ? "left" : "right";
  this.character = getCharacter(localStorage.getItem(CHARACTER_KEY) ?? "").id;
},
```

- [ ] **Step 3: Install the listener in BuddyRoot**

In `BuddyRoot.vue` `onMounted`, add a `storage` listener so the buddy reflects character/animation changes made in the panel's settings view:

```ts
const onStorage = () => settings.syncFromStorage();
window.addEventListener("storage", onStorage);
```

and remove it in `onUnmounted` (`window.removeEventListener("storage", onStorage)`).

- [ ] **Step 4: Run tests (fail → pass); Step 5: Commit**

```bash
git add src/stores/settings.ts src/roots/BuddyRoot.vue tests/settings-store.test.ts
git commit -m "feat(ui): sync buddy settings across windows via storage events"
```

### Task 12: Delete the single-window frontend machinery

**Files:**
- Delete: `src/App.vue`, `src/composables/useCompanionWindow.ts`, `src/composables/companionPlacement.ts`
- Delete: `tests/companion-window.test.ts`, `tests/companion-placement.test.ts`, `tests/app-layout.test.ts`
- Modify: `tests/companion-character.test.ts` (drop the `drag-cancelled`/blur assertions that referenced App.vue; keep the gesture/threshold/`start_buddy_drag` coverage)

- [ ] **Step 1: Remove the files**

```bash
git rm src/App.vue src/composables/useCompanionWindow.ts src/composables/companionPlacement.ts \
       tests/companion-window.test.ts tests/companion-placement.test.ts tests/app-layout.test.ts
```

- [ ] **Step 2: Prune `companion-character.test.ts`**

Remove the `drag-cancelled` test and any assertion coupling to App.vue's blur suppression; keep: click→toggle, drag threshold→`start_buddy_drag`, stale-button drop, pointer capture, context menu, recording dot, disabled dragging. `CompanionCharacter` still emits `drag-start` and invokes `start_buddy_drag`; BuddyRoot (not App) now consumes `drag-start`.

- [ ] **Step 3: Verify the full suite + typecheck**

Run: `npm test` → all green. Run: `npm run build` → `vue-tsc` passes (no dangling imports of deleted modules). Fix any residual imports (e.g. a `SpeechBubble`/`ActionPanel` import that lived only in `App.vue`).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(ui): delete the single-window geometry, focus, and mask code"
```

---

## Phase 4 — Docs

### Task 13: Update AGENTS.md

**Files:**
- Modify: `AGENTS.md` (IPC surface + the window-geometry section)

- [ ] **Step 1: Rewrite the window section**

In `AGENTS.md`:
- IPC surface: remove `set_panel_offset`, `set_window_geometry`; add `toggle_panel`, `close_panel`, `close_bubble`.
- Replace the "window geometry system" section with the three-window model: buddy (`main`, fixed 88×88, only moves), `panel`/`bubble` (separate, positioned by `core::companion_placement::panel_position`, shown/hidden by Rust); focus-out watcher hides the panel; drag closes the panel; the frontend is three roots over one bundle coordinating via capture events + `localStorage`.
- Delete the offset/shift, `panelTransitionsSettled`, and atomic-`SetWindowPos` invariants (the code is gone). Keep the drag-crash invariants (metronome, main-thread saves, `start_buddy_drag`).

- [ ] **Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "docs: describe the three-window buddy/panel architecture"
```

---

## Self-Review Notes (coverage against the spec)

- Windows table → Tasks 2, 5. Placement (core) → Task 1; used by Tasks 3, 5. `toggle_panel`/`close_panel`/`close_bubble` → Tasks 3, 5. Focus watcher → Task 4. Frontend three roots → Tasks 7–10. Cross-window state: capture events (existing, used in Tasks 8–9), settings/`localStorage` → Task 11, panel open/close owned by Rust → Tasks 3–4, 9. Deletions → Tasks 6, 12. Docs → Task 13.
- **Manual Windows check (not automatable here):** open the panel with the buddy at each of the four screen-edge cases and confirm (a) no flash, (b) the panel lands beside the buddy on-screen, (c) click-away / Escape / drag all close it, (d) the greeting shows on launch and dismisses.
- **Known follow-ups deferred:** refreshing the vault list on every panel *re-open* (Task 9 note — wire `onFocusChanged`), and tuning the panel/bubble pixel sizes during the manual check.
