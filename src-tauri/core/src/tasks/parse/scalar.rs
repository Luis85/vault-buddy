//! Low-level, id-focused scalar-frontmatter primitives: top-level
//! case-insensitive key lookup, the strict (full-YAML-escape-set) scalar
//! decoder, the block/flow non-scalar guards, and the id-property
//! assignability predicates built on top of them.
//!
//! Split out of `parse.rs` (now `parse/mod.rs`) purely for the crate's
//! nonblank-LOC cap — a pure move, not a rewrite. Every name here is
//! re-exported from `parse`'s own namespace at its ORIGINAL visibility (see
//! the `use scalar::{...}` lines in `parse/mod.rs`), so `super::parse::X`
//! still resolves identically from every existing caller (`tasks::disk`,
//! `tasks::structural`, `tasks::collect`, `tasks::parent`, `tasks::id`, and
//! `services::tasks::parent` for the `pub(crate)` names) — this split
//! changed no call site anywhere in the crate.

/// The on-disk casing of the first TOP-LEVEL `key:` line, matched
/// CASE-INSENSITIVELY — `None` when the key never appears at the top level
/// before the closing fence, or the fence is missing. Factored out of
/// `frontmatter_scalar_ci` so `scalar_id_ci` can pair the exact same
/// case-insensitive key search with the STRICT decode (`strict_scalar_field`)
/// instead of `frontmatter_scalar_ci`'s lenient `scalar_field` one — the two
/// disagreeing on an id's decode was Defect A (see `scalar_id_ci`'s doc
/// comment). Skips indented/nested lines: a nested `  task-id:` under a
/// mapping is never the top-level property `set_fields` would rewrite.
fn top_level_key_ci<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = content.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return None;
    }
    for line in lines {
        let t = line.trim_end();
        if t == "---" {
            return None; // closing fence — key not found in frontmatter
        }
        if t.starts_with([' ', '\t']) {
            continue;
        }
        if let Some((k, _)) = t.split_once(':') {
            let k = k.trim();
            if k.eq_ignore_ascii_case(key) {
                return Some(k);
            }
        }
    }
    None
}

/// The first TOP-LEVEL `key:` line matched CASE-INSENSITIVELY: its ACTUAL
/// on-disk key name AND parsed scalar value. Obsidian folds frontmatter key
/// case and `is_valid_id_property` accepts case variants, so reads and writes
/// must agree despite casing. The id-stamp (`update_task_fields`) uses BOTH
/// halves: the value to decide "already has a usable id" — a bare `task-id:`
/// reads as `Some("")`, treated as MISSING so a fresh id is still stamped
/// (Codex, PR #59) — and the on-disk NAME to rewrite a present-but-blank line
/// under its own casing, so `set_fields` (case-sensitive) matches it instead of
/// inserting a case-mismatched DUPLICATE the CI read would then shadow.
/// Delegates the key search to `top_level_key_ci`, then decodes the value via
/// the lenient `scalar_field` — the pairing `scalar_id_ci` below deliberately
/// does NOT use (it needs the strict decode instead; see its doc comment).
pub(in crate::tasks) fn frontmatter_scalar_ci(
    content: &str,
    key: &str,
) -> Option<(String, String)> {
    let on_disk = top_level_key_ci(content, key)?;
    super::scalar_field(content, on_disk).map(|v| (on_disk.to_string(), v))
}

