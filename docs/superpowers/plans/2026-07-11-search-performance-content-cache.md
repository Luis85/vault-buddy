# Search Performance — In-Memory Content Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make cross-vault search fast as vaults grow by reusing note content across searches, without a persistent index and without changing what a search returns.

**Architecture:** Add a process-lifetime, `(mtime,size)`-invalidated in-memory cache of lowercased note text in a new `core/src/search_cache.rs`. The existing on-demand scan reads content through the cache instead of re-reading + re-lowercasing every file every search; a shell-owned `static` instance is fed into the existing `spawn_blocking`, and a background `search-prewarm` thread warms it on launch. The per-search match predicate stays the identical `lowered.contains(query_lower)`, so results are byte-for-byte identical for a stable filesystem snapshot.

**Tech Stack:** Rust (pure `core` crate + Tauri shell crate), `std::sync::{Mutex, Arc, atomic}`, existing `core::vault_walk`. No new dependencies.

## Global Constraints

- **Spec:** `docs/superpowers/specs/2026-07-11-search-performance-content-cache-design.md` — the source of truth for every decision below.
- **Results unchanged:** search output must stay byte-for-byte identical to today for a stable filesystem snapshot. The cache changes only how content bytes are obtained; the match predicate stays `lowered.contains(query_lower)`.
- **LOC gate:** Rust source files cap at **800 nonblank LOC** (`npm run check:loc`; `search.rs` is at 714 — keep the cache in `search_cache.rs`). New files above the cap are rejected.
- **Rust gates (member crates):** `cd src-tauri/core && cargo test` and `cargo clippy --all-targets -- -D warnings`; whole-workspace `cargo fmt --check`; coverage floor 94 lines over member crates (`cargo llvm-cov -p vault_buddy_core ... --fail-under-lines 94`) — new code needs tests.
- **Shell gates:** require `npm ci` (Node 22), `npm run setup:linux` (GUI/WebView libs, once), and a built `../dist` (`npm run build`) present. Compile gate: `npx tauri build --no-bundle`; shell tests: `cargo test -p vault-buddy --lib`; workspace clippy: `cargo clippy --workspace --all-targets -- -D warnings`. Invoke the CLI as `npx tauri`, never via an npm script.
- **Memory:** default cap **256 MiB**; fill-to-limit (no eviction — uniform access makes it pointless).
- **Threads:** every spawned thread is **named** (`std::thread::Builder`). The prewarm thread is reclaimed on process exit (no explicit shutdown flag), like the metronome and `capture-recovery` threads.
- **Invariants preserved:** search stays async/off-main; the cache is touched only inside `spawn_blocking`; no main-thread locks; search never writes to a vault.
- **TDD:** failing test first, then the minimal implementation. Regression tests name the failure mode in a comment.
- **Commits:** Conventional Commits (`feat(search)`, `perf(search)`, `docs(search)`). Committer identity must be `noreply@anthropic.com` / `Claude` (run `git config user.email noreply@anthropic.com && git config user.name Claude` if not already set) so commits verify.

---

### Task 1: The `SearchCache` type and `cached_lowered` primitive

**Files:**
- Create: `src-tauri/core/src/search_cache.rs`
- Modify: `src-tauri/core/src/lib.rs` (add the module declaration)
- Test: inline `#[cfg(test)] mod tests` in `search_cache.rs`

**Interfaces:**
- Consumes: `crate::search::MAX_CONTENT_BYTES` (existing `pub const`).
- Produces:
  - `pub struct SearchCache` with `pub fn new() -> Self`, `pub fn with_cap(cap_bytes: u64) -> Self`, `pub fn cached_bytes(&self) -> u64`.
  - `pub(crate) fn cached_lowered(path: &Path, cache: &SearchCache) -> Option<Arc<str>>` — lowercased note text from cache or a fresh read; `None` for oversize/unreadable/non-UTF-8.
  - `#[cfg(test)]` helpers `peek_text(&self, &Path) -> Option<Arc<str>>` and `is_uncacheable(&self, &Path) -> bool`.

- [ ] **Step 1: Add the module declaration**

In `src-tauri/core/src/lib.rs`, add the module in alphabetical order between `pub mod search;` and `pub mod services;`:

```rust
pub mod search;
pub mod search_cache;
pub mod services;
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/core/src/search_cache.rs` with ONLY the test module first (the code under test comes in Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn write(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn miss_reads_then_hit_reuses_same_arc() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "n.md", "Hello WORLD\n");
        let c = SearchCache::new();
        let first = cached_lowered(&p, &c).unwrap();
        assert_eq!(&*first, "hello world\n");
        assert_eq!(c.cached_bytes(), first.len() as u64);
        let second = cached_lowered(&p, &c).unwrap();
        // Same Arc allocation => served from cache, not re-read.
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(c.cached_bytes(), first.len() as u64); // not double-counted
    }

    #[test]
    fn size_change_invalidates() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "n.md", "alpha\n");
        let c = SearchCache::new();
        let first = cached_lowered(&p, &c).unwrap();
        // Append: size changes, so (mtime,size) mismatches regardless of the
        // filesystem's mtime granularity.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&p)
            .unwrap()
            .write_all(b"beta\n")
            .unwrap();
        let second = cached_lowered(&p, &c).unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(&*second, "alpha\nbeta\n");
        assert_eq!(c.cached_bytes(), second.len() as u64); // old bytes reconciled
    }

    #[test]
    fn oversize_note_is_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let big = "x".repeat(crate::search::MAX_CONTENT_BYTES as usize + 1);
        let p = write(dir.path(), "big.md", &big);
        let c = SearchCache::new();
        assert!(cached_lowered(&p, &c).is_none());
        assert_eq!(c.cached_bytes(), 0);
        assert!(c.peek_text(&p).is_none());
    }

    #[test]
    fn non_utf8_is_memoized_uncacheable() {
        // Regression: a binary/non-UTF-8 note must be remembered so it is not
        // re-read on every search (name-only match still applies elsewhere).
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bin.md");
        std::fs::write(&p, [0xFF, 0xFE, 0x00, b'a']).unwrap();
        let c = SearchCache::new();
        assert!(cached_lowered(&p, &c).is_none());
        assert!(c.is_uncacheable(&p));
        assert_eq!(c.cached_bytes(), 0);
        assert!(cached_lowered(&p, &c).is_none()); // still None on the second call
    }

    #[test]
    fn cap_stops_storing_but_still_returns_content() {
        // Fill-to-cap: past the ceiling, content is still returned to the
        // search that asked for it, just not retained.
        let dir = tempfile::tempdir().unwrap();
        let a = write(dir.path(), "a.md", &"a".repeat(100));
        let b = write(dir.path(), "b.md", &"b".repeat(100));
        let c = SearchCache::with_cap(120); // fits ~one 100-byte entry
        let ra = cached_lowered(&a, &c).unwrap();
        assert_eq!(ra.len(), 100);
        assert_eq!(c.cached_bytes(), 100);
        let rb = cached_lowered(&b, &c).unwrap();
        assert_eq!(rb.len(), 100); // returned...
        assert!(c.cached_bytes() <= 120); // ...but not stored past the cap
        assert!(c.peek_text(&b).is_none());
        assert!(c.peek_text(&a).is_some());
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd src-tauri/core && cargo test --lib search_cache`
Expected: FAIL to compile — `SearchCache`, `cached_lowered`, `Arc` not found.

- [ ] **Step 4: Write the implementation**

Prepend the implementation above the test module in `src-tauri/core/src/search_cache.rs`:

```rust
//! The in-memory content cache behind cross-vault search: a process-lifetime,
//! `(mtime, size)`-invalidated mirror of note *text* so repeated and
//! pre-warmed searches skip the read + lowercase that dominates a cold scan.
//! NOT an index — no tokenization, no persistence, no write-hooks; `(mtime,
//! size)` self-invalidates. See
//! docs/superpowers/specs/2026-07-11-search-performance-content-cache-design.md.

use crate::search::MAX_CONTENT_BYTES;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Default ceiling: 256 MiB of cached lowered note text. A search touches every
/// file, so a cache smaller than the corpus helps only up to the cap (the tail
/// re-reads); 256 MiB covers the 5k–50k-note target fully.
const DEFAULT_CAP_BYTES: u64 = 256 * 1024 * 1024;

/// Process-lifetime, `(mtime, size)`-invalidated mirror of note content.
/// `Sync`: the per-vault scan threads share one `&SearchCache`. The map lock is
/// held ONLY for get/insert — file I/O, lowercasing and matching run outside
/// it, and a hit hands out an `Arc<str>` so it never copies text.
pub struct SearchCache {
    entries: Mutex<HashMap<PathBuf, CacheEntry>>,
    /// Sum of cached lowered-text bytes; the cap is a fill-to-limit stop, not an
    /// evicting budget. Atomic so the byte check needs no map lock.
    bytes: AtomicU64,
    cap_bytes: u64,
}

enum CacheEntry {
    /// A note ≤ MAX_CONTENT_BYTES and valid UTF-8: its lowercased text.
    Text {
        mtime: SystemTime,
        size: u64,
        lowered: Arc<str>,
    },
    /// A note that failed to read as UTF-8: remembered so it is not re-read on
    /// every search. Oversize notes need no entry — the size check skips them
    /// before any read.
    Uncacheable { mtime: SystemTime, size: u64 },
}

impl CacheEntry {
    fn is_current(&self, mtime: SystemTime, size: u64) -> bool {
        match self {
            CacheEntry::Text { mtime: m, size: s, .. }
            | CacheEntry::Uncacheable { mtime: m, size: s } => *m == mtime && *s == size,
        }
    }
}

impl Default for SearchCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchCache {
    pub fn new() -> Self {
        Self::with_cap(DEFAULT_CAP_BYTES)
    }

    /// Test seam: a small cap exercises the fill-to-limit path deterministically.
    pub fn with_cap(cap_bytes: u64) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            bytes: AtomicU64::new(0),
            cap_bytes,
        }
    }

    /// Total cached lowered-text bytes. Diagnostics + tests.
    pub fn cached_bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Recover a poisoned lock rather than propagate: a panicked scan thread
    /// must never turn a later search into a crash (the scan already tolerates
    /// a panicked vault thread).
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, CacheEntry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[cfg(test)]
    fn peek_text(&self, path: &Path) -> Option<Arc<str>> {
        match self.lock().get(path) {
            Some(CacheEntry::Text { lowered, .. }) => Some(lowered.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    fn is_uncacheable(&self, path: &Path) -> bool {
        matches!(self.lock().get(path), Some(CacheEntry::Uncacheable { .. }))
    }
}

/// Lowercased note text for matching, from the cache or a fresh read. `None`
/// when the note is oversize (`> MAX_CONTENT_BYTES`, decided by the `stat` with
/// no read), unreadable, or not UTF-8 (the latter recorded as `Uncacheable` so
/// it is not re-read next search). One `stat` yields `(mtime, size)` for both
/// validation and the size cap. On a miss under `cap_bytes` the lowered text is
/// stored; over the cap it is returned for this search but not stored
/// (correctness without unbounded memory). A benign double-read is possible if
/// two threads miss the same path at once — last-write-wins; `Arc` refcounts
/// stay sound.
pub(crate) fn cached_lowered(path: &Path, cache: &SearchCache) -> Option<Arc<str>> {
    let meta = std::fs::metadata(path).ok()?;
    let size = meta.len();
    if size > MAX_CONTENT_BYTES {
        return None; // oversize: name-only, exactly as before — no entry needed
    }
    let mtime = meta.modified().ok()?;

    // Fast path: a still-current cached entry for this (mtime, size).
    {
        let map = cache.lock();
        if let Some(entry) = map.get(path) {
            if entry.is_current(mtime, size) {
                return match entry {
                    CacheEntry::Text { lowered, .. } => Some(lowered.clone()),
                    CacheEntry::Uncacheable { .. } => None,
                };
            }
        }
    }

    // Miss (absent or stale): read + lowercase OUTSIDE the lock.
    let lowered: Option<Arc<str>> = match std::fs::read_to_string(path) {
        Ok(text) => Some(Arc::from(text.to_lowercase())),
        Err(_) => None, // non-UTF-8 / unreadable → Uncacheable below
    };

    let mut map = cache.lock();
    // Reconcile the byte counter with any Text entry we are replacing.
    if let Some(CacheEntry::Text { lowered: old, .. }) = map.get(path) {
        cache.bytes.fetch_sub(old.len() as u64, Ordering::Relaxed);
    }
    match &lowered {
        Some(text) => {
            let len = text.len() as u64;
            if cache.bytes.load(Ordering::Relaxed) + len <= cache.cap_bytes {
                cache.bytes.fetch_add(len, Ordering::Relaxed);
                map.insert(
                    path.to_path_buf(),
                    CacheEntry::Text { mtime, size, lowered: text.clone() },
                );
            } else {
                // Over the cap: don't retain. Drop any stale entry so a later
                // search can't serve it.
                map.remove(path);
            }
        }
        None => {
            map.insert(path.to_path_buf(), CacheEntry::Uncacheable { mtime, size });
        }
    }
    lowered
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri/core && cargo test --lib search_cache`
Expected: PASS (5 tests).

- [ ] **Step 6: Lint, format, and LOC gate**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Then from the repo root: `npm run check:loc`
Expected: no warnings; `check:loc` passes (new `search_cache.rs` is well under 800 nonblank).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/core/src/search_cache.rs src-tauri/core/src/lib.rs
git commit -m "feat(search): add mtime-invalidated in-memory content cache

Introduce SearchCache + cached_lowered: a process-lifetime, (mtime,size)-
invalidated mirror of lowercased note text, 256 MiB fill-to-cap, so
repeated searches skip the read+lowercase that dominates a cold scan.
Not an index. Pure core module, unit-tested on Linux."
```

---

### Task 2: Thread the cache through the search scan

**Files:**
- Modify: `src-tauri/core/src/search.rs` (`content_snippet`, `scan_vault`, the entry points; re-export `SearchCache`)
- Test: inline `#[cfg(test)] mod tests` in `search.rs` (one new equivalence test; existing tests must stay green)

**Interfaces:**
- Consumes: `SearchCache`, `cached_lowered` from Task 1.
- Produces: `pub fn search_vaults_with_cache(vaults: &[Vault], query: &str, cache: &SearchCache, is_cancelled: &(dyn Fn() -> bool + Sync)) -> SearchResponse`; unchanged public `search_vaults` / `search_vaults_with_cancel` (now cache-backed wrappers); `pub use crate::search_cache::SearchCache;`.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/core/src/search.rs` (reuses the module's existing `vault` and `write` helpers):

```rust
#[test]
fn with_cache_equals_plain_search() {
    // A caller-supplied cache must not change results vs the cache-less entry
    // point (the cache is a pure optimization).
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "Notes/idea.md", "intro\nProject Alpha kickoff\n");
    write(dir.path(), "Alpha plan.md", "nothing relevant\n");
    write(dir.path(), "slides/Alpha deck.pdf", "%PDF-fake");
    let vs = [vault("v1", "Work", dir.path())];
    let plain = search_vaults(&vs, "alpha");
    let cache = SearchCache::new();
    let cached = search_vaults_with_cache(&vs, "alpha", &cache, &|| false);
    assert_eq!(plain, cached);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri/core && cargo test --lib search::tests::with_cache_equals_plain_search`
Expected: FAIL to compile — `search_vaults_with_cache` and `SearchCache` not found in `search`.

- [ ] **Step 3: Re-export the cache and add its import**

Near the top of `src-tauri/core/src/search.rs`, below the existing `use` lines, add:

```rust
pub use crate::search_cache::SearchCache;
use crate::search_cache::cached_lowered;
```

- [ ] **Step 4: Refactor `content_snippet` to read through the cache**

Replace the existing `content_snippet` function body (the one that does `std::fs::metadata` + `read_to_string` + `to_lowercase().contains`) with this cache-backed version. It takes a `cache` parameter and keeps the exact `contains` predicate; the snippet re-reads the original for correct casing, falling back to the lowered text if that re-read fails:

```rust
/// Snippet for a content match, or `None` when the note doesn't match (or is
/// oversize / unreadable / non-UTF-8). Matching reuses the cache — the lowered
/// text is read + lowercased once per file-version, not per search — and the
/// predicate stays the exact `contains` the pre-cache code ran. The display
/// snippet re-reads the ORIGINAL so its casing is unchanged; if that re-read
/// fails (the file changed since the cached read — best-effort), it falls back
/// to the lowered text so the confirmed match still surfaces.
fn content_snippet(path: &Path, query_lower: &str, cache: &SearchCache) -> Option<String> {
    let lowered = cached_lowered(path, cache)?;
    if !lowered.contains(query_lower) {
        return None;
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|orig| orig.lines().find_map(|l| snippet_from_line(l, query_lower)))
        .or_else(|| lowered.lines().find_map(|l| snippet_from_line(l, query_lower)))
}
```

- [ ] **Step 5: Thread `cache` through `scan_vault`**

Change the `scan_vault` signature to accept the cache, and pass it to the `content_snippet` call. The signature becomes:

```rust
fn scan_vault(
    vault: &Vault,
    query_lower: &str,
    cache: &SearchCache,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<VaultHits> {
```

Inside `scan_vault`, the existing snippet line changes from `content_snippet(path, query_lower)` to `content_snippet(path, query_lower, cache)`:

```rust
            let snippet = if name_matched || content_hits.len() < PER_VAULT_CAP {
                content_snippet(path, query_lower, cache)
            } else {
                None
            };
```

- [ ] **Step 6: Add `search_vaults_with_cache` and make the old entry points wrappers**

Rename the current body of `search_vaults_with_cancel` into `search_vaults_with_cache` by adding a `cache` parameter, and pass `cache` into every `scan_vault(...)` call inside the `std::thread::scope` closure (capture it by shared reference — `SearchCache` is `Sync`, and the scoped threads join before the borrow ends). Then make `search_vaults_with_cancel` a thin wrapper. The three functions become:

```rust
/// Search every vault, in the given order, until `MAX_RESULTS` hits are
/// collected. A trimmed query shorter than `MIN_QUERY_CHARS` returns an empty
/// response. Read-only, best-effort (see `scan_vault`).
pub fn search_vaults(vaults: &[Vault], query: &str) -> SearchResponse {
    search_vaults_with_cancel(vaults, query, &|| false)
}

/// Cancellable variant with a fresh, un-warmed cache (callers that don't hold a
/// long-lived cache still get correct results, just no cross-call reuse).
pub fn search_vaults_with_cancel(
    vaults: &[Vault],
    query: &str,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> SearchResponse {
    search_vaults_with_cache(vaults, query, &SearchCache::new(), is_cancelled)
}

/// Cancellable search reusing a caller-owned `cache`, so repeated and
/// pre-warmed searches skip the read + lowercase of unchanged notes. Output is
/// identical to `search_vaults_with_cancel` — the cache only changes how
/// content bytes are obtained.
pub fn search_vaults_with_cache(
    vaults: &[Vault],
    query: &str,
    cache: &SearchCache,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> SearchResponse {
    let trimmed = query.trim();
    if trimmed.chars().count() < MIN_QUERY_CHARS {
        return SearchResponse::default();
    }
    let query_lower = trimmed.to_lowercase();
    let mut per_vault: Vec<Option<VaultHits>> = Vec::with_capacity(vaults.len());
    std::thread::scope(|scope| {
        let mut pending = Vec::with_capacity(vaults.len());
        for (i, vault) in vaults.iter().enumerate() {
            let query_lower = &query_lower;
            let cache = &*cache;
            let spawned = std::thread::Builder::new()
                .name(format!("search-vault-{i}"))
                .spawn_scoped(scope, move || {
                    scan_vault(vault, query_lower, cache, is_cancelled)
                });
            match spawned {
                Ok(handle) => pending.push(Ok(handle)),
                Err(e) => {
                    log::warn!("search: spawning scan thread failed: {e}");
                    pending.push(Err(scan_vault(vault, query_lower, cache, is_cancelled)));
                }
            }
        }
        for entry in pending {
            per_vault.push(match entry {
                Ok(handle) => handle.join().unwrap_or_else(|_| {
                    log::warn!("search: a vault scan thread panicked");
                    None
                }),
                Err(inline) => inline,
            });
        }
    });
    merge_vault_hits(per_vault)
}
```

Note: this preserves the existing doc-comment intent on the two original functions; keep their original comments where practical and only add the cache detail.

- [ ] **Step 7: Run the full search suite**

Run: `cd src-tauri/core && cargo test --lib search`
Expected: PASS — the new `with_cache_equals_plain_search` plus **every pre-existing `search.rs` test** (they exercise the cold cache path through the wrappers, pinning equivalence).

- [ ] **Step 8: Lint, format, LOC gate**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Then repo root: `npm run check:loc`
Expected: no warnings; `search.rs` stays under 800 nonblank. If `check:loc` reports `search.rs` over the cap, move the new `with_cache_equals_plain_search` test into `search_cache.rs`'s test module (it can call `crate::search::search_vaults` / `search_vaults_with_cache`).

- [ ] **Step 9: Commit**

```bash
git add src-tauri/core/src/search.rs
git commit -m "perf(search): read note content through the SearchCache

Route scan_vault's content match through cached_lowered so repeated
searches skip the per-search read+lowercase; add search_vaults_with_cache
and keep search_vaults/search_vaults_with_cancel as cache-backed wrappers.
Predicate unchanged (lowered.contains); snippet re-reads the original for
identical casing. Existing search tests pass unchanged."
```

---

### Task 3: `warm_vault` and cache-integration tests

**Files:**
- Modify: `src-tauri/core/src/search.rs` (add `warm_vault`)
- Test: inline `#[cfg(test)] mod tests` in `src-tauri/core/src/search_cache.rs` (integration tests that drive `crate::search`)

**Interfaces:**
- Consumes: `walk_vault` / `Flow` (already imported in `search.rs`), `md_stem`, `cached_lowered`, `SearchCache`.
- Produces: `pub fn warm_vault(vault: &Vault, cache: &SearchCache, is_cancelled: &(dyn Fn() -> bool + Sync))`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/core/src/search_cache.rs`. These drive the real search entry points, so add the imports they need at the top of that test module (`use crate::discovery::Vault;` and `use crate::search::{search_vaults, search_vaults_with_cache, warm_vault};`), plus a local `vault` helper:

```rust
    fn vault(id: &str, name: &str, path: &Path) -> Vault {
        Vault {
            id: id.to_string(),
            name: name.to_string(),
            path: path.to_string_lossy().into_owned(),
            open: false,
        }
    }

    #[test]
    fn warm_then_search_is_a_pure_cache_hit() {
        let dir = tempfile::tempdir().unwrap();
        let note = write(dir.path(), "n.md", "Project Alpha\n");
        let v = vault("v1", "W", dir.path());
        let cache = SearchCache::new();
        warm_vault(&v, &cache, &|| false);
        assert!(cache.cached_bytes() > 0);
        let before = cache.peek_text(&std::fs::canonicalize(&note).unwrap()).unwrap();
        let _ = search_vaults_with_cache(&[v], "alpha", &cache, &|| false);
        let after = cache.peek_text(&std::fs::canonicalize(&note).unwrap()).unwrap();
        // The search reused the warmed entry — no re-insert.
        assert!(Arc::ptr_eq(&before, &after));
    }

    #[test]
    fn warmed_search_matches_cold_search_including_multibyte() {
        // Equivalence: a warmed cache must not change results, including
        // multibyte content and a non-ASCII query (the lowercase fallback path).
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Notes/idea.md", "intro\nProject Alpha kickoff\n");
        write(dir.path(), "cafe.md", "Beim Kaffee in der Straße\n");
        write(dir.path(), "Alpha plan.md", "nothing relevant\n");
        let v = vault("v1", "Work", dir.path());
        for q in ["alpha", "straße", "kaffee"] {
            let cold = search_vaults(&[v.clone()], q);
            let cache = SearchCache::new();
            warm_vault(&v, &cache, &|| false);
            let warm = search_vaults_with_cache(&[v.clone()], q, &cache, &|| false);
            assert_eq!(cold, warm, "query {q:?} diverged");
        }
    }

    #[test]
    fn warm_honors_cancellation() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        for i in 0..50 {
            write(dir.path(), &format!("n{i:02}.md"), "some body text\n");
        }
        let v = vault("v1", "W", dir.path());
        let cache = SearchCache::new();
        let polls = AtomicUsize::new(0);
        let cancel = move || polls.fetch_add(1, Ordering::Relaxed) >= 3;
        warm_vault(&v, &cache, &cancel);
        // Stopped early: not all 50 notes were warmed.
        assert!(cache.cached_bytes() < 50 * "some body text\n".len() as u64);
    }

    #[test]
    fn multi_vault_one_shared_cache_matches_serial() {
        // Regression: the per-vault scan threads share ONE &SearchCache; the
        // concurrent get/insert + byte-counter path must still produce the
        // serial-identical, vault-ordered, budget-capped result.
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let c = tempfile::tempdir().unwrap();
        for i in 0..60 {
            write(a.path(), &format!("alpha a{i:02}.md"), "x\n");
            write(b.path(), &format!("alpha b{i:02}.md"), "x\n");
            write(c.path(), &format!("alpha c{i:02}.md"), "x\n");
        }
        let vs = [
            vault("va", "A", a.path()),
            vault("vb", "B", b.path()),
            vault("vc", "C", c.path()),
        ];
        let serial = search_vaults(&vs, "alpha");
        let shared = SearchCache::new();
        let concurrent = search_vaults_with_cache(&vs, "alpha", &shared, &|| false);
        assert_eq!(serial, concurrent);
    }
```

`Vault` must be `Clone` for `v.clone()` above — it already derives `Clone` (see `discovery.rs`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri/core && cargo test --lib search_cache`
Expected: FAIL to compile — `warm_vault` not found in `crate::search`.

- [ ] **Step 3: Implement `warm_vault`**

Add to `src-tauri/core/src/search.rs` (near the other public entry points):

```rust
/// Populate `cache` with one vault's note content WITHOUT matching — the
/// pre-warm's unit of work, so the warm and lazy search paths share
/// `cached_lowered` and can never diverge. Walks the same reparse-safe walk a
/// search does and polls `is_cancelled` per file (a shutdown or superseded warm
/// stops promptly). Best-effort: an unresolvable vault path is skipped, and
/// per-file read failures degrade exactly as the live scan's do. Only notes are
/// warmed — attachments are name-only, so there is nothing to cache for them.
pub fn warm_vault(vault: &Vault, cache: &SearchCache, is_cancelled: &(dyn Fn() -> bool + Sync)) {
    let Ok(canon_root) = std::fs::canonicalize(Path::new(&vault.path)) else {
        return;
    };
    walk_vault(&canon_root, &mut |path, name| {
        if is_cancelled() {
            return Flow::Stop;
        }
        if !name.starts_with('.') && md_stem(name).is_some() {
            let _ = cached_lowered(path, cache);
        }
        Flow::Continue
    });
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri/core && cargo test --lib search_cache`
Expected: PASS (Task 1's 5 tests plus the 3 integration tests).

- [ ] **Step 5: Lint, format, LOC gate, full core suite**

Run: `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Then repo root: `npm run check:loc`
Expected: all pass; both `search.rs` and `search_cache.rs` under 800 nonblank.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/src/search.rs src-tauri/core/src/search_cache.rs
git commit -m "feat(search): add warm_vault to bulk-populate the content cache

warm_vault walks a vault and fills the cache via the same cached_lowered
primitive the live scan uses, so pre-warm and lazy paths can't diverge.
Cancellable per file. Integration tests pin warm==cold equivalence
(incl. multibyte + non-ASCII query) and cache reuse across searches."
```

---

### Task 4: Shell — long-lived cache wired into the search command

**Files:**
- Modify: `src-tauri/src/search_commands.rs`

**Interfaces:**
- Consumes: `search::search_vaults_with_cache`, `search::SearchCache`.
- Produces: `pub(crate) fn search_cache() -> &'static search::SearchCache` (used by the command here and the prewarm thread in Task 5).

- [ ] **Step 1: Add the static cache accessor**

In `src-tauri/src/search_commands.rs`, add `OnceLock` to the imports and the accessor below the existing `SCAN_GENERATION` static:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use vault_buddy_core::{discovery, search, uri};

// ... existing SCAN_GENERATION ...

/// The process-lifetime search content cache. Lazily created on first use and
/// shared by every `search_vaults` call and the `search-prewarm` thread, so
/// searches reuse note content read once. Touched ONLY inside `spawn_blocking`
/// / the prewarm thread — never on the main thread — so no window invariant is
/// affected.
static SEARCH_CACHE: OnceLock<search::SearchCache> = OnceLock::new();

pub(crate) fn search_cache() -> &'static search::SearchCache {
    SEARCH_CACHE.get_or_init(search::SearchCache::new)
}
```

- [ ] **Step 2: Route the command through the cache**

In the `search_vaults` command, change the `spawn_blocking` closure to call the cache-backed entry point. Replace the `search::search_vaults_with_cancel(&vaults, &query, &stale)` call with:

```rust
        search::search_vaults_with_cache(&vaults, &query, search_cache(), &stale)
```

(The surrounding `SCAN_GENERATION` bump, `stale` closure, discovery, and error handling are unchanged.)

- [ ] **Step 3: Compile-gate the shell**

Prerequisite (once per environment): `npm ci && npm run setup:linux && npm run build` (produces `../dist`).
Run: `npx tauri build --no-bundle`
Expected: builds clean (this is a compile gate — it does not launch the app).

- [ ] **Step 4: Shell tests and workspace clippy**

Run:
```bash
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
cd src-tauri && cargo test -p vault-buddy --lib
cd src-tauri && cargo fmt --check
```
Expected: no warnings; existing shell tests pass. (No new shell unit test: the command is thin glue over discovery + the core entry point, which Tasks 1–3 cover; behavior is verified by the compile gate plus the core suite.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/search_commands.rs
git commit -m "perf(search): back the search command with a process-lifetime cache

Add a static SEARCH_CACHE (OnceLock) and feed it into the existing
spawn_blocking scan so repeated searches reuse note content. Touched only
off the main thread; async/off-main and never-writes invariants intact."
```

---

### Task 5: Shell — background `search-prewarm` thread

**Files:**
- Modify: `src-tauri/src/lib.rs` (add `schedule_search_prewarm`, wire it into `setup`)

**Interfaces:**
- Consumes: `search_commands::search_cache`, `vault_buddy_core::search::warm_vault`, `vault_buddy_core::discovery::discover_vaults`, `capture_commands::is_recording`.
- Produces: `fn schedule_search_prewarm(app: &tauri::AppHandle)`.

- [ ] **Step 1: Add the prewarm function**

In `src-tauri/src/lib.rs`, add this function next to `schedule_show_bubble` (around line 190). It mirrors `run_recovery`'s coarse `is_recording` gate and `schedule_show_bubble`'s settle-then-work shape:

```rust
/// How many 5s waits the prewarm will sit through for an in-progress recording
/// before leaving the rest of the warm to the lazy path — 720 × 5s ≈ 1h, far
/// beyond a normal session, so a wedged recording state can't pin the thread.
const PREWARM_MAX_RECORDING_WAITS: u32 = 720;

/// Warm the search content cache in the background so the FIRST search of a
/// launch is fast too (every later one already is, once the cache is warm).
/// Scheduled last in `setup`, past the critical startup sequence, and settles
/// briefly before touching the disk in bulk. Warms one vault at a time, pausing
/// while a recording is active — the same coarse per-vault `is_recording` gate
/// `run_recovery` uses — so it never contends with the capture MP3 stream's
/// fsync. Best-effort and read-only: it only fills RAM, never writes, and is
/// reclaimed on process exit like the metronome and `capture-recovery` threads.
/// A spawn failure just skips the warm (the lazy path still warms on first use).
fn schedule_search_prewarm(app: &tauri::AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("search-prewarm".into())
        .spawn(move || {
            // Let restore/tray/recovery/MCP settle before bulk disk reads.
            std::thread::sleep(std::time::Duration::from_secs(3));
            let cache = search_commands::search_cache();
            for vault in vault_buddy_core::discovery::discover_vaults() {
                // Coarse politeness: never warm while recording — wait it out
                // (bounded) so we can't fight the encoder's fsync. One check
                // per vault, exactly like capture recovery.
                let mut waited = 0u32;
                while capture_commands::is_recording(&app) {
                    if waited >= PREWARM_MAX_RECORDING_WAITS {
                        log::info!(
                            "search-prewarm: still recording; leaving the rest to lazy warm"
                        );
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    waited += 1;
                }
                vault_buddy_core::search::warm_vault(&vault, cache, &|| false);
                // Stay low-priority: a short breath between vaults.
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            log::info!(
                "search-prewarm: content cache warmed ({} bytes)",
                cache.cached_bytes()
            );
        });
    if let Err(e) = spawned {
        log::warn!("could not spawn search-prewarm thread: {e}");
    }
}
```

- [ ] **Step 2: Wire it into `setup`**

In `src-tauri/src/lib.rs`, immediately after the existing `mcp_commands::start_if_enabled(app.handle());` line (around line 481), add:

```rust
            mcp_commands::start_if_enabled(app.handle());
            schedule_search_prewarm(app.handle());
```

- [ ] **Step 3: Compile-gate the shell**

Run: `npx tauri build --no-bundle`
Expected: builds clean.

- [ ] **Step 4: Workspace clippy, shell tests, format**

Run:
```bash
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
cd src-tauri && cargo test -p vault-buddy --lib
cd src-tauri && cargo fmt --check
```
Expected: no warnings; shell tests pass.

- [ ] **Step 5: Manual smoke (Windows or a GUI-capable run — verification note)**

This background thread cannot be exercised by CI's headless jobs. When running the full app (`npm run test-build` on Windows, or a manual local run), confirm the log contains `search-prewarm: content cache warmed (<N> bytes)` a few seconds after launch, and that a first search returns quickly. Record the observation in the PR. (Automated coverage is the compile gate + the `warm_vault` core tests.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(search): pre-warm the content cache on launch

Add a named search-prewarm thread that warms the cache in the background
after the critical startup sequence, pausing per-vault while a recording
is active so it never contends with the capture fsync. Reclaimed on exit
like the metronome/recovery threads. Makes the first search fast too."
```

---

### Task 6: Documentation

**Files:**
- Modify: `AGENTS.md` (search domain section, repository map, named-threads note)
- Modify: `docs/Gaps.md` (a Low entry for the fill-to-cap tail / dead entries)

**Interfaces:** none (docs only).

- [ ] **Step 1: Update the AGENTS.md search domain opening**

In `AGENTS.md`, the search domain section currently opens: "Cross-vault, read-only, on-demand search (no index): `core::search::search_vaults` walks every registered vault...". Replace the "(no index)" framing and add the cache. Change the opening sentence to:

```
Cross-vault, read-only, on-demand search — **no persistent index**, but backed
by a process-lifetime, `(mtime,size)`-invalidated in-memory **content cache**
(`core::search_cache`, 256 MiB fill-to-cap) so repeated and pre-warmed searches
skip the read + lowercase that dominates a cold scan: `core::search::search_vaults`
walks every registered vault...
```

Then, near the async-command discussion in that same section, add a sentence:

```
The scan reads note content through the cache (`search_vaults_with_cache`); a
shell-owned `static SEARCH_CACHE` (in `search_commands.rs`) is fed into the
`spawn_blocking` scan, and a named `search-prewarm` thread (wired last in
`setup`, paused per-vault while recording) warms it on launch so even the first
search is fast. The cache is touched only off the main thread, holds lowered
text keyed by `(mtime,size)`, and never changes what a search returns.
```

- [ ] **Step 2: Update the repository map**

In `AGENTS.md`, the `core/src/` line of the repository map lists modules: "discovery, uri, daily_notes, search, tasks, services,". Add `search_cache`:

```
    │   ├── core/src/            # PURE crate: discovery, uri, daily_notes, search, search_cache, tasks, services,
```

- [ ] **Step 3: Add a Gaps.md entry**

In `docs/Gaps.md`, under the appropriate Low section, add:

```
### GAP-56 · Low · Search content cache: fill-to-cap tail and dead entries
`core/src/search_cache.rs`. The cache fills to 256 MiB then stops inserting
(no eviction — uniform per-search access makes LRU pointless), so once total
note text exceeds the cap the last-walked vaults' notes re-read on every search
(still far cheaper than the pre-cache path). Entries for deleted files also
linger until process exit, bounded by the cap. A per-walk mark-and-sweep and/or
a larger/tunable cap would address both; deferred as documented in the spec.
```

(GAP-55 is the current highest; GAP-56 is the next free number.)

- [ ] **Step 4: Verify docs render and commit**

Re-read the edited AGENTS.md section and Gaps.md entry for accuracy (no broken references; the module name `search_cache` matches the created file).

```bash
git add AGENTS.md docs/Gaps.md
git commit -m "docs(search): document the content cache and prewarm thread

Rewrite the search domain 'no index' framing to describe the mtime-
invalidated in-memory content cache + search-prewarm thread, add
search_cache to the repository map, and log the fill-to-cap tail as a
Low gap."
```

---

## Verification (whole-feature, before opening the PR)

- [ ] **Core:** `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
- [ ] **Coverage floor:** `cd src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe --fail-under-lines 94`
- [ ] **Shell:** `npx tauri build --no-bundle` then `cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib`
- [ ] **LOC/quality:** from repo root `npm run lint && npm run check:loc && npm run check:quality`
- [ ] **Performance evidence (the point of the change):** add temporary debug logging of scan duration + cache hit/miss counts (or reuse `cached_bytes()`), build a synthetic ~20k-note corpus, and record before/after warm-search timings in the PR description. Remove the temporary logging (or keep it at `debug` level) before merge. **Do not claim the speedup without these numbers.**
