//! The in-memory content cache behind cross-vault search: a process-lifetime,
//! `(mtime, size)`-invalidated mirror of note *text* so repeated and
//! pre-warmed searches skip the read + lowercase that dominates a cold scan.
//! NOT an index — no tokenization, no persistence, no write-hooks; `(mtime,
//! size)` self-invalidates. See
//! docs/superpowers/specs/2026-07-11-search-performance-content-cache-design.md.

use crate::search::MAX_CONTENT_BYTES;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    /// Latched once an insert is rejected for being over `cap_bytes`. Never
    /// cleared: the cache only grows for the process lifetime, so once full it
    /// stays full. Lets `warm_vault` stop walking instead of reading +
    /// lowercasing notes it can only discard (PR #52 Codex P2) — a live search
    /// ignores this flag, since it still needs the content to match.
    full: AtomicBool,
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
            CacheEntry::Text {
                mtime: m, size: s, ..
            }
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
            full: AtomicBool::new(false),
        }
    }

    /// Total cached lowered-text bytes. Diagnostics + tests.
    pub fn cached_bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// Whether an insert has ever been rejected for being over `cap_bytes`.
    /// `warm_vault` polls this to stop pre-warming once further reads can
    /// only be discarded; a live search ignores it (matching still needs the
    /// content even when it won't be retained).
    pub fn is_full(&self) -> bool {
        self.full.load(Ordering::Relaxed)
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
        // A path cached as Text while <= MAX that has since grown past the cap
        // must not keep its now-unreachable bytes counted for the process
        // lifetime — drop and reconcile any existing entry before bailing.
        let mut map = cache.lock();
        if let Some(CacheEntry::Text { lowered, .. }) = map.remove(path) {
            cache
                .bytes
                .fetch_sub(lowered.len() as u64, Ordering::Relaxed);
        }
        return None;
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
        Ok(text) => Some(Arc::<str>::from(text.to_lowercase())),
        // Only genuine non-UTF-8 is a permanent property of this (mtime,size)
        // and worth memoizing as Uncacheable below. A transient IO error
        // (Windows sharing violation, AV/backup lock) after `metadata` already
        // succeeded must NOT be memoized — the pre-cache search path retried
        // every scan; return None WITHOUT recording an entry so the next search
        // retries once the file is readable again.
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => None,
        Err(_) => {
            // Transient error (e.g. a Windows sharing violation / AV-backup lock):
            // reconcile a stale Text entry so its bytes aren't retained for the
            // process lifetime, but do NOT memoize an Uncacheable entry — the next
            // search must still retry once the file is readable again.
            let mut map = cache.lock();
            if let Some(CacheEntry::Text { lowered, .. }) = map.remove(path) {
                cache
                    .bytes
                    .fetch_sub(lowered.len() as u64, Ordering::Relaxed);
            }
            return None;
        }
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
                    CacheEntry::Text {
                        mtime,
                        size,
                        lowered: text.clone(),
                    },
                );
            } else {
                // Over the cap: don't retain. Drop any stale entry so a later
                // search can't serve it, and latch `full` so warm_vault stops
                // reading notes it can only discard (a live search ignores it).
                map.remove(path);
                cache.full.store(true, Ordering::Relaxed);
            }
        }
        None => {
            map.insert(path.to_path_buf(), CacheEntry::Uncacheable { mtime, size });
        }
    }
    lowered
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::Vault;
    use crate::search::{search_vaults, search_vaults_with_cache, warm_vault};
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn write(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    }

    fn vault(id: &str, name: &str, path: &Path) -> Vault {
        Vault {
            id: id.to_string(),
            name: name.to_string(),
            path: path.to_string_lossy().into_owned(),
            open: false,
        }
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
    fn growing_past_the_cap_after_being_cached_reconciles_stale_bytes() {
        // Regression: a path cached as Text while <= MAX_CONTENT_BYTES that
        // later grows past the cap must not keep its now-unreachable bytes
        // counted for the process lifetime. The old oversize branch returned
        // None before ever consulting the map, so a stale Text entry (and its
        // byte-counter contribution) was never removed.
        let dir = tempfile::tempdir().unwrap();
        let p = write(dir.path(), "grows.md", "alpha\n");
        let c = SearchCache::new();
        assert!(cached_lowered(&p, &c).is_some());
        assert!(c.cached_bytes() > 0);
        let big = "x".repeat(crate::search::MAX_CONTENT_BYTES as usize + 1);
        std::fs::write(&p, big).unwrap();
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

    #[cfg(unix)]
    #[test]
    fn transient_read_error_is_not_memoized() {
        // Regression (PR #52 Codex P2): a read failure that is NOT invalid
        // UTF-8 (here EISDIR, standing in for a Windows sharing/AV lock) must
        // not be memoized as Uncacheable — the pre-cache search path retried
        // every scan, so we return None without recording an entry.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.md");
        std::fs::create_dir(&p).unwrap(); // a directory named like a note
        let c = SearchCache::new();
        assert!(cached_lowered(&p, &c).is_none());
        assert!(!c.is_uncacheable(&p)); // NOT memoized — retried next search
        assert_eq!(c.cached_bytes(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn transient_error_with_prior_entry_reconciles_bytes() {
        // Regression: a transient (non-InvalidData) read error on a path
        // previously cached as Text must reconcile the stale entry's bytes
        // (not leak them for the process lifetime) while still NOT memoizing.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.md");
        std::fs::write(&p, "hello world\n").unwrap();
        let c = SearchCache::new();
        assert!(cached_lowered(&p, &c).is_some());
        assert!(c.cached_bytes() > 0); // cached as Text
                                       // Replace the file with a directory: changes (mtime,size) so the entry
                                       // is stale, and the next read fails with EISDIR (a non-InvalidData,
                                       // transient-class error).
        std::fs::remove_file(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
        assert!(cached_lowered(&p, &c).is_none());
        assert_eq!(c.cached_bytes(), 0); // stale bytes reconciled, not leaked
        assert!(!c.is_uncacheable(&p)); // still not memoized
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

    #[test]
    fn warm_vault_stops_once_cache_is_full() {
        // Regression (PR #52): once the cache hits its cap, warm_vault must
        // stop walking instead of reading + lowercasing notes it can only
        // discard. A tiny cap fits one ~100-byte note; the second trips
        // `full`, and a later small note that WOULD fit the headroom is not
        // warmed (it falls to the lazy path).
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.md", &"a".repeat(100));
        write(dir.path(), "b.md", &"b".repeat(100));
        let v = vault("v1", "W", dir.path());
        let cache = SearchCache::with_cap(120); // fits one 100-byte note
        warm_vault(&v, &cache, &|| false);
        assert!(cache.is_full());
        let after = cache.cached_bytes();
        assert!(after <= 120);
        // A second vault with a 10-byte note (fits the 20-byte headroom) must
        // NOT be warmed now that `full` is latched.
        let dir2 = tempfile::tempdir().unwrap();
        write(dir2.path(), "c.md", &"c".repeat(10));
        warm_vault(&vault("v2", "W2", dir2.path()), &cache, &|| false);
        assert_eq!(cache.cached_bytes(), after); // is_full stopped the warm
    }

    #[test]
    fn warm_then_search_is_a_pure_cache_hit() {
        let dir = tempfile::tempdir().unwrap();
        let note = write(dir.path(), "n.md", "Project Alpha\n");
        let v = vault("v1", "W", dir.path());
        let cache = SearchCache::new();
        warm_vault(&v, &cache, &|| false);
        assert!(cache.cached_bytes() > 0);
        let before = cache
            .peek_text(&std::fs::canonicalize(&note).unwrap())
            .unwrap();
        let _ = search_vaults_with_cache(&[v], "alpha", &cache, &|| false);
        let after = cache
            .peek_text(&std::fs::canonicalize(&note).unwrap())
            .unwrap();
        // The search reused the warmed entry — no re-insert.
        assert!(Arc::ptr_eq(&before, &after));
    }

    #[test]
    fn warmed_search_matches_cold_search_including_multibyte() {
        // Equivalence: a warmed cache must not change results, including
        // multibyte content and a non-ASCII query (the lowercase fallback path).
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "Notes/idea.md",
            "intro\nProject Alpha kickoff\n",
        );
        write(dir.path(), "cafe.md", "Beim Kaffee in der Straße\n");
        write(dir.path(), "Alpha plan.md", "nothing relevant\n");
        let v = vault("v1", "Work", dir.path());
        for q in ["alpha", "straße", "kaffee"] {
            let cold = search_vaults(std::slice::from_ref(&v), q);
            let cache = SearchCache::new();
            warm_vault(&v, &cache, &|| false);
            let warm = search_vaults_with_cache(std::slice::from_ref(&v), q, &cache, &|| false);
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
}