/// STRICT optional-field scalar decode: the FULL YAML escape set (unlike
/// `scalar_field`'s shallow one-layer outer-quote strip), for the two places
/// where a wrong or partially-decoded value is worse than none — a parent
/// reference (`tasks/parent.rs`, which calls this via its `scalar` wrapper)
/// and a task's own id (`scalar_id_ci` below). Sharing this decoder between
/// the two is the fix for Defect A: `scalar_field`'s outer-quote strip is
/// `&stripped[1..len-1]`, a single character sliced off each end, which
/// leaves an embedded YAML `''` doubled-quote escape intact — a task stamped
/// `task-id: 'a''b'` read as `a''b` while a child's identical
/// `parent-id: 'a''b'`, decoded through the parent-id reader's full decode,
/// read as the correct `a'b`. The two could never compare equal, so
/// `parent_index` silently resolved no edge. Routing both reads through this
/// one decoder makes them agree by construction.
///
/// Deliberately NOT `decode_scalar_lenient`: that decoder exists for TITLES,
/// where falling back to raw text is right because a title must never
/// vanish. A parent reference (and an id) is the opposite: a wrong value
/// manufactures a phantom relationship, so unsupported/null-ish forms yield
/// `None` here, matching `description_field`'s rules (Codex P2, PR #77).
///
/// `link` gates the `[[wikilink]]` flow-sequence exemption: only
/// `parent_link_field` (via `parent.rs::scalar`) passes `true` — the form
/// users type for `parent`, never parsed for meaning. Every other caller —
/// `parent_id_field` and `scalar_id_ci` — passes `false`: an id must be a
/// plain scalar, so a wikilink-shaped value is exactly the kind of flow
/// value the block/flow guard below exists to reject.
///
/// `pub(crate)`, not `pub(super)`: `services::tasks::parent`'s phase-1
/// assignability forecast (design spec §2) calls this directly to decide
/// whether `ensure_id` will later be able to resolve the parent's id, before
/// Task IDs are enabled for the vault.
pub(crate) fn strict_scalar_field(content: &str, key: &str, link: bool) -> Option<String> {
    let raw = crate::capture_note::raw_scalar_field(content, key)?.trim();
    if raw.is_empty() {
        return None;
    }
    // A block (`|`/`>`) or flow (`{..}`) value is the user's own structure,
    // not our scalar. `[[wikilink]]` is exempt ONLY when `link` is set.
    let wikilink_exempt = link && raw.starts_with("[[");
    if !wikilink_exempt && (raw.starts_with(['|', '>', '{']) || raw.starts_with('[')) {
        return None;
    }
    // A leading `#` is a YAML comment — the property is null.
    if raw.starts_with('#') {
        return None;
    }
    let decoded = if raw.starts_with('"') {
        // An unterminated quoted scalar is multi-line; reject rather than
        // surfacing its first line.
        crate::yaml_scalar::yaml_unquote_multiline(super::super::description::double_quoted_slice(
            raw,
        )?)
    } else if raw.starts_with('\'') {
        super::super::description::decode_single_quoted(raw)?
    } else {
        // A PLAIN scalar can continue onto following indented lines: YAML folds
        // `key: abc` + an indented `def` into `abc def`. Surfacing only the first
        // physical line would report a value Obsidian does not agree with — and
        // for an id REFERENCE that is worse than useless: `abc` may be some other
        // task's id, so the hierarchy would resolve to the WRONG parent, or a
        // guard would see a phantom reference (Codex P2, PR #77). Reject, exactly
        // as the multi-line quoted and block forms above are rejected.
        if plain_scalar_continues(content, key) {
            return None;
        }
        let stripped = super::strip_inline_comment(raw).trim();
        if matches!(stripped, "null" | "Null" | "NULL" | "~") {
            return None;
        }
        stripped.to_string()
    };
    (!decoded.trim().is_empty()).then_some(decoded)
}

/// True when the top-level `key`'s PLAIN value continues onto the next line —
/// i.e. the line after it is non-empty and indented, which YAML folds into the
/// same scalar. Callers use this to refuse a value they would otherwise read
/// only the first line of.
fn plain_scalar_continues(content: &str, key: &str) -> bool {
    let mut lines = content.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    let mut at_key = false;
    for line in lines {
        let t = line.trim_end();
        if t == "---" {
            return false; // closing fence
        }
        if at_key {
            // YAML permits a plain scalar to continue ACROSS blank lines
            // (`key: abc`, blank, indented `def` folds to "abc\ndef"), so a
            // blank is not the end of the value — keep scanning and let the
            // first non-blank line decide (Codex P2, PR #77).
            let trimmed = t.trim();
            if trimmed.is_empty() {
                continue;
            }
            // A COMMENT-ONLY line is not part of the scalar either. Without
            // this, `task-id: abc` followed by an indented `# note` reads as a
            // continuation and the task LOSES its id (and a child its parent
            // link) until the comment is deleted (Codex P2, PR #77).
            if trimmed.starts_with('#') {
                continue;
            }
            return t.starts_with([' ', '\t']);
        }
        if t.starts_with([' ', '\t']) {
            continue; // nested line, not a top-level key
        }
        if let Some((k, _)) = t.split_once(':') {
            if k.trim().eq_ignore_ascii_case(key) {
                at_key = true;
            }
        }
    }
    false
}

