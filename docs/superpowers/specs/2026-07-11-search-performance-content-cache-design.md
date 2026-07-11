# Search Performance — In-Memory Content Cache Design

- **Date:** 2026-07-11
- **Status:** Approved
- **Source:** User request — "let's improve the search feature, a growing
  number of vaults and their contents make the search pretty slow. Are there
  ways we can use the Obsidian metadata or its cache to improve search? I
  don't want to build an own index." Product decisions confirmed with the
  user: the slowness is felt on **every** search (cold or warm), the vault set
  is **5k–50k notes**, cache memory is capped at **256 MB**, and v1 **includes
  background pre-warming**.

## Goal

Make cross-vault search fast as vaults grow, **without** a persistent index
and **without** changing what a search returns. Today every debounced
keystroke re-walks every vault and re-reads *and re-lowercases* every note's
full text from scratch (`core::search::content_snippet`) — cost grows linearly
with total note bytes, on every search, with zero reuse. The fix is a
process-lifetime, mtime-invalidated **in-memory content cache** so the second
search onward does almost no I/O and no re-lowercasing, plus a background
pre-warm so the first search of a launch is fast too. Search stays read-only,
async/off-main, and its results stay byte-for-byte identical to today.

## Why not Obsidian's cache (the user's first instinct)

Obsidian's metadata cache is **not** a vault file we can read. It lives in
Chromium **IndexedDB** (a LevelDB store) inside Obsidian's own app-data
directory, rebuilt from the notes on demand. Three independent blockers:

1. **It is locked while Obsidian runs.** LevelDB takes a single-process
   exclusive lock. Vault Buddy's whole premise is being a companion to a
   *running* Obsidian — so at the exact moment it matters the store is locked
   and unreadable; a copy would be stale and risk corruption.
2. **It is undocumented and version-fragile** (LevelDB keys + V8
   structured-clone blobs); reverse-engineering it would break on Obsidian
   updates, against this app's local-first robustness principles.
3. **The only sanctioned access is the plugin API** (`app.metadataCache`),
   which means running inside Obsidian as a plugin — a different product — and
   even then it is a *metadata* cache (headings/tags/links/frontmatter), not
   the full-text index Obsidian's search builds separately and never persists.

Sources: <https://help.obsidian.md/data-storage>,
<https://docs.obsidian.md/Reference/TypeScript+API/MetadataCache>.

## Approach (and the ones rejected)

**A lazily-populated, mtime-invalidated in-memory content cache** owned by the
shell and threaded through the existing on-demand scan, combined with a
**background pre-warm** on launch. This is the middle ground the user was
reaching for with Obsidian's cache — a self-invalidating mirror of file
contents — except we own it and it is trivially correct.

**A refinement discovered while specifying this — please note it.** In
brainstorming we sketched an "ASCII fast-path" case-insensitive byte scan over
the *original* cached text. While writing this up, caching the **lowercased**
text instead turned out strictly better: the per-search match becomes the
*identical* `lowered.contains(query_lower)` the code runs today (allocation-free,
memmem-class), so the change is **provably non-functional** — no custom
matcher, no Unicode edge cases (e.g. U+212A KELVIN SIGN folding to `k`), no
behavior drift for a reviewer to worry about. The one-time `to_lowercase` cost
moves from *every search* to *once per file-version* (at cache insert / during
pre-warm). This still delivers the "C" win the user approved (no per-search
re-lowercasing); it just gets there more safely. The only consequence is that
the display **snippet** for a content match re-reads the original file so its
casing stays exactly as today — one re-read per content match *collected*
during the scan (a handful for a normal query; bounded by the per-vault and
global caps for a pathological match-everything query, which is truncated
anyway), and the file is typically hot in the OS page cache from the same
session, so the cost is negligible.

Rejected:

- **A persistent full-text index** (tantivy / hand-rolled) — explicitly out of
  scope per the user; heavy dependency, on-disk format, tokenization, and
  write-invalidation against a vault Obsidian edits constantly. The cache needs
  none of that: no tokenization, no persistence, no write-hooks, and mtime
  self-invalidates.
