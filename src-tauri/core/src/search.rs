//! Read-only cross-vault search backing the panel's Search view. Walks every
//! registered vault on demand (no index, no background state) with the same
//! reparse-safe discipline as the tasks scan, matching case-insensitive
//! substrings against note stems + note text and attachment filenames. Never
//! writes; opening a hit is delegated to Obsidian via `obsidian://`. See
//! docs/superpowers/specs/2026-07-09-vault-search-design.md.

use crate::discovery::Vault;
use crate::vault_walk::{walk_vault, Flow};
use std::path::Path;

use crate::search_cache::cached_lowered;
pub use crate::search_cache::SearchCache;

/// Trimmed CHARS (not bytes) a query needs before anything is scanned — a
/// 1-char query would match nearly every file in every vault.
pub const MIN_QUERY_CHARS: usize = 2;
/// Global hit budget across all vaults; the UI shows a "refine your query"
/// footer when it is exceeded.
pub const MAX_RESULTS: usize = 100;
/// Files larger than this are matched by NAME only — reading arbitrarily
/// large files on every keystroke-pause would make live search crawl.
pub const MAX_CONTENT_BYTES: u64 = 1024 * 1024;

/// Max chars of a content snippet. One line of context is enough to decide
/// whether a hit is the right note; the panel is 360px wide.
pub const SNIPPET_CHARS: usize = 120;

/// One search hit. `file` is exactly the `obsidian://open` `file` parameter:
/// vault-relative, `/`-separated, extension dropped ONLY for exactly-`.md`
/// notes (Obsidian's canonical form, `uri::vault_relative_no_ext`) and KEPT
/// otherwise — for attachments (else Obsidian resolves `report` as
/// `report.md`) and for `.MD`-style notes (exact-path open instead of
/// resolver guessing). Serializes camelCase — the command returns this type
/// directly (no DTO layer; `discovery::Vault` precedent).
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

/// Collect at most this many hits per class per vault. The bound keeps
/// transient memory small on a pathological query, and the `+ 1` makes
/// "more existed" observable even when a single vault fills the whole budget
/// (a full list always exceeds any budget ≤ MAX_RESULTS, so the merge's
/// accounting reports it as truncated).
const PER_VAULT_CAP: usize = MAX_RESULTS + 1;

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
/// content bytes are obtained. `is_cancelled` is polled once per file — a
/// superseded scan stops walking instead of running a stale multi-vault
/// walk to completion. A cancelled scan returns what it has; the caller is
/// about to discard it. See `scan_vault` for the per-vault contract.
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
    // One NAMED scoped thread per vault (crash records must identify the
    // dying thread), merged in the given vault order afterward — output is
    // identical to a serial loop, wall-clock is ~the slowest vault. Vault
    // counts are small (single digits), so no pooling.
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
                    // Thread spawn failed (resource pressure): degrade to an
                    // inline scan on this thread — never a panic, and the
                    // failure leaves a trace.
                    log::warn!("search: spawning scan thread failed: {e}");
                    pending.push(Err(scan_vault(vault, query_lower, cache, is_cancelled)));
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

/// A vault's collected matches, split by class (see `scan_vault`).
struct VaultHits {
    name_hits: Vec<SearchHit>,
    content_hits: Vec<SearchHit>,
}

