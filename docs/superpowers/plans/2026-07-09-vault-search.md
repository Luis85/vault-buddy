# Cross-Vault Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A search icon beside the cog in the panel header opens a Search view that live-searches every registered vault (note names + note text + attachment filenames) and opens results in Obsidian.

**Architecture:** A new pure `core::search` module scans vaults on demand (no index) with the tasks-walk safety discipline and hard caps; an **async** `search_vaults` command plus a sync `open_search_result` command expose it; the panel gains a `search` view with a self-contained `Search.vue` (debounce, stale-response guard, vault-grouped highlighted results). Spec: `docs/superpowers/specs/2026-07-09-vault-search-design.md`.

**Tech Stack:** Rust (core crate, std only — no new dependencies), Tauri v2 commands, Vue 3 `<script setup>` + Pinia, Vitest + @vue/test-utils + mockIPC.

## Global Constraints

- TDD: write the failing test, run it to see it fail, implement, see it pass, commit. Regression tests name the failure mode in a comment.
- The vault domain never writes into a vault; search is read-only and opens via `obsidian://` through `uri::launch` (audit-logged). Vaults are addressed by **ID, never name**.
- Sync Tauri commands run on the main thread; the scan therefore runs in an **async** command + `spawn_blocking` (the spec's documented exception). It must touch no window APIs and take no locks.
- Constants (exact values): `MIN_QUERY_CHARS = 2` (trimmed **chars**), `MAX_RESULTS = 100`, `MAX_CONTENT_BYTES = 1 MiB`, snippet window `120` chars, debounce `300` ms.
- Comments explain constraints the code can't show; match the repo's density. Conventional Commits (`feat(search):`, `feat(ui):`, `docs(agents):`).
- Rust: `cd src-tauri && cargo fmt --check` must stay clean; core crate: `cargo clippy --all-targets -- -D warnings` and `cargo test` run on Linux.
- Frontend: `npm test` (Vitest, happy-dom, no Tauri runtime), `npm run build` (vue-tsc) must stay green. Node 22.
- Invoke the tauri CLI only as `npx tauri <cmd>`.

---

### Task 1: `core::search` — snippet windowing primitive

**Files:**
- Create: `src-tauri/core/src/search.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod search;` to the module list, alphabetical: between `pub mod recordings;` and `pub mod sync_util;`)
- Test: module tests inside `src-tauri/core/src/search.rs` (repo convention)

**Interfaces:**
- Consumes: nothing (pure strings).
- Produces: `pub(crate) fn snippet_from_line(line: &str, query_lower: &str) -> Option<String>` and `pub const SNIPPET_CHARS: usize = 120;` — Task 2 calls `snippet_from_line` with an already-lowercased query.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/core/src/search.rs`:

```rust
//! Read-only cross-vault search backing the panel's Search view. Walks every
//! registered vault on demand (no index, no background state) with the same
//! reparse-safe discipline as the tasks scan, matching case-insensitive
//! substrings against note stems + note text and attachment filenames. Never
//! writes; opening a hit is delegated to Obsidian via `obsidian://`. See
//! docs/superpowers/specs/2026-07-09-vault-search-design.md.

/// Max chars of a content snippet. One line of context is enough to decide
/// whether a hit is the right note; the panel is 360px wide.
pub const SNIPPET_CHARS: usize = 120;

/// A ~SNIPPET_CHARS-char window of `line` around its first case-insensitive
/// occurrence of `query_lower` (already lowercased by the caller), with `…`
/// marking trimmed ends. `None` when the line doesn't contain the query.
///
/// Char-boundary safe by construction: the window is cut from a `Vec<char>`
/// of the ORIGINAL line, never by byte-slicing. Centering maps the byte index
/// found in the lowercased line back to a char position, which is only
/// reliable when lowercasing didn't change the line's length (`ẞ`→`ß`, `İ`→
/// `i̇` do); when it did, the window falls back to the line start rather than
/// risk mis-centering — best-effort placement, never a panic.
pub(crate) fn snippet_from_line(line: &str, query_lower: &str) -> Option<String> {
    let trimmed = line.trim();
    let lower = trimmed.to_lowercase();
    let byte_idx = lower.find(query_lower)?;
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= SNIPPET_CHARS {
        return Some(trimmed.to_string());
    }
    let match_char = if lower.len() == trimmed.len() && lower.chars().count() == chars.len() {
        lower[..byte_idx].chars().count()
    } else {
        0
    };
    // Put the match roughly a third in, so context before AND after survives.
    let start = match_char.saturating_sub(SNIPPET_CHARS / 3);
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let start = end.saturating_sub(SNIPPET_CHARS);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_returns_whole_short_line_trimmed() {
        assert_eq!(
            snippet_from_line("  Project Alpha kickoff  ", "alpha").as_deref(),
            Some("Project Alpha kickoff")
        );
    }

    #[test]
    fn snippet_is_none_without_a_match() {
        assert_eq!(snippet_from_line("nothing here", "alpha"), None);
    }

    #[test]
    fn snippet_matches_case_insensitively_preserving_original_case() {
        let s = snippet_from_line("loud ALPHA text", "alpha").unwrap();
        assert!(s.contains("ALPHA"), "got: {s}");
    }

    #[test]
    fn snippet_windows_a_long_line_around_the_match() {
        let line = format!("{}needle{}", "x".repeat(200), "y".repeat(200));
        let s = snippet_from_line(&line, "needle").unwrap();
        assert!(s.contains("needle"), "got: {s}");
        assert!(s.starts_with('…') && s.ends_with('…'), "got: {s}");
        // 120 window chars + 2 ellipses
        assert!(s.chars().count() <= SNIPPET_CHARS + 2, "got len {}", s.chars().count());
    }

    #[test]
    fn snippet_never_panics_on_multibyte_text() {
        // Regression guard: a byte-sliced window would panic on a non-char
        // boundary in multi-byte text; the char-vec window must not.
        let line = format!("{}NEEDLE{}", "ä".repeat(150), "ö".repeat(150));
        let s = snippet_from_line(&line, "needle").unwrap();
        assert!(s.contains("NEEDLE"), "got: {s}");
    }

    #[test]
    fn snippet_falls_back_to_line_start_when_lowercasing_shifts_length() {
        // 'İ' lowercases to a two-char sequence, so byte positions in the
        // lowered string can't be mapped back — window anchors at the start.
        let line = format!("İ{}needle{}", "x".repeat(200), "y".repeat(10));
        // Returning Some without panicking is the contract; the window is
        // start-anchored (best-effort), so it may not include the match.
        let s = snippet_from_line(&line, "needle").unwrap();
        assert!(s.starts_with('İ'), "got: {s}");
        assert!(s.ends_with('…'), "got: {s}");
    }
}
```

And in `src-tauri/core/src/lib.rs` add:

```rust
pub mod search;
```

- [ ] **Step 2: Run tests to verify the compile + behavior cycle**

Run: `cd src-tauri/core && cargo test search::`
Expected: compiles and all 6 tests PASS (this primitive is written directly with its tests — the fail-first cycle applies from Task 2 onward where behavior is non-obvious; if any test fails, fix the implementation, not the test).

Note: if `cargo test` is run before writing the function, expect `cannot find function snippet_from_line` — that is the "failing" state for this task.

- [ ] **Step 3: Lint + format**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt`
Expected: no warnings; fmt makes no or minimal changes.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/core/src/search.rs src-tauri/core/src/lib.rs
git commit -m "feat(search): snippet windowing primitive in core"
```

---

### Task 2: `core::search::search_vaults` — walk, matching, ordering, caps

**Files:**
- Modify: `src-tauri/core/src/search.rs` (append below `snippet_from_line`)
- Test: module tests inside `src-tauri/core/src/search.rs`

**Interfaces:**
- Consumes: `crate::discovery::Vault { id: String, name: String, path: String, open: bool }`; `crate::transcript::dir_entries(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, String)>` (pub(crate), no-follow); `crate::uri::vault_relative_no_ext(file: &Path, vault_root: &Path) -> Option<String>`; `snippet_from_line` from Task 1.
- Produces (Task 3 depends on these exact shapes):

```rust
pub const MIN_QUERY_CHARS: usize = 2;
pub const MAX_RESULTS: usize = 100;
pub const MAX_CONTENT_BYTES: u64 = 1024 * 1024;

