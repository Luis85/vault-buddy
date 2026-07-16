//! Parsing/normalization: due/tag validity, comment stripping, scalar and
//! tags frontmatter reads. Lenient by design — invalid entries are dropped,
//! never an error — matching the vault domain's defensive-read posture.

use crate::capture_note::note_field;
use std::collections::HashSet;

/// True iff `s` is a plain `YYYY-MM-DD` (digits and hyphens in position — no
/// calendar validity check; Obsidian tolerates e.g. 2026-02-31 and the UI
/// uses a native date picker). Shared by the shell's write validation and the
/// sort's "does this due count" test so they can never disagree.
pub fn is_valid_due(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// True iff `s` is a valid Obsidian tag: letters (any script), digits, `-`,
/// `_`, `/`, and at least one non-digit character. Shared by the lenient
/// read-side normalization (invalid entries are dropped) and the shell's
/// strict write validation (invalid entries are an error) so the two sides
/// can never disagree on what a tag is.
pub fn is_valid_tag(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '/'))
        && s.chars().any(|c| !c.is_ascii_digit())
}

/// Normalize one raw tag token from frontmatter: unquote (double- or
/// single-quoted — Obsidian accepts both YAML scalar forms), trim, strip a
/// leading `#`; None when the result fails `is_valid_tag` (dropped by the
/// lenient reader).
fn normalize_tag(raw: &str) -> Option<String> {
    let unquoted = crate::capture_note::unquote_yaml(raw.trim());
    let t = unquoted.trim();
    // Single-quoted YAML scalar: strip the surrounding pair (the charset
    // forbids quotes inside a tag, so '' escapes can't occur in a valid one).
    let t = t
        .strip_prefix('\'')
        .and_then(|r| r.strip_suffix('\''))
        .unwrap_or(t);
    let t = t.strip_prefix('#').unwrap_or(t);
    if is_valid_tag(t) {
        Some(t.to_string())
    } else {
        None
    }
}

/// Case-insensitive dedupe preserving first-seen casing (Obsidian matches
/// tags case-insensitively but displays the authored case).
fn dedupe_tags(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in items {
        if seen.insert(t.to_lowercase()) {
            out.push(t);
        }
    }
    out
}

/// Cut an inline YAML comment off a tags value or block item. A `#` preceded
/// by whitespace starts a comment (YAML's rule). A `#` at the very start is
/// kept as a lenient `#`-prefixed tag UNLESS followed by whitespace/end —
/// `#work` stays a tag, `# note` is a pure comment. Codex review, PR #46: a
/// hand-authored `tags: work # private note` was tokenizing the comment text
/// into phantom tags that then rendered, filtered, and persisted on rewrite.
pub(super) fn strip_inline_comment(rest: &str) -> &str {
    let b = rest.as_bytes();
    for i in 0..b.len() {
        if b[i] != b'#' {
            continue;
        }
        if i == 0 {
            if b.len() == 1 || b[1].is_ascii_whitespace() {
                return "";
            }
        } else if b[i - 1].is_ascii_whitespace() {
            return rest[..i].trim_end();
        }
    }
    rest
}

/// Cut a trailing YAML comment off the SCALAR (non-flow, non-block) tags
/// form only — `tags: work # note` / `tags: #work #home`. `strip_inline_comment`
/// treats every whitespace-preceded `#` as a comment start, which is correct
/// for status/created/due/priority (their valid values never contain `#`,
/// so there's nothing to disambiguate) but wrong here: the lenient reader
/// also accepts a leading `#` as ANOTHER tag token
/// (`tags: #work #home` → two tags), and that token's own leading `#` is
/// whitespace-preceded too. The discriminator: a `#` starts a comment only
/// when it is NOT glued to a following tag character — i.e. followed by
/// whitespace or end-of-value (`tags: #work # areas` strips at the second
/// `#`); a `#` immediately followed by a non-whitespace character is another
/// tag token and scanning continues past it. Codex review, PR #46 round 2:
/// the plain strip truncated `tags: #work #home` to `#work`, dropping every
/// tag after the first from chips/filtering and from what a later tags edit
/// would persist.
///
/// The glued-`#`-is-a-tag leniency belongs ONLY to that leading-`#` list form
/// — a value that STARTS with `#`. A bare-first scalar (`tags: work #private
/// note`) is plain YAML, where a whitespace-preceded `#` always starts a
/// comment regardless of the next byte, so it falls back to
/// `strip_inline_comment`. Codex review, PR #46 round 3: without this gate,
/// `tags: work #private note` tokenized the comment into phantom `private`/
/// `note` tags.
fn strip_scalar_tags_comment(rest: &str) -> &str {
    // Only the Obsidian leading-`#` tag-list form (value starts with `#`)
    // treats a glued `#tag` after whitespace as another tag; everything else
    // is plain YAML.
    if !rest.trim_start().starts_with('#') {
        return strip_inline_comment(rest);
    }
    let b = rest.as_bytes();
    for i in 0..b.len() {
        if b[i] != b'#' {
            continue;
        }
        let is_comment_start = if i == 0 {
            b.len() == 1 || b[1].is_ascii_whitespace()
        } else {
            b[i - 1].is_ascii_whitespace() && (i + 1 == b.len() || b[i + 1].is_ascii_whitespace())
        };
        if is_comment_start {
            return rest[..i].trim_end();
        }
    }
    rest
}

