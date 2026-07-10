# Search Polish Design (follow-up to Cross-Vault Search)

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** User request for "a thorough improvement and polishing pass on
  top of PR #44", scoped against a multi-angle review of the shipped diff
  (eight independent finder passes + adversarial verification; six
  correctness findings confirmed). Builds on
  [2026-07-09-vault-search-design.md](2026-07-09-vault-search-design.md);
  ships on the same branch / PR #44. User approved all four buckets:
  correctness fixes, code consolidation, performance, UX upgrades.

## Goal

Make the shipped search correct on the verified edge cases, single-source
its safety-critical walk, cut its hot-path waste, and round off the UX with
keyboard navigation and clearer result rows — without changing the feature's
shape: on-demand read-only scan, `obsidian://` handoff, self-contained panel
view.

## Correctness fixes (all verified against the code)

1. **Case-insensitive `.md` detection.** `collect` classifies a file as a
   note when its name ends with `.md` in any case (byte-wise ASCII
   case-insensitive suffix compare — no allocation, no char-boundary risk;
   the 3-byte ASCII suffix makes `name[..len-3]` a safe stem slice). A
   `Plan.MD` note is content-searched and displayed by stem. Its `file` URI
   param **keeps the extension unless it is exactly `.md`** —
   `obsidian://open?file=Plan.MD` opens the note by exact path, whereas
   guessing that Obsidian resolves the extensionless `Plan` to `Plan.MD`
   would gamble on resolver behavior. Exactly-`.md` notes keep today's
   dropped-extension form.
2. **Filename-first becomes a hard guarantee.** The per-vault collection
   splits into two capped lists — filename matches and content-only matches
   (`PER_VAULT_CAP` each). When the content list fills, the walk stops
   *reading file contents* but keeps checking *names* to the end of the
   vault; the walk only aborts entirely when the filename list also fills.
   Name checks are dirent string ops, so the tail walk is cheap. Result
   order is filename list then content list (walk order within each — the
   sort and the `RawHit` struct are deleted). A filename match can never
   again be dropped in favor of earlier-walking content matches (previously:
   a vault with >101 content matches walked before a name-matched file
   silently dropped the name match). `truncated` = any hit dropped by
   either cap or the global budget.
3. **Scan failure is an error, not an empty success.** The `search_vaults`
   command returns `Result<SearchResponse, String>`: the `spawn_blocking`
   join-error arm becomes `Err` (still `log::warn!`ed). The frontend's
   existing catch branch then keeps the previous results and shows the
   banner — a panicked scan can no longer blank a working list via the
   success path.
4. **Code-point query gate.** `Search.vue` counts the trimmed query in
   Unicode code points (spread/`Array.from` length, mirroring Rust's
   `chars().count()`), so a single astral-plane character (one emoji) shows
   the "type at least 2 characters" hint instead of a false "No matches".
   The count lives in one tiny helper beside `MIN_QUERY_CHARS` with a
   comment naming the Rust constant it mirrors.
5. **Extensionless files are excluded from results.** Obsidian does not
   index extensionless files, so `obsidian://open?file=Notes%2Fidea` opens
   the like-named *note* — the row would silently open the wrong file, and
   its `file` value collides with the note's dropped-`.md` form (duplicate
   Vue `:key`, identical open payload). Attachments therefore require an
   extension. Belt-and-braces: hits carry `is_note` (serialized `isNote`)
   and row keys append a kind discriminator, so even an exotic collision
   (attachment `idea.transcript` vs note `idea.transcript.md`) cannot
   mis-key rows.
6. **Superseded scans abort backend-side.** A process-global `AtomicU64`
   generation in the shell: each `search_vaults` call bumps it and passes a
   cancellation closure (`generation changed → cancelled`) into the core
   scan, which checks it once per file. A superseded scan stops walking
   instead of running to completion and contending for disk with the scan
   whose results will actually be shown. Core API:
   `search_vaults_with_cancel(vaults, query, is_cancelled: &(dyn Fn() -> bool + Sync))`;
   the existing `search_vaults(vaults, query)` delegates with a
   never-cancelled closure. A cancelled scan just returns what it has — the
   frontend ticket already discards the stale response.

## Consolidation

