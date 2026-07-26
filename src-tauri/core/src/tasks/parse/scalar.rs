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

/// The RAW (un-decoded, comment still attached, quotes/tags/anchors intact)
/// text after the colon on the first TOP-LEVEL `key:` line, matched
/// CASE-INSENSITIVELY. Composes `top_level_key_ci` (for the case-insensitive
/// search) with `capture_note::raw_scalar_field` (for the value, keyed on the
/// now-known EXACT on-disk casing — its own `strip_prefix` match is already
/// top-level-only, so this adds no additional scan). Used by
/// `tasks::id::mirror_id_reference` to decide how to re-encode an inherited
/// id for a child's `parent-id`: the DECODED value alone cannot tell an
/// implicitly-typed `123` from an explicitly-quoted `"123"`, but a decoder
/// resolves the two to different YAML types, so the decision needs the raw
/// source text, not just its decoded string.
pub(in crate::tasks) fn top_level_raw_value_ci<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    let on_disk = top_level_key_ci(content, key)?;
    crate::capture_note::raw_scalar_field(content, on_disk)
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
///
/// A leading YAML anchor (`&name`) is stripped before any of the above —
/// see the anchor-handling block at the top of the body for the regression
/// this closes (the write side, `tasks::id::mirror_id_reference`, already
/// stripped it; this decoder did not, so a task's own anchored id could
/// never equal what the write side mirrors into a child's `parent-id`).
pub(crate) fn strict_scalar_field(content: &str, key: &str, link: bool) -> Option<String> {
    let raw = crate::capture_note::raw_scalar_field(content, key)?.trim();
    if raw.is_empty() {
        return None;
    }
    // An anchor decorates whatever node follows it — it is not a distinct
    // type of its own — so it is peeled off BEFORE any of the block/flow/
    // alias/quote/plain classification below, mirroring the write side's
    // own anchor-first order (`tasks::id::mirror_or_fall_back`). Before this,
    // a task's own id (read here, via `scalar_id_ci`) or a `parent-id`
    // reference carrying `&name` on disk decoded to the LITERAL text
    // "&name value" — so the parent's own id, as `list_tasks` reports it,
    // could never equal what the write side mirrors into a child's
    // `parent-id`: the hierarchy the app itself had just written was
    // orphaned the instant it landed. `tasks::id::strip_anchor` is the write
    // side's own helper, reused rather than re-implemented here — two
    // independently-maintained notions of "where does the anchor name end"
    // is exactly the class of bug that let the two sides drift apart the
    // first time.
    //
    // Behavior change, stated plainly: a task's own id (via `scalar_id_ci`/
    // `list_tasks`) now reports an anchor-decorated value with the
    // annotation stripped (`abc`, not `&stable abc`) — MORE correct (it
    // matches what js-yaml/Dataview see, and makes the copy-id affordance
    // produce a value that actually works), but a visible change from the
    // literal text this decoder used to surface.
    // `?` propagates a malformed anchor (no anchor name, or nothing
    // separates it from whatever follows) straight to `None`: this reader
    // can't tell where the name ends, so — like every other undecodable form
    // below (an unterminated quote, a folding continuation) — it declines
    // rather than risk a wrong value. A phantom id/reference is worse than
    // none.
    let raw = if raw.starts_with('&') {
        super::super::id::strip_anchor(raw)?
    } else {
        raw
    };
    // The anchor may have decorated an otherwise-blank value (`&stable`
    // alone, or `&stable ` with nothing after the required separator) —
    // that is a truly blank scalar underneath, same as an empty `key:` line.
    if raw.is_empty() {
        return None;
    }
    // A block (`|`/`>`) or flow (`{..}`) value is the user's own structure,
    // not our scalar. `[[wikilink]]` is exempt ONLY when `link` is set.
    let wikilink_exempt = link && raw.starts_with("[[");
    if !wikilink_exempt && (raw.starts_with(['|', '>', '{']) || raw.starts_with('[')) {
        return None;
    }
    // A leading `*` is a YAML ALIAS — its value lives wherever the matching
    // `&name` anchor was defined elsewhere in the SAME document, which this
    // per-key line scan has no way to resolve. Decoding it as the literal
    // text "*name" would be worse than useless for a reference: mirroring
    // that text into another document (a child's `parent-id`) either
    // dangles (no such anchor there) or silently binds to a DIFFERENT node
    // if that document happens to define its own anchor under the same
    // name. Treated like a block/flow value — not a usable scalar here.
    if raw.starts_with('*') {
        return None;
    }
    // A leading `#` is a YAML comment — the property is null.
    if raw.starts_with('#') {
        return None;
    }
    let decoded = if raw.starts_with('"') {
        // An unterminated quoted scalar is multi-line; reject rather than
        // surfacing its first line.
        let span = super::super::description::double_quoted_slice(raw)?;
        // double_quoted_slice only locates the closing quote — it says
        // nothing about what follows. `"abc"junk` is not one valid YAML
        // scalar, so decoding just the quoted prefix would surface a USABLE
        // id for what is actually garbage, exactly the "worse than useless"
        // outcome the multi-line-scalar rejection above already guards
        // against for a parent reference.
        if !trailing_is_blank_or_comment(&raw[span.len()..]) {
            return None;
        }
        crate::yaml_scalar::yaml_unquote_multiline(span)
    } else if raw.starts_with('\'') {
        // Same reasoning, single-quoted: decode_single_quoted also stops at
        // the first unescaped closing quote and is silent about the rest.
        let span = super::super::description::single_quoted_slice(raw)?;
        if !trailing_is_blank_or_comment(&raw[span.len()..]) {
            return None;
        }
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

/// True when `remainder` — everything on the raw value's line AFTER a quoted
/// scalar's closing quote — is trailing content real YAML actually allows:
/// nothing at all, or a comment separated from the quote by at least one
/// whitespace character. A `#` glued directly onto the quote (no separating
/// whitespace) is NOT a valid YAML comment start, so it is rejected exactly
/// like any other stray text — `double_quoted_slice`/`single_quoted_slice`
/// only find where the scalar's own closing quote is; they say nothing about
/// what follows it, which is precisely why a caller must ask this too before
/// trusting the decoded value.
fn trailing_is_blank_or_comment(remainder: &str) -> bool {
    match remainder.chars().next() {
        None => true,
        Some(c) if c.is_whitespace() => {
            let rest = remainder.trim_start();
            rest.is_empty() || rest.starts_with('#')
        }
        Some(_) => false,
    }
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
mod tests;