/// Read a STRUCTURED frontmatter scalar (status/created/due/priority): the
/// raw `note_field` value with an inline YAML comment stripped, plus one
/// unquote pass for the quoted-then-commented corner (`due: "…" # x` —
/// note_field's own unquote no-ops there because the raw value doesn't end
/// with the quote). Valid values of these fields never contain `#`, spaces,
/// or quotes, so the strip can't eat real content. Titles deliberately stay
/// on raw `note_field`: free text, where the lenient read keeps everything.
/// Codex review, PR #46: `due: 2026-07-15 # client` was failing is_valid_due
/// and bucketing as no-date; `priority: high # urgent` degraded to normal.
pub(super) fn scalar_field(content: &str, key: &str) -> Option<String> {
    let raw = note_field(content, key)?;
    let stripped = strip_inline_comment(raw.trim()).trim();
    let unwrapped = if stripped.len() >= 2
        && ((stripped.starts_with('"') && stripped.ends_with('"'))
            || (stripped.starts_with('\'') && stripped.ends_with('\'')))
    {
        &stripped[1..stripped.len() - 1]
    } else {
        stripped
    };
    Some(unwrapped.to_string())
}

/// True iff the frontmatter declares `key` under ANY casing. The id-stamp
/// uses this (not the exact `scalar_field`) to decide "already has an id":
/// Obsidian folds frontmatter key case and `is_valid_id_property` accepts
/// case variants, so a task stamped `Task-ID:` must count as present when the
/// config later uses `task-id`, or a second conflicting line would be stamped.
pub(super) fn has_frontmatter_key_ci(content: &str, key: &str) -> bool {
    let mut lines = content.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    for line in lines {
        let t = line.trim_end();
        if t.trim_end() == "---" {
            return false; // closing fence — key not found in frontmatter
        }
        // Only TOP-LEVEL keys count: set_fields inserts/rewrites unindented
        // `key:` lines, so an indented (nested) `task-id:` under a mapping is
        // NOT the top-level property and must not suppress a stamp (Codex) —
        // else a task whose only `task-id` is nested would never get a usable
        // top-level id.
        if t.starts_with([' ', '\t']) {
            continue;
        }
        if let Some((k, _)) = t.split_once(':') {
            if k.trim().eq_ignore_ascii_case(key) {
                return true;
            }
        }
    }
    false
}