- **Reading Obsidian's IndexedDB** — see the section above.
- **Streaming / filename-first results (option "B")** — return cheap filename
  matches instantly and stream content matches after. Deferred: it improves
  *perceived* first-paint but doesn't reduce the actual content-scan cost, and
  it needs an event-streamed frontend. Revisit only if first-paint still
  bothers the user after the cache lands.
- **Per-search-smaller cap with LRU eviction** — because a search touches
  *every* file each time, access is uniform, so LRU degenerates and eviction
  just churns entries we need again the same search. We instead fill to the cap
  and stop inserting (below).

## Rust — `core/src/search.rs` (pure, unit-tested on Linux)

### The cache type

```rust
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicU64;
use std::time::SystemTime;
use std::path::PathBuf;
use std::collections::HashMap;

/// Process-lifetime, mtime-invalidated mirror of note *content* used to make
/// repeated/refined searches cheap. NOT an index: no tokenization, no
/// persistence, no write-hooks — `mtime`+`size` self-invalidate. `Sync`, so
/// the per-vault scoped scan threads share one `&SearchCache`. The lock is
/// held ONLY for the map get/insert; file I/O, lowercasing and matching happen
/// outside it, and entries hand out `Arc<str>` so a hit never copies content.
pub struct SearchCache {
    entries: Mutex<HashMap<PathBuf, CacheEntry>>,
    /// Sum of cached lowered-text bytes; the cap is a fill-to-limit stop, not
    /// an evicting budget (uniform access makes eviction pointless — see
    /// Approach). Atomic so the byte check needs no map lock.
    bytes: AtomicU64,
    cap_bytes: u64,
}

enum CacheEntry {
    /// A note ≤ MAX_CONTENT_BYTES, valid UTF-8: its lowercased text, ready to
    /// `contains(query_lower)` with no per-search allocation.
    Text { mtime: SystemTime, size: u64, lowered: Arc<str> },
    /// A note that read as non-UTF-8 (or failed to read): remembered so it is
    /// not re-read on every search. Oversize notes need NO entry — the `stat`
    /// size check skips them before any read.
    Uncacheable { mtime: SystemTime, size: u64 },
}

impl SearchCache {
    pub fn new() -> Self { Self::with_cap(256 * 1024 * 1024) }
    /// Test seam: a small cap exercises the fill-to-limit path deterministically.
    pub fn with_cap(cap_bytes: u64) -> Self { /* ... */ }
}
```

`Default for SearchCache` = `new()` (256 MB).

### The shared primitive

```rust
/// Lowercased note text for matching, from the cache or a fresh read.
/// `None` when the note is oversize (`> MAX_CONTENT_BYTES`, decided by the
/// `stat` with no read), unreadable, or not UTF-8 (the latter two recorded as
/// `Uncacheable` so they are not re-read next search). One `stat` per call
/// yields `(mtime, size)` for validation and the size cap. On a miss under
/// `cap_bytes` the lowered text is stored; over the cap it is returned for
/// this search but not stored (correctness without unbounded memory). A benign
/// double-read is possible if two threads miss the same path at once —
/// last-write-wins, `Arc` refcounts stay sound.
fn cached_lowered(path: &Path, cache: &SearchCache) -> Option<Arc<str>>;
```

### Matching stays identical

`scan_vault`'s content branch changes only in *how it obtains the lowered
text*. Today:

```rust
let content = std::fs::read_to_string(path).ok()?;      // every search
if !content.to_lowercase().contains(query_lower) { return None; } // every search
```

Becomes:

```rust
let lowered = cached_lowered(path, cache)?;              // cached after first read
if !lowered.contains(query_lower) { return None; }       // identical predicate
// content match → re-read the ORIGINAL for the snippet (once per collected
// match; the file the size check already bounded to ≤ MAX_CONTENT_BYTES and it
// is usually page-cache-hot), so snippet casing is exactly today's;
// best-effort, None on any read failure.
let snippet = std::fs::read_to_string(path).ok()
    .and_then(|orig| orig.lines().find_map(|l| snippet_from_line(l, query_lower)));
```

Filename/stem matching, the `filename-before-content` two-list discipline, the
`PER_VAULT_CAP`, the `MAX_RESULTS` merge, dot-skips, the extensionless
exclusion and `MAX_CONTENT_BYTES` are **unchanged**. `snippet_from_line` is
unchanged.