/// True when the top-level `key:` line (exact, ON-DISK casing) opens a BLOCK
/// value — a nested mapping or block list on the following lines — rather than
/// standing alone as a truly blank scalar. The id-stamp asks this before
/// rewriting a present-but-empty key: `set_fields`' rewrite CONSUMES a block
/// (indented continuations and `- ` items, comment lines deferred) along with
/// the key line, so stamping over `uid:` + `  source: jira` would DELETE the
/// user's nested frontmatter (review, PR #59). The walk mirrors the writer's
/// consumption predicate exactly, biased safe: whitespace-only lines count as
/// block (the writer would consume them), a blank line or the closing fence
/// ends the block before it starts.
pub(in crate::tasks) fn key_opens_block(content: &str, key: &str) -> bool {
    let mut lines = content.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    let mut past_key = false;
    for line in lines {
        let t = line.trim_end();
        if t == "---" {
            return false;
        }
        if !past_key {
            // Find the key's own line: top-level, exact casing, colon-anchored
            // (the same match set_fields will make).
            if !line.starts_with([' ', '\t'])
                && t.strip_prefix(key)
                    .is_some_and(|rest| rest.starts_with(':'))
            {
                past_key = true;
            }
            continue;
        }
        let trimmed = t.trim_start();
        if trimmed.starts_with('#') {
            continue; // deferred, like the writer's pending_comments buffer
        }
        return line.starts_with([' ', '\t']) || trimmed.starts_with("- ");
    }
    false
}

/// True when the top-level `key:` line (exact, ON-DISK casing) holds a FLOW
/// collection value — an inline mapping `{...}` or sequence `[...]` on the SAME
/// line — rather than a plain or quoted scalar. Unlike a block collection
/// (`key_opens_block`, multi-line), a flow value is one line, so `set_fields`
/// would rewrite that single line and DELETE the user's inline structure; the
/// id-stamp/strip must skip it just as it skips a block (Codex P2, PR #76). The
/// RAW value is inspected: a quoted `"[x]"` scalar starts with a quote, not a
/// bracket, so it is correctly NOT treated as flow (a plain YAML scalar can
/// never start with `{`/`[`).
pub(in crate::tasks) fn key_opens_flow(content: &str, key: &str) -> bool {
    let mut lines = content.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return false;
    }
    for line in lines {
        let t = line.trim_end();
        if t == "---" {
            return false; // closing fence — key not found
        }
        if line.starts_with([' ', '\t']) {
            continue; // nested key, never the top-level property
        }
        // Match the key's own line, colon-anchored — the same match set_fields
        // makes — then look at the raw value after the colon.
        if let Some(rest) = t.strip_prefix(key).filter(|r| r.starts_with(':')) {
            return rest[1..].trim_start().starts_with(['{', '[']);
        }
    }
    false
}

/// Read the configured id property as a stable PLAIN-SCALAR id (on-disk casing
/// insensitive), decoded through `strict_scalar_field` — the SAME strict
/// decoder a `parent-id` reference uses (`tasks/parent.rs`), NOT the lenient
/// `scalar_field` that `frontmatter_scalar_ci` pairs with the structured
/// fields (due/priority/created/status/title). This is Defect A's fix: the
/// two used to disagree — `scalar_field`'s outer-quote strip is shallow, so a
/// task's own id and an identical on-disk `parent-id` string could decode to
/// two different values and never compare equal, silently breaking
/// `parent_index`'s edge resolution (see `strict_scalar_field`'s doc
/// comment). Returns None when the property is absent, blank, or holds a
/// NON-SCALAR value — a block (`key:` then indented lines) or flow (`key: {..}`
/// / `[..]`) collection is the user's structure, not an id, and must never
/// surface AS an id. Without this, a duplicate that PRESERVED a flow-valued
/// property (the never-clobber posture) would read as sharing the source's
/// stable id — two tasks with one id (Codex P2, PR #76). The scalar READ
/// agrees with the write guards (`key_opens_block`/`key_opens_flow`):
/// non-scalar = non-id on both sides.
pub(in crate::tasks) fn scalar_id_ci(content: &str, key: &str) -> Option<String> {
    let on_disk = top_level_key_ci(content, key)?;
    if key_opens_block(content, on_disk) || key_opens_flow(content, on_disk) {
        return None;
    }
    strict_scalar_field(content, on_disk, false)
}

