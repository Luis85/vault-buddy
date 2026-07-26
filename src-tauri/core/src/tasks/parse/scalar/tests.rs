//! `scalar.rs`'s tests, split out for the Rust LOC cap — same module
//! position (`tasks::parse::scalar::tests`), so every `super` path below is
//! unchanged and the tests still sit beside the code they pin (the
//! `tasks::disk`/`tasks::disk::tests` precedent this mirrors).

use super::*;

#[test]
fn strict_scalar_field_strips_a_leading_anchor_before_classifying() {
    // Regression (task report): mirror_id_reference (the WRITE side,
    // tasks::id) already strips a leading YAML anchor so a copied id
    // resolves to the same value js-yaml would — this decoder (the READ
    // side behind scalar_id_ci and parent_id_field/parent_link_field)
    // did not, so a task's own id — what list_tasks reports — could
    // never equal what the write side mirrors into a child's
    // `parent-id`. Anchor-stripping happens BEFORE quote/plain
    // classification (mirroring the write side's own order), so it must
    // work underneath every raw form, not just a bare plain token.
    for (raw, expected) in [
        ("&stable abc", Some("abc")),
        ("&stable 123", Some("123")), // still implicitly-typed underneath
        ("&stable \"abc\"", Some("abc")), // anchor + quoted
        ("&stable \"abc\" # note", Some("abc")), // + trailing comment
    ] {
        let doc = format!("---\ntype: Task\ntask-id: {raw}\n---\n");
        assert_eq!(
            strict_scalar_field(&doc, "task-id", false).as_deref(),
            expected,
            "{raw:?}"
        );
    }
    // A malformed anchor (no separating whitespace between the name and
    // whatever follows) — this reader can't tell where the name ends, so
    // it declines rather than risking a wrong value, exactly like every
    // other undecodable form below (an unterminated quote, a folding
    // continuation): a phantom id/reference is worse than none.
    assert_eq!(
        strict_scalar_field(
            "---\ntype: Task\ntask-id: &stableabc\n---\n",
            "task-id",
            false
        ),
        None
    );
    // An anchored NON-scalar (flow map) is still correctly rejected —
    // stripping the anchor FIRST is what lets the existing flow guard
    // see it at all; without that ordering, the whole "&stable {a: b}"
    // text was read as one opaque plain-scalar string (Some(garbage)),
    // not recognized as non-scalar.
    assert_eq!(
        strict_scalar_field(
            "---\ntype: Task\ntask-id: &stable {a: b}\n---\n",
            "task-id",
            false
        ),
        None
    );
}

#[test]
fn scalar_id_ci_strips_a_leading_anchor() {
    // The exact regression named in the task report: list_tasks (via
    // scalar_id_ci) reported a parent's own id as the literal
    // "&stable abc" instead of "abc" — the anchor decoration a
    // Dataview cross-reference elsewhere in the vault might hang off a
    // task's id. See the parity test below for why this specific string
    // must equal what the write side produces.
    assert_eq!(
        scalar_id_ci("---\ntype: Task\ntask-id: &stable abc\n---\n", "task-id").as_deref(),
        Some("abc")
    );
}

#[test]
fn scalar_id_ci_and_mirror_id_reference_agree_on_an_anchored_id() {
    // THE regression, asserted as parity rather than two isolated
    // "reasonable on its own" checks — a test checking only one side is
    // what let this split happen the first time. mirror_id_reference
    // (write side, fixed on this branch before this task) already
    // strips a leading anchor so a copied id resolves the same way
    // js-yaml would; scalar_id_ci (read side) must decode the identical
    // raw source to the identical value, or the parent's own displayed
    // id and what the write side mirrors into a child's `parent-id` can
    // never match — the hierarchy the app itself just wrote is orphaned
    // the instant it lands.
    let doc = "---\ntype: Task\ntask-id: &stable abc\n---\n";
    let reader = scalar_id_ci(doc, "task-id");
    // A deliberately WRONG fallback proves the agreement below comes
    // from the shared raw-text handling, not from some test-only wiring
    // that threads the reader's own value through as the writer's
    // decoded fallback.
    let writer = crate::tasks::mirror_id_reference(doc, "task-id", "WRONG-FALLBACK");
    assert_eq!(
        reader.as_deref(),
        Some(writer.as_str()),
        "reader and writer must resolve an anchored id identically"
    );
}

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
fn strict_scalar_field_rejects_an_alias_value() {
    // `task-id: *stable` is a YAML ALIAS: its value lives wherever the
    // matching `&stable` anchor was defined elsewhere in the document, so
    // a single-key line scan cannot resolve it — decoding it as the
    // literal text "*stable" would be worse than useless for a
    // REFERENCE (mirroring that text into a child's own frontmatter would
    // either dangle, or silently bind to a DIFFERENT node if the child
    // happens to define its own anchor under the same name). Treated like
    // a block/flow value: not a usable scalar (review, PR #77).
    assert_eq!(
        strict_scalar_field("---\ntype: Task\ntask-id: *stable\n---\n", "task-id", false),
        None
    );
    // The `link` exemption is for `[[wikilink]]` only — an alias must
    // still be rejected on the `parent` key too.
    assert_eq!(
        strict_scalar_field("---\ntype: Task\nparent: *stable\n---\n", "parent", true),
        None
    );
}

#[test]
fn strict_scalar_field_rejects_trailing_content_after_a_double_quoted_scalar() {
    // double_quoted_slice only locates the closing quote — it says
    // nothing about what follows. Without a trailing check, `"abc"junk`
    // decoded to the USABLE id `abc`: not a valid YAML scalar, but a
    // child could still resolve to this phantom parent, and
    // set_task_parent could persist a reference Obsidian/Dataview will
    // never agree exists.
    let doc = "---\ntype: Task\ntask-id: \"abc\"junk\n---\n";
    assert_eq!(strict_scalar_field(doc, "task-id", false), None);
}