/// One vault's matches: filename matches then content-only matches, walk
/// order within each class. Two independently-capped lists make "filename
/// matches surface before content-only matches" a HARD guarantee: when the
/// content list fills, the walk stops READING file contents but keeps
/// checking NAMES to the end of the vault (dirent string ops — cheap), so a
/// late-walking filename match can never be displaced by earlier content
/// matches; only a full filename list aborts the walk (its hits alone
/// already exceed any budget, so nothing later could surface anyway).
/// `is_cancelled` is polled once per file. `None` = unresolvable vault path
/// (moved/deleted), skipped silently.
fn scan_vault(
    vault: &Vault,
    query_lower: &str,
    cache: &SearchCache,
    is_cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<VaultHits> {
    let canon_root = std::fs::canonicalize(Path::new(&vault.path)).ok()?;
    let mut name_hits: Vec<SearchHit> = Vec::new();
    let mut content_hits: Vec<SearchHit> = Vec::new();
    walk_vault(&canon_root, &mut |path, name| {
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
                content_snippet(path, query_lower, cache)
            } else {
                None
            };
            if name_matched {
                if name_hits.len() < PER_VAULT_CAP {
                    if let Some(hit) = make_hit(vault, &canon_root, path, stem, snippet, true) {
                        name_hits.push(hit);
                    }
                } else {
                    return Flow::Stop; // neither class can grow further
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
        .or_else(|| {
            lowered
                .lines()
                .find_map(|l| snippet_from_line(l, query_lower))
        })
}

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

/// A ~SNIPPET_CHARS-char window of `line` around its first case-insensitive
/// occurrence of `query_lower` (already lowercased by the caller), with `…`
/// marking trimmed ends. `None` when the line doesn't contain the query.
///
/// Char-boundary safe by construction: the window is cut from a `Vec<char>`
/// of the ORIGINAL line, never by byte-slicing. Centering maps the byte index
/// found in the lowercased line back to a char position, which is only
/// reliable when lowercasing changed neither the byte nor the char length
/// (`ẞ`→`ß`, `İ`→`i̇` do); when it did, the window falls back to the line
/// start rather than risk mis-centering — best-effort placement, never a
/// panic.
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
        assert!(
            s.chars().count() <= SNIPPET_CHARS + 2,
            "got len {}",
            s.chars().count()
        );
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
        write(
            dir.path(),
            "Notes/idea.md",
            "intro\nProject Alpha kickoff\n",
        );
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
        write(
            dir.path(),
            "t.md",
            "---\ntitle: \"Alpha review\"\n---\nbody\n",
        );
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert_eq!(r.hits.len(), 1);
        assert!(r.hits[0]
            .snippet
            .as_deref()
            .unwrap()
            .contains("Alpha review"));
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
        assert!(!h.is_note);
    }

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
            write(dir.path(), &format!("a {i:03}.md"), "contains alpha here\n");
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
        for key in [
            "vaultId",
            "vaultName",
            "isNote",
            "folder",
            "file",
            "snippet",
            "name",
        ] {
            assert!(v.get(key).is_some(), "missing wire key {key}");
        }
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
        assert_eq!(
            search_vaults(&[vault("v", "V", dir.path())], "read")
                .hits
                .len(),
            1
        );
        assert_eq!(
            search_vaults(&[vault("v", "V", dir.path())], "text")
                .hits
                .len(),
            1
        );
    }

    #[test]
    fn dot_dirs_and_dot_files_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".obsidian/alpha.md", "alpha\n");
        write(dir.path(), ".trash/alpha gone.md", "alpha\n");
        write(dir.path(), ".alpha hidden.md", "alpha\n");
        let r = search_vaults(&[vault("v1", "Work", dir.path())], "alpha");
        assert!(
            r.hits.is_empty(),
            "got: {:?}",
            r.hits.iter().map(|h| &h.file).collect::<Vec<_>>()
        );
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
        assert_eq!(
            search_vaults(&[vault("v1", "Work", dir.path())], "ää")
                .hits
                .len(),
            1
        );
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

    #[test]
    fn with_cache_equals_plain_search() {
        // A caller-supplied cache must not change results vs the cache-less entry
        // point (the cache is a pure optimization).
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "Notes/idea.md",
            "intro\nProject Alpha kickoff\n",
        );
        write(dir.path(), "Alpha plan.md", "nothing relevant\n");
        write(dir.path(), "slides/Alpha deck.pdf", "%PDF-fake");
        let vs = [vault("v1", "Work", dir.path())];
        let plain = search_vaults(&vs, "alpha");
        let cache = SearchCache::new();
        let cached = search_vaults_with_cache(&vs, "alpha", &cache, &|| false);
        assert_eq!(plain, cached);
    }
}
