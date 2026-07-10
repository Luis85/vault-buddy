# Cross-Vault Search Design

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** User request — "add a search feature to the buddy [that] can
  search all vaults and its contents. The search should be an icon in the top
  bar besides the cog icon and open an own search view with its results."
  Product decisions confirmed with the user: case-insensitive **substring**
  matching, **live** search as you type (debounced), a result click **opens
  the note in Obsidian and closes the panel**, and the scope is note names +
  note text **plus attachment filenames**.

## Goal

A magnifying-glass icon beside the cog in the panel header opens a dedicated
**Search** view. Typing a query (2+ characters) live-searches **every
registered vault**: markdown notes by filename and full text, every other
(non-dot) file by filename. Results are grouped by vault, show a snippet of
the first matching line for content matches, and highlight the query. Clicking
a result opens it in Obsidian via `obsidian://open` and closes the panel —
the same feel as opening a vault today. Search is strictly **read-only**: it
never writes into a vault, and opening is delegated to Obsidian through the
existing launch-logged URI path.

## Approach (and the ones rejected)

**On-demand scan per query** in a new pure `core::search` module — no index,
no background state, no staleness; what's on disk is what you find. Bounded by
a debounce, a result cap, and a per-file size cap. This mirrors how every
other read surface works (recordings/tasks scan on open, discovery re-runs on
every panel open).

Rejected: a **persistent full-text index** (tantivy or hand-rolled) — a heavy
dependency, memory per vault, invalidation on external edits (Obsidian writes
constantly), and background threads in an app whose threading invariants are
its most guarded area; YAGNI for v1. Also rejected: **delegating to
`obsidian://search`** — it cannot aggregate across vaults, needs Obsidian
running, and shows results in Obsidian instead of the buddy's own view, which
the request explicitly asks for.

## Rust — new `core/src/search.rs` (pure, unit-tested on Linux)

Public surface:

```rust
pub struct SearchHit {
    pub vault_id: String,
    pub vault_name: String,
    /// Display name: file stem for `.md` notes, full filename for attachments.
    pub name: String,
    /// Vault-relative parent folder, `/`-separated, "" for the vault root.
    pub folder: String,
    /// The `obsidian://open` `file` parameter: vault-relative, `/`-separated,
    /// `.md` dropped for notes (Obsidian's expected form, mirroring
    /// `uri::vault_relative_no_ext`), extension KEPT for attachments —
    /// without it Obsidian would resolve `report` as `report.md`.
    pub file: String,
    /// First matching content line (~120-char window around the first match,
    /// char-boundary safe, best-effort centering). None for filename-only
    /// matches and attachments.
    pub snippet: Option<String>,
}

pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}

pub const MIN_QUERY_CHARS: usize = 2; // trimmed chars, not bytes
pub const MAX_RESULTS: usize = 100;
pub const MAX_CONTENT_BYTES: u64 = 1024 * 1024; // 1 MiB