### Entry points

```rust
/// New primary entry point: the on-demand scan, reusing a caller-owned cache.
pub fn search_vaults_with_cache(
    vaults: &[Vault], query: &str, cache: &SearchCache,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> SearchResponse;

/// Populate `cache` for one vault WITHOUT matching — the pre-warm's unit of
/// work. Reuses `walk_vault` + `cached_lowered` so the warm and lazy paths can
/// never diverge. Polls `is_cancelled` per file.
pub fn warm_vault(vault: &Vault, cache: &SearchCache, is_cancelled: &(dyn Fn() -> bool + Sync));
```

`search_vaults` and `search_vaults_with_cancel` are **kept** as thin wrappers
that build a fresh empty `SearchCache` per call, so **every existing test in
`search.rs` passes unchanged** and callers that don't want a shared cache stay
simple.

## Shell — `src-tauri/src/search_commands.rs` and `lib.rs`

### The long-lived cache + the command

```rust
// search_commands.rs
static SEARCH_CACHE: OnceLock<search::SearchCache> = OnceLock::new();
fn search_cache() -> &'static search::SearchCache {
    SEARCH_CACHE.get_or_init(search::SearchCache::new)
}
```

`search_vaults` (the async command) is unchanged except that the closure passed
to `spawn_blocking` calls `search::search_vaults_with_cache(&vaults, &query,
search_cache(), &stale)` instead of `search_vaults_with_cancel`. The cache is
touched **only** inside `spawn_blocking` — never on the main thread — so every
main-thread window invariant is untouched, and `&'static SearchCache` is shared
across the per-vault scoped threads. `open_search_result` is unchanged. **No new
IPC command** — the IPC surface in AGENTS.md does not change.

### Background pre-warm

A **named `search-prewarm` thread** (every spawned thread is named — the
diagnostics invariant), spawned from `setup` in `lib.rs` **after** the critical
startup sequence (crash handler → marker → restore → tray → recovery →
transcriber → MCP `start_if_enabled`) plus a short settle delay, so it never
competes with launch-critical work — mirroring how the greeting uses
`schedule_show_bubble` rather than running in `setup` synchronously. It:

- discovers vaults (`discovery::discover_vaults`) and calls
  `search::warm_vault` for each, filling `search_cache()` up to the 256 MB cap;
- **pauses while a recording is active** — disk I/O must not contend with the
  capture MP3 stream's fsync; this reuses the same "postpone while a recording
  is active" discipline as capture recovery. The shell owns the capture-active
  state, which is why the thread lives here and not in pure `core`;
- **yields between files** (a brief sleep every N files) so it never pegs a
  core or saturates disk — the transcription worker's "yield while recording"
  politeness, generalized to "stay low-priority";
- observes a shutdown/cancellation `AtomicBool` (passed as `is_cancelled`) and
  stops cleanly on quit; abandoning it mid-run is harmless — it only fills RAM
  and never writes.

It runs **once per launch**. Later edits are picked up lazily by the
`(mtime,size)` check on the next search — no periodic re-warm (YAGNI). Running
concurrently with a live search is safe by construction: both share the
byte-counted cache and write identical bytes for identical mtimes; whichever
reaches a file first warms it.

## Concurrency & memory

- **Lock discipline:** the map `Mutex` is held only for get/insert (microseconds);
  reads, `to_lowercase`, and `contains` run outside it. Hits clone an `Arc<str>`.
  Single-digit vault threads over a microsecond-held lock → negligible
  contention; per-vault sharding is a future refinement if profiling shows a
  hotspot, not v1.
- **Memory bound:** `bytes` (atomic) tracks cached lowered-text bytes; at
  `cap_bytes` new `Text` inserts stop (the search still returns correct results
  by reading uncached files fresh). An mtime-miss re-read replaces its entry in
  place (subtract old, add new). `Uncacheable` markers carry no text and are
  bounded by file count. Dead entries for deleted files linger until process
  exit but are bounded by the cap; a per-walk mark-and-sweep is a possible
  refinement, deliberately out of v1.

## Error handling summary

