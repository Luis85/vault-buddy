# Buddy / Panel Window Split — Design

- **Date:** 2026-07-05
- **Status:** Approved for implementation
- **Supersedes:** the panel-open flicker fixes (`fix(ui): mask the buddy…`
  and the atomic-`SetWindowPos` change) — both become unnecessary and are
  removed by this increment.

## Goal

Opening the panel near a screen edge flashes the buddy to the corner of the
grown window for a frame. The root cause is structural: the buddy lives in
the window that resizes, and WebView2 re-presents its last-painted frame at
the new bounds until the webview reflows (an upstream wry/WebView2 resize
race with no anchor/defer knob). Every previous fix worked *around* the
resize.

This increment removes the cause instead: **the buddy gets its own window
that only ever moves, never resizes**, so it is structurally flicker-proof
in every state. The panel and the greeting bubble become their own windows,
positioned beside the buddy and shown/hidden — never resized either.

Decided during brainstorming:

- The greeting bubble also leaves the buddy window (buddy is a fixed 88×88 in
  every state).
- Dragging the buddy **closes** the panel (a drag is "move the widget", a
  distinct gesture) — which also deletes the drag-vs-panel focus coordination
  that caused earlier bugs.

## Architecture

### Windows (shell)

Three windows, all transparent / undecorated / always-on-top /
`skipTaskbar` / non-resizable / no shadow. The panel and bubble are created
at startup **hidden** so showing them is instant (no window-creation flash).

| Window | Label | Size | Visibility | Position |
| --- | --- | --- | --- | --- |
| Buddy | `main` | 88×88 fixed | always (unless tray-hidden) | user-dragged, persisted via `tauri-plugin-window-state` (POSITION only, as today) |
| Panel | `panel` | fixed (panel card size, ~360×340) | hidden → shown on open | computed beside the buddy on each open |
| Bubble | `bubble` | fixed (~260×150) | hidden → shown on launch | computed beside the buddy |

The buddy window keeps label `main` so the persisted window-state key and
existing single-instance/tray wiring are unchanged.

**Rust owns panel/bubble visibility and placement.** Position-before-show
plus never-resize is what makes the flash impossible: a hidden window is
moved to its computed spot, then shown already-correct.

### Placement (core crate)

The "which side / which alignment" logic that lives in
`companionPlacement.ts` today moves into the **core crate** as a pure
function, unit-tested on Linux:

```
panel_position(buddy_rect, monitor_work_area, panel_size) -> Point
```

- Prefer opening to the **right** of the buddy; near the right edge, open to
  the **left**.
- Prefer **top-aligned** with the buddy; near the bottom edge, **bottom-align**
  so the panel unfolds upward.
- Never place the panel off the work area.

This is the same edge logic as `planPanelPlacement` today, but it now returns
an absolute window position for a *separate* window instead of an offset for
growing one window. (`companionPlacement.ts` and its tests are ported to the
core crate; the TS copy is removed.)

### Commands (shell)

- `toggle_panel` — if the panel window is visible, hide it; else compute the
  placement from the buddy's current position + monitor, position the panel
  window, show and focus it, and hide the bubble.
- `close_panel` — hide the panel window (idempotent). Called by Escape, drag
  start, a launched vault action, and the updater.
- `open_vault` / `open_daily_note` stay; on success they call `close_panel`.

`set_panel_offset`, `set_window_geometry`, and `prepare_update_install`'s
offset-restore are **removed** (see Deletions).

### Focus / close (shell-centralized)

"Click away closes the panel" is the one genuinely multi-window concern, so
Rust — the only side that sees focus across windows — owns it:

- Rust listens to `WindowEvent::Focused` on `main` and `panel`. When **both**
  are unfocused (focus left the app — desktop or another app), it hides the
  panel. A one-tick debounce (re-check on the next event-loop iteration)
  absorbs the instant during a buddy↔panel click when neither is focused yet.
- Clicking the buddy while the panel is open: the buddy window takes focus
  (app still focused → the watcher does **not** hide), then `toggle_panel`
  runs and explicitly hides it — no reopen race.
- `Escape` in the panel and drag start on the buddy call `close_panel`.

This replaces the single-window `onFocusChanged` close logic in `App.vue`
and the entire drag-blur-suppression mechanism.

### Frontend (one bundle, three roots)

A single Vite build. `main.ts` branches on `getCurrentWindow().label` and
mounts the matching root component (each window loads the same
`index.html`):

- **BuddyRoot** — `CompanionCharacter` only (character + recording dot).
  Nearly stateless: reads character/animation/facing/dragging from
  `localStorage` (settings store), shows the dot from capture events,
  forwards `click → toggle_panel`, `drag → start_buddy_drag + close_panel`,
  `right-click → show_buddy_menu`, and handles the `buddy-toggle-animation` /
  `buddy-toggle-dragging` menu events (writing the setting → `localStorage`).