- **One safe walk, two domains.** New `core/src/vault_walk.rs` (pub(crate)):
  the reparse-safe recursive walk — walked-set cycle guard, canonicalized
  child + `starts_with(canon_root)` containment, dot-**directory** skip,
  name-sorted entries for determinism — driving a per-file callback
  `FnMut(&Path, &str) -> Flow` (`Flow::Continue | Flow::Stop`). Dot-*file*
  handling stays per-domain (search skips dot-files; tasks deliberately
  keeps today's behavior of considering them). `tasks::collect_tasks` and
  the search collector become callbacks; the symlink-escape and cycle tests
  already present in BOTH domains gate the refactor. (Tasks' final sort
  makes its output insensitive to the now-sorted walk order.)
- **DTO layer deleted.** `core::search::{SearchHit, SearchResponse}` derive
  `serde::Serialize` + `#[serde(rename_all = "camelCase")]` (exact
  `discovery::Vault` precedent — core already depends on serde/derive) and
  gain `pub is_note: bool`. `search_commands.rs` returns core types
  directly; `SearchHitDto`/`SearchResponseDto`/`From` go away.
- **`uri::vault_relative`.** New keep-extension sibling of
  `vault_relative_no_ext`; both share the strip-prefix/`\`→`/`
  normalize/empty-check tail so the `obsidian://` file form is derived in
  exactly one place. `make_hit` computes the vault-relative path once and
  chooses dropped-vs-kept extension per fix 1.
- **`HighlightText.vue`.** One small presentational component
  (props `text`, `query`; internal computed calls `highlightParts`) replaces
  the two duplicated mark-rendering template blocks. Being prop-gated, Vue
  skips re-rendering it while the user types (previously `highlightParts`
  re-ran for every row on every keystroke re-render).
- **Header titles as a map.** `ActionPanel.vue`'s 8-branch nested ternary
  becomes a `Record<view, string>` lookup with a `"Vaults"` fallback.
- **One `results` ref.** `Search.vue`'s `hits` / `truncated` /
  `resultsQuery` (which only ever change together) collapse into a single
  `results: { query, hits, truncated } | null` ref; reset and publish become
  one assignment each; groups/empty-state/footer derive from it.
- **Comment hygiene.** The narration comment in `search.rs`'s test module
  (explaining `use super::*;`) is removed per the repo's
  comments-explain-constraints rule.
- **Consciously skipped** (reviewed, rejected as YAGNI at current scale): a
  shared group-by util (two views, different shapes), an Escape-handling
  composable (two 5-line handlers), extracting the announce+close open flow
  (three call sites with differing error handling), backend-computed
  highlight ranges (protocol redesign; repo precedent tolerates mirrored
  frontend logic).

## Performance

- **Whole-file early-out.** `content_snippet` lowercases the file content
  once and does a single `contains(query_lower)`; only matching files run
  the per-line snippet pass. One allocation per file instead of one per
  line; the overwhelmingly common non-matching file exits after one scan.
- **Parallel vault scans.** `search_vaults` scans each vault on its own
  **named** scoped thread (`std::thread::Builder` +
  `std::thread::scope` — every spawned thread is named, per the diagnostics
  invariant; std only, no new deps), then merges per-vault results in vault
  order applying the same budget/truncate accounting as the serial loop —
  output is byte-identical, wall-clock becomes ~the slowest vault instead of
  the sum. A thread that fails to spawn logs a warning and that vault scans
  inline on the current thread (never a panic).
- **Deferred** (documented, not implemented): replacing per-directory
  `canonicalize` with a reparse-point attribute check needs Windows-specific
  attribute handling on the security-critical path; the two wins above
  dominate and the canonicalize discipline stays as-is.

## UX

- **Keyboard navigation.** A selection index over the flat hit list:
  ArrowDown/ArrowUp on the search input moves it (clamped at both ends,
  `preventDefault` so the caret doesn't move), Enter opens the selected hit
  (index 0 — the top hit — by default), no-op when there are no results.
  The selected row is visually highlighted and scrolled into view
  (`scrollIntoView({ block: "nearest" })`); selection resets to 0 whenever a
  new result set lands. Accessibility: the list gets `role="listbox"`, rows
  get stable ids + `role="option"`/`aria-selected`, and the input sets
  `aria-activedescendant` to the selected row's id.
- **Row kind icons.** Each row shows a small leading icon from `isNote`:
  file-text for notes, paperclip for attachments — attachments are visually
  distinct at a glance.
- **Refinement indicator.** While a search is in flight *and* results are
  already showing, a subtle animated indicator (`data-testid="search-refreshing"`)
  appears at the input's right edge, so a live refinement doesn't look
  stale. The no-results "Searching…" line stays as is.
- **Group hit counts.** Each vault group header gets a count chip in the
  existing header-chip style (`rounded-full bg-white/10 …`).

## Testing

- **Rust (`core`)**: `Plan.MD` is content-searched, displays by stem, and
  its `file` keeps `.MD` while `plan.md` drops it; a filename match walking
  after 150 content matches still surfaces first (the hard-guarantee
  regression test); extensionless files never surface; a cancellation
  closure flipping true mid-scan stops the walk early; multi-vault results
  are deterministic and ordered by given vault order (exercises the parallel
  merge); `uri::vault_relative` round-trips notes, sidecars, attachments and
  rejects outside-vault paths; the existing symlink-escape + cycle tests in
  BOTH `tasks` and `search` stay green across the `vault_walk` extraction;
  serde camelCase field names asserted via `serde_json::to_value` on a
  `SearchHit`.
- **Vitest**: emoji query shows the hint and never invokes; ArrowDown/Up
  move the selection (clamped) and update `aria-activedescendant`; Enter
  opens the selected hit and no-ops with no results; selection resets on new
  results; the refinement indicator shows only while refining with results
  up; group headers show counts; `HighlightText` renders match/non-match
  parts (new `tests/highlight-text.test.ts` or within `search.test.ts`);
  existing suites (`search`, `action-panel`, `vaults-store`, `highlight`)
  stay green, updated only where markup/keys changed.
- Full battery before push: `npm test`, `npm run build`,
  `cargo fmt --check`, core clippy + tests, `npx tauri build --no-bundle`.

## Out of scope

Everything "consciously skipped" and "deferred" above; fuzzy matching /
ranking; persistent index; per-vault scoping UI; search history; result
pagination beyond the cap; mouse-hover moving the keyboard selection.
