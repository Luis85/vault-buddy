# Search UX Improvements Design

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** User request for "an UX improvements and polishing pass for
  search", following the cross-vault search
  ([2026-07-09-vault-search-design.md](2026-07-09-vault-search-design.md))
  and its correctness/consolidation polish
  ([2026-07-09-search-polish-design.md](2026-07-09-search-polish-design.md)).
  Ships on the same branch / PR #44. User approved three buckets: quick
  access + flow, result overview, recent searches — and explicitly deferred
  a global summon hotkey (new plugin + window plumbing). **Frontend-only**:
  no Rust changes, no IPC changes.

## Goal

Make the search feel fast to reach, fast to work through, and legible at a
glance — within the fixed 360×340 panel (the panel window never resizes;
resizing reintroduces the WebView2 stale-frame flash the three-window split
eliminated).

## Quick access + flow

- **`/` and `Ctrl+F` open search from the vault list.** A window-level
  keydown listener owned by `ActionPanel.vue` (it knows the current view),
  active only when `view === "list"`: `/` calls `store.openSearch()` unless
  the event target is a text input (typing `/` into the vault filter must
  keep typing); `Ctrl+F` (and `Cmd+F`) opens search regardless of focus and
  `preventDefault()`s the WebView find bar. The search input autofocuses on
  mount as it already does. The listener is added on mount and removed on
  unmount (the panel webview mounts `ActionPanel` exactly once).
- **Ctrl+Enter / Ctrl+click open without closing the panel.**
  `openHit(hit, keepOpen)`: after a successful `open_search_result` +
  announce, `close_panel` is skipped when `keepOpen` is true. Enter passes
  `event.ctrlKey || event.metaKey`; row click passes the same from the
  MouseEvent. Plain Enter/click keep today's open-and-close behavior. The
  buddy announce fires per open (already gated by the Buddy-messages
  setting). **Amendment (Codex review finding, confirmed):** skipping
  `close_panel` alone is not enough — Obsidian grabs foreground focus while
  handling the `obsidian://` URI, and the panel's focus-out check would hide
  the panel moments later. `keepOpen` therefore travels to Rust
  (`open_search_result(id, file, keep_open)`), which stamps a ~3 s
  **panel pin** (`PANEL_PIN_UNTIL` in `lib.rs`) that the focus-out check
  consults before hiding; the check remains only-ever-hides (a pin declines
  a hide, never shows), so the toggle_panel invariants stand. This is the
  one Rust change in this otherwise frontend-only increment.
- **Hover syncs the keyboard selection.** Rows set `selected` to their
  visible index on **`mousemove`** — deliberately not `mouseenter`, which
  fires when arrow-key scrolling slides rows under a stationary cursor and
  would fight the keyboard. With hover and keyboard unified, Enter always
  opens the visually highlighted row, and the row's separate `hover:`
  background is dropped in favor of the single selection style.

## Result overview

- **Summary line.** Above the results: `N matches in M vaults` (`match`/
  `vault` singular when 1), and `100+ matches in M vaults` when `truncated`
  — counts come from the full response (before kind filtering). Rendered
  with `role="status"` + `aria-live="polite"` (`data-testid="search-summary"`),
  so screen readers hear result updates; rendered only when the response has
  hits (the existing "No matches" line covers zero).
- **Collapsible vault groups.** A chevron on each group header toggles that
  vault's rows (`collapsed: Set<vaultId>`, component state — survives
  refinements while the view lives, resets when the view unmounts). The
  header keeps showing the group's (kind-filtered) count while collapsed.
  The toggle is a distinct button with `aria-expanded` and
  `aria-controls` pointing at the group's row container
  (`data-testid="group-toggle"`).
- **Kind filter chips.** A three-chip segmented row — All / Notes / Files
  (`kindFilter: "all" | "notes" | "files"`) — shown whenever the response
  has hits; filtering is client-side over the returned (≤100) hits,
  documented as such. Chips carry `aria-pressed`
  (`data-testid="search-filter-all|notes|files"`). State survives
  refinements while the view lives. When the filter empties a non-empty
  response, a dedicated line shows "Nothing matches this filter."