- `cached_lowered` is best-effort: any `stat`/read/UTF-8 failure returns `None`
  and the file degrades to name-only — exactly today's behavior; non-UTF-8 and
  read errors are additionally memoized so they aren't retried each search.
- A poisoned cache `Mutex` (from a panicked scan thread) is recovered via
  `into_inner()` rather than propagated — search already tolerates a panicked
  vault thread; the cache must never turn a scan into a crash.
- The prewarm thread swallows nothing silently: a spawn failure is
  `log::warn!`'d (never `.expect`), and per-file scan failures degrade exactly
  as the live scan's do.
- Cancellation (the scan-generation atomic) is unchanged; cache writes from a
  superseded/cancelled scan are harmless — they are valid entries for the next
  search.

## Invariants preserved

- **Results byte-for-byte identical to today.** The cache changes only byte
  provenance and the match predicate is the *same* `lowered.contains`; snippets
  re-read the original. Pinned by an equivalence test (below).
- Search stays **async/off-main**, takes **no main-thread locks**, and **never
  writes** to a vault. Per-vault **named parallel** scoped threads, the 1 MiB
  content cap, extensionless exclusion, dot-file/dir skips, and the
  filename-before-content hard guarantee all stand.

## Testing (TDD, failing test first; regression tests name the failure mode)

- **`core/src/search.rs`:**
  - *Cache hit avoids re-read:* populate via one search, mutate a file's content
    **without** changing its mtime, search again → the stale (cached) result is
    served, proving no re-read; then bump mtime → the new content is found,
    proving `(mtime,size)` invalidation.
  - *Cap holds:* with `SearchCache::with_cap(small)`, fill past the cap →
    `bytes` never exceeds `cap_bytes`, and files past the cap still return
    correct results (fresh-read fallback).
  - *`Uncacheable` memoization:* a non-UTF-8 note is recorded and not re-read on
    the next search (observed via a read counter or an mtime-stable re-search),
    still degrading to name-only.
  - *Equivalence:* run one corpus (ASCII + multibyte content, mixed-case
    queries, non-ASCII query `ää`, name-only, content-only, attachment,
    oversize, non-UTF-8) through `search_vaults` (empty cache) and through a
    warmed `search_vaults_with_cache` → identical `SearchResponse`.
  - *Shared-cache concurrency:* the existing parallel-scan test, rerun with one
    shared `&SearchCache`, keeps vault order and budget semantics.
  - *`warm_vault` then search:* warming a vault makes the subsequent search a
    pure cache hit (same read-counter technique) and returns identical hits.
  - All **existing** `search.rs` tests remain and must pass unchanged.
- **Performance validation (not a unit test):** add debug-level instrumentation
  (scan duration, cache hit/miss counts) and run a manual **before/after
  benchmark on a synthetic ~20k-note corpus**, recorded in the plan's
  verification step. The speedup is *proven with numbers*, not asserted.
- **Shell:** the prewarm thread's politeness is covered by a small unit around
  its "pause while recording / honor cancel" decision where feasible; the
  thread wiring itself is exercised by the `linux-app` shell build + tests.

## Docs

- **`AGENTS.md` search section:** it currently opens "Cross-vault, read-only,
  on-demand search (no index)" and later leans on "no background state." Rewrite
  to: still **no index**, but now backed by a process-lifetime, mtime-invalidated
  in-memory **content cache** (256 MB cap) plus a `search-prewarm` background
  thread; note the cache is touched only inside `spawn_blocking` (main-thread
  invariants intact) and that results are unchanged. Add `search-prewarm` to the
  named-threads set. The IPC surface table does **not** change.
- **`docs/Gaps.md`:** if the fill-to-cap "last vaults may not cache past the
  cap" degradation or the deleted-file dead-entry point is worth tracking,
  add a Low gap entry; otherwise leave as documented-in-spec.

## Out of scope (deliberate)

A persistent on-disk index; streaming / filename-first results (option B);
per-vault cache sharding; LRU/mark-and-sweep eviction; periodic re-warming;
a user-configurable cap; caching the directory-walk structure; searching
attachment contents. The `SearchCache` seam (cache in, results out) leaves room
for sharding and walk-structure caching later without reshaping the IPC surface.
