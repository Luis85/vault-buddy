# Check for Updates on Start Design — quiet startup check, buddy asks, zero trace when current

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** User request: the buddy self-checks for updates on startup
  (own settings toggle); if an update exists it *asks* whether to run it;
  if not, the user notices nothing. Approved choices: bubble announcement +
  next panel open lands on settings (no auto-opened panel), toggle **on**
  by default.

## Goals

1. A **"Check on startup" toggle** in the Updates card (default on).
2. A **quiet startup check** that leaves zero user-visible trace when the
   app is current or the check fails.
3. When an update exists, the **buddy asks**: a speech-bubble announcement,
   and the next panel open lands on the settings view where the existing
   Install & restart button is the answer.

## Setting (`src/stores/settings.ts`)

`checkUpdatesOnStart: boolean`, default **true**, persisted as
`vault-buddy.checkUpdatesOnStart` with the `!== "off"` read used by the
other boolean settings; `toggleCheckUpdatesOnStart()` action;
`syncFromStorage()` re-reads it. Rendered as a toggle row inside the
existing Updates card (`UpdateSettings.vue`), under the version row:
label "Check on startup", hint "Asks before installing · silent when up
to date", `data-testid="update-on-start-toggle"`.

## Quiet check (`src/stores/updates.ts`)

New action `checkForUpdatesQuietly()` beside `checkForUpdates()`:

- Guard: no-op unless `phase === "idle"` (never fight a manual check or a
  running install).
- **Update found** → `available = markRaw(update)`, `phase = "available"`
  — identical to the manual path, so the settings view's existing
  available/install UI works unchanged.
- **Up to date** → phase stays `idle` (NOT `upToDate`): the button-driven
  "You're up to date." line is a response to a user action; the background
  check must leave no trace.
- **Failure** → phase stays `idle`, `error` stays null, `logWarning` only
  (silent per the feature, logged per the no-swallowed-errors rule).

## The ask (`src/composables/useStartupUpdateCheck.ts` + `PanelRoot.vue`)

A composable installed by **PanelRoot only** — the panel webview mounts
hidden exactly once at app start, owns the updates UI, and is already an
announcer (vault opens). On mount, when `settings.checkUpdatesOnStart`:

- Wait `STARTUP_CHECK_DELAY_MS = 15_000` (exported for tests): with
  autostart, the check can race login networking; a silent failure would
  waste the session's one shot.
- Run `updates.checkForUpdatesQuietly()`.
- If it produced `phase === "available"`:
  - `announce(updateAvailableMessage(version))` — a new
    `buddyMessages.ts` helper in the house voice:
    `Update v${version} is ready — click me! ⬆️` (generic
    "An update is ready — click me! ⬆️" when the version string is
    empty). `announce()` already respects the Buddy-messages toggle; a
    messages-off user still gets the pending settings view below.
  - Arm the panel so its **next open lands on settings** via a new
    `vaults` store action `requestViewOnNextOpen(view)`: sets only
    `pendingView` (+ clears `pendingCaptureVaultId`), unlike
    `requestView` which also flips the live `view` — an already-open
    panel must not be yanked to settings mid-task.
- The delay timer is cleared on unmount (prod panel never unmounts;
  tests do).

## Not changing

No Rust changes. No auto-download or auto-install (the check is
metadata-only; installing stays behind the user's click). No periodic
re-checks while running. `checkForUpdates()` (manual button) unchanged.

## Testing

- **settings store:** default on; toggle persists; `syncFromStorage`
  re-reads.
- **updates store** (mock `@tauri-apps/plugin-updater`'s `check`):
  quiet check sets available/`available` phase on an update; stays `idle`
  on up-to-date; stays `idle` + `logWarning` + no `error` on failure;
  no-ops when phase isn't `idle`.
- **composable** (fake timers, mock `../announce`): enabled → checks after
  the delay, announces and arms `pendingView` on an update; no announce /
  no pending view when up to date; disabled → never checks; unmount before
  the delay → no check.
- **vaults store:** `requestViewOnNextOpen("settings")` leaves `view`
  untouched and makes the next `refresh()` land on settings.
- **UpdateSettings:** the toggle renders in the Updates card, mirrors and
  persists the setting.
- **buddyMessages:** `updateAvailableMessage` includes the version and has
  the blank-version fallback.

## Out of scope

Auto-install, scheduled/periodic checks, release notes in the bubble, a
tray menu item for checking.