/// Parse one frontmatter tags-ish key. None when the key is absent; Some of
/// the normalized (possibly empty) list when present — so a present-but-empty
/// `tags:` still shadows the `tag:` alias.
fn parse_tags_key(content: &str, key: &str) -> Option<Vec<String>> {
    let mut lines = content.lines().peekable();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    while let Some(line) = lines.next() {
        if line.trim_end() == "---" {
            return None; // end of frontmatter — the body is never scanned
        }
        // Top-level keys only: an indented list item can't match (leading
        // space), same convention as note_field.
        let Some(rest) = line.strip_prefix(&prefix) else {
            continue;
        };
        // Strip a trailing YAML comment BEFORE the empty-value check, so
        // `tags: # comment` correctly falls into the block-list branch below
        // rather than being read as a (nonexistent) inline value. A FLOW list
        // is bracket-delimited, so nothing inside `[...]` is a comment — the
        // whole-value strip was eating the second leading-# tag in
        // `[#work, #home]` (whitespace-preceded `#`) and truncating the list
        // (Codex, PR #46). There, keep everything through the closing `]`
        // and drop only what trails it; an unterminated `[` degrades to the
        // scalar strip as before.
        let rest_raw = rest.trim();
        let rest = if rest_raw.starts_with('[') {
            match rest_raw.find(']') {
                Some(end) => rest_raw[..=end].trim(),
                None => strip_inline_comment(rest_raw).trim(),
            }
        } else {
            // Scalar branch: use the tag-aware discriminator, not the plain
            // strip — see strip_scalar_tags_comment's doc comment.
            strip_scalar_tags_comment(rest_raw).trim()
        };
        let raw_items: Vec<&str> = if rest.is_empty() {
            // Block style: consume the following `- item` lines.
            let mut items = Vec::new();
            while let Some(next) = lines.peek() {
                if next.trim_end() == "---" {
                    break;
                }
                let t = next.trim_start();
                // A comment-only line before/between items is YAML-skippable
                // — breaking here silently dropped every tag after it
                // (Codex, PR #46). A whole line starting `#` is always a
                // comment (the lenient `#work`-as-tag form only exists
                // inside a VALUE, never as a line).
                if t.starts_with('#') {
                    lines.next();
                    continue;
                }
                let Some(item) = t.strip_prefix("- ") else {
                    break;
                };
                items.push(strip_inline_comment(item.trim()).trim());
                lines.next();
            }
            items
        } else if rest.starts_with('[') {
            // Flow `[a, b]` style: strip brackets, split only on commas.
            // An unquoted item with a space (e.g. `[a, two words]`) would fail
            // validation because space is not in the tag charset, so it's
            // dropped by the lenient reader.
            let inner = rest
                .strip_prefix('[')
                .and_then(|r| r.strip_suffix(']'))
                .unwrap_or(rest);
            inner.split(',').map(str::trim).collect()
        } else {
            // Legacy `a, b` / `a b` format: split on commas AND whitespace.
            rest.split(',').flat_map(str::split_whitespace).collect()
        };
        return Some(dedupe_tags(raw_items.into_iter().filter_map(normalize_tag)));
    }
    None
}