/// Search every vault, in the given order, until MAX_RESULTS hits are
/// collected. A trimmed query shorter than MIN_QUERY_CHARS returns an empty
/// response. Read-only, best-effort: missing/unreadable vaults, dirs and
/// files degrade silently (documented scan-noise exception to the no-swallow
/// rule, same as tasks/recordings).
pub fn search_vaults(vaults: &[discovery::Vault], query: &str) -> SearchResponse;
```

Matching is **case-insensitive substring** (`to_lowercase()` on both sides —
consistent with the vault filter's semantics). Per file:

- **`.md` note**: match the file **stem** against the query, and the file
  **content** when the file is ≤ `MAX_CONTENT_BYTES` and valid UTF-8 (larger
  or non-UTF-8 files: filename match only). A content match produces the
  snippet from the first matching line; frontmatter lines count as content.
- **Any other file** (attachment): match the full **filename**; no snippet.
- Dot-**files** are skipped (`.DS_Store`, our own `.mp3.part` temps).

The walk is the tasks walk's safety discipline, extracted to a shared shape
rather than re-invented: canonicalize the vault root (unresolvable → skip the
vault), skip dot-**directories** (`.obsidian`, `.trash`, `.git`), descend into
a subdirectory only after canonicalizing it and confirming it still resolves
under the canonical vault root (a symlink/junction escaping the vault is never
walked — the no-follow dirent type can't be trusted for a junction), and track
walked dirs in a set so a reparse-point cycle terminates. Directory entries
are processed in **name order** so the walk — and therefore which hits survive
the cap — is deterministic and testable.

Ordering and the cap: within a vault, hits are collected in walk order, then
stable-sorted **filename matches first, content-only matches after** (walk
order preserved within each class). Per vault at most `MAX_RESULTS + 1` hits
are collected — the bound keeps transient memory small on a pathological query
like `"e"`, and the `+ 1` makes "more existed" observable even when a single
vault fills the whole budget;
the response takes vaults in the given order until the global `MAX_RESULTS`
budget is exhausted. Anything left over sets `truncated: true`. When
truncated, which hits survive is the documented best-effort cut above — the
UI's job is only to say "there's more, refine the query".

## IPC — new `src-tauri/src/search_commands.rs` (registered in `lib.rs`)

Mirrors `task_commands.rs` as its own small module. DTOs are
`#[serde(rename_all = "camelCase")]`.

- `search_vaults(query: String) -> SearchResponseDto` — **async**, the one
  deliberate deviation from the sync-command idiom: a sync command runs on the
  main thread, and a multi-vault content scan there would freeze window
  show/hide, drags and the upkeep tick. The async command runs off-main; the
  blocking filesystem walk is wrapped in `tauri::async_runtime::spawn_blocking`
  so it also never stalls the async runtime's worker threads. It touches no
  window APIs and takes no locks, so none of the main-thread window
  invariants apply. Resolves vaults via `discovery::discover_vaults()`
  (name-sorted, same list the panel shows) and delegates to
  `core::search::search_vaults`. A `spawn_blocking` join failure returns an
  empty, untruncated response and `log::warn!`s (no swallowed error).
- `open_search_result(id: String, file: String) -> Result<(), String>` —
  sync (fast, no scan): resolve the vault by id (`find_vault`, made
  `pub(crate)` in `commands.rs`), build `uri::open_file_uri(&vault.id, &file)`
  — by **ID, never name** — and `uri::launch` it (the audit-trail log line).
  `file` is the URI-form path the search itself returned; Obsidian resolves it
  inside the vault, so the command performs no filesystem access and no write.

## Frontend

- **`stores/vaults.ts`**: add `"search"` to the `view` union; `openSearch()`
  sets it; `back()` maps `search` → `showList()` (fixed one-parent tree, no
  history stack). No per-vault id — search is cross-vault. The panel-shown
  `refresh()` already defaults the view back to the list, which unmounts the
  search view and clears its transient state on every reopen.
- **`components/ActionPanel.vue`**: a search icon button
  (`data-testid="search-toggle"`, aria-label "Search vaults") in the header
  button group **beside the cog**, both rendered only on the `list` view
  (every other view shows the back button, unchanged); header title case
  `search` → "Search"; a `v-else-if="view === 'search'"` outlet rendering
  `<Search />` inside the usual `panel-scroll` wrapper.
