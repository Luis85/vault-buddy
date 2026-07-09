---
type: UseCase
status: shipped
domain: desktop-companion
shipped_in: v0.3.0
source_prd: "docs/PRD - Product Vision.md"
related_specs:
  - "docs/superpowers/specs/2026-07-03-increment-1-companion-daily-note-design.md"
  - "docs/superpowers/specs/2026-07-04-increment-3-greeting-speech-bubble-design.md"
  - "docs/superpowers/specs/2026-07-04-rpg-characters-settings-design.md"
  - "docs/superpowers/specs/2026-07-04-drag-crash-diagnostics-logging-design.md"
  - "docs/superpowers/specs/2026-07-05-buddy-panel-window-split-design.md"
  - "docs/superpowers/specs/2026-07-05-buddy-acknowledgement-messages-design.md"
tags: [use-case, desktop-companion, foundation]
---

# Desktop Companion

> A small, always-on-top animated character lives on the Windows desktop; clicking it opens a panel of actions. It never resizes, never blocks other windows, and reacts to where it sits on screen.

## Source

Main PRD, [§10 User Experience Vision](../PRD%20-%20Product%20Vision.md) and [§11 Core Capabilities → Desktop Companion](../PRD%20-%20Product%20Vision.md): animated character, emotional states, drag & drop, transparent window, always-on-top, multi-monitor, system tray, startup with Windows. Listed as shipped in Phase 1 — Foundation.

## Status: Shipped (v0.3.0, refined through v0.5.0)

## Implementation

Three separate always-on-top transparent windows (`main`/buddy, `panel`, `bubble`) instead of one resizing window — see [AGENTS.md § The window system](../../AGENTS.md) for the full invariant set. Key surfaces:

- Rust commands: `toggle_panel`, `close_panel`, `close_bubble`, `get_buddy_facing`, `get_bubble_anchor`, `start_buddy_drag`, `show_buddy_menu`, `announce`, `open_logs_folder`, `rearm_crash_detection` (`src-tauri/src/commands.rs`).
- Placement logic: `core::companion_placement::place_beside` (unit-tested), tray chokepoint `tray::hide_buddy`.
- Frontend: `src/roots/{BuddyRoot,PanelRoot,BubbleRoot}.vue`, `CompanionCharacter.vue`, `SpeechBubble.vue`, character/animation selection persisted in the `settings` Pinia store (localStorage, synced cross-window via `useSettingsStorageSync`).
- Single-instance enforcement via `tauri-plugin-single-instance`.

## Not covered by this note

Global hotkeys (listed in the PRD's Desktop Companion bullet list) have no corresponding IPC command or keybinding registration found in `src-tauri/src/lib.rs` — treat as **not yet shipped**, tracked implicitly by the PRD bullet rather than a dedicated spec.

## Related use-cases

- [Software Auto-Update](software-auto-update.md) — reuses the buddy's window lifecycle (`prepare_update_install`) to relaunch cleanly.