#[test]
fn strict_scalar_field_rejects_trailing_content_after_a_single_quoted_scalar() {
    // Same flaw, the single-quoted branch: decode_single_quoted also
    // stops at the first unescaped closing quote and is silent about
    // whatever comes after it.
    let doc = "---\ntype: Task\ntask-id: 'abc'junk\n---\n";
    assert_eq!(strict_scalar_field(doc, "task-id", false), None);
}

#[test]
fn strict_scalar_field_still_accepts_a_real_comment_after_a_quoted_scalar() {
    // The fix must reject stray trailing text without rejecting the
    // ordinary, YAML-legal case: a comment separated from the quote by
    // whitespace.
    assert_eq!(
        strict_scalar_field(
            "---\ntype: Task\ntask-id: \"abc\" # note\n---\n",
            "task-id",
            false
        )
        .as_deref(),
        Some("abc")
    );
    assert_eq!(
        strict_scalar_field(
            "---\ntype: Task\ntask-id: 'abc' # note\n---\n",
            "task-id",
            false
        )
        .as_deref(),
        Some("abc")
    );
}

#[test]
fn strict_scalar_field_rejects_a_hash_glued_directly_to_the_closing_quote() {
    // YAML requires a comment's `#` to be separated from the previous
    // token by whitespace; a `#` glued straight onto the closing quote is
    // stray trailing text, not a comment, so it must reject exactly like
    // any other junk rather than being treated as an (empty) comment.
    assert_eq!(
        strict_scalar_field(
            "---\ntype: Task\ntask-id: \"abc\"#note\n---\n",
            "task-id",
            false
        ),
        None
    );
}

#[test]
fn strict_scalar_field_rejects_a_space_then_non_comment_text_after_the_close() {
    // A separating space alone does not make trailing text a comment —
    // only a space THEN a `#` does. `"abc" bogus` is still stray text,
    // not `"abc"` followed by a legal comment, on both quoted forms.
    assert_eq!(
        strict_scalar_field(
            "---\ntype: Task\ntask-id: \"abc\" bogus\n---\n",
            "task-id",
            false
        ),
        None
    );
    assert_eq!(
        strict_scalar_field(
            "---\ntype: Task\ntask-id: 'abc' bogus\n---\n",
            "task-id",
            false
        ),
        None
    );
}

#[test]
fn strict_scalar_field_rejects_trailing_content_after_a_quoted_wikilink_too() {
    // The `link` parameter only widens which UNQUOTED forms are exempt
    // from the block/flow guard (a bare `[[wikilink]]`) — it must not
    // loosen the quoted branches' own trailing-content rule, or a quoted
    // `parent` value could silently drop the same junk a `parent-id`/
    // `task-id` scalar no longer can.
    let doc = "---\ntype: Task\nparent: \"[[Tasks/p]]\"junk\n---\n";
    assert_eq!(strict_scalar_field(doc, "parent", true), None);
    // The unmangled quoted wikilink must still read fine.
    let ok = "---\ntype: Task\nparent: \"[[Tasks/p]]\"\n---\n";
    assert_eq!(
        strict_scalar_field(ok, "parent", true).as_deref(),
        Some("[[Tasks/p]]")
    );
}

#[test]
fn id_property_unassignable_refuses_an_alias_valued_property() {
    // Cascades from strict_scalar_field's alias rejection above: an
    // ALIAS-valued id property must be treated like a block/flow value —
    // `ensure_id` (disk.rs) must never overwrite it (it might be a real,
    // if unresolvable-from-here, reference) and must never report it as
    // a usable id, and `services::tasks::parent`'s phase-1 forecast must
    // refuse a parent assignment through it rather than mirroring the
    // literal "*stable" text into a child (review, PR #77).
    assert!(id_property_unassignable(
        "---\ntype: Task\ntask-id: *stable\n---\n",
        "task-id"
    ));
}

#[test]
fn top_level_raw_value_ci_returns_the_undecoded_text_case_insensitively() {
    // Comment, quotes, a tag, and an anchor all survive intact — this
    // is deliberately the layer BELOW strict_scalar_field's decode.
    assert_eq!(
        top_level_raw_value_ci("---\ntype: Task\nTask-ID: 123 # note\n---\n", "task-id"),
        Some("123 # note")
    );
    assert_eq!(
        top_level_raw_value_ci("---\ntype: Task\ntask-id: \"abc\"\n---\n", "TASK-ID"),
        Some("\"abc\"")
    );
    assert_eq!(
        top_level_raw_value_ci("---\ntype: Task\ntask-id: !!str 123\n---\n", "task-id"),
        Some("!!str 123")
    );
    assert_eq!(
        top_level_raw_value_ci("---\ntype: Task\ntask-id: &stable abc\n---\n", "task-id"),
        Some("&stable abc")
    );
}

#[test]
fn top_level_raw_value_ci_is_none_when_absent_or_nested() {
    assert_eq!(
        top_level_raw_value_ci("---\ntype: Task\n---\n", "task-id"),
        None
    );
    // An indented key is never the top-level property.
    assert_eq!(
        top_level_raw_value_ci("---\ntype: Task\nmeta:\n  task-id: nope\n---\n", "task-id"),
        None
    );
    // Body-only occurrence (after the closing fence) doesn't count.
    assert_eq!(
        top_level_raw_value_ci("---\ntype: Task\n---\ntask-id: sneaky\n", "task-id"),
        None
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