- **Keyboard navigation over VISIBLE rows only.** One computed derives,
  in a single pass over the kind-filtered hits: the group list (every
  group, with per-group counts and collapsed state) and the **flat visible
  list** (kind-filtered hits belonging to non-collapsed groups, in order).
  Selection, `hitId`, Enter, and `aria-activedescendant` all index the flat
  visible list — arrows can never land on a hidden row. The selection
  resets to 0 on every new result set (existing behavior) and **clamps**
  to the last visible index when collapsing/filtering shrinks the list.

## Recent searches

- **Storage util** `src/utils/recentSearches.ts`:
  `loadRecentSearches(): string[]`, `pushRecentSearch(query): string[]`
  (returns the updated list), `clearRecentSearches(): void`;
  `MAX_RECENT_SEARCHES = 5`. localStorage key `vault-buddy:recent-searches`
  (JSON string array). Dedup case-insensitively, keeping the **latest**
  casing and moving the query to the front; cap at 5. Every localStorage
  touch is wrapped in `try/catch` with `logWarning` on failure (no
  swallowed errors) and degrades to an empty list / no-op. localStorage is
  the buddy-settings precedent; recents never cross the IPC boundary.
- **Recording.** `runSearch` pushes the trimmed query when a response
  **publishes successfully** (not on error, not for dropped stale
  responses).
- **UI.** When the input is too short (including empty) and recents exist:
  a "Recent" mini-section under the hint — the queries as clickable chips
  (`data-testid="recent-chip"`; click sets `query`, and the normal
  debounce runs) plus a small Clear control
  (`data-testid="recent-clear"`) that empties the list and hides the
  section.

## Alternatives considered (rejected)

- App-side `config.json` for recents — an IPC write path for a UI nicety;
  localStorage matches the settings precedent.
- `mouseenter` for hover-sync — fires under a stationary cursor during
  keyboard scrolling and fights the arrows.
- `/` handler in `PanelRoot` — `ActionPanel` owns view-awareness; the root
  stays thin.
- A recents dropdown — chips are simpler and fit the 360px panel.
- Panel resizing for a roomier search view — architecturally off the table
  (stale-frame flash).

## Error handling

- localStorage failures: warn + degrade (empty recents, chips hidden).
- Everything else reuses the existing search error paths unchanged.

## Testing (Vitest; no Rust changes)

- `tests/recent-searches.test.ts` (new): load on empty storage → `[]`;
  push adds to front; case-insensitive dedup keeps latest casing; cap at 5;
  clear empties; corrupted JSON in storage degrades to `[]` with a warning.
- `tests/search.test.ts` (extend): Ctrl+Enter and Ctrl+click open the hit
  but never call `close_panel`; plain Enter/click still do; mousemove on a
  row moves `aria-activedescendant`; summary line shows `2 matches in 2
  vaults`, the `100+` form when truncated, and has `role="status"`;
  collapsing a group hides its rows and ArrowDown skips into the next
  group; kind chips filter rows and show the filtered-empty line; recents:
  chip shown after a successful search + reopen-to-empty-input, click
  re-runs the query (invoke called with it), clear hides the section; a
  failed search records nothing.
- `tests/action-panel.test.ts` (extend): `/` on the list view opens
  search; `/` typed into the vault filter does NOT; Ctrl+F opens search;
  neither fires on non-list views.
- Full battery before push: `npm test`, `npm run build`,
  `cargo fmt --check`, core clippy + tests, `npx tauri build --no-bundle`
  (re-confirmation only — no Rust diff).

## Out of scope

The global summon hotkey (deferred by choice); panel resizing; fuzzy
matching/ranking; search history beyond 5 queries; syncing recents across
machines; mouse-hover opening previews; result pagination.
