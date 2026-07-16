# Pandoc-status cache + intake-button icon — Design

- **Date:** 2026-07-16
- **Status:** Approved (design only — implementation is a separate step)
- **Source:** User request: "when pandoc is found it should not recheck
  everytime when the user opens the vaults intake menu, the intake menu
  button is a microphone and not suitable anymore."

Two small, frontend-only refinements to the knowledge-intake surface (all
in the panel webview). No Rust, IPC, or config changes.

## 1. Cache the Pandoc detection so it isn't re-probed every menu open

### Problem

Three panel components each keep their own `pandoc` ref and call
`detect_pandoc` (which spawns `pandoc --version` off the main thread) on
**every mount**:

- `RecordMode.vue` — the intake menu (the "Capture knowledge" chooser).
- `ImportVaultPicker.vue` — the drag-drop vault chooser.
- `DocumentImportSettings.vue` — the app-global Pandoc settings card.

So every time the user opens the intake menu, Pandoc is re-probed even
though it was already found. The detection is duplicated three times, and a
fix made in Settings (Recheck / new path override) is invisible to a
separately-cached intake menu.

### Design

Introduce one small Pinia store, **`usePandocStore`** (`src/stores/pandoc.ts`),
that owns the app-global Pandoc status for the panel session and is shared by
all three surfaces (they already share the panel's single Pinia instance).

State:

- `status: PandocStatus | null` — the last resolved status (null before the
  first probe / on a failed probe).
- `checking: boolean` — true while the first probe is in flight with no
  cached status yet (drives the components' existing "Checking Pandoc…" /
  pre-probe button gating).

Actions:

- **`ensureDetected()`** — the on-mount entry point for the two **intake**
  surfaces (`RecordMode`, `ImportVaultPicker`). If `status?.installed` is
  already true, return immediately (no probe — the "found → don't re-check"
  behavior). Otherwise probe once via `detect_pandoc`, store the result, and
  clear `checking`. (No cross-component concurrent-probe dedup: the two
  consumers are never mounted at the same time, so an in-flight guard would be
  unused machinery — YAGNI. A rare double-probe would only re-store the same
  status.)
- **`markDetected(status)`** — a write-through setter. The Pandoc **settings**
  card (`DocumentImportSettings`) is the diagnostic surface where a fresh
  probe on every open is the *right* behavior, so it keeps its own on-mount
  probe / Recheck / path-override re-detect unchanged. It just calls
  `markDetected` with each successful result, so a settings-side fix (Recheck
  or a new override) keeps the shared intake-menu cache current — without the
  settings card itself reading the cache.

`ensureDetected` degrades exactly as the components did: a failed/absent Tauri
runtime leaves `status` null (treated as "not installed"), logged via
`logWarning`.

**Caching policy — cache a positive result only.** Once Pandoc is detected
as **installed**, `ensureDetected()` never re-probes for the rest of the
panel session. When Pandoc is **not** installed (null / `installed: false`),
`ensureDetected()` still probes on each open, so a freshly installed Pandoc
is picked up automatically without the user hunting for Recheck. An
"installed but too old (<2.15)" Pandoc counts as found and is cached too;
updating it is then reflected via **Recheck** (or a path-override change),
not automatically on the next menu open — the simplest reading of "found =
don't re-check," and the too-old case is rare.

**Why a store, not a module singleton.** The three surfaces share one panel
Pinia instance, so a store shares the cache *and* makes a Settings fix
(Recheck / a new override that re-detects) immediately visible in the intake
menu — today each caches independently. It also resets cleanly between
Vitest cases, which already call `setActivePinia(createPinia())` in
`beforeEach`; a module-level singleton would leak the cache across tests.

### Component wiring

Each of the three components drops its local `pandoc` ref, `checking` ref,
and `detectPandoc()` copy, and instead reads `store.status` / `store.checking`
and calls `store.ensureDetected()` on mount:

- `RecordMode.vue`: `importStatus` reads `store.status`; the button's
  `checking` gate reads `store.checking`; `onMounted` calls
  `store.ensureDetected()` in place of `detectPandoc()`.
- `ImportVaultPicker.vue`: same substitution for its gate computed and
  on-mount probe.
- `DocumentImportSettings.vue`: **unchanged detection logic** — it keeps its
  own `detect()` (with the monotonic `detectTicket` guard), Recheck, path
  override, `dirtied`/`saving`/`error` state, and the configured-path seed.
  The only addition is a **write-through**: in `detect()`'s success branch,
  after `status.value = s` (inside the same ticket guard), call
  `pandocStore.markDetected(s)` so a settings-side probe refreshes the shared
  cache the intake menu reads. This adds a Pinia dependency to the component,
  so its test suite gains a `setActivePinia(createPinia())` in `beforeEach`.

## 2. Replace the microphone icon on the intake button

The "Capture knowledge" button in `VaultList.vue` renders an inline
microphone SVG (a `<rect>` mic body + stand path). Now that the chooser
covers document import and browsing, not just audio, swap it for a
**plus-in-a-rounded-square** glyph: a rounded `<rect>` with a centered `+`
(two short strokes). Keep the button's 16px size, stroke styling,
`aria-label`/`title` ("Capture knowledge"), disabled state, and click
handler unchanged — only the SVG child paths change.

## Testing

- **`tests/pandoc-store.test.ts`** (new): `ensureDetected()` probes once and
  caches when installed (a second call issues **no** `detect_pandoc`);
  re-probes when the cached status is null / not-installed; degrades to
  `status: null` and logs on a probe failure; `markDetected(s)` caches a
  status with no probe (and a following `ensureDetected()` short-circuits).
  Uses `mockIPC` + a call counter, `setActivePinia(createPinia())` per test.
- `tests/record-mode.test.ts` (RecordMode) and
  `tests/import-vault-picker.test.ts` (ImportVaultPicker): these already
  `setActivePinia` per test and mock `detect_pandoc`; the observable behavior
  (button gating, "Checking…", the blocked-click routes) is unchanged now that
  the probe is issued by the store, so they pass as-is. Add one assertion in
  the RecordMode suite that a second mount after an installed result issues
  **no** new `detect_pandoc`.
- `tests/documentImport.test.ts` (DocumentImportSettings + RecordMode): add
  `setActivePinia(createPinia())` to the DocumentImportSettings `describe`'s
  `beforeEach` (the component now touches a store via the write-through);
  every existing detection / Recheck / savePath / browse assertion stays — the
  component's own logic is unchanged.
- `tests/vault-list.test.ts`: keep the existing "Capture knowledge" button
  label/title assertion; the icon is presentational (`aria-hidden`), so no
  new assertion is required beyond the button still rendering.

## Non-goals

- No change to `detect_pandoc`, `set_pandoc_path`, or any Rust/IPC/config.
- No persistence of the status across app restarts (a panel-session cache is
  enough; the conversion path re-resolves Pandoc at call time regardless).
- No change to how a blocked import routes to the Pandoc setup screen.
