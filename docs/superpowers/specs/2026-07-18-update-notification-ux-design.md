# Update-notification UX — dedicated view + clickable bubble — design

Date: 2026-07-18
Status: accepted (user request: "when there is an update and the buddy
informs me about it, the update path should be more user-friendly. Right
now it opens the first tab of the settings when I click it, it should open
a dedicated update view instead. The bubble should also indicate to be
hoverable if the bubble has a click action attached")

## Problem

The startup update check (`useStartupUpdateCheck`, PanelRoot) announces an
available update through the buddy's speech bubble with copy that literally
says *"Update v… is ready — click me! ⬆️"* (`updateAvailableMessage`) and
arms `requestViewOnNextOpen("settings")`. Two things make that promise
false today:

1. **The bubble isn't clickable.** It is a separate always-on-top window
   (`bubble`) that only renders text and auto-dismisses; there is no click
   handler and no interactive affordance. "Click me!" can only be satisfied
   by clicking the *buddy character*, which opens the panel — an
   indirection the copy doesn't describe.
2. **The destination is the whole Buddy-settings page.** Opening the panel
   lands on `view: "settings"` (`BuddySettings.vue`), where the Updates
   section (`UpdateSettings.vue`) is one card among character/animation/
   messages/integrations. The user has to scroll to find the update they
   were just told about.

Requested: a **dedicated update view**, and a **bubble that is genuinely
clickable and looks it** when (and only when) it carries a click action.

## Design

### New panel view: `update` (`UpdateView.vue`)

A focused view rendered by `ActionPanel` when `view === "update"`, backed
by the existing `updates` store — no new store, self-contained like
`Recordings.vue`. Content, in the "show what's new" spirit:

- The available version, with current → new framing and the release
  `date` when present.
- **Release notes** from `updates.available?.body`, rendered as **plain
  preformatted text** in a scrollable region (`whitespace-pre-wrap`), with
  a graceful *"No release notes provided."* fallback. Deliberately NOT a
  markdown render and NOT `v-html`: `body` is release-controlled content,
  but plain-text rendering sidesteps injection entirely and adds no
  markdown-renderer dependency (local-first, light-installer principle).
- **Install & restart** — reuses `updates.installUpdate()` unchanged,
  including the in-button installing spinner and the inline error/retry
  state (`showInstall` gate: `available | installing | error-with-available`,
  the same computed `UpdateSettings` uses today).
- A friendly empty state (*"No update is available right now."*) for the
  defensive case of reaching the view with `available === null`; the real
  entry points below all imply an available (or just-failed) update.

The view is title-barred "Update" (`VIEW_TITLES.update`) and gets the ←
back button every non-list view already renders; `back()` falls through to
the vault list (no new case needed).

### View + routing state (`vaults` store)

- Add `"update"` to the `view` union and to the `pendingView` union.
- Add `openUpdate()` (`view = "update"`).
- Extend `requestView` / `requestViewOnNextOpen` parameter types to include
  `"update"`. `refresh()` already applies `pendingView` verbatim, so no
  branch change there; the buddy-drop / add-document drains run first and
  are inert for the update path.
- `useStartupUpdateCheck` arms `requestViewOnNextOpen("update")` (was
  `"settings"`).
- The failed-install reopen in `updates.ts` uses `requestView("update")`
  (was `"settings"`) so the retry/error UI reopens on the focused view.

### Buddy-settings Updates section stays, links to the view

`UpdateSettings.vue` keeps "Check for updates" and the "Check on startup"
toggle. When an update is available, its inline **Install** button becomes a
**"View update →"** button calling `store.openUpdate()`. One install path
(only `UpdateView` installs), no duplicated button; manual checking still
lives where users look for it.

### Clickable bubble (cross-window routing)

The bubble window and the panel window are separate webviews with separate
Pinia stores, so the click must reach the panel through Rust. Chosen
mechanism — **reuse the armed `pendingView` + a thin idempotent
`open_panel` command**:

- `announce(text, action?)` (frontend `announce.ts`) gains an optional
  action. Rust `announce` gains `action: Option<String>`; the
  `bubble-message` emit carries it. Every existing caller passes no action
  and is unchanged; only the update announcement passes `"openUpdate"`.
- `useBuddyBubble.show(message, durationMs, action?)` tracks a current
  `action` ref alongside `text`, cleared on `dismiss` — latest-wins, same
  as the text, so a later greeting/ack that carries no action makes the
  bubble non-clickable again.
- `SpeechBubble` gains a `clickable` prop (derived from action presence by
  `BubbleRoot`) and emits `@click`. When clickable it shows a **pointer
  cursor, a persistent subtle interactive treatment, and a stronger hover
  lift**. Persistent (not hover-only) because the bubble auto-dismisses in
  a few seconds — a hover-only cue is easy to miss. Keyboard-activatable
  with an accessible label; non-clickable bubbles (greeting, acks) keep the
  default cursor and no handler.
- `BubbleRoot` maps the action on click: `"openUpdate"` → invoke
  `open_panel` (best-effort, `.catch()` like its other bubble commands) →
  `dismiss()`.
- `open_panel` (new Rust command) wraps the existing internal
  `commands::show_panel`: position-while-hidden → show → focus → emit
  `panel-shown`. It is **idempotent** — safe on an already-open panel (a
  harmless re-show/re-focus/re-emit; the panel only ever moves, never
  resizes) — and always emits `panel-shown`, so the panel's `refresh()`
  runs and consumes `pendingView="update"` whether the panel was closed or
  already open. Registered in `lib.rs` `generate_handler`; no capability
  change (custom commands aren't capability-gated, and the bubble window
  already invokes custom commands like `close_bubble`).

Why not the alternatives — see below.

### Data flow (after the change)

1. Startup check finds an update → `announce(updateAvailableMessage(v),
   "openUpdate")` + `requestViewOnNextOpen("update")`.
2. Rust `announce` → `show_bubble` (beside the buddy) → emit
   `bubble-message {text, action:"openUpdate"}`.
3. `BubbleRoot` shows it; `SpeechBubble` renders the clickable treatment.
4. **Bubble click** → `open_panel` → `panel-shown` → `refresh()` consumes
   `pendingView="update"` → `UpdateView`; bubble dismisses. **Or buddy
   click** → `toggle_panel` → same refresh → `UpdateView`.
5. Install & restart → `updates.installUpdate()` (unchanged). On failure →
   `requestView("update")` + reopen → `UpdateView` shows error/retry.

Manual path: Buddy settings → Updates → Check → "View update →" →
`openUpdate()` → `UpdateView`.

## Alternatives considered

- **Rust-owned stash for the bubble→panel intent** (mirroring
  `begin_document_import` / `take_pending_import`): rejected as heavier than
  warranted — a stash field + a drain command + refresh wiring for a single
  action, when the announcing code already arms `pendingView` and
  `open_panel`'s idempotent `panel-shown` makes reusing it robust for both
  panel-open and panel-closed states.
- **Frontend event from the bubble to the panel** (`reveal-update`) that
  sets the panel's own view: rejected — the frontend can't reliably *show*
  the panel window (window show is a main-thread Rust concern), so it would
  still need a Rust command, and it splits show + route across two
  channels; it also races the `panel-shown` refresh, the same reason
  `begin_document_import` is a stash rather than an emit.
- **Reuse `toggle_panel` for the bubble click** instead of a new
  `open_panel`: rejected — `toggle_panel` HIDES an already-open panel, and
  the update bubble *can* be shown while the panel is open (`show_bubble`
  gates on the buddy being visible, not the panel), so a click would then
  hide the panel instead of routing it. `open_panel` (idempotent show) is
  correct in both states.
- **Move all update UI into the new view, shrinking settings to a link**:
  rejected by the product decision — manual "Check for updates" and the
  startup toggle stay discoverable in Buddy settings.

## Testing

Vitest (happy-dom + `mockIPC`), matching the repo's frontend conventions:

- `UpdateView`: renders version + release notes (and the "no notes"
  fallback); Install triggers `updates.installUpdate()`; installing/error/
  empty states; back button returns to the list.
- `vaults` store: `openUpdate()` sets the view; `requestView("update")` and
  `requestViewOnNextOpen("update")` round-trip through `refresh()`; `back()`
  from `update` → list.
- `SpeechBubble`: clickable prop → pointer/interactive class + `@click`
  emitted; non-clickable → no pointer, no emit.
- `BubbleRoot`: a `bubble-message` with `action:"openUpdate"` makes the
  bubble clickable and a click invokes `open_panel` + dismisses; a message
  without an action is not clickable.
- `useStartupUpdateCheck`: arms `requestViewOnNextOpen("update")` on an
  available update (adjust the existing assertion from `"settings"`).
- `updates.ts`: a failed install requests the `"update"` view.
- Note (GAP): `UpdateSettings.vue` is only indirectly tested today; the
  "View update →" branch will be covered where feasible.

Rust: `open_panel` is a thin window command (like `show_panel`,
`toggle_panel`) — no unit test; covered by the Linux compile gate
(`npx tauri build --no-bundle`) + `cargo fmt --check`, with CI's
`windows-app` as the desktop-behavior gate. The `announce` signature change
is a compile-gate concern; the emit-shape change is exercised by the
`BubbleRoot` frontend test.

## Docs to update on landing

`AGENTS.md`: the IPC command table (add `open_panel`; note `announce`'s
`action` param), the Events table (`bubble-message` now carries an optional
`action`), the Updater-flow section (dedicated update view + clickable
bubble), and the frontend-state view union list. `docs/Gaps.md` if the
`UpdateSettings` coverage note warrants an entry.

## Out of scope

- Rendering release notes as formatted markdown (plain text only this
  slice).
- Making non-update bubbles actionable, or a general multi-action bubble
  framework (one action, `"openUpdate"`; the `action` field leaves room but
  YAGNI otherwise).
- Any change to the download/install/relaunch mechanics or the updater feed.
