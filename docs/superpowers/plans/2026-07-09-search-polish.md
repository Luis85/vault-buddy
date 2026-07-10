# Search Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the six verified correctness fixes, the walk/DTO/highlight consolidation, the two scan-performance wins, and the keyboard/icon/indicator/count UX upgrades on the existing cross-vault search — same branch, PR #44 updates in place.

**Architecture:** Core gains a shared `vault_walk` (single-sourced escape/cycle discipline), a two-list per-vault collector (filename-first as a hard guarantee), cancellation + parallel named-thread scans, and Serialize derives (DTO layer deleted). The shell command becomes `Result` + a scan-generation atomic. `Search.vue` gets a code-point query gate, one `results` ref, a `HighlightText` subcomponent, and keyboard navigation. Spec: `docs/superpowers/specs/2026-07-09-search-polish-design.md`.

**Tech Stack:** Rust std only (no new crates; `std::thread::scope` + named `Builder` threads), serde derives already in core, Vue 3 + Vitest as before.

## Global Constraints

- TDD: failing test → implement → pass → commit. Regression tests name the failure mode in a comment.
- Search stays read-only; `obsidian://` opens via `uri::launch`; vaults by ID.
- Every spawned thread is NAMED (`std::thread::Builder`); no swallowed errors (log via `log::warn!` / `src/logging.ts`).
- Constants unchanged: `MIN_QUERY_CHARS = 2` (code points), `MAX_RESULTS = 100`, `MAX_CONTENT_BYTES = 1 MiB`, `PER_VAULT_CAP = MAX_RESULTS + 1`, debounce `300` ms.
- Gates stay green after every task: relevant Vitest files / `cargo test` in core; full battery in the final task. `cd src-tauri && cargo fmt --check`; clippy `-D warnings` on core.
- Wire names are camelCase (`vaultId`, `vaultName`, `isNote`, …) — pinned by a serde test.

---

### Task 1: `uri::vault_relative` (keep-extension) + shared tail

**Files:**
- Modify: `src-tauri/core/src/uri.rs`

**Interfaces:**
- Produces: `pub fn vault_relative(file: &Path, vault_root: &Path) -> Option<String>` (extension kept) and refactored `pub fn vault_relative_no_ext(...)` — both via one private `fn rel_to_uri_form(rel: &Path) -> Option<String>`. Task 3's `make_hit` consumes both.

- [ ] **Step 1: Write the failing tests** (append to `uri.rs` tests)

```rust
    #[test]
    fn vault_relative_keeps_the_extension() {
        use std::path::Path;
        let root = Path::new("/vault");
        assert_eq!(
            vault_relative(Path::new("/vault/slides/Alpha deck.pdf"), root).as_deref(),
            Some("slides/Alpha deck.pdf")
        );
        // outside the vault → None; the root itself → None
        assert_eq!(vault_relative(Path::new("/elsewhere/x.pdf"), root), None);
        assert_eq!(vault_relative(Path::new("/vault"), root), None);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cd src-tauri/core && cargo test uri::`
Expected: FAIL to compile — `cannot find function vault_relative`.

- [ ] **Step 3: Implement**

In `src-tauri/core/src/uri.rs`, add above `vault_relative_no_ext` and refactor it:

```rust
/// `file`'s location under `vault_root` in the `obsidian://open?file=` form
/// with the extension KEPT: vault-relative, `/`-separated. Used for
/// attachments (and non-lowercase-`.md` notes), where dropping the extension
/// would make Obsidian resolve the name as `<name>.md`. `None` when `file`
/// is not inside `vault_root`.
pub fn vault_relative(file: &std::path::Path, vault_root: &std::path::Path) -> Option<String> {
    rel_to_uri_form(file.strip_prefix(vault_root).ok()?)
}