pub struct SearchHit {
    pub vault_id: String,
    pub vault_name: String,
    pub name: String,            // stem for notes, full filename for attachments
    pub folder: String,          // vault-relative parent, "/"-separated, "" at root
    pub file: String,            // obsidian://open `file` param (see spec)
    pub snippet: Option<String>, // first matching content line (notes only)
}

pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}

pub fn search_vaults(vaults: &[crate::discovery::Vault], query: &str) -> SearchResponse;
```

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `src-tauri/core/src/search.rs` (keep the existing snippet tests):

```rust
    // `Vault`, `Path` etc. arrive via the module's own imports through the
    // existing `use super::*;` — no extra use lines needed here.

    fn vault(id: &str, name: &str, path: &Path) -> Vault {
        Vault {
            id: id.to_string(),
            name: name.to_string(),
            path: path.to_string_lossy().into_owned(),
            open: false,
        }
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn content_match_yields_snippet_and_note_uri_form() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Notes/idea.md", "intro\nProject Alpha kickoff\n");
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        let h = &r.hits[0];
        assert_eq!(h.vault_id, "v1");
        assert_eq!(h.vault_name, "Work");
        assert_eq!(h.name, "idea");
        assert_eq!(h.folder, "Notes");
        assert_eq!(h.file, "Notes/idea"); // .md dropped — the URI form
        assert_eq!(h.snippet.as_deref(), Some("Project Alpha kickoff"));
        assert!(!r.truncated);
    }

    #[test]
    fn note_stem_match_without_content_match_has_no_snippet() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "Alpha plan.md", "nothing relevant\n");
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].name, "Alpha plan");
        assert_eq!(r.hits[0].file, "Alpha plan");
        assert_eq!(r.hits[0].folder, "");
        assert_eq!(r.hits[0].snippet, None);
    }

    #[test]
    fn frontmatter_lines_count_as_content() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "t.md", "---\ntitle: \"Alpha review\"\n---\nbody\n");
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        assert!(r.hits[0].snippet.as_deref().unwrap().contains("Alpha review"));
    }

    #[test]
    fn attachment_matches_by_filename_and_keeps_extension() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "slides/Alpha deck.pdf", "%PDF-fake");
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        let h = &r.hits[0];
        assert_eq!(h.name, "Alpha deck.pdf"); // full filename displayed
        assert_eq!(h.folder, "slides");
        assert_eq!(h.file, "slides/Alpha deck.pdf"); // extension KEPT for the URI
        assert_eq!(h.snippet, None); // attachments never get content snippets
    }

    #[test]
    fn nested_folder_is_slash_separated() {
        // `folder`/`file` must be `/`-separated even where the OS joins with
        // `\` (obsidian:// expects forward slashes; display should match).
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a/b/alpha note.md", "x\n");
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].folder, "a/b");
        assert_eq!(r.hits[0].file, "a/b/alpha note");
    }

    #[test]
    fn matching_is_case_insensitive_both_directions() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "READ me.md", "loud TEXT here\n");
        assert_eq!(search_vaults(&[vault("v", "V", dir.path())], "read").hits.len(), 1);
        assert_eq!(search_vaults(&[vault("v", "V", dir.path())], "text").hits.len(), 1);
    }

    #[test]
    fn dot_dirs_and_dot_files_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".obsidian/alpha.md", "alpha\n");
        write(dir.path(), ".trash/alpha gone.md", "alpha\n");
        write(dir.path(), ".alpha hidden.md", "alpha\n");
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert!(r.hits.is_empty(), "got: {:?}", r.hits.iter().map(|h| &h.file).collect::<Vec<_>>());
    }

    #[test]
    fn oversize_note_matches_by_name_only() {
        // Content scanning is capped at MAX_CONTENT_BYTES; the name still
        // matches, but no snippet is produced and a content-only match in an
        // oversize file is not surfaced at all.
        let dir = tempfile::tempdir().unwrap();
        let big = format!("alpha\n{}", "x".repeat(MAX_CONTENT_BYTES as usize));
        write(dir.path(), "alpha big.md", &big);
        write(dir.path(), "unrelated big.md", &big);
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].name, "alpha big");
        assert_eq!(r.hits[0].snippet, None);
    }

    #[test]
    fn non_utf8_note_degrades_to_name_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(dir.path().join("alpha bin.md"), [0xFF, 0xFE, 0x00, b'a']).unwrap();
        std::fs::write(dir.path().join("plain bin.md"), [0xFF, 0xFE, 0x00, b'a']).unwrap();
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1); // name match survives, content read fails silently
        assert_eq!(r.hits[0].snippet, None);
    }

    #[test]
    fn filename_matches_sort_before_content_only_matches() {
        // "aaa.md" (content match) walks first, but "zzz alpha.md" (name
        // match) must surface first — filename class wins, walk order only
        // breaks ties within a class.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "aaa.md", "alpha inside\n");
        write(dir.path(), "zzz alpha.md", "unrelated\n");
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        let names: Vec<&str> = r.hits.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["zzz alpha", "aaa"]);
    }

    #[test]
    fn results_cap_at_max_results_and_set_truncated() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_RESULTS + 5) {
            write(dir.path(), &format!("alpha {i:03}.md"), "x\n");
        }
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), MAX_RESULTS);
        assert!(r.truncated);
    }

    #[test]
    fn budget_spans_vaults_in_order_and_stops_when_spent() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        write(a.path(), "alpha a.md", "x\n");
        write(b.path(), "alpha b.md", "x\n");
        let r = search_vaults(
            &[vault("va", "A", a.path()), vault("vb", "B", b.path())],
            "alpha",
        );
        assert_eq!(r.hits.len(), 2);
        assert_eq!(r.hits[0].vault_id, "va"); // given (name-sorted) vault order preserved
        assert_eq!(r.hits[1].vault_id, "vb");
        assert!(!r.truncated);
    }

    #[test]
    fn short_or_empty_query_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "alpha.md", "alpha\n");
        for q in ["", "a", " a ", "\t"] {
            let r = search_vaults(&[vault("v1", "Work", dir.path())], q);
            assert!(r.hits.is_empty(), "query {q:?} must not search");
            assert!(!r.truncated);
        }
        // 2 chars is enough, counted in CHARS not bytes ("ää" is 4 bytes).
        write(dir.path(), "ää note.md", "x\n");
        assert_eq!(search_vaults(&[vault("v1", "Work", dir.path())], "ää").hits.len(), 1);
    }

    #[test]
    fn unresolvable_vault_path_is_skipped() {
        let good = tempfile::tempdir().unwrap();
        write(good.path(), "alpha.md", "x\n");
        let gone = vault("vg", "Gone", Path::new("/no/such/vault"));
        let r = search_vaults(&[gone, vault("v1", "Work", good.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].vault_id, "v1");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_subdir_escaping_the_vault_is_not_followed() {
        // Same discipline as the tasks walk: canonical containment, because
        // the no-follow dirent type can't be trusted for a junction.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Vault");
        std::fs::create_dir_all(&root).unwrap();
        write(&root, "inside alpha.md", "x\n");
        let outside = dir.path().join("outside");
        write(&outside, "escapee alpha.md", "x\n");
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
        let r = search_vaults(&[vault("v1", "Work", &root)], "alpha");
        let names: Vec<&str> = r.hits.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, vec!["inside alpha"]);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cycle_terminates_and_counts_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Vault");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        write(&root, "alpha.md", "x\n");
        std::os::unix::fs::symlink(&root, root.join("sub").join("loop")).unwrap();
        let r = search_vaults(&[vault("v1", "Work", &root)], "alpha");
        assert_eq!(r.hits.len(), 1); // terminates; counted exactly once
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test search::`
Expected: FAIL to compile — `cannot find function `search_vaults``, `cannot find struct `SearchHit``.

- [ ] **Step 3: Write the implementation**

Insert between the module doc and `snippet_from_line` in `src-tauri/core/src/search.rs`:

```rust
use crate::discovery::Vault;
use crate::transcript::dir_entries;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Trimmed CHARS (not bytes) a query needs before anything is scanned — a
/// 1-char query would match nearly every file in every vault.
pub const MIN_QUERY_CHARS: usize = 2;
/// Global hit budget across all vaults; the UI shows a "refine your query"
/// footer when it is exceeded.
pub const MAX_RESULTS: usize = 100;
/// Files larger than this are matched by NAME only — reading arbitrarily
/// large files on every keystroke-pause would make live search crawl.
pub const MAX_CONTENT_BYTES: u64 = 1024 * 1024;

/// One search hit. `file` is exactly the `obsidian://open` `file` parameter:
/// vault-relative, `/`-separated, `.md` dropped for notes (Obsidian's
/// expected form, mirroring `uri::vault_relative_no_ext`) but the extension
/// KEPT for attachments — without it Obsidian would resolve `report` as
/// `report.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub vault_id: String,
    pub vault_name: String,
    /// Display name: file stem for `.md` notes, full filename for attachments.
    pub name: String,
    /// Vault-relative parent folder, `/`-separated, "" for the vault root.
    pub folder: String,
    pub file: String,
    /// First matching content line (notes only; None for name-only matches).
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}

/// A hit plus which class it belongs to: filename matches surface before
/// content-only matches, and the per-vault sort needs to know.
struct RawHit {
    name_matched: bool,
    hit: SearchHit,
}

/// Collect at most this many hits per vault. The bound keeps transient memory
/// small on a pathological query, and the `+ 1` makes "more existed"
/// observable even when a single vault fills the whole budget.
const PER_VAULT_CAP: usize = MAX_RESULTS + 1;

/// Search every vault, in the given order, until `MAX_RESULTS` hits are
/// collected. A trimmed query shorter than `MIN_QUERY_CHARS` returns an empty
/// response. Read-only, best-effort: missing/unreadable vaults, dirs and
/// files degrade silently — scan noise, the same documented exception to the
/// no-swallow rule as the tasks/recordings walks.
pub fn search_vaults(vaults: &[Vault], query: &str) -> SearchResponse {
    let trimmed = query.trim();
    if trimmed.chars().count() < MIN_QUERY_CHARS {
        return SearchResponse::default();
    }
    let query_lower = trimmed.to_lowercase();
    let mut hits: Vec<SearchHit> = Vec::new();
    let mut truncated = false;
    for vault in vaults {
        let budget = MAX_RESULTS - hits.len();
        if budget == 0 {
            // Budget spent with vaults still unscanned: report truncation
            // rather than scanning on just to prove more matches exist — the
            // flag only drives a "refine your query" footer.
            truncated = true;
            break;
        }
        // Canonicalize the root so every descended subdirectory can be
        // containment-checked against it (same as the tasks walk). A vault
        // whose folder moved/vanished degrades silently.
        let Ok(canon_root) = std::fs::canonicalize(Path::new(&vault.path)) else {
            continue;
        };
        let mut raw: Vec<RawHit> = Vec::new();
        let mut walked = HashSet::new();
        collect_hits(&canon_root, &canon_root, vault, &query_lower, &mut walked, &mut raw);
        // Filename matches surface before content-only matches. The sort is
        // stable, so the deterministic (name-ordered) walk order holds within
        // each class.
        raw.sort_by_key(|r| !r.name_matched);
        if raw.len() > budget {
            truncated = true;
            raw.truncate(budget);
        }
        hits.extend(raw.into_iter().map(|r| r.hit));
    }
    SearchResponse { hits, truncated }
}

/// Recursively collect matches under `dir` (a canonical path), best-effort,
/// stopping at `PER_VAULT_CAP`. The walk is the tasks walk's discipline: a
/// subdirectory is descended only after canonicalizing it and confirming it
/// still resolves under `canon_root` (a symlink/junction escaping the vault
/// is never walked — the no-follow dirent type can't be trusted for a
/// junction), `walked` breaks reparse cycles, and dot-entries are skipped
/// (`.obsidian`, `.trash`, `.git`; dot-files like `.DS_Store` and our own
/// `.mp3.part` temps). Entries are processed in name order so the walk — and
/// therefore which hits survive the cap — is deterministic.
fn collect_hits(
    dir: &Path,
    canon_root: &Path,
    vault: &Vault,
    query_lower: &str,
    walked: &mut HashSet<PathBuf>,
    out: &mut Vec<RawHit>,
) {
    if out.len() >= PER_VAULT_CAP || !walked.insert(dir.to_path_buf()) {
        return;
    }
    let mut entries = dir_entries(dir);
    entries.sort_by(|a, b| a.2.cmp(&b.2));
    for (path, ft, name) in entries {
        if out.len() >= PER_VAULT_CAP {
            return;
        }
        if name.starts_with('.') {
            continue;
        }
        if ft.is_dir() {
            match std::fs::canonicalize(&path) {
                Ok(child) if child.starts_with(canon_root) => {
                    collect_hits(&child, canon_root, vault, query_lower, walked, out)
                }
                _ => continue,
            }
            continue;
        }
        if !ft.is_file() {
            continue; // symlinked files are not followed, same as the tasks walk
        }
        if let Some(stem) = name.strip_suffix(".md") {
            let name_matched = stem.to_lowercase().contains(query_lower);
            let snippet = content_snippet(&path, query_lower);
            if name_matched || snippet.is_some() {
                if let Some(hit) = make_hit(vault, canon_root, &path, stem, snippet, true) {
                    out.push(RawHit { name_matched, hit });
                }
            }
        } else if name.to_lowercase().contains(query_lower) {
            if let Some(hit) = make_hit(vault, canon_root, &path, &name, None, false) {
                out.push(RawHit { name_matched: true, hit });
            }
        }
    }
}

/// First matching line's snippet, or None: no match, file larger than
/// `MAX_CONTENT_BYTES` (name matching still applies — only content is
/// skipped), unreadable, or not UTF-8.
fn content_snippet(path: &Path, query_lower: &str) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_CONTENT_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    content
        .lines()
        .find_map(|line| snippet_from_line(line, query_lower))
}