/// A task's tags from frontmatter, in every form Obsidian accepts (see
/// parse_tags_key). `tags:` wins; the `tag:` singular alias is read only
/// when `tags:` is absent. Body `#hashtags` are deliberately out of scope —
/// the scanner stays frontmatter-only like the rest of the vault domain.
pub fn note_tags(content: &str) -> Vec<String> {
    parse_tags_key(content, "tags")
        .or_else(|| parse_tags_key(content, "tag"))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_frontmatter_key_ci_matches_regardless_of_casing() {
        // A task stamped `Task-ID:` must be seen as "already has an id" by a
        // config using the lowercase `task-id` property name, and vice versa
        // — Obsidian folds frontmatter key case, so a case-sensitive check
        // would stamp a second, conflicting id line (the disk.rs regression).
        let upper = "---\ntype: Task\nTask-ID: abc123\n---\n";
        assert!(has_frontmatter_key_ci(upper, "task-id"));
        assert!(has_frontmatter_key_ci(upper, "TASK-ID"));
        let lower = "---\ntype: Task\ntask-id: abc123\n---\n";
        assert!(has_frontmatter_key_ci(lower, "Task-ID"));
    }

    #[test]
    fn has_frontmatter_key_ci_false_for_absent_key_and_body_only_occurrence() {
        assert!(!has_frontmatter_key_ci("---\ntype: Task\n---\n", "task-id"));
        // A same-named line AFTER the closing fence is body content, not
        // frontmatter — it must never count as "present".
        assert!(!has_frontmatter_key_ci(
            "---\ntype: Task\n---\ntask-id: sneaky\n",
            "task-id"
        ));
        assert!(!has_frontmatter_key_ci("no frontmatter", "task-id"));
    }

    #[test]
    fn has_frontmatter_key_ci_handles_colonless_lines_and_unterminated_frontmatter() {
        // A frontmatter line with no colon at all (malformed/hand-edited)
        // must not match and must not panic — split_once yields None, so the
        // scan just continues to the next line.
        assert!(!has_frontmatter_key_ci(
            "---\ntype: Task\nnotacolonhere\ntask-id: abc\n---\n",
            "missing"
        ));
        // Unterminated frontmatter (opens but the closing fence never comes)
        // falls through to false rather than treating a stray line as a
        // match past where a real document would have closed.
        assert!(!has_frontmatter_key_ci("---\ntype: Task\n", "due"));
    }

    #[test]
    fn has_frontmatter_key_ci_ignores_nested_indented_keys() {
        // Codex PR #59: an indented `task-id:` nested under a mapping is NOT
        // the top-level property. set_fields only inserts/rewrites unindented
        // keys, so treating the nested key as "present" would suppress a stamp
        // and leave the task with no usable top-level id. The top-level scan
        // must skip indented lines.
        let nested = "---\ntype: Task\nmetadata:\n  task-id: old\n---\n";
        assert!(!has_frontmatter_key_ci(nested, "task-id"));
        assert!(!has_frontmatter_key_ci(
            "---\ntype: Task\nmeta:\n\ttask-id: old\n---\n", // tab-indented too
            "task-id"
        ));
        // A genuinely top-level key (any casing) still matches.
        assert!(has_frontmatter_key_ci(
            "---\ntype: Task\nTask-ID: abc\n---\n",
            "task-id"
        ));
    }

    #[test]
    fn is_valid_due_accepts_only_plain_dates() {
        assert!(is_valid_due("2026-07-15"));
        assert!(!is_valid_due("2026-7-15"));
        assert!(!is_valid_due("tomorrow"));
        assert!(!is_valid_due("2026-07-15T10:00"));
        assert!(!is_valid_due(""));
    }

    #[test]
    fn is_valid_tag_accepts_obsidian_charset_and_rejects_the_rest() {
        for ok in ["work", "home/errands", "a-b_c", "año", "q3-2026", "1-2"] {
            assert!(is_valid_tag(ok), "{ok} should be valid");
        }
        // all-digits, empty, spaces, punctuation → invalid
        for bad in ["123", "", "two words", "a.b", "#work", "a,b"] {
            assert!(!is_valid_tag(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn note_tags_parses_flow_block_and_legacy_forms() {
        let flow = "---\ntype: Task\ntags: [work, home/errands]\n---\n";
        assert_eq!(note_tags(flow), vec!["work", "home/errands"]);
        let block = "---\ntype: Task\ntags:\n  - work\n  - \"home/errands\"\n---\n";
        assert_eq!(note_tags(block), vec!["work", "home/errands"]);
        let legacy = "---\ntype: Task\ntags: work, home/errands\n---\n";
        assert_eq!(note_tags(legacy), vec!["work", "home/errands"]);
        let spaces = "---\ntype: Task\ntags: work home/errands\n---\n";
        assert_eq!(note_tags(spaces), vec!["work", "home/errands"]);
    }

    #[test]
    fn note_tags_normalizes_and_dedupes() {
        // `#` stripped, invalid entries dropped, case-insensitive dedupe keeps
        // the first-seen casing — lenient read, never an error.
        let doc = "---\ntype: Task\ntags: [#Work, work, 123, two words, urgent]\n---\n";
        assert_eq!(note_tags(doc), vec!["Work", "urgent"]);
    }

    #[test]
    fn note_tags_reads_the_tag_alias_only_when_tags_is_absent() {
        let alias = "---\ntype: Task\ntag: work\n---\n";
        assert_eq!(note_tags(alias), vec!["work"]);
        let both = "---\ntype: Task\ntags: [a1]\ntag: b1\n---\n";
        assert_eq!(note_tags(both), vec!["a1"]); // tags: wins
    }

    #[test]
    fn note_tags_is_empty_without_frontmatter_or_key_and_never_reads_the_body() {
        assert!(note_tags("no frontmatter").is_empty());
        assert!(note_tags("---\ntype: Task\n---\n").is_empty());
        // A `tags:`-looking line in the body must not be read.
        assert!(note_tags("---\ntype: Task\n---\ntags: [body]\n").is_empty());
        // Block list stops at the closing fence.
        let fenced = "---\ntype: Task\ntags:\n- work\n---\n- not-a-tag\n";
        assert_eq!(note_tags(fenced), vec!["work"]);
    }

    #[test]
    fn note_tags_keeps_leading_hash_tags_inside_a_flow_list() {
        // Codex review, PR #46: the whole-value comment strip saw the second
        // `#` in `[#work, #home]` as whitespace-preceded (= comment start)
        // and truncated the value to `[#work,` before the flow split, so the
        // tags vanished and a later edit could clobber them. Inside brackets
        // nothing is a comment; only a trailing comment after `]` is.
        assert_eq!(
            note_tags("---\ntype: Task\ntags: [#work, #home]\n---\n"),
            vec!["work", "home"]
        );
        assert_eq!(
            note_tags("---\ntype: Task\ntags: [#work, #home] # areas\n---\n"),
            vec!["work", "home"]
        );
        // Flow with a trailing comment and no leading-# tags — still stripped.
        assert_eq!(
            note_tags("---\ntype: Task\ntags: [work] # areas\n---\n"),
            vec!["work"]
        );
    }

    #[test]
    fn note_tags_edge_forms() {
        // Empty flow list is PRESENT — it yields no tags and still shadows the alias.
        assert!(note_tags("---\ntype: Task\ntags: []\ntag: work\n---\n").is_empty());
        // CRLF content parses (str::lines strips \r).
        assert_eq!(
            note_tags("---\r\ntype: Task\r\ntags: [work]\r\n---\r\n"),
            vec!["work"]
        );
        // No space after the colon.
        assert_eq!(
            note_tags("---\ntype: Task\ntags:[work]\n---\n"),
            vec!["work"]
        );
    }

    #[test]
    fn note_tags_skips_comment_only_lines_inside_a_block_list() {
        // Codex review, PR #46: a comment-only line before or between block
        // items (`tags:` / `  # areas` / `  - work`) broke the item scan and
        // silently dropped every tag after it — YAML skips comment lines
        // inside a block list, so the reader must scan past them.
        assert_eq!(
            note_tags("---\ntype: Task\ntags:\n  # areas\n  - work\n- home\n---\n"),
            vec!["work", "home"]
        );
        assert_eq!(
            note_tags("---\ntype: Task\ntags:\n- work\n# midway\n- home\ntitle: \"T\"\n---\n"),
            vec!["work", "home"]
        );
    }

    #[test]
    fn note_tags_strips_inline_yaml_comments() {
        // Codex review (PR #46): comment words after ` #` must not become
        // phantom tags that render, filter, and persist on the next rewrite.
        assert_eq!(
            note_tags("---\ntype: Task\ntags: work # private note\n---\n"),
            vec!["work"]
        );
        assert_eq!(
            note_tags("---\ntype: Task\ntags: [work, home] # q3\n---\n"),
            vec!["work", "home"]
        );
        // A pure-comment value is a present-but-empty tags key (still shadows
        // the tag: alias, same as `tags:` with nothing at all).
        assert!(note_tags("---\ntype: Task\ntags: # none yet\n---\n").is_empty());
        // Block items with trailing comments.
        assert_eq!(
            note_tags("---\ntype: Task\ntags:\n- work # main\n---\n"),
            vec!["work"]
        );
        // A bare #-prefixed value stays a lenient tag, not a comment.
        assert_eq!(
            note_tags("---\ntype: Task\ntags: #work\n---\n"),
            vec!["work"]
        );
    }

    #[test]
    fn scalar_tag_list_keeps_every_leading_hash_tag() {
        // Codex PR #46 round 2: `tags: #work #home` lost every tag after the
        // first — the scalar branch's comment strip treated the whitespace
        // before the second `#` as starting a YAML comment, truncating the
        // value to `#work` before the legacy whitespace/comma split ran.
        let content = "---\ntype: Task\ntitle: \"t\"\ntags: #work #home\n---\n";
        assert_eq!(note_tags(content), vec!["work", "home"]);
    }

    #[test]
    fn scalar_tag_list_still_strips_a_real_comment() {
        // The discriminator: `#` glued to the next character is another tag
        // token; `#` followed by whitespace (or end of value) is a comment.
        let content = "---\ntype: Task\ntitle: \"t\"\ntags: #work # areas\n---\n";
        assert_eq!(note_tags(content), vec!["work"]);
    }

    #[test]
    fn scalar_tag_bare_value_strips_a_compact_comment() {
        // Codex PR #46 round 3: `tags: work #private note` — a bare-first
        // scalar is plain YAML, so a whitespace-preceded `#` starts a comment
        // even when glued to the next char (no space after `#`). The
        // glued-`#`-is-another-tag leniency belongs only to the Obsidian
        // leading-`#` list form, where the VALUE ITSELF starts with `#`
        // (`tags: #work #home`). Without this the comment words tokenized into
        // phantom `private`/`note` tags that rendered, filtered, and would
        // persist on the next rewrite.
        let content = "---\ntype: Task\ntitle: \"t\"\ntags: work #private note\n---\n";
        assert_eq!(note_tags(content), vec!["work"]);
    }

    #[test]
    fn note_tags_unquotes_single_quoted_scalars() {
        // Regression (Codex review, PR #46): YAML-valid single-quoted tags were
        // left with their apostrophes, failed the charset check, and silently
        // vanished — losing chips and letting a later edit clobber the line.
        assert_eq!(
            note_tags("---\ntype: Task\ntags: ['work', 'home/errands']\n---\n"),
            vec!["work", "home/errands"]
        );
        assert_eq!(
            note_tags("---\ntype: Task\ntags:\n  - 'work'\n---\n"),
            vec!["work"]
        );
    }
}