/// Whether `key`'s current value in `content` blocks `ensure_id`
/// (`tasks::disk::update_task_fields`) from stamping OR reading it as an id:
/// true for a BLOCK/FLOW collection (the user's own frontmatter — consuming
/// it on a stamp would delete their data) or a present, non-blank scalar the
/// strict decoder cannot resolve (a comment-only value, an unterminated
/// quote, a folding continuation). Absent or a truly BLANK scalar is never
/// unassignable — that is exactly what `ensure_id` generates a fresh id
/// into, so it reads `false` here.
///
/// The block/flow arm is checked UNCONDITIONALLY, before looking at whether
/// the lenient value `v` is empty: a block value's own key line (`task-id:`
/// then indented children) has nothing after the colon, so `v` reads empty
/// exactly like a truly-blank scalar does. Checking `key_opens_block` first
/// is what tells the two apart; swapping the order (or short-circuiting on
/// `v.is_empty()` first) would fold the block case back into "blank" — the
/// bug this function exists to close (design spec
/// docs/superpowers/specs/2026-07-25-task-subtasks-and-parent-tasks-design.md
/// §2).
///
/// SINGLE-SOURCED on purpose. `services::tasks::parent`'s phase-1 pre-lock
/// forecast must predict a doomed stamp before Task IDs are switched on for
/// the vault (enabling them is itself a persisted write phase 1 must not
/// let happen ahead of a refusal only phase 3a would otherwise catch). An
/// earlier version of that forecast re-implemented only a single-line raw
/// scan, which cannot see an IMPLICIT block value with no `|`/`>`/`{`/`[`
/// marker on the key's own line: it read that blank first line as
/// assignable, let phase 2 enable Task IDs, and only then hit THIS
/// function's own block detection inside `ensure_id` — after the vault's
/// setting was already flipped. Two independent notions of "assignable" is
/// precisely that failure mode; both callers now consult this one.
pub(crate) fn id_property_unassignable(content: &str, key: &str) -> bool {
    match frontmatter_scalar_ci(content, key) {
        Some((on_disk, _))
            if key_opens_block(content, &on_disk) || key_opens_flow(content, &on_disk) =>
        {
            true
        }
        Some((on_disk, v)) if !v.is_empty() => {
            strict_scalar_field(content, &on_disk, false).is_none()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_id_ci_matches_regardless_of_casing() {
        // A task stamped `Task-ID:` must be readable by a config using the
        // lowercase `task-id` property name, and vice versa — Obsidian folds
        // frontmatter key case, so a case-sensitive read would miss a stable
        // on-disk id (and the stamp would write a second, conflicting line).
        let upper = "---\ntype: Task\nTask-ID: abc123\n---\n";
        assert_eq!(scalar_id_ci(upper, "task-id").as_deref(), Some("abc123"));
        assert_eq!(scalar_id_ci(upper, "TASK-ID").as_deref(), Some("abc123"));
        let lower = "---\ntype: Task\ntask-id: abc123\n---\n";
        assert_eq!(scalar_id_ci(lower, "Task-ID").as_deref(), Some("abc123"));
    }

    #[test]
    fn scalar_id_ci_none_for_absent_key_and_body_only_occurrence() {
        assert_eq!(scalar_id_ci("---\ntype: Task\n---\n", "task-id"), None);
        // A same-named line AFTER the closing fence is body content, not
        // frontmatter — it must never be read as the property.
        assert_eq!(
            scalar_id_ci("---\ntype: Task\n---\ntask-id: sneaky\n", "task-id"),
            None
        );
        assert_eq!(scalar_id_ci("no frontmatter", "task-id"), None);
        // Unterminated frontmatter (opens but the closing fence never comes)
        // falls through to None.
        assert_eq!(scalar_id_ci("---\ntype: Task\n", "due"), None);
    }

    #[test]
    fn scalar_id_ci_treats_blank_and_non_scalar_as_absent_and_skips_nested_keys() {
        // A bare `task-id:` (an Obsidian property panel / template leaves the
        // key valueless) reads as ABSENT — the id-stamp treats it as MISSING and
        // writes a usable id (Codex, PR #59), and the display read agrees so ""
        // is never surfaced as an id.
        assert_eq!(
            scalar_id_ci("---\ntype: Task\ntask-id:\n---\n", "task-id"),
            None
        );
        // A NON-SCALAR value — a block map/list, or an inline flow map/seq — is
        // the user's structure, never an id (Codex P2, PR #76).
        assert_eq!(
            scalar_id_ci(
                "---\ntype: Task\ntask-id:\n  source: jira\n---\n",
                "task-id"
            ),
            None
        );
        assert_eq!(
            scalar_id_ci("---\ntype: Task\ntask-id: {source: jira}\n---\n", "task-id"),
            None
        );
        assert_eq!(
            scalar_id_ci("---\ntype: Task\ntask-id: [a, b]\n---\n", "task-id"),
            None
        );
        // An indented `task-id:` nested under a mapping is NOT the top-level
        // property set_fields rewrites — the top-level scan skips it (space and
        // tab indentation alike).
        assert_eq!(
            scalar_id_ci(
                "---\ntype: Task\nmetadata:\n  task-id: old\n---\n",
                "task-id"
            ),
            None
        );
        assert_eq!(
            scalar_id_ci("---\ntype: Task\nmeta:\n\ttask-id: old\n---\n", "task-id"),
            None
        );
        // A colonless malformed line neither matches nor panics; a genuine
        // top-level key later in the block still reads.
        assert_eq!(
            scalar_id_ci(
                "---\ntype: Task\nnotacolonhere\ntask-id: abc\n---\n",
                "task-id"
            )
            .as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn scalar_id_ci_decodes_a_doubled_single_quote_escape_fully() {
        // Defect A: scalar_field's outer-quote strip is SHALLOW
        // (`&stripped[1..len-1]`), leaving an embedded `''` doubled-quote
        // escape intact — `'a''b'` decoded to `a''b` instead of the correct
        // YAML `a'b`. A `parent-id` reference (tasks/parent.rs::scalar) does
        // the full decode, so the two sides of an id comparison disagreed on
        // identical on-disk text and `parent_index` could never resolve the
        // edge. `scalar_id_ci` must decode identically to the parent-id
        // reader.
        assert_eq!(
            scalar_id_ci("---\ntype: Task\ntask-id: 'a''b'\n---\n", "task-id").as_deref(),
            Some("a'b")
        );
    }

    #[test]
    fn key_opens_block_mirrors_the_writer_consumption_rules() {
        // The stamp's non-scalar guard (review, PR #59): true exactly when
        // set_fields' rewrite of the empty-valued key would consume following
        // lines — a nested mapping, a block list (indented or not), stray
        // indented whitespace — so stamping over the user's data is refused.
        let map = "---\ntype: Task\nuid:\n  source: jira\n---\n";
        assert!(key_opens_block(map, "uid"));
        let list = "---\ntype: Task\nuid:\n- a\n- b\n---\n";
        assert!(key_opens_block(list, "uid"));
        let indented_list = "---\ntype: Task\nuid:\n  - a\n---\n";
        assert!(key_opens_block(indented_list, "uid"));
        // A comment DEFERS the decision, like the writer's pending buffer.
        let comment_then_item = "---\ntype: Task\nuid:\n# note\n- a\n---\n";
        assert!(key_opens_block(comment_then_item, "uid"));
        let comment_then_fence = "---\ntype: Task\nuid:\n# note\n---\n";
        assert!(!key_opens_block(comment_then_fence, "uid"));
        // Truly blank: next line is another key, a blank line, or the fence.
        assert!(!key_opens_block("---\ntype: Task\nuid:\n---\n", "uid"));
        assert!(!key_opens_block(
            "---\ntype: Task\nuid:\nstatus: new\n---\n",
            "uid"
        ));
        assert!(!key_opens_block(
            "---\ntype: Task\nuid:\n\n- body\n---\n",
            "uid"
        ));
        // Absent key / no frontmatter → false (nothing to guard).
        assert!(!key_opens_block("---\ntype: Task\n---\n", "uid"));
        assert!(!key_opens_block("no frontmatter", "uid"));
    }
}