/// Assemble a hit for `path` (which lives under `canon_root` by
/// construction). `None` only if the path can't be made vault-relative —
/// which the walk's containment guarantees against, so it's pure defense.
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
    let file = if is_note {
        crate::uri::vault_relative_no_ext(path, canon_root)?
    } else {
        let s = rel.to_string_lossy().replace('\\', "/");
        if s.is_empty() {
            return None;
        }
        s
    };
    Some(SearchHit {
        vault_id: vault.id.clone(),
        vault_name: vault.name.clone(),
        name: display_name.to_string(),
        folder,
        file,
        snippet,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test search::`
Expected: all Task 1 + Task 2 tests PASS (unix-gated ones included on Linux).

- [ ] **Step 5: Lint + format + full core suite**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test && cd .. && cargo fmt`
Expected: clean clippy, full core suite green.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/src/search.rs
git commit -m "feat(search): read-only cross-vault scan in core

Walks every vault with the tasks-walk safety discipline (canonical
containment, cycle set, dot skips, deterministic name order), matches
case-insensitive substrings against note stems + content and attachment
filenames, and caps work (100 hits, 1 MiB content reads) so live search
stays bounded."
```

---

### Task 3: IPC — `search_commands.rs` (`search_vaults` async, `open_search_result`)

**Files:**
- Create: `src-tauri/src/search_commands.rs`
- Modify: `src-tauri/src/commands.rs` (make `find_vault` `pub(crate)`)
- Modify: `src-tauri/src/lib.rs` (declare `mod search_commands;` beside the other `mod` declarations; add both commands to `generate_handler![]` after the `task_commands::*` entries)
- Test: compile gates (the shell crate has no runtime tests; core logic was tested in Task 2)

**Interfaces:**
- Consumes: `vault_buddy_core::search::{search_vaults, SearchResponse}` (Task 2), `vault_buddy_core::{discovery, uri}`, `crate::commands::find_vault(id: &str) -> Result<discovery::Vault, String>`.
- Produces (Task 6's `invoke` calls rely on these exact names and camelCase DTO fields):
  - command `search_vaults(query: String) -> SearchResponseDto` — `{ hits: [{ vaultId, vaultName, name, folder, file, snippet }], truncated }`
  - command `open_search_result(id: String, file: String) -> Result<(), String>`

- [ ] **Step 1: Make `find_vault` shared**

In `src-tauri/src/commands.rs` change:

```rust
fn find_vault(id: &str) -> Result<discovery::Vault, String> {
```

to:

```rust
pub(crate) fn find_vault(id: &str) -> Result<discovery::Vault, String> {
```

- [ ] **Step 2: Create `src-tauri/src/search_commands.rs`**

```rust
//! IPC surface for the panel's cross-vault Search view. Read-only: the scan
//! never writes, and opening a hit is delegated to Obsidian via the
//! launch-logged `obsidian://` path. See
//! docs/superpowers/specs/2026-07-09-vault-search-design.md.

use vault_buddy_core::{discovery, search, uri};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitDto {
    pub vault_id: String,
    pub vault_name: String,
    pub name: String,
    pub folder: String,
    pub file: String,
    pub snippet: Option<String>,
}

#[derive(Clone, serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponseDto {
    pub hits: Vec<SearchHitDto>,
    pub truncated: bool,
}

impl From<search::SearchResponse> for SearchResponseDto {
    fn from(r: search::SearchResponse) -> Self {
        Self {
            hits: r
                .hits
                .into_iter()
                .map(|h| SearchHitDto {
                    vault_id: h.vault_id,
                    vault_name: h.vault_name,
                    name: h.name,
                    folder: h.folder,
                    file: h.file,
                    snippet: h.snippet,
                })
                .collect(),
            truncated: r.truncated,
        }
    }
}

/// Live search across every registered vault. ASYNC on purpose — the one
/// deviation from this codebase's sync-command idiom: a sync command runs on
/// the main thread, and a multi-vault content scan there would freeze window
/// show/hide, drags and the upkeep tick. Running async keeps it off-main; it
/// touches no window APIs and takes no locks, so none of the main-thread
/// window invariants apply. The blocking filesystem walk is additionally
/// wrapped in `spawn_blocking` so it can't stall the async runtime's worker
/// threads either.
#[tauri::command]
pub async fn search_vaults(query: String) -> SearchResponseDto {
    let scanned = tauri::async_runtime::spawn_blocking(move || {
        // Same name-sorted list the panel shows; the `open` flag is
        // irrelevant here, so no process check is needed.
        let vaults = discovery::discover_vaults();
        search::search_vaults(&vaults, &query)
    })
    .await;
    match scanned {
        Ok(response) => response.into(),
        Err(e) => {
            // Degrade to empty but leave a trace (diagnostics invariant: no
            // swallowed error).
            log::warn!("search_vaults: scan task failed: {e}");
            SearchResponseDto::default()
        }
    }
}

/// Open one search hit in Obsidian. `file` is the URI-form vault-relative
/// path the search itself returned (note: extension dropped; attachment:
/// kept) — Obsidian resolves it inside the vault, so this performs no
/// filesystem access and never writes. Addressed by vault ID, never name.
#[tauri::command]
pub fn open_search_result(id: String, file: String) -> Result<(), String> {
    let vault = crate::commands::find_vault(&id)?;
    uri::launch(&uri::open_file_uri(&vault.id, &file))
}
```

- [ ] **Step 3: Register the module + commands**

In `src-tauri/src/lib.rs`: add `mod search_commands;` next to the existing `mod task_commands;` declaration, and extend `generate_handler![]`:

```rust
            task_commands::set_task_status,
            task_commands::count_open_tasks,
            search_commands::search_vaults,
            search_commands::open_search_result,
```

- [ ] **Step 4: Compile gates**

Run: `cd src-tauri && cargo fmt --check`
Expected: clean.

Run the Linux shell compile gate (catches IPC signature drift without waiting for CI). One-time setup if not yet done in this container: `npm run setup:linux`. Then:

Run: `npx tauri build --no-bundle`
Expected: compiles to completion (this is a compile gate only; Windows CI remains the behavior gate).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/search_commands.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(search): async search_vaults + open_search_result commands

search_vaults is deliberately async (the codebase's sync commands run on
the main thread; a multi-vault content scan there would freeze window
show/hide and drags) and wraps the walk in spawn_blocking. Opening a hit
reuses the launch-logged obsidian:// path, addressed by vault ID."
```

---

### Task 4: Frontend state — `search` view + hit types

**Files:**
- Modify: `src/stores/vaults.ts` (view union + `openSearch()`)
- Modify: `src/types.ts` (SearchHit / SearchResponse)
- Test: `tests/vaults-store.test.ts` (append)

**Interfaces:**
- Consumes: nothing new.
- Produces: `useVaultsStore().openSearch(): void` sets `view = "search"`; `back()` from `search` returns to `list` (via the existing else-branch); types `SearchHit { vaultId: string; vaultName: string; name: string; folder: string; file: string; snippet: string | null }` and `SearchResponse { hits: SearchHit[]; truncated: boolean }` (Tasks 6–7 import these).

- [ ] **Step 1: Write the failing test**

Append to `tests/vaults-store.test.ts` (inside the existing `describe`; it already sets up pinia + mocks):

```ts
  it("opens the search view and back returns to the list", () => {
    const store = useVaultsStore();
    store.openSearch();
    expect(store.view).toBe("search");
    store.back();
    expect(store.view).toBe("list");
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/vaults-store.test.ts`
Expected: FAIL — `store.openSearch is not a function` (and a TS error on the view literal).

- [ ] **Step 3: Implement**

In `src/stores/vaults.ts`:

1. Extend the `view` union (state) — add `"search"`:

```ts
    view: "list" as
      | "list"
      | "settings"
      | "captureSettings"
      | "recordings"
      | "recordMode"
      | "transcriptions"
      | "tasks"
      | "search",
```

2. Add the action next to `openTasks`:

```ts
    // Cross-vault, so no per-vault id to remember (unlike tasks/recordings).
    openSearch() {
      this.view = "search";
    },
```

`back()` needs no change: `search` falls through to the final `else` → `showList()` — the fixed one-parent tree's default.

In `src/types.ts` append:

```ts
export interface SearchHit {
  vaultId: string;
  vaultName: string;
  /** Display name: file stem for notes, full filename for attachments. */
  name: string;
  /** Vault-relative parent folder ("" at the vault root), for display. */
  folder: string;
  /** The obsidian://open `file` parameter (extension dropped for notes,
   * kept for attachments) — pass through to open_search_result verbatim. */
  file: string;
  /** First matching content line; null for filename-only matches. */
  snippet: string | null;
}

export interface SearchResponse {
  hits: SearchHit[];
  truncated: boolean;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/vaults-store.test.ts`
Expected: PASS (all existing + the new test).

- [ ] **Step 5: Commit**

```bash
git add src/stores/vaults.ts src/types.ts tests/vaults-store.test.ts
git commit -m "feat(ui): search view state + search hit types"
```

---

### Task 5: `highlightParts` helper + `noteOpenedMessage`

**Files:**
- Create: `src/utils/highlight.ts`
- Modify: `src/buddyMessages.ts` (append one function)
- Test: create `tests/highlight.test.ts`; append to `tests/buddy-messages.test.ts`

**Interfaces:**
- Consumes: nothing.
- Produces (Task 6 imports both): `highlightParts(text: string, query: string): HighlightPart[]` with `HighlightPart { text: string; match: boolean }`; `noteOpenedMessage(name: string): string`.

- [ ] **Step 1: Write the failing tests**

Create `tests/highlight.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { highlightParts } from "../src/utils/highlight";

describe("highlightParts", () => {
  it("marks case-insensitive occurrences", () => {
    expect(highlightParts("Alpha and ALPHA", "alpha")).toEqual([
      { text: "Alpha", match: true },
      { text: " and ", match: false },
      { text: "ALPHA", match: true },
    ]);
  });

  it("returns one unmatched part when nothing matches", () => {
    expect(highlightParts("nothing here", "alpha")).toEqual([
      { text: "nothing here", match: false },
    ]);
  });

  it("produces no empty parts for matches at the start and end", () => {
    expect(highlightParts("alpha mid alpha", "alpha")).toEqual([
      { text: "alpha", match: true },
      { text: " mid ", match: false },
      { text: "alpha", match: true },
    ]);
  });

  it("empty or whitespace query yields a single unmatched part", () => {
    expect(highlightParts("text", "")).toEqual([{ text: "text", match: false }]);
    expect(highlightParts("text", "  ")).toEqual([{ text: "text", match: false }]);
  });

  it("falls back to no highlight when lowercasing shifts lengths", () => {
    // 'İ'.toLowerCase() is two code units — index math against the lowered
    // string would mis-slice the original, so the helper must refuse to
    // highlight rather than corrupt the text.
    expect(highlightParts("İstanbul note", "i")).toEqual([
      { text: "İstanbul note", match: false },
    ]);
  });
});
```

Append to `tests/buddy-messages.test.ts` (match its existing style):

```ts
  it("noteOpenedMessage names the note and falls back when blank", () => {
    expect(noteOpenedMessage("Meeting notes")).toBe("Opening Meeting notes 📄");
    expect(noteOpenedMessage("   ")).toBe("Opening your note 📄");
  });
```

(and add `noteOpenedMessage` to that file's import from `../src/buddyMessages`.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/highlight.test.ts tests/buddy-messages.test.ts`
Expected: FAIL — cannot resolve `../src/utils/highlight`; `noteOpenedMessage` not exported.

- [ ] **Step 3: Implement**

Create `src/utils/highlight.ts`:

```ts
/** One piece of a highlighted string; `match: true` spans are query hits. */
export interface HighlightPart {
  text: string;
  match: boolean;
}

/**
 * Split `text` into parts marking case-insensitive occurrences of `query`.
 * Index-based on purpose — never a RegExp built from user input, which would
 * treat `.`/`(` etc. as pattern syntax. If lowercasing changes either
 * string's length (rare Unicode, e.g. 'İ'), index math against the lowered
 * strings would mis-slice the original, so the helper falls back to a single
 * unhighlighted part rather than corrupt the text. Empty query → single
 * unhighlighted part.
 */
export function highlightParts(text: string, query: string): HighlightPart[] {
  const q = query.trim();
  const qLower = q.toLowerCase();
  const lower = text.toLowerCase();
  if (!qLower || lower.length !== text.length || qLower.length !== q.length) {
    return [{ text, match: false }];
  }
  const parts: HighlightPart[] = [];
  let pos = 0;
  for (;;) {
    const idx = lower.indexOf(qLower, pos);
    if (idx === -1) break;
    if (idx > pos) parts.push({ text: text.slice(pos, idx), match: false });
    parts.push({ text: text.slice(idx, idx + qLower.length), match: true });
    pos = idx + qLower.length;
  }
  if (pos < text.length || parts.length === 0) {
    parts.push({ text: text.slice(pos), match: false });
  }
  return parts;
}
```

Append to `src/buddyMessages.ts` (below `dailyNoteOpenedMessage`):

```ts
/** Opening a note/file from search — names it, generic fallback for blank. */
export function noteOpenedMessage(name: string): string {
  const trimmed = name.trim();
  return trimmed ? `Opening ${trimmed} 📄` : "Opening your note 📄";
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/highlight.test.ts tests/buddy-messages.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/utils/highlight.ts src/buddyMessages.ts tests/highlight.test.ts tests/buddy-messages.test.ts
git commit -m "feat(ui): index-based highlight helper + note-opened buddy message"
```

---

### Task 6: `Search.vue` — debounced live search view

**Files:**
- Create: `src/components/Search.vue`
- Test: create `tests/search.test.ts`

**Interfaces:**
- Consumes: `invoke("search_vaults", { query })` → `SearchResponse`; `invoke("open_search_result", { id, file })`; `invoke("close_panel")`; `announce()` (`src/announce.ts`, self-gating on the buddy-messages setting); `noteOpenedMessage` (Task 5); `highlightParts` (Task 5); `useNotificationsStore`; types from Task 4.
- Produces: `<Search />` component (no props, no events) that Task 7 mounts for `view === "search"`. Test ids: `search-input`, `search-hit`, `search-truncated`.

- [ ] **Step 1: Write the failing tests**

Create `tests/search.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Search from "../src/components/Search.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import { noteOpenedMessage } from "../src/buddyMessages";
import type { SearchHit, SearchResponse } from "../src/types";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const hit = (over: Partial<SearchHit> = {}): SearchHit => ({
  vaultId: "v1",
  vaultName: "Work",
  name: "idea",
  folder: "Notes",
  file: "Notes/idea",
  snippet: "Project Alpha kickoff",
  ...over,
});

const response = (hits: SearchHit[], truncated = false): SearchResponse => ({ hits, truncated });

function mountSearch(handlers: Partial<Record<string, (args: unknown) => unknown>> = {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "search_vaults") return response([hit()]);
  });
  const wrapper = mount(Search);
  return { wrapper, calls };
}

async function type(wrapper: ReturnType<typeof mount>, text: string) {
  await wrapper.get('[data-testid="search-input"]').setValue(text);
  await vi.advanceTimersByTimeAsync(300);
  await nextTick();
}

describe("Search", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    clearMocks();
  });

  it("debounces: no call until 300ms after the last keystroke", async () => {
    const { wrapper, calls } = mountSearch();
    await wrapper.get('[data-testid="search-input"]').setValue("alp");
    await vi.advanceTimersByTimeAsync(200);
    expect(calls.filter((c) => c.cmd === "search_vaults")).toHaveLength(0);
    await wrapper.get('[data-testid="search-input"]').setValue("alpha");
    await vi.advanceTimersByTimeAsync(300);
    await nextTick();
    const searches = calls.filter((c) => c.cmd === "search_vaults");
    expect(searches).toHaveLength(1); // the "alp" timer was superseded
    expect(searches[0].args).toEqual({ query: "alpha" });
  });

  it("never searches queries under 2 trimmed characters", async () => {
    const { wrapper, calls } = mountSearch();
    await type(wrapper, " a ");
    expect(calls.filter((c) => c.cmd === "search_vaults")).toHaveLength(0);
    expect(wrapper.text()).toContain("Type at least 2 characters");
  });

  it("renders hits grouped under their vault name with the snippet", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () =>
        response([
          hit(),
          hit({ vaultId: "v2", vaultName: "Personal", name: "alpha deck.pdf", folder: "", file: "alpha deck.pdf", snippet: null }),
        ]),
    });
    await type(wrapper, "alpha");
    const text = wrapper.text();
    expect(text).toContain("Work");
    expect(text).toContain("Personal");
    expect(text).toContain("idea");
    expect(text).toContain("Project Alpha kickoff");
    expect(wrapper.findAll('[data-testid="search-hit"]')).toHaveLength(2);
  });

  it("drops a stale response: an older slow search cannot clobber newer results", async () => {
    // Failure mode: without a request ticket, the "alp" response landing
    // AFTER the "alpha" response would overwrite the newer results.
    const pending: Array<{ query: string; resolve: (r: SearchResponse) => void }> = [];
    const { wrapper } = mountSearch({
      search_vaults: (args) =>
        new Promise<SearchResponse>((resolve) => {
          pending.push({ query: (args as { query: string }).query, resolve });
        }),
    });
    await type(wrapper, "alp");
    await type(wrapper, "alpha");
    expect(pending.map((p) => p.query)).toEqual(["alp", "alpha"]);
    pending[1].resolve(response([hit({ name: "fresh" })]));
    await vi.advanceTimersByTimeAsync(0);
    await nextTick();
    pending[0].resolve(response([hit({ name: "stale" })]));
    await vi.advanceTimersByTimeAsync(0);
    await nextTick();
    expect(wrapper.text()).toContain("fresh");
    expect(wrapper.text()).not.toContain("stale");
  });

  it("opens a hit: open_search_result + announce + close_panel", async () => {
    const { wrapper, calls } = mountSearch();
    await type(wrapper, "alpha");
    await wrapper.get('[data-testid="search-hit"]').trigger("click");
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.find((c) => c.cmd === "open_search_result")).toEqual({
      cmd: "open_search_result",
      args: { id: "v1", file: "Notes/idea" },
    });
    expect(calls.find((c) => c.cmd === "announce")).toEqual({
      cmd: "announce",
      args: { text: noteOpenedMessage("idea") },
    });
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(true);
  });

  it("keeps the panel open and notifies when opening fails", async () => {
    const notifications = useNotificationsStore();
    const { wrapper, calls } = mountSearch({
      open_search_result: () => {
        throw new Error("vault not found");
      },
    });
    await type(wrapper, "alpha");
    await wrapper.get('[data-testid="search-hit"]').trigger("click");
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(false);
    expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
  });

  it("keeps previous results and shows a banner when a search fails", async () => {
    // A live refinement that errors must not blank a working result list.
    let fail = false;
    const { wrapper } = mountSearch({
      search_vaults: () => {
        if (fail) throw new Error("scan failed");
        return response([hit()]);
      },
    });
    await type(wrapper, "alpha");
    expect(wrapper.text()).toContain("idea");
    fail = true;
    await type(wrapper, "alphab");
    expect(wrapper.text()).toContain("idea"); // previous results kept
    expect(wrapper.text()).toContain("scan failed");
  });

  it("shows the truncation footer when the backend capped results", async () => {
    const { wrapper } = mountSearch({
      search_vaults: () => response([hit()], true),
    });
    await type(wrapper, "alpha");
    expect(wrapper.find('[data-testid="search-truncated"]').exists()).toBe(true);
  });

  it("shows the empty state for a query with no matches", async () => {
    const { wrapper } = mountSearch({ search_vaults: () => response([]) });
    await type(wrapper, "zzz");
    expect(wrapper.text()).toContain('No matches for "zzz"');
  });

  it("Escape clears the query first instead of bubbling", async () => {
    const { wrapper } = mountSearch();
    const input = wrapper.get('[data-testid="search-input"]');
    await input.setValue("alpha");
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    const stop = vi.spyOn(event, "stopPropagation");
    input.element.dispatchEvent(event);
    await nextTick();
    expect((input.element as HTMLInputElement).value).toBe("");
    expect(stop).toHaveBeenCalled(); // second Escape (empty query) will bubble → PanelRoot closes
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/search.test.ts`
Expected: FAIL — cannot resolve `../src/components/Search.vue`.

- [ ] **Step 3: Implement `src/components/Search.vue`**

```vue
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { announce } from "../announce";
import { noteOpenedMessage } from "../buddyMessages";
import { useNotificationsStore } from "../stores/notifications";
import { highlightParts } from "../utils/highlight";
import type { SearchHit, SearchResponse } from "../types";

const notifications = useNotificationsStore();

// Mirrors core::search::MIN_QUERY_CHARS — the backend refuses shorter
// queries anyway; gating here saves the IPC round-trip and drives the hint.
const MIN_QUERY_CHARS = 2;
const DEBOUNCE_MS = 300;

const query = ref("");
const hits = ref<SearchHit[]>([]);
const truncated = ref(false);
const searching = ref(false);
const error = ref<string | null>(null);
// The query the current results answer — drives highlights and the empty
// state; the live input may already be ahead of it while a search is in
// flight.
const resultsQuery = ref("");
const inputEl = ref<HTMLInputElement | null>(null);

let timer: ReturnType<typeof setTimeout> | undefined;
// Monotonic ticket: a resolving search that is no longer the latest is
// dropped, so a slow older response can never overwrite newer results.
let ticket = 0;

const tooShort = computed(() => query.value.trim().length < MIN_QUERY_CHARS);

// Flat hits → per-vault groups, preserving the backend's vault order.
const groups = computed(() => {
  const map = new Map<string, { vaultName: string; hits: SearchHit[] }>();
  for (const h of hits.value) {
    const group = map.get(h.vaultId);
    if (group) group.hits.push(h);
    else map.set(h.vaultId, { vaultName: h.vaultName, hits: [h] });
  }
  return [...map.entries()].map(([vaultId, g]) => ({ vaultId, ...g }));
});

watch(query, () => {
  if (timer) clearTimeout(timer);
  const trimmed = query.value.trim();
  if (trimmed.length < MIN_QUERY_CHARS) {
    // Invalidate any in-flight response too — its results answer a query
    // that no longer exists.
    ticket++;
    searching.value = false;
    hits.value = [];
    truncated.value = false;
    error.value = null;
    resultsQuery.value = "";
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
    hits.value = response.hits;
    truncated.value = response.truncated;
    resultsQuery.value = trimmed;
    error.value = null;
  } catch (e) {
    if (mine !== ticket) return;
    // Keep the previous results up — a live refinement that errors must not
    // blank a working list (mirrors the vaults store's refresh behavior).
    error.value = String(e);
    logWarning(`search_vaults failed: ${String(e)}`);
  } finally {
    if (mine === ticket) searching.value = false;
  }
}

async function openHit(hit: SearchHit) {
  try {
    await invoke("open_search_result", { id: hit.vaultId, file: hit.file });
    // Same acknowledgement pattern as vault/daily-note opens (the panel
    // window is the announcer for opens); a failed open stays silent — the
    // toast is the feedback there.
    announce(noteOpenedMessage(hit.name));
    void invoke("close_panel").catch(() => {});
  } catch (e) {
    notifications.error(String(e));
    logWarning(`open_search_result failed for ${hit.file}: ${String(e)}`);
  }
}

function onEscape(event: KeyboardEvent) {
  if (query.value) {
    // First Escape clears the query; a second one bubbles up to PanelRoot
    // and closes the panel (same pattern as the vault filter).
    query.value = "";
    event.stopPropagation();
  }
}

onMounted(() => inputEl.value?.focus());
onUnmounted(() => {
  if (timer) clearTimeout(timer);
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <input
      ref="inputEl"
      v-model="query"
      data-testid="search-input"
      type="search"
      placeholder="Search all vaults…"
      aria-label="Search all vaults"
      class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      @keydown.escape="onEscape"
    />
    <p v-if="error" class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200">
      {{ error }}
    </p>
    <p v-if="tooShort" class="text-xs text-slate-400">
      Type at least {{ MIN_QUERY_CHARS }} characters to search.
    </p>
    <p v-else-if="searching && hits.length === 0" class="text-xs text-slate-400">
      Searching…
    </p>
    <p v-else-if="hits.length === 0 && resultsQuery" class="text-xs text-slate-400">
      No matches for "{{ resultsQuery }}".
    </p>
    <div v-for="group in groups" :key="group.vaultId" class="flex flex-col gap-1">
      <h2 class="text-xs font-semibold uppercase tracking-wide text-slate-400">
        {{ group.vaultName }}
      </h2>
      <button
        v-for="hitItem in group.hits"
        :key="hitItem.file"
        type="button"
        data-testid="search-hit"
        class="flex w-full cursor-pointer flex-col items-start gap-0.5 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="openHit(hitItem)"
      >
        <span class="w-full truncate text-sm text-slate-100" :title="hitItem.name">
          <template v-for="(part, i) in highlightParts(hitItem.name, resultsQuery)" :key="i">
            <mark v-if="part.match" class="rounded bg-violet-500/40 text-inherit">{{ part.text }}</mark>
            <template v-else>{{ part.text }}</template>
          </template>
        </span>
        <span v-if="hitItem.folder" class="w-full truncate text-xs text-slate-500">
          {{ hitItem.folder }}
        </span>
        <span v-if="hitItem.snippet" class="w-full truncate text-xs text-slate-400">
          <template v-for="(part, i) in highlightParts(hitItem.snippet, resultsQuery)" :key="i">
            <mark v-if="part.match" class="rounded bg-violet-500/40 text-inherit">{{ part.text }}</mark>
            <template v-else>{{ part.text }}</template>
          </template>
        </span>
      </button>
    </div>
    <p v-if="truncated" data-testid="search-truncated" class="text-xs text-slate-500">
      Showing the first {{ hits.length }} matches — refine your query.
    </p>
  </div>
</template>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/search.test.ts`
Expected: PASS (all 11).

- [ ] **Step 5: Commit**

```bash
git add src/components/Search.vue tests/search.test.ts
git commit -m "feat(ui): Search view with live debounced cross-vault results

300ms debounce with a 2-char minimum, a monotonic request ticket so a
slow older response can never clobber newer results, vault-grouped rows
with index-based highlighting, and open-in-Obsidian + close-panel on
click (toast + stay open on failure)."
```

---

### Task 7: ActionPanel wiring — icon, title, outlet

**Files:**
- Modify: `src/components/ActionPanel.vue`
- Test: `tests/action-panel.test.ts` (append)

**Interfaces:**
- Consumes: `store.openSearch()` (Task 4), `<Search />` (Task 6).
- Produces: the user-visible entry point — `data-testid="search-toggle"` in the header, only on the `list` view.

- [ ] **Step 1: Write the failing tests**

Append to `tests/action-panel.test.ts` (inside the existing `describe`):

```ts
  it("shows the search icon beside the cog on the list view and opens the search view", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.find('[data-testid="search-toggle"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="settings-toggle"]').exists()).toBe(true);
    await wrapper.get('[data-testid="search-toggle"]').trigger("click");
    expect(store.view).toBe("search");
    expect(wrapper.text()).toContain("Search");
    expect(wrapper.find('[data-testid="search-input"]').exists()).toBe(true);
    // Off the list view the header swaps to the back button — no search icon.
    expect(wrapper.find('[data-testid="search-toggle"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="back-button"]').exists()).toBe(true);
  });

  it("back from the search view returns to the vault list", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper.get('[data-testid="search-toggle"]').trigger("click");
    await wrapper.get('[data-testid="back-button"]').trigger("click");
    expect(store.view).toBe("list");
    expect(wrapper.text()).toContain("Personal");
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/action-panel.test.ts`
Expected: the two new tests FAIL — `[data-testid="search-toggle"]` not found.

- [ ] **Step 3: Implement in `src/components/ActionPanel.vue`**

1. Import the component (with the other component imports):

```ts
import Search from "./Search.vue";
```

2. Header title — extend the nested ternary with a `search` case (insert before the final `"Vaults"` fallback):

```
                    : view === "tasks"
                      ? "Tasks"
                      : view === "search"
                        ? "Search"
                        : "Vaults"
```

3. Search button — insert BEFORE the settings-toggle button inside the header's `flex items-center gap-2` div (its `v-if` matches the cog's; the cog's existing `v-else` back-button pairing is untouched because it binds to the cog's `v-if`):

```vue
        <button
          v-if="view === 'list'"
          type="button"
          class="cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          aria-label="Search vaults"
          title="Search vaults"
          data-testid="search-toggle"
          @click="store.openSearch()"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <circle cx="11" cy="11" r="8" />
            <path d="m21 21-4.35-4.35" />
          </svg>
        </button>
```

4. View outlet — insert after the `tasks` branch, before the final `v-else` (the vault list):

```vue
    <div
      v-else-if="view === 'search'"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <Search />
    </div>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/action-panel.test.ts tests/search.test.ts`
Expected: PASS.

- [ ] **Step 5: Full frontend gates**

Run: `npm test`
Expected: entire suite green.

Run: `npm run build`
Expected: vue-tsc + vite build succeed.

- [ ] **Step 6: Commit**

```bash
git add src/components/ActionPanel.vue tests/action-panel.test.ts
git commit -m "feat(ui): header search icon opens the cross-vault search view"
```

---

### Task 8: Docs + full verification

**Files:**
- Modify: `AGENTS.md` (IPC surface list; new search-domain paragraph; frontend `view` union mention)

**Interfaces:**
- Consumes: everything above.
- Produces: up-to-date agent guidance (a repo invariant: AGENTS.md is the single source of truth and must track the repo).

- [ ] **Step 1: Update `AGENTS.md`**

1. IPC surface paragraph — after the tasks-surface sentence (`get_tasks_config`, … `set_task_status`), add the search surface:

```
and the search surface: `search_vaults` (async — the scan must not run on
the main thread) + `open_search_result` — in `src-tauri/src/search_commands.rs`.
```

(Adjust the existing sentence's `— commands live in …` list to include `search_commands.rs`.)

2. New domain subsection after "### The tasks domain": add a short paragraph:

```markdown
### The search domain (`core/src/search.rs` + `search_commands.rs` + `Search.vue`)

Cross-vault, read-only, on-demand search (no index): `core::search::search_vaults`
walks every registered vault with the tasks-walk discipline (canonical
containment, cycle set, dot-entry skips, deterministic name-ordered walk),
matching case-insensitive substrings against note stems + note content
(≤ 1 MiB, UTF-8 — larger/binary files match by name only) and attachment
filenames. Hard caps: 2-char minimum query, 100 hits globally (`truncated`
flag → "refine your query" footer), filename matches surface before
content-only matches. Each hit carries the ready-made `obsidian://open`
`file` parameter (`.md` dropped for notes, extension kept for attachments);
`open_search_result` launches it via `uri::launch` — search never writes.
`search_vaults` is deliberately **async** (sync commands run on the main
thread; a content scan there would freeze window show/hide and drags) and
wraps the walk in `spawn_blocking`; it touches no window APIs and no locks.
The panel's `search` view (parent: the vault list) is a self-contained
`Search.vue` — 300 ms debounce, monotonic request ticket against stale
responses, vault-grouped rows, index-based highlighting (never a RegExp from
user input).
```

3. Frontend state section — extend the view union sentence: `view: list | settings | captureSettings | recordings | recordMode | transcriptions | tasks | search` and mention the header search icon beside the cog on the list view.

- [ ] **Step 2: Full verification battery**

```bash
npm test                                             # entire Vitest suite
npm run build                                        # vue-tsc + vite
cd src-tauri && cargo fmt --check && cd ..
cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test && cd ../..
npx tauri build --no-bundle                          # Linux shell compile gate
```

Expected: every command green. If `npx tauri build` fails on missing system libs, run `npm run setup:linux` once and retry.

- [ ] **Step 3: Commit**

```bash
git add AGENTS.md
git commit -m "docs(agents): document the search domain"
```

- [ ] **Step 4: Push and open the PR**

```bash
git push -u origin claude/buddy-vault-search-7wialz
```

Then create the PR (ready for review) for `claude/buddy-vault-search-7wialz` → `main` summarizing: the spec, the core scan + caps, the async command rationale, the new view, and the test coverage. Subscribe to PR activity afterward.
