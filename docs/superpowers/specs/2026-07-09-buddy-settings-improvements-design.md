# Buddy Settings Improvements Design — grouped layout, character preview, message duration, autostart

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** User request to improve the Buddy settings view (the panel's cog
  view), following the Record-view improvements. Three approved directions:
  visual/layout polish, character-picker UX, and two new settings —
  **Start with Windows** and **Message duration**.

## Goals

1. **Group the view into consistent carded sections** — the three bare
   toggles float today between the character grid and the carded
   Updates/Diagnostics sections.
2. **Preview a character before picking it** — hover/focus plays the
   character in motion; the selected card reads instantly.
3. **Message duration** — how long the buddy's speech bubbles stay up
   (short / normal / long).
4. **Start with Windows** — launch Vault Buddy at login (tauri
   autostart plugin).

## Layout (`BuddySettings.vue`)

Order: **Buddy character → Behavior → System → Updates → Diagnostics.**
"Behavior" and "System" get the same section-header + `rounded-xl border
border-white/10 bg-white/5 p-2` card treatment Updates and Diagnostics
already use, so the whole view speaks one visual language.

- **Behavior card:** the existing Animations, Dragging, and Buddy messages
  toggle rows, plus the new Message duration row (below).
- **System card:** the Start with Windows toggle row (below).
- Updates and Diagnostics are unchanged.

## Character picker UX

The 3-column radiogroup grid stays; two upgrades:

- **Motion preview on hover/focus.** While a card is hovered (`pointerenter`/
  `pointerleave`) or focused (`focusin`/`focusout`), its avatar gets
  `working: true` — sprite characters play their run loop, the classic buddy
  pulses (BuddyAvatar's existing `working` behavior; no new animation code).
  Gated on `settings.animationsEnabled` so animations-off also silences
  previews (BuddyAvatar's `.still` would freeze them anyway — the gate keeps
  the semantics honest). Pointer state must clear on leave, so a card never
  sticks in preview.
- **Selected badge.** The selected card (violet border today) additionally
  shows a small ✓ badge in its top-right corner (`aria-hidden` — selection is
  already announced via `aria-checked`; cards become `relative` to anchor it).

## Message duration

- **Setting:** `messageDuration: "short" | "normal" | "long"`, default
  `"normal"` (today's exact timings). Persisted as
  `vault-buddy.messageDuration` in localStorage; unknown stored values fall
  back to `"normal"` (the `getCharacter` normalization pattern). The settings
  store gains the state, a `setMessageDuration` action, and a
  `syncFromStorage` extension.
- **Timings:** one pure exported map in `useBuddyBubble.ts` (unit-tested),
  replacing the bare `ACK_MS`/`GREETING_MS` constants:

  | tier | ack bubble | launch greeting |
  | --- | --- | --- |
  | short | 2000 ms | 3000 ms |
  | normal | 3200 ms | 5000 ms |
  | long | 6000 ms | 9000 ms |

  Normal preserves today's values, so the default behavior is unchanged.
- **Wiring:** durations are resolved from the settings store **at show time**
  — `useBuddyBubble` reads it for the mount greeting, `BubbleRoot` for each
  `bubble-message` ack. `BubbleRoot` also installs `useSettingsStorageSync`
  (it doesn't today — the bubble is its own webview with its own Pinia, so
  without the sync a duration picked in the panel would never reach the live
  bubble window). AGENTS.md's frontend-state paragraph is updated to include
  the bubble root among the settings-sync installers.
- **UI:** a "Message duration" row in the Behavior card — label + hint
  ("How long the buddy's bubbles stay up") and a `SelectMenu`
  (Short / Normal / Long), the CaptureSettings bitrate-row pattern.

## Start with Windows (autostart)

- **Rust:** add `tauri-plugin-autostart = "2"` and register
  `tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None)` with the
  other plugins — **after** `tauri-plugin-single-instance`, which must stay
  first (repo invariant). Two thin custom commands in `commands.rs`, wrapping
  the plugin's Rust API (`app.autolaunch()`):
  - `get_autostart() -> Result<bool, String>` (`is_enabled`)
  - `set_autostart(enabled: bool) -> Result<(), String>` (`enable`/`disable`,
    `log::info!` on success — state changes are audit-logged like capture
    config saves)

  Custom commands (not the JS guest binding) keep the IPC surface pattern
  consistent and skip the npm package + capability entries plugin commands
  would need. Both registered in `generate_handler!`; AGENTS.md's IPC list
  gains them. The `auto-launch` crate is cross-platform, so the Linux
  compile gate covers the build; real login behavior is Windows-verified.
- **Frontend (System card row):** "Start with Windows" toggle with hint
  ("Launch the buddy when you log in"). State is **OS-owned** — read via
  `get_autostart` on mount (the UpdateSettings load-on-mount pattern), never
  stored in localStorage or the settings store. Until the read resolves (or
  if it fails) the checkbox is disabled; a read failure shows an inline
  error. Toggling is optimistic with revert-on-failure + inline error +
  `logWarning` (the Tasks-toggle pattern), and the checkbox is disabled while
  a write is in flight so toggles can't race.

## Testing

- **Store:** `messageDuration` defaults to normal, persists, survives
  `syncFromStorage`, normalizes unknown stored values.
- **Durations:** the tier map is exported and pure; greeting and ack shows
  use the configured tier (bubble/greeting tests updated — they currently
  assert the bare constants).
- **`BuddySettings.vue`:** Behavior/System cards render with their rows;
  duration select persists the setting; hover/focus sets the avatar preview
  and clears on leave; animations-off suppresses preview; selected card
  shows the ✓ badge; autostart row loads OS state via `get_autostart`
  (mockIPC), toggle invokes `set_autostart`, failure reverts the checkbox
  and shows the inline error; disabled while in flight.
- **Rust:** the two commands are thin plugin wrappers with no core-crate
  logic — covered by the Linux compile gate (`npx tauri build --no-bundle`)
  and the Windows CI build.

## Out of scope

Buddy size (would touch the fixed-88×88 window invariant), a startup-greeting
on/off toggle, new characters, tray-menu parity for the new settings, and any
Updates/Diagnostics changes.
