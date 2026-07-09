//! Read-only cross-vault search backing the panel's Search view. Walks every
//! registered vault on demand (no index, no background state) with the same
//! reparse-safe discipline as the tasks scan, matching case-insensitive
//! substrings against note stems + note text and attachment filenames. Never
//! writes; opening a hit is delegated to Obsidian via `obsidian://`. See
//! docs/superpowers/specs/2026-07-09-vault-search-design.md.

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

/// Max chars of a content snippet. One line of context is enough to decide
/// whether a hit is the right note; the panel is 360px wide.
pub const SNIPPET_CHARS: usize = 120;

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
        collect_hits(
            &canon_root,
            &canon_root,
            vault,
            &query_lower,
            &mut walked,
            &mut raw,
        );
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
                out.push(RawHit {
                    name_matched: true,
                    hit,
                });
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

    // `Vault`, `Path` etc. arrive via the module's own imports through the
    // `use super::*;` above — no extra use lines needed here.

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
}