- **PanelRoot** — today's `ActionPanel` and its stores (`vaults`, `capture`,
  `updates`, `settings`) essentially unchanged. Adds: `Escape → close_panel`,
  and a vault-launch still sets `panelOpen = false`-equivalent by calling
  `close_panel`.
- **BubbleRoot** — the `SpeechBubble` + greeting text (the `useGreeting`
  logic).

Under happy-dom, `getCurrentWindow().label` is mocked; tests mount roots
directly.

### Cross-window state coordination

Only three things are shared, each over a channel that already exists:

- **Capture status** (buddy's dot ↔ panel's RecordingBar / capture settings):
  Rust already emits capture events to all windows; both roots subscribe
  independently. No change to the capture domain.
- **Settings** (character / animation / facing / dragging): `localStorage`
  is per-origin and both windows share `tauri://localhost`, so a `storage`
  event fires in the other window whenever one mutates it — the buddy updates
  live when the panel's settings view changes the character. The settings
  store gains a `storage`-event listener that re-reads on external change.
- **Panel open/close**: owned by Rust window visibility — the `vaults` store's
  `panelOpen` flag is **removed** (Rust visibility is the sole source of
  truth). `togglePanel`'s vault-refresh-on-open moves to PanelRoot: it
  re-runs discovery whenever the panel window becomes visible (a Tauri
  `show`/focus listener), so a user who just launched Obsidian still gets a
  fresh list.

## Data flow

Buddy click → `toggle_panel` (Rust) → compute placement from `main` position
+ monitor → position `panel` window → show + focus, hide `bubble`. Panel
interaction stays within the app (focus on `panel`). Click desktop → both
windows blur → focus watcher hides `panel`. Drag buddy → `start_buddy_drag`
+ `close_panel`. Settings change in panel → `localStorage` write → `storage`
event → buddy re-reads. Capture state change → Rust event → both roots.

## Deletions (net simplification)

Because the buddy window never resizes or shifts:

- `PanelOffset`, `set_panel_offset`, `tray::restore_home_position`, and the
  offset/shift math in `useCompanionWindow` — removed. Window-state saves the
  buddy position directly; no unshift on any exit path.
- `set_window_geometry` (the atomic-`SetWindowPos` command) and the
  `windows-sys` `Win32_UI_WindowsAndMessaging` / `Win32_Foundation` features
  added for it — removed. Panel placement uses `set_position` on a
  non-resizing window (no stale-bitmap concern when only moving).
- `panelTransitionsSettled` + the transition queue, and the whole
  grow/collapse state machine in `useCompanionWindow` — removed. The updater
  calls `close_panel` before install instead of awaiting a settle.
- The drag-blur suppression in `App.vue` (`dragBlurPending`,
  `DRAG_CLOSE_SUPPRESS_MS`, `drag-cancelled`) and the flicker mask
  (`maskBuddy`, `afterPaint`) — removed.

Kept unchanged: the drag-crash fix (the buddy still drags — the metronome,
main-thread window-state saves, and `start_buddy_drag` with its stale-request
guard), the capture domain, tray, updater, and diagnostics. The metronome
now re-asserts always-on-top for whichever companion windows are visible and
still checkpoints the buddy position.

## Error handling

- Placement with no monitor info degrades to "right of the buddy, top-aligned"
  (the current fallback), clamped to the work area if known.
- `toggle_panel` / `close_panel` are best-effort and idempotent; a failed
  show/hide logs via the existing diagnostics funnel, never throws to the UI.
- A window that fails to create at startup logs and degrades (e.g. no bubble)
  rather than aborting the app; the buddy window is required.
- Cross-window `storage`/event listeners are `try`/`catch`-guarded and no-op
  outside Tauri, per `src/logging.ts` conventions.

## Testing

- **Core (Linux CI):** `panel_position` — right/left choice at the right
  edge, top/bottom alignment at the bottom edge, corner (both), and the
  no-monitor fallback; round-trip against the ported `companionPlacement`
  cases.
- **Frontend (Vitest):** BuddyRoot renders the character + dot from settings
  + capture events and forwards click/drag/menu to the right commands;
  PanelRoot keeps the existing `ActionPanel` / store coverage; BubbleRoot
  renders the greeting; the settings `storage`-event sync updates the buddy.
  Window-label branching in `main.ts` is covered by mocking the label.
- **Shell (Windows-only compile gate + manual check):** window creation, the
  focus watcher (both-unfocused hides the panel; buddy click does not),
  `toggle_panel` placement near each edge. Mirror existing shell patterns;
  `cargo fmt --check`; CI's `windows-app` job is the compile gate; the visual
  result (no flash, panel positions correctly at all four edge cases) needs a
  manual check on Windows.

## Out of scope

- Multi-monitor panel placement beyond the buddy's current monitor.
- Animating the panel open/close (it shows/hides instantly, as today).
- Reworking the capture, updater, or tray domains beyond the command/close
  wiring above.