- **New `components/Search.vue`** — self-contained local state, mirroring
  `Tasks.vue`; **no new Pinia store**:
  - An autofocused `type="search"` input. Escape with a non-empty query
    clears the query and stops propagation; a second Escape bubbles to
    PanelRoot and closes the panel (exactly the vault filter's pattern).
  - **Debounce 300 ms** after the last keystroke; a trimmed query under 2
    characters cancels any pending call and clears results to the hint state.
    The pending timer is cleared on unmount.
  - **Stale-response guard**: a monotonic request counter; a resolving
    `invoke` whose ticket is no longer current is dropped, so out-of-order
    responses can never overwrite newer results.
  - States: hint ("Type at least 2 characters…"), searching (only when no
    results are up — a live refinement must not flash the list away), results
    grouped under vault-name headers (flat hit list grouped client-side,
    vault order preserved), empty ("No matches for …"), error banner that
    **keeps the previous results** (a working list never blanks, mirroring
    the vaults store), and a truncation footer ("Showing the first 100
    matches — refine your query.").
  - Each row: highlighted `name`, muted `folder` path, highlighted `snippet`
    for content matches. Highlighting splits on lowercase `indexOf` — never a
    `RegExp` built from user input — via a small pure helper
    (`src/utils/highlight.ts`) with its own tests; if lowercasing changes the
    string length (rare Unicode), it falls back to no highlight rather than
    mis-slicing.
  - Click invokes `open_search_result`, announces via the existing buddy
    bubble path with a new `noteOpenedMessage(name)` in `buddyMessages.ts`
    (mirroring `vaultOpenedMessage`), and closes the panel (`close_panel`,
    best-effort). On failure: `notifications.error` toast + `logWarning`,
    panel stays open — mirroring `Tasks.vue`.
- **`types.ts`**: `SearchHit { vaultId; vaultName; name; folder; file;
  snippet: string | null }` and `SearchResponse { hits: SearchHit[];
  truncated: boolean }` (camelCase matches the Rust DTOs).

## Error handling summary

- Per-file/per-dir failures inside the scan degrade silently by design
  (documented above — scan noise, same as tasks/recordings walks).
- `search_vaults` (command) never rejects for domain reasons; infrastructure
  failures (spawn_blocking join) are logged and degrade to empty.
- Frontend `invoke` rejections → in-view error banner, previous results kept.
- `open_search_result` failures → toast + log, panel stays open.

## Testing

- **Rust (`core/src/search.rs`)**: content match with snippet; filename match
  (note stem) without snippet; attachment filename match with extension kept
  in `file` while a note's `file` drops `.md`; case-insensitivity both
  directions; snippet windowing on a long line (char-boundary safety on
  multi-byte text); dot-dir and dot-file skips; oversize file still matches by
  name but not content; non-UTF-8 degrades to name-only; unresolvable vault
  path skipped; symlinked subdir escaping the vault not followed (unix);
  reparse cycle terminates (unix); `MAX_RESULTS` cap + `truncated` flag;
  filename-before-content ordering; multi-vault order preserved; short/empty
  query returns empty; folder field `/`-separated on a nested hit.
- **Vitest**: new `tests/search.test.ts` — debounced invoke with the typed
  query (fake timers); short query never invokes; stale-response guard (older
  slow response cannot clobber newer); grouped rendering; row click →
  `open_search_result` with `{ id, file }` + `close_panel` + announce;
  failure keeps panel open and raises a notification; error banner keeps
  previous results; truncation footer. `tests/highlight.test.ts` — the split
  helper (multiple matches, no match, case-insensitive, Unicode fallback).
  Additions: `action-panel.test.ts` — search icon on the list view only,
  click opens the search view, "Search" title + back button; a Search-view
  smoke render. `vaults-store.test.ts` — `openSearch()` sets the view,
  `back()` returns to the list. `buddy-messages.test.ts` —
  `noteOpenedMessage`.
- TDD per repo convention: failing test first; regression tests name the
  failure mode in a comment.

## Docs

`AGENTS.md`: add `search_vaults` / `open_search_result` to the IPC surface
list (noting the async exception and why), and a short **search domain**
paragraph beside the vault domain — read-only, caps, walk-safety discipline
shared with the tasks scan, `search` view in the panel view tree (parent:
list).

## Out of scope (deliberate)

Fuzzy/word/regex matching and ranking beyond filename-first; a persistent
index; searching attachment *contents* (PDF text etc.); per-vault scoping UI
or filters; search history; opening results anywhere but Obsidian; keyboard
result navigation (arrow keys); folder-name matching. The core module's
signature (vault list in, response out) leaves room for all of these without
reshaping the IPC surface.