/// Shared tail of the URI path form — one place derives it: `/`-separated,
/// empty (the vault root itself) → None.
fn rel_to_uri_form(rel: &std::path::Path) -> Option<String> {
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
```

and change `vault_relative_no_ext`'s body to:

```rust
pub fn vault_relative_no_ext(
    file: &std::path::Path,
    vault_root: &std::path::Path,
) -> Option<String> {
    let rel = file.strip_prefix(vault_root).ok()?;
    rel_to_uri_form(&rel.with_extension(""))
}
```

- [ ] **Step 4: Run tests** — `cargo test uri::` → all PASS (existing no_ext tests included).
- [ ] **Step 5: Commit** — `git add src-tauri/core/src/uri.rs && git commit -m "feat(search): keep-extension uri::vault_relative sharing the no_ext tail"`

---

### Task 2: extract `core::vault_walk`, rebase the tasks walk on it

**Files:**
- Create: `src-tauri/core/src/vault_walk.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod vault_walk;` — alphabetical, after `uri`), `src-tauri/core/src/tasks.rs`

**Interfaces:**
- Produces: `pub(crate) enum Flow { Continue, Stop }` and `pub(crate) fn walk_vault(canon_root: &Path, on_file: &mut dyn FnMut(&Path, &str) -> Flow)`. Tasks 2 (tasks) and 3 (search) consume it.
- Gate: the EXISTING tasks tests (subdirectory recursion, dot-dir skip, symlink no-escape, cycle termination) — this is a pure refactor, no new behavior.

- [ ] **Step 1: Create `src-tauri/core/src/vault_walk.rs`**

```rust
//! The shared reparse-safe recursive vault walk. Every vault-scanning
//! domain (tasks, search) drives this ONE walk through a per-file callback,
//! so the escape/cycle discipline is single-sourced instead of hand-synced
//! copies that can drift.

use crate::transcript::dir_entries;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Callback verdict: keep walking, or abort the entire walk (caps,
/// cancellation).
pub(crate) enum Flow {
    Continue,
    Stop,
}

/// Depth-first, name-ordered walk over every plain file under `canon_root`
/// (which the caller must have canonicalized). Discipline: dot-DIRECTORIES
/// are skipped (`.obsidian`, `.trash`, `.git`, …); a subdirectory is
/// descended only after canonicalizing it and confirming it still resolves
/// under `canon_root` — a symlink/junction escaping the vault is never
/// walked (the no-follow dirent type can't be trusted for a junction on
/// Windows); a walked-set breaks reparse cycles; symlinked files are
/// skipped (no-follow). Dot-FILES are passed through — per-domain filters
/// belong in the callback (search skips them, tasks deliberately considers
/// them). Entries are visited in name order so walk order — and anything
/// derived from it — is deterministic. Unreadable dirs degrade silently
/// (scan noise, the documented exception to the no-swallow rule).
pub(crate) fn walk_vault(canon_root: &Path, on_file: &mut dyn FnMut(&Path, &str) -> Flow) {
    let mut walked = HashSet::new();
    walk_dir(canon_root, canon_root, &mut walked, on_file);
}

fn walk_dir(
    dir: &Path,
    canon_root: &Path,
    walked: &mut HashSet<PathBuf>,
    on_file: &mut dyn FnMut(&Path, &str) -> Flow,
) -> Flow {
    if !walked.insert(dir.to_path_buf()) {
        return Flow::Continue; // already walked — reparse-point cycle guard
    }
    let mut entries = dir_entries(dir);
    entries.sort_by(|a, b| a.2.cmp(&b.2));
    for (path, ft, name) in entries {
        if ft.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            match std::fs::canonicalize(&path) {
                Ok(child) if child.starts_with(canon_root) => {
                    if let Flow::Stop = walk_dir(&child, canon_root, walked, on_file) {
                        return Flow::Stop;
                    }
                }
                _ => continue,
            }
            continue;
        }
        if !ft.is_file() {
            continue; // symlinked files are not followed
        }
        if let Flow::Stop = on_file(&path, &name) {
            return Flow::Stop;
        }
    }
    Flow::Continue
}
```

- [ ] **Step 2: Register** — in `src-tauri/core/src/lib.rs` add `pub mod vault_walk;` after `pub mod uri;`.

- [ ] **Step 3: Rebase `tasks::list_tasks` on the shared walk**

In `src-tauri/core/src/tasks.rs`: delete `collect_tasks` (and the now-unused `use crate::transcript::dir_entries;` + `use std::collections::HashSet;`), and replace with:

```rust
pub fn list_tasks(root: &Path) -> Vec<TaskItem> {
    let mut out = Vec::new();
    // The walk discipline (canonical containment, cycle set, dot-dir skip)
    // lives in vault_walk, single-sourced with the search scan. A missing/
    // unresolvable root → empty list (best-effort, unchanged).
    if let Ok(canon_root) = std::fs::canonicalize(root) {
        crate::vault_walk::walk_vault(&canon_root, &mut |path, name| {
            collect_task_file(path, name, &mut out);
            crate::vault_walk::Flow::Continue
        });
    }
    // Open first; within each group newest created first, then title. Sorting
    // once here (not per directory) orders the whole subtree as one list.
    out.sort_by(|a, b| {
        a.done
            .cmp(&b.done)
            .then(b.created.cmp(&a.created))
            .then(a.title.cmp(&b.title))
    });
    out
}

/// The per-file half of the old recursive collector: read, keep `type: Task`
/// files, map to a TaskItem. Unreadable files and non-tasks degrade silently.
fn collect_task_file(path: &Path, name: &str, out: &mut Vec<TaskItem>) {
    if !name.ends_with(".md") {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    if !is_task(&content) {
        return;
    }
    let stem = name.strip_suffix(".md").unwrap_or(name).to_string();
    let title = note_field(&content, "title").unwrap_or(stem);
    let status = note_field(&content, "status").unwrap_or_else(|| "new".to_string());
    // Archived tasks are removed from view — never surfaced in the list.
    if status == "archived" {
        return;
    }
    let created = note_field(&content, "created").unwrap_or_default();
    let done = status == "done";
    out.push(TaskItem {
        path: path.to_path_buf(),
        title,
        status,
        created,
        done,
    });
}
```

(Keep the doc comment on `list_tasks` as-is; the old `collect_tasks` doc's walk details now live on `vault_walk`.)

- [ ] **Step 4: Run the gate** — `cd src-tauri/core && cargo test tasks:: && cargo test search::`
Expected: ALL existing tests pass unchanged (recursion, dot-dir skip, symlink escape, cycle, sort order). Search still uses its own walk until Task 3 — both suites must be green.

- [ ] **Step 5: Clippy + fmt + commit**

`cargo clippy --all-targets -- -D warnings`, `cd .. && cargo fmt --check`, then:
`git add src-tauri/core/src/vault_walk.rs src-tauri/core/src/lib.rs src-tauri/core/src/tasks.rs && git commit -m "refactor(core): single-source the reparse-safe vault walk"`

---

### Task 3: search matching rework (two-list guarantee, case-insensitive .md, extensionless exclusion, is_note, early-out)

**Files:**
- Modify: `src-tauri/core/src/search.rs`

**Interfaces:**
- Consumes: `vault_walk::{walk_vault, Flow}` (Task 2), `uri::{vault_relative, vault_relative_no_ext}` (Task 1).
- Produces: `SearchHit` gains `pub is_note: bool`; both structs derive `serde::Serialize` + `#[serde(rename_all = "camelCase")]`; internal `fn scan_vault(vault, query_lower, is_cancelled) -> Option<VaultHits>` and `struct VaultHits { name_hits: Vec<SearchHit>, content_hits: Vec<SearchHit> }` (Task 4's merge consumes these); `search_vaults` signature unchanged for now.

- [ ] **Step 1: Write the failing tests** (append to `search.rs` tests; also DELETE the narration comment lines "// `Vault`, `Path` etc. arrive via the module's own imports..." above `fn vault`)

```rust
    #[test]
    fn uppercase_md_extension_is_a_note() {
        // Regression: case-sensitive strip_suffix(".md") classified Plan.MD
        // as an attachment, so its content was silently never searched
        // (Windows filesystems are case-insensitive; other tools save .MD).
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Plan.MD", "project alpha\n");
        let r = search_vaults(&[vault("v1", "W", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        let h = &r.hits[0];
        assert_eq!(h.name, "Plan"); // displayed by stem
        assert!(h.is_note);
        // The URI keeps the extension unless it is exactly ".md": file=Plan.MD
        // opens by exact path instead of gambling on resolver guessing.
        assert_eq!(h.file, "Plan.MD");
        assert!(h.snippet.as_deref().unwrap().contains("alpha"));
    }

    #[test]
    fn filename_match_survives_a_flood_of_content_matches() {
        // Regression: v1 capped collection in walk order BEFORE the
        // filename-first sort, so a name-matched file walking after 101
        // content matches was dropped entirely — the exact-name hit vanished
        // while content-only hits filled the list.
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_RESULTS + 20) {
            write(
                dir.path(),
                &format!("a {i:03}.md"),
                "contains alpha here\n",
            );
        }
        write(dir.path(), "zzz alpha last.md", "no match in body\n");
        let r = search_vaults(&[vault("v1", "W", dir.path())], "alpha");
        assert!(r.truncated);
        assert_eq!(r.hits.len(), MAX_RESULTS);
        assert_eq!(r.hits[0].name, "zzz alpha last"); // hard guarantee
    }

    #[test]
    fn extensionless_files_are_excluded() {
        // Obsidian doesn't index extensionless files: obsidian://open on one
        // resolves to the like-named NOTE, and its `file` value collides
        // with the note's dropped-.md form (duplicate row keys). Never
        // surfaced.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Notes/idea.md", "x\n");
        write(dir.path(), "Notes/idea", "x\n");
        let r = search_vaults(&[vault("v1", "W", dir.path())], "idea");
        assert_eq!(r.hits.len(), 1);
        assert!(r.hits[0].is_note);
        assert_eq!(r.hits[0].file, "Notes/idea");
    }

    #[test]
    fn search_hit_serializes_camel_case() {
        // The command returns core types directly (DTO layer deleted) — the
        // wire names the TS interfaces expect are pinned here.
        let hit = SearchHit {
            vault_id: "v".into(),
            vault_name: "V".into(),
            name: "n".into(),
            folder: String::new(),
            file: "n".into(),
            snippet: None,
            is_note: true,
        };
        let v = serde_json::to_value(&hit).unwrap();
        for key in ["vaultId", "vaultName", "isNote", "folder", "file", "snippet", "name"] {
            assert!(v.get(key).is_some(), "missing wire key {key}");
        }
    }
```

Also update the existing `attachment_matches_by_filename_and_keeps_extension` test: add `assert!(!h.is_note);` after the snippet assert.

- [ ] **Step 2: Run to verify failure** — `cargo test search::`
Expected: FAIL to compile (`is_note` unknown field), plus the two behavior tests failing once it compiles.

- [ ] **Step 3: Implement the rework**

In `src-tauri/core/src/search.rs`:

1. Struct derives + field (replace both derive lines and add the field):

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub vault_id: String,
    pub vault_name: String,
    /// Display name: file stem for notes, full filename for attachments.
    pub name: String,
    /// Vault-relative parent folder, `/`-separated, "" for the vault root.
    pub folder: String,
    pub file: String,
    /// First matching content line (notes only; None for name-only matches).
    pub snippet: Option<String>,
    /// Note (any-case `.md`) vs attachment — drives the row icon and the
    /// kind-suffixed row key in the frontend.
    pub is_note: bool,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}
```

2. Delete `struct RawHit` and the whole `collect_hits` function. Replace `search_vaults` and add the new collector:

```rust
/// Search every vault, in the given order, until `MAX_RESULTS` hits are
/// collected. A trimmed query shorter than `MIN_QUERY_CHARS` returns an
/// empty response. Read-only, best-effort: missing/unreadable vaults, dirs
/// and files degrade silently — scan noise, the same documented exception
/// to the no-swallow rule as the tasks/recordings walks.
pub fn search_vaults(vaults: &[Vault], query: &str) -> SearchResponse {
    search_vaults_with_cancel(vaults, query, &|| false)
}

/// One vault's matches: filename matches then content-only matches, walk
/// order within each class. Two independently-capped lists make "filename
/// matches surface before content-only matches" a HARD guarantee: when the
/// content list fills, the walk stops READING file contents but keeps
/// checking NAMES to the end of the vault (dirent string ops — cheap), so a
/// late-walking filename match can never be displaced by earlier content
/// matches; only a full filename list aborts the walk. Since PER_VAULT_CAP
/// (101) exceeds any possible budget (≤ 100), a full list always surfaces
/// as `truncated` through the merge's budget accounting — no separate
/// overflow flag is needed. `is_cancelled` is polled once per file; a
/// cancelled scan returns what it has (the caller is about to discard it).
/// `None` = unresolvable vault path (moved/deleted), skipped silently.
fn scan_vault(
    vault: &Vault,
    query_lower: &str,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<VaultHits> {
    let canon_root = std::fs::canonicalize(Path::new(&vault.path)).ok()?;
    let mut name_hits: Vec<SearchHit> = Vec::new();
    let mut content_hits: Vec<SearchHit> = Vec::new();
    crate::vault_walk::walk_vault(&canon_root, &mut |path, name| {
        use crate::vault_walk::Flow;
        if is_cancelled() {
            return Flow::Stop;
        }
        if name.starts_with('.') {
            return Flow::Continue; // dot-files: .DS_Store, our .mp3.part temps
        }
        if let Some(stem) = md_stem(name) {
            let name_matched = stem.to_lowercase().contains(query_lower);
            // Once the content list is full the read is pure waste for
            // content classification — but a name-matched note still reads
            // for its display snippet.
            let snippet = if name_matched || content_hits.len() < PER_VAULT_CAP {
                content_snippet(path, query_lower)
            } else {
                None
            };
            if name_matched {
                if name_hits.len() < PER_VAULT_CAP {
                    if let Some(hit) = make_hit(vault, &canon_root, path, stem, snippet, true) {
                        name_hits.push(hit);
                    }
                } else {
                    return Flow::Stop; // both classes can't grow further
                }
            } else if snippet.is_some() && content_hits.len() < PER_VAULT_CAP {
                if let Some(hit) = make_hit(vault, &canon_root, path, stem, snippet, true) {
                    content_hits.push(hit);
                }
            }
        } else {
            // Attachment: filename match only. Extensionless files are
            // excluded — Obsidian doesn't index them, so opening would
            // resolve to the like-named note, and their `file` value
            // collides with a note's dropped-.md form.
            if Path::new(name).extension().is_none() {
                return Flow::Continue;
            }
            if name.to_lowercase().contains(query_lower) {
                if name_hits.len() < PER_VAULT_CAP {
                    if let Some(hit) = make_hit(vault, &canon_root, path, name, None, false) {
                        name_hits.push(hit);
                    }
                } else {
                    return Flow::Stop;
                }
            }
        }
        Flow::Continue
    });
    Some(VaultHits {
        name_hits,
        content_hits,
    })
}

/// A vault's collected matches, split by class (see `scan_vault`).
struct VaultHits {
    name_hits: Vec<SearchHit>,
    content_hits: Vec<SearchHit>,
}

/// Case-insensitive `.md` note check returning the stem. The suffix is 3
/// ASCII bytes, so the byte compare can't split a char boundary and
/// `len - 3` is a valid boundary; `> 3` keeps the stem non-empty (a bare
/// ".md"/".MD" is a dot-file anyway).
fn md_stem(name: &str) -> Option<&str> {
    let b = name.as_bytes();
    if b.len() > 3 && b[b.len() - 3..].eq_ignore_ascii_case(b".md") {
        Some(&name[..name.len() - 3])
    } else {
        None
    }
}
```

3. `search_vaults_with_cancel` — in THIS task keep it serial (Task 4 parallelizes); add below `search_vaults`:

```rust
/// Cancellable variant: `is_cancelled` is polled once per file. See
/// `scan_vault` for the per-vault collection contract.
pub fn search_vaults_with_cancel(
    vaults: &[Vault],
    query: &str,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> SearchResponse {
    let trimmed = query.trim();
    if trimmed.chars().count() < MIN_QUERY_CHARS {
        return SearchResponse::default();
    }
    let query_lower = trimmed.to_lowercase();
    merge_vault_hits(
        vaults
            .iter()
            .map(|v| scan_vault(v, &query_lower, is_cancelled))
            .collect(),
    )
}

/// Vault-order merge with the global budget: filename hits before content
/// hits per vault, `truncated` when anything is dropped (or the budget is
/// spent with vaults still unmerged — the flag only drives a "refine your
/// query" footer).
fn merge_vault_hits(per_vault: Vec<Option<VaultHits>>) -> SearchResponse {
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut truncated = false;
    for scanned in per_vault {
        let budget = MAX_RESULTS - hits.len();
        if budget == 0 {
            truncated = true;
            break;
        }
        let Some(v) = scanned else { continue };
        let mut vault_hits = v.name_hits;
        vault_hits.extend(v.content_hits);
        if vault_hits.len() > budget {
            truncated = true;
            vault_hits.truncate(budget);
        }
        hits.extend(vault_hits);
    }
    SearchResponse { hits, truncated }
}
```

4. `content_snippet` early-out (replace the body's scan):

```rust
/// First matching line's snippet, or None: no match, file larger than
/// `MAX_CONTENT_BYTES` (name matching still applies — only content is
/// skipped), unreadable, or not UTF-8.
fn content_snippet(path: &Path, query_lower: &str) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_CONTENT_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    // One whole-file lowercase + contains, then a per-line pass only on the
    // rare matching file — the per-line variant allocated a lowered String
    // for every line of every file on the live-search hot path. (A query
    // can only match within a line, so the pre-filter never lies.)
    if !content.to_lowercase().contains(query_lower) {
        return None;
    }
    content
        .lines()
        .find_map(|line| snippet_from_line(line, query_lower))
}
```

5. `make_hit` (replace; extension rule + is_note):

```rust
/// Assemble a hit for `path` (inside `canon_root` by the walk's containment).
/// The URI `file` param drops the extension ONLY for exactly-".md" notes
/// (Obsidian's canonical form); any other name keeps it — `file=Plan.MD`
/// opens by exact path instead of gambling that the resolver maps the
/// extensionless form back. `None` only if the path can't be made
/// vault-relative, which the walk guarantees against (pure defense).
fn make_hit(
    vault: &Vault,
    canon_root: &Path,
    path: &Path,
    display_name: &str,
    snippet: Option<String>,
    is_note: bool,
) -> Option<SearchHit> {
    let rel = path.strip_prefix(canon_root).ok()?;
    let folder = rel
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let exactly_md = path.extension().and_then(|e| e.to_str()) == Some("md");
    let file = if is_note && exactly_md {
        crate::uri::vault_relative_no_ext(path, canon_root)?
    } else {
        crate::uri::vault_relative(path, canon_root)?
    };
    Some(SearchHit {
        vault_id: vault.id.clone(),
        vault_name: vault.name.clone(),
        name: display_name.to_string(),
        folder,
        file,
        snippet,
        is_note,
    })
}
```

6. Remove the now-unused `use crate::transcript::dir_entries;` and `use std::collections::HashSet;` / `PathBuf` imports from `search.rs` (the walk owns them now).

- [ ] **Step 4: Run** — `cargo test search:: && cargo test` → all green (existing ordering/cap/symlink/cycle tests included).
- [ ] **Step 5: Clippy + fmt + commit**

`git add src-tauri/core/src/search.rs && git commit -m "fix(search): filename-first guarantee, any-case .md notes, extensionless exclusion"` (body: name the three verified failure modes fixed + the early-out).

---

### Task 4: cancellation test + parallel named-thread scans in core

**Files:**
- Modify: `src-tauri/core/src/search.rs`

**Interfaces:**
- Consumes: `scan_vault`, `VaultHits`, `merge_vault_hits` (Task 3).
- Produces: `search_vaults_with_cancel` now scans vaults on named scoped threads; signature unchanged (Task 5 consumes it).

- [ ] **Step 1: Write the failing/locking tests**

```rust
    #[test]
    fn cancellation_stops_the_scan_early() {
        // A superseded scan must not walk the whole vault: the closure is
        // polled per file, and flipping true aborts the walk.
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        for i in 0..50 {
            write(dir.path(), &format!("alpha {i:02}.md"), "x\n");
        }
        let polls = AtomicUsize::new(0);
        let cancelled = move || polls.fetch_add(1, Ordering::Relaxed) >= 3;
        let r = search_vaults_with_cancel(&[vault("v1", "W", dir.path())], "alpha", &cancelled);
        assert!(r.hits.len() <= 3, "walk kept going: {} hits", r.hits.len());
    }

    #[test]
    fn parallel_scan_keeps_vault_order_and_budget_semantics() {
        // The scans run on one named thread per vault; the merge must keep
        // the given vault order and the exact serial budget accounting.
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let c = tempfile::tempdir().unwrap();
        for i in 0..60 {
            write(a.path(), &format!("alpha a{i:02}.md"), "x\n");
            write(b.path(), &format!("alpha b{i:02}.md"), "x\n");
            write(c.path(), &format!("alpha c{i:02}.md"), "x\n");
        }
        let r = search_vaults(
            &[
                vault("va", "A", a.path()),
                vault("vb", "B", b.path()),
                vault("vc", "C", c.path()),
            ],
            "alpha",
        );
        assert_eq!(r.hits.len(), MAX_RESULTS);
        assert!(r.truncated); // 180 total; C never fits the budget
        assert!(r.hits[..60].iter().all(|h| h.vault_id == "va"));
        assert!(r.hits[60..].iter().all(|h| h.vault_id == "vb"));
    }
```

- [ ] **Step 2: Run** — `cargo test search::` → `cancellation_stops_the_scan_early` PASSES already (Task 3 wired the closure); the parallel test passes against the serial implementation too — these tests LOCK the contract the parallel rewrite must preserve. Verify both green before the rewrite.

- [ ] **Step 3: Parallelize** — replace `search_vaults_with_cancel`'s body:

```rust
pub fn search_vaults_with_cancel(
    vaults: &[Vault],
    query: &str,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> SearchResponse {
    let trimmed = query.trim();
    if trimmed.chars().count() < MIN_QUERY_CHARS {
        return SearchResponse::default();
    }
    let query_lower = trimmed.to_lowercase();
    // One NAMED scoped thread per vault (crash records must identify the
    // dying thread), merged in the given vault order afterward — output is
    // identical to the serial loop, wall-clock is ~the slowest vault. Vault
    // counts are small (single digits), so no pooling.
    let mut per_vault: Vec<Option<VaultHits>> = Vec::with_capacity(vaults.len());
    std::thread::scope(|scope| {
        let mut pending = Vec::with_capacity(vaults.len());
        for (i, vault) in vaults.iter().enumerate() {
            let query_lower = &query_lower;
            let spawned = std::thread::Builder::new()
                .name(format!("search-vault-{i}"))
                .spawn_scoped(scope, move || scan_vault(vault, query_lower, is_cancelled));
            match spawned {
                Ok(handle) => pending.push(Ok(handle)),
                Err(e) => {
                    // Thread spawn failed (resource pressure): degrade to an
                    // inline scan on this thread — never a panic, and the
                    // failure leaves a trace.
                    log::warn!("search: spawning scan thread failed: {e}");
                    pending.push(Err(scan_vault(vault, query_lower, is_cancelled)));
                }
            }
        }
        for entry in pending {
            per_vault.push(match entry {
                Ok(handle) => handle.join().unwrap_or_else(|_| {
                    // A panicked scan thread loses that vault's hits, not the
                    // whole search; the panic hook already recorded it.
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

- [ ] **Step 4: Run** — `cargo test search::` → all green, including both Step-1 contract tests and the unix symlink/cycle tests.
- [ ] **Step 5: Clippy + fmt + commit** — `git add src-tauri/core/src/search.rs && git commit -m "feat(search): cancellable scans on named parallel vault threads"`

---

### Task 5: shell — Result command + scan generation, DTO layer deleted

**Files:**
- Modify: `src-tauri/src/search_commands.rs`

**Interfaces:**
- Consumes: `search::search_vaults_with_cancel`, `search::SearchResponse` (Serialize since Task 3).
- Produces: command `search_vaults(query) -> Result<search::SearchResponse, String>`; `open_search_result` unchanged. Frontend contract: rejection → error path (Task 6 relies on it).

- [ ] **Step 1: Rewrite `src-tauri/src/search_commands.rs`** (full new contents)

```rust
//! IPC surface for the panel's cross-vault Search view. Read-only: the scan
//! never writes, and opening a hit is delegated to Obsidian via the
//! launch-logged `obsidian://` path. See
//! docs/superpowers/specs/2026-07-09-vault-search-design.md and the polish
//! follow-up docs/superpowers/specs/2026-07-09-search-polish-design.md.

use std::sync::atomic::{AtomicU64, Ordering};
use vault_buddy_core::{discovery, search, uri};

/// Bumped by every `search_vaults` call; a scan whose generation is no
/// longer current aborts at its next per-file poll instead of running a
/// stale multi-vault walk to completion. Relaxed suffices — it's a
/// freshness hint, not a synchronization point.
static SCAN_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Live search across every registered vault. ASYNC on purpose — the one
/// deviation from this codebase's sync-command idiom: a sync command runs on
/// the main thread, and a multi-vault content scan there would freeze window
/// show/hide, drags and the upkeep tick. Running async keeps it off-main; it
/// touches no window APIs and takes no locks, so none of the main-thread
/// window invariants apply. The blocking walk runs under `spawn_blocking` so
/// it can't stall the async runtime's workers either. Returns `Err` on an
/// infrastructure failure (panicked scan task): an empty SUCCESS would blank
/// a working result list, while the frontend's error path keeps the previous
/// results up.
#[tauri::command]
pub async fn search_vaults(query: String) -> Result<search::SearchResponse, String> {
    let my_gen = SCAN_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let scanned = tauri::async_runtime::spawn_blocking(move || {
        // Same name-sorted list the panel shows; the `open` flag is
        // irrelevant here, so no process check is needed.
        let vaults = discovery::discover_vaults();
        let stale = move || SCAN_GENERATION.load(Ordering::Relaxed) != my_gen;
        search::search_vaults_with_cancel(&vaults, &query, &stale)
    })
    .await;
    match scanned {
        Ok(response) => Ok(response),
        Err(e) => {
            log::warn!("search_vaults: scan task failed: {e}");
            Err("Search failed — see the logs for details.".to_string())
        }
    }
}

/// Open one search hit in Obsidian. `file` is the URI-form vault-relative
/// path the search itself returned (exactly-".md" note: extension dropped;
/// anything else: kept) — Obsidian resolves it inside the vault, so this
/// performs no filesystem access and never writes. Addressed by vault ID,
/// never name.
#[tauri::command]
pub fn open_search_result(id: String, file: String) -> Result<(), String> {
    let vault = crate::commands::find_vault(&id)?;
    uri::launch(&uri::open_file_uri(&vault.id, &file))
}
```

- [ ] **Step 2: Compile gates**

`cd src-tauri && cargo fmt --check`, then `npx tauri build --no-bundle` (Linux compile gate; system libs already installed).
Expected: compiles to completion.

- [ ] **Step 3: Commit** — `git add src-tauri/src/search_commands.rs && git commit -m "fix(search): Result-based scan errors + generation-cancelled stale scans"` (body: the empty-success-blanks-results failure mode; DTO layer deleted — core types serialize directly, discovery::Vault precedent).

---

### Task 6: frontend correctness + consolidation (code-point gate, results ref, HighlightText, isNote)

**Files:**
- Create: `src/components/HighlightText.vue`
- Modify: `src/types.ts`, `src/components/Search.vue`
- Test: `tests/search.test.ts` (update + add)

**Interfaces:**
- Consumes: `SearchHit.isNote` (Task 5 wire), `highlightParts`.
- Produces: `<HighlightText :text :query />`; `Search.vue` internals `results: Ref<{query,hits,truncated}|null>`, `charCount`; row keys `hit.file + (hit.isNote ? ":n" : ":a")`. Task 7 builds keyboard nav on `results`/`hits`.

- [ ] **Step 1: Failing tests** — in `tests/search.test.ts`: add `isNote: true` to the `hit()` factory defaults (and `isNote: false` to the attachment override in the grouping test); add:

```ts
  it("shows the too-short hint for a single emoji instead of searching", async () => {
    // Regression: '😀'.length === 2 (UTF-16) passed the old gate while the
    // backend counts chars and refused it — the UI then claimed "No matches".
    const { wrapper, calls } = mountSearch();
    await type(wrapper, "😀");
    expect(calls.filter((c) => c.cmd === "search_vaults")).toHaveLength(0);
    expect(wrapper.text()).toContain("Type at least 2 characters");
  });
```

- [ ] **Step 2: Run** — `npx vitest run tests/search.test.ts` → the emoji test FAILS (invoke fired, hint absent).

- [ ] **Step 3: Implement**

`src/types.ts` — add to `SearchHit` (after `snippet`):

```ts
  /** Note (any-case .md) vs attachment — drives the row icon and key. */
  isNote: boolean;
```

Create `src/components/HighlightText.vue`:

```vue
<script setup lang="ts">
import { computed } from "vue";
import { highlightParts } from "../utils/highlight";

const props = defineProps<{ text: string; query: string }>();
// Prop-gated: Vue skips this component's render while the parent re-renders
// on every keystroke, so the index-based split runs only when the result
// set (or the query it answered) actually changes.
const parts = computed(() => highlightParts(props.text, props.query));
</script>

<template>
  <template v-for="(part, i) in parts" :key="i">
    <mark v-if="part.match" class="rounded bg-violet-500/40 text-inherit">{{
      part.text
    }}</mark>
    <template v-else>{{ part.text }}</template>
  </template>
</template>
```

`src/components/Search.vue` — script changes:

```ts
// Mirrors core::search::MIN_QUERY_CHARS. Counted in Unicode code points
// (matching Rust's chars().count()) — String.length counts UTF-16 units and
// let a single emoji through to a backend refusal, which then rendered a
// false "No matches".
const MIN_QUERY_CHARS = 2;
const DEBOUNCE_MS = 300;
const charCount = (s: string) => [...s].length;

const query = ref("");
// The last response and the query it answers — one value, so highlights,
// the empty state and the truncation footer can never disagree with the
// hits they describe.
const results = ref<{ query: string; hits: SearchHit[]; truncated: boolean } | null>(null);
const searching = ref(false);
const error = ref<string | null>(null);
const inputEl = ref<HTMLInputElement | null>(null);

let timer: ReturnType<typeof setTimeout> | undefined;
let ticket = 0;

const tooShort = computed(() => charCount(query.value.trim()) < MIN_QUERY_CHARS);
const hits = computed(() => results.value?.hits ?? []);
const resultsQuery = computed(() => results.value?.query ?? "");
const truncated = computed(() => results.value?.truncated ?? false);

const groups = computed(() => {
  const map = new Map<string, { vaultName: string; rows: { hit: SearchHit; i: number }[] }>();
  hits.value.forEach((hit, i) => {
    const group = map.get(hit.vaultId);
    if (group) group.rows.push({ hit, i });
    else map.set(hit.vaultId, { vaultName: hit.vaultName, rows: [{ hit, i }] });
  });
  return [...map.entries()].map(([vaultId, g]) => ({ vaultId, ...g }));
});

watch(query, () => {
  if (timer) clearTimeout(timer);
  const trimmed = query.value.trim();
  if (charCount(trimmed) < MIN_QUERY_CHARS) {
    ticket++; // an in-flight response answers a query that no longer exists
    searching.value = false;
    results.value = null;
    error.value = null;
    return;
  }
  timer = setTimeout(() => void runSearch(trimmed), DEBOUNCE_MS);
});

async function runSearch(trimmed: string) {
  const mine = ++ticket;
  searching.value = true;
  try {
    const response = await invoke<SearchResponse>("search_vaults", { query: trimmed });
    if (mine !== ticket) return; // stale — a newer search superseded this one
    results.value = { query: trimmed, hits: response.hits, truncated: response.truncated };
    error.value = null;
  } catch (e) {
    if (mine !== ticket) return;
    // Keep the previous results up — a live refinement that errors must not
    // blank a working list (the backend now rejects on infrastructure
    // failures precisely so this branch handles them).
    error.value = String(e);
    logWarning(`search_vaults failed: ${String(e)}`);
  } finally {
    if (mine === ticket) searching.value = false;
  }
}
```

Template: replace `hits`/`resultsQuery` usages accordingly; rows keyed
`:key="row.hit.file + (row.hit.isNote ? ':n' : ':a')"`; both highlight spans
become `<HighlightText :text="row.hit.name" :query="resultsQuery" />` /
`<HighlightText :text="row.hit.snippet" :query="resultsQuery" />` (import it
with the other component imports). Empty state: `v-else-if="hits.length === 0 && resultsQuery"`.

- [ ] **Step 4: Run** — `npx vitest run tests/search.test.ts tests/highlight.test.ts` → all green (existing stale/debounce/error tests exercise the consolidation).
- [ ] **Step 5: Commit** — `git add src/types.ts src/components/HighlightText.vue src/components/Search.vue tests/search.test.ts && git commit -m "fix(ui): code-point query gate; one results ref + HighlightText"`

---

### Task 7: frontend UX — keyboard nav, icons, refinement indicator, group counts, title map

**Files:**
- Modify: `src/components/Search.vue`, `src/components/ActionPanel.vue`
- Test: `tests/search.test.ts`, `tests/action-panel.test.ts` (title map is behavior-invariant — existing tests gate it)

**Interfaces:**
- Consumes: `results`/`hits` computed (Task 6).
- Produces: user-visible keyboard navigation + row icons + `data-testid="search-refreshing"` + group count chips; `ActionPanel` `VIEW_TITLES` map.

- [ ] **Step 1: Failing tests** (append to `tests/search.test.ts`)

```ts
  it("ArrowDown/ArrowUp move the selection, clamped, and Enter opens the selected hit", async () => {
    const { wrapper, calls } = mountSearch({
      search_vaults: () =>
        response([hit(), hit({ name: "second", file: "Notes/second" })]),
    });
    await type(wrapper, "alpha");
    const input = wrapper.get('[data-testid="search-input"]');
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-0");
    await input.trigger("keydown", { key: "ArrowDown" });
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-1");
    await input.trigger("keydown", { key: "ArrowDown" }); // clamped at the end
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-1");
    await input.trigger("keydown", { key: "ArrowUp" });
    await input.trigger("keydown", { key: "ArrowUp" }); // clamped at the top
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-0");
    await input.trigger("keydown", { key: "ArrowDown" });
    await input.trigger("keydown", { key: "Enter" });
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.find((c) => c.cmd === "open_search_result")).toEqual({
      cmd: "open_search_result",
      args: { id: "v1", file: "Notes/second" },
    });
  });

  it("Enter with no results is a no-op and selection resets on a new result set", async () => {
    const { wrapper, calls } = mountSearch({
      search_vaults: (args) =>
        (args as { query: string }).query === "zzz"
          ? response([])
          : response([hit(), hit({ name: "second", file: "Notes/second" })]),
    });
    await type(wrapper, "zzz");
    await wrapper.get('[data-testid="search-input"]').trigger("keydown", { key: "Enter" });
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.some((c) => c.cmd === "open_search_result")).toBe(false);
    await type(wrapper, "alpha");
    const input = wrapper.get('[data-testid="search-input"]');
    await input.trigger("keydown", { key: "ArrowDown" });
    await type(wrapper, "alphab"); // new result set → selection back to 0
    expect(input.attributes("aria-activedescendant")).toBe("search-hit-0");
  });

  it("shows the refinement indicator only while refining with results up", async () => {
    const pending: Array<{ resolve: (r: SearchResponse) => void }> = [];
    const { wrapper } = mountSearch({
      search_vaults: () =>
        new Promise<SearchResponse>((resolve) => pending.push({ resolve })),
    });
    await type(wrapper, "alpha"); // first search: no results up yet
    expect(wrapper.find('[data-testid="search-refreshing"]').exists()).toBe(false);
    pending[0].resolve(response([hit()]));
    await vi.advanceTimersByTimeAsync(0);
    await nextTick();
    await type(wrapper, "alphab"); // refinement: results up + in flight
    expect(wrapper.find('[data-testid="search-refreshing"]').exists()).toBe(true);
    pending[1].resolve(response([hit()]));
    await vi.advanceTimersByTimeAsync(0);
    await nextTick();
    expect(wrapper.find('[data-testid="search-refreshing"]').exists()).toBe(false);
  });

  it("group headers show a hit count and rows show a kind icon", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () =>
        response([
          hit(),
          hit({ name: "deck.pdf", file: "deck.pdf", isNote: false, snippet: null }),
        ]),
    });
    await type(wrapper, "alpha");
    expect(wrapper.get('[data-testid="group-count"]').text()).toBe("2");
    expect(wrapper.findAll('[data-testid="hit-icon-note"]')).toHaveLength(1);
    expect(wrapper.findAll('[data-testid="hit-icon-file"]')).toHaveLength(1);
  });
```

- [ ] **Step 2: Run** — `npx vitest run tests/search.test.ts` → the four new tests FAIL.

- [ ] **Step 3: Implement in `Search.vue`**

Script additions:

```ts
const selected = ref(0);
const hitId = (i: number) => `search-hit-${i}`;

// New result set → selection back to the top hit.
watch(results, () => {
  selected.value = 0;
});

function onArrow(event: KeyboardEvent, delta: 1 | -1) {
  if (hits.value.length === 0) return;
  event.preventDefault(); // the list owns arrows; keep the caret still
  selected.value = Math.min(Math.max(selected.value + delta, 0), hits.value.length - 1);
  void nextTick(() => {
    document.getElementById(hitId(selected.value))?.scrollIntoView({ block: "nearest" });
  });
}

function onEnter() {
  const hit = hits.value[selected.value];
  if (hit) void openHit(hit);
}
```

(import `nextTick` from vue.)

Template:
- input gains `@keydown.down="onArrow($event, 1)" @keydown.up="onArrow($event, -1)" @keydown.enter="onEnter" role="combobox" aria-expanded="true" aria-controls="search-results" :aria-activedescendant="hits.length ? hitId(selected) : undefined"`, wrapped in `<div class="relative">…</div>` with, after the input:

```vue
      <span
        v-if="searching && hits.length > 0"
        data-testid="search-refreshing"
        class="absolute right-2 top-1/2 h-2 w-2 -translate-y-1/2 animate-pulse rounded-full bg-violet-400"
        aria-hidden="true"
      ></span>
```

- the groups container gets `id="search-results" role="listbox" aria-label="Search results"` (a wrapper div around the group `v-for`).
- group header becomes a flex row with the chip:

```vue
      <h2 class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-400">
        {{ group.vaultName }}
        <span data-testid="group-count" class="rounded-full bg-white/10 px-1.5 py-0.5 text-[10px] font-normal normal-case text-slate-400">{{ group.rows.length }}</span>
      </h2>
```

- rows: `v-for="row in group.rows"`, `:id="hitId(row.i)"`, `role="option"`, `:aria-selected="row.i === selected"`, selection class binding
  `:class="row.i === selected ? 'border-violet-400/60 bg-white/10' : 'border-white/10 bg-white/5'"` (base classes keep everything else), click `openHit(row.hit)`; the name line becomes a flex with the icon:

```vue
        <span class="flex w-full min-w-0 items-center gap-1.5">
          <svg v-if="row.hit.isNote" data-testid="hit-icon-note" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" class="shrink-0 text-slate-400">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <path d="M14 2v6h6M16 13H8M16 17H8M10 9H8" />
          </svg>
          <svg v-else data-testid="hit-icon-file" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" class="shrink-0 text-slate-400">
            <path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48" />
          </svg>
          <span class="min-w-0 flex-1 truncate text-sm text-slate-100" :title="row.hit.name">
            <HighlightText :text="row.hit.name" :query="resultsQuery" />
          </span>
        </span>
```

In `ActionPanel.vue`: delete the nested ternary; add to the script

```ts
// One line per view; the fallback is the vault list's title.
const VIEW_TITLES: Record<string, string> = {
  settings: "Buddy settings",
  captureSettings: "Vault settings",
  recordings: "Recordings",
  recordMode: "Record",
  transcriptions: "Transcriptions",
  tasks: "Tasks",
  search: "Search",
};
const title = computed(() => VIEW_TITLES[view.value] ?? "Vaults");
```

and the `<h1>` becomes `{{ title }}`.

- [ ] **Step 4: Run** — `npx vitest run tests/search.test.ts tests/action-panel.test.ts` → all green.
- [ ] **Step 5: Commit** — `git add src/components/Search.vue src/components/ActionPanel.vue tests/search.test.ts && git commit -m "feat(ui): search keyboard navigation, kind icons, refinement indicator, group counts"`

---

### Task 8: AGENTS.md + full verification + push

**Files:**
- Modify: `AGENTS.md` (search-domain paragraph + tasks-domain walk sentence)

- [ ] **Step 1: Update AGENTS.md**

In the search-domain paragraph: replace "walks every registered vault with the tasks-walk discipline (canonical containment, cycle set, dot-entry skips, deterministic name-ordered walk)" with "walks every registered vault via the shared `core::vault_walk` helper (canonical containment, cycle set, dot-dir skips, deterministic name-ordered walk — single-sourced with the tasks scan)"; note notes are any-case `.md` (content ≤ 1 MiB, one whole-file lowercase early-out), extensionless files are excluded (Obsidian can't open them), filename-before-content is a **hard guarantee** (two capped lists; names checked to the vault's end), hits carry `isNote`, scans run per-vault on named parallel threads and honor a cancellation closure (`search_vaults_with_cancel`; the shell bumps a scan generation per call so superseded scans abort), and the command returns `Result` (infrastructure failure → frontend keeps previous results). Frontend sentence: mention keyboard navigation (arrows + Enter, `aria-activedescendant`). In the tasks-domain `list_tasks` bullet, point the walk description at `core::vault_walk` ("the recursive walk is the shared `core::vault_walk` helper — canonical containment, cycle set, dot-dir skips — with the per-file `type: Task` filter in `tasks.rs`").

- [ ] **Step 2: Full battery**

```bash
npm test && npm run build
cd src-tauri && cargo fmt --check && cd core && cargo clippy --all-targets -- -D warnings && cargo test && cd ../..
npx tauri build --no-bundle
```

Expected: everything green.

- [ ] **Step 3: Commit + push**

```bash
git add AGENTS.md && git commit -m "docs(agents): search polish — shared walk, guarantees, cancellation"
git push -u origin claude/buddy-vault-search-7wialz
```

PR #44 updates in place (no new PR). Post no comment; the diff is the record.
