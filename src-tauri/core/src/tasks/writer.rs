//! The surgical frontmatter writer: `set_fields` and its `set_status`
//! convenience. Reader/writer agreement tests live here — they pin what
//! `set_fields` writes against what the parse side reads.

use super::doc::is_task;
use super::parse::strip_inline_comment;

/// Return `content` with the named frontmatter lines updated, preserving every
/// other line and its exact ending. For each `(key, value)`: `Some(v)` rewrites
/// the existing `key:` line in place (first occurrence) or inserts `key: v` at
/// the closing fence; `None` removes the line (a missing line is a no-op).
/// Values are written VERBATIM — the caller quotes user text (`yaml_quote`).
/// `None` result iff the file is not `type: Task` or its frontmatter never
/// closes (no safe anchor; the caller skips + warns) — same contract as the
/// old single-key set_status.
pub fn set_fields(content: &str, updates: &[(&str, Option<&str>)]) -> Option<String> {
    if !is_task(content) {
        return None;
    }
    // Inserted lines need their own terminator so they can't glue onto a
    // fence that lacks a trailing newline. Match the document's convention.
    let nl = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = String::with_capacity(content.len() + 32 * updates.len());
    let mut handled = vec![false; updates.len()];
    let mut in_frontmatter = false;
    let mut seen_open = false;
    let mut closed = false;
    // True while consuming the `- item` lines of a block-style value whose
    // key was just rewritten/removed — the items belong to the replaced
    // value, so they are dropped with it. Also consumes indented continuation
    // lines (nested-mapping items), since YAML block items can span multiple
    // lines when they are mappings. Cleared by the first non-item,
    // non-indented line (including the closing fence), so body bullets are
    // never at risk: the fence always clears the flag before the body starts,
    // and top-level frontmatter keys and the fence are never indented. A
    // blank line inside a block list ends consumption early — read and write
    // agree on that boundary (`parse_tags_key`'s block-item loop stops there
    // too) — so blank-separated block entries are a known, degenerate-input
    // limitation.
    let mut skip_list_items = false;
    // Comment-only lines seen while consuming a block list. INTERIOR comments
    // (more items follow) belong to the replaced value and are dropped with
    // it — the reader scans past them the same way — but a comment TRAILING
    // the last item is the user's frontmatter (Codex, PR #46: a tags edit was
    // deleting it), so the decision is deferred until the next non-comment
    // line shows whether the block continues.
    let mut pending_comments: Vec<&str> = Vec::new();
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if skip_list_items {
            let starts_indented = line.starts_with([' ', '\t']);
            let t = trimmed.trim_start();
            if t.starts_with('#') {
                pending_comments.push(line);
                continue;
            }
            if starts_indented || t.starts_with("- ") {
                // Item or its indented continuation — any buffered comments
                // were interior after all; they go with the value.
                pending_comments.clear();
                continue;
            }
            // Block over. Buffered comments trailed the list — keep them.
            for c in pending_comments.drain(..) {
                out.push_str(c);
            }
            skip_list_items = false;
        }
        if !seen_open {
            // First line is the opening `---` (guaranteed by is_task).
            seen_open = true;
            in_frontmatter = true;
            out.push_str(line);
            continue;
        }
        // trim_end() so a closing fence with trailing whitespace is accepted,
        // matching is_task/note_field — the list and the writer must agree.
        if in_frontmatter && trimmed.trim_end() == "---" {
            // Closing fence: insert every not-yet-handled Set here; a pending
            // removal of a line that never existed is simply done.
            for (i, (key, value)) in updates.iter().enumerate() {
                if !handled[i] {
                    if let Some(v) = value {
                        out.push_str(&format!("{key}: {v}{nl}"));
                    }
                    handled[i] = true;
                }
            }
            in_frontmatter = false;
            closed = true;
            out.push_str(line);
            continue;
        }
        if in_frontmatter {
            // Key match requires the colon right after the key so `due` can't
            // rewrite `duedate:`. Only the first occurrence of a key is edited.
            let matched = updates.iter().enumerate().find(|(i, (key, _))| {
                !handled[*i]
                    && trimmed
                        .strip_prefix(*key)
                        .is_some_and(|rest| rest.starts_with(':'))
            });
            if let Some((i, (key, value))) = matched {
                // `key:` with nothing after the colon means the value is a
                // block-style list on the following lines — consume it along
                // with the key line (rewrite and removal alike), so a
                // hand-authored block list round-trips to one flow line
                // instead of leaving orphaned `- item` lines. "Nothing" uses
                // the READER's rule (strip an inline YAML comment first, same
                // helper): `tags: # areas` + items parses as a block list, so
                // the writer must consume it too or an edit would orphan the
                // items and a clear would leave stale tags (Codex, PR #46).
                let rest = &trimmed[key.len() + 1..];
                if strip_inline_comment(rest.trim()).trim().is_empty() {
                    skip_list_items = true;
                }
                if let Some(v) = value {
                    let ending = &line[trimmed.len()..]; // "\r\n", "\n", or ""
                    out.push_str(&format!("{key}: {v}{ending}"));
                }
                // drop the line (its newline goes with it) if value is None
                handled[i] = true;
                continue;
            }
        }
        out.push_str(line);
    }
    closed.then_some(out)
}

/// Single-key convenience over `set_fields` — kept because the status toggle
/// is the hot path and its list/toggle-agreement tests pin the contract.
pub fn set_status(content: &str, new_status: &str) -> Option<String> {
    set_fields(content, &[("status", Some(new_status))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_status_flips_only_the_status_line_preserving_body() {
        let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-08\n---\n\nSome body\n- [ ] sub\n";
        let flipped = set_status(doc, "done").unwrap();
        assert!(flipped.contains("status: done\n"));
        assert!(!flipped.contains("status: new\n"));
        // Everything else byte-for-byte intact.
        assert!(flipped.contains("title: \"A\"\n"));
        assert!(flipped.contains("created: 2026-07-08\n"));
        assert!(flipped.contains("\nSome body\n- [ ] sub\n"));
    }

    #[test]
    fn set_status_preserves_crlf_endings() {
        let doc = "---\r\ntype: Task\r\nstatus: new\r\ntitle: \"A\"\r\n---\r\n\r\nbody\r\n";
        let flipped = set_status(doc, "done").unwrap();
        assert!(flipped.contains("status: done\r\n"));
        assert!(flipped.contains("body\r\n"));
    }

    #[test]
    fn set_status_refuses_non_task() {
        // Not our document — never rewrite it.
        assert!(set_status("---\ntype: Meeting\nstatus: new\n---\n", "done").is_none());
        assert!(set_status("no frontmatter", "done").is_none());
    }

    #[test]
    fn set_status_inserts_line_when_missing() {
        // A hand-authored type: Task with no status line is surfaced in the list
        // as an unchecked row, so it MUST become toggleable — insert the status.
        let doc = "---\ntype: Task\ntitle: \"x\"\n---\n\nbody\n";
        let out = set_status(doc, "done").unwrap();
        assert!(out.contains("status: done\n"));
        assert!(out.contains("title: \"x\"\n"));
        assert!(out.contains("\nbody\n"));
    }

    #[test]
    fn set_status_inserts_line_when_missing_no_trailing_newline() {
        // Regression: a hand-authored task with no status line AND no trailing
        // newline after the closing fence must not glue status onto the fence.
        let doc = "---\ntype: Task\ntitle: \"x\"\n---";
        let out = set_status(doc, "done").unwrap();
        assert_eq!(out, "---\ntype: Task\ntitle: \"x\"\nstatus: done\n---");
    }

    #[test]
    fn set_status_insert_preserves_crlf_when_missing() {
        let doc = "---\r\ntype: Task\r\ntitle: \"x\"\r\n---\r\n";
        let out = set_status(doc, "done").unwrap();
        assert!(out.contains("status: done\r\n"));
        assert!(out.contains("---\r\n"));
    }

    #[test]
    fn set_status_none_for_unterminated_frontmatter() {
        // Opening fence + type: Task but no closing --- : malformed; refuse
        // rather than guess where to insert (documented narrow contract).
        let doc = "---\ntype: Task\ntitle: \"x\"\n";
        assert!(set_status(doc, "done").is_none());
    }

    #[test]
    fn set_status_inserts_when_closing_fence_has_trailing_whitespace() {
        // Regression: is_task/note_field accept a closing fence with trailing
        // whitespace, so set_status must too — otherwise a listed status-less
        // task would be un-toggleable (set_status returns None, toggle errors).
        let doc = "---\ntype: Task\ntitle: \"x\"\n---  \n";
        assert!(is_task(doc)); // the list surfaces it…
        let out = set_status(doc, "done").unwrap(); // …so the toggle must accept it
        assert!(out.contains("status: done\n"));
        assert!(out.contains("---  \n")); // fence preserved verbatim
    }

    #[test]
    fn set_fields_updates_multiple_keys_in_one_pass() {
        let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-08\ndue: 2026-07-10\n---\n\nbody\n";
        let out = set_fields(
            doc,
            &[
                ("title", Some("\"B\"")),
                ("due", Some("2026-07-20")),
                ("priority", Some("high")),
            ],
        )
        .unwrap();
        assert!(out.contains("title: \"B\"\n"));
        assert!(out.contains("due: 2026-07-20\n"));
        assert!(out.contains("priority: high\n")); // inserted at the fence
        assert!(out.contains("status: new\n")); // untouched key preserved
        assert!(out.contains("created: 2026-07-08\n"));
        assert!(out.contains("\nbody\n")); // body byte-for-byte
    }

    #[test]
    fn set_fields_removes_a_line_with_none() {
        let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\ndue: 2026-07-10\npriority: low\n---\n\nbody\n";
        let out = set_fields(doc, &[("due", None), ("priority", None)]).unwrap();
        assert!(!out.contains("due:"));
        assert!(!out.contains("priority:"));
        assert!(out.contains("title: \"A\"\n"));
        assert!(out.contains("\nbody\n"));
    }

    #[test]
    fn set_fields_removing_a_missing_key_is_a_no_op() {
        let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n";
        assert_eq!(set_fields(doc, &[("due", None)]).unwrap(), doc);
    }

    #[test]
    fn set_fields_preserves_crlf_and_unknown_keys() {
        let doc = "---\r\ntype: Task\r\nstatus: new\r\ncustom: keep-me\r\n---\r\n\r\nbody\r\n";
        let out = set_fields(doc, &[("due", Some("2026-07-20"))]).unwrap();
        assert!(out.contains("due: 2026-07-20\r\n")); // inserted line matches CRLF
        assert!(out.contains("custom: keep-me\r\n"));
        assert!(out.contains("body\r\n"));
    }

    #[test]
    fn set_fields_refuses_non_task_and_unclosed_fence() {
        assert!(set_fields("---\ntype: Meeting\n---\n", &[("due", Some("x"))]).is_none());
        assert!(set_fields("---\ntype: Task\ntitle: \"x\"\n", &[("due", Some("x"))]).is_none());
    }

    #[test]
    fn set_fields_does_not_match_a_key_prefix() {
        // "due" must not rewrite a "duedate:" line — key match requires the colon
        // immediately after the key.
        let doc = "---\ntype: Task\nstatus: new\nduedate: keep\n---\n";
        let out = set_fields(doc, &[("due", Some("2026-07-20"))]).unwrap();
        assert!(out.contains("duedate: keep\n"));
        assert!(out.contains("due: 2026-07-20\n")); // inserted, not substituted
    }

    #[test]
    fn set_fields_rewrites_a_block_list_to_one_flow_line() {
        // A hand-authored block-style tags list must round-trip to the canonical
        // flow line — orphaned `- item` lines would corrupt the frontmatter.
        let doc =
            "---\ntype: Task\nstatus: new\ntags:\n  - work\n  - home\ntitle: \"A\"\n---\nbody\n";
        let out = set_fields(doc, &[("tags", Some("[urgent]"))]).unwrap();
        assert!(out.contains("tags: [urgent]\n"));
        assert!(!out.contains("- work"));
        assert!(!out.contains("- home"));
        assert!(out.contains("title: \"A\"\n")); // key after the block untouched
        assert!(out.contains("\nbody\n"));
    }

    #[test]
    fn set_fields_removes_a_block_list_entirely() {
        let doc = "---\ntype: Task\nstatus: new\ntags:\n- work\n- home\n---\n";
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert!(!out.contains("tags"));
        assert!(!out.contains("- work"));
        assert!(out.contains("status: new\n"));
    }

    #[test]
    fn set_fields_consumes_a_block_list_under_a_comment_only_key_line() {
        // Codex review, PR #46: `tags: # areas` + following `- item` lines is
        // read as a block list (the reader strips the comment before its
        // empty-value check), but the writer checked the raw rest — so an
        // edit rewrote only the key line and orphaned the items, and a clear
        // left stale tags behind. Reader and writer must share one rule.
        let doc = "---\ntype: Task\nstatus: new\ntags: # areas\n- work\n- home\n---\nbody\n";
        let out = set_fields(doc, &[("tags", Some("[crafts]"))]).unwrap();
        assert!(out.contains("tags: [crafts]\n"));
        assert!(!out.contains("- work"));
        assert!(!out.contains("- home"));
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert_eq!(out, "---\ntype: Task\nstatus: new\n---\nbody\n");
    }

    #[test]
    fn set_fields_keeps_items_after_a_key_with_a_real_value_and_comment() {
        // The mirror case: a real inline value plus a trailing comment is NOT
        // a block key — following `- item` lines belong to the body of the
        // next construct, not to this key, and must survive a rewrite.
        let doc = "---\ntype: Task\nstatus: new # wip\ntitle: \"A\"\n---\n- body bullet\n";
        let out = set_fields(doc, &[("status", Some("done"))]).unwrap();
        assert!(out.contains("status: done\n"));
        assert!(out.contains("title: \"A\"\n"));
        assert!(out.contains("- body bullet\n"));
    }

    #[test]
    fn set_fields_block_consumption_preserves_crlf() {
        let doc = "---\r\ntype: Task\r\nstatus: new\r\ntags:\r\n  - work\r\n---\r\n";
        let out = set_fields(doc, &[("tags", Some("[home]"))]).unwrap();
        assert!(out.contains("tags: [home]\r\n"));
        assert!(!out.contains("- work"));
        assert!(out.contains("status: new\r\n"));
    }

    #[test]
    fn set_fields_block_list_running_to_the_fence_keeps_the_fence() {
        let doc = "---\ntype: Task\nstatus: new\ntags:\n- work\n---\nbody\n";
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert_eq!(out, "---\ntype: Task\nstatus: new\n---\nbody\n");
    }

    #[test]
    fn set_fields_empty_value_key_without_items_consumes_nothing() {
        // A bare `tags:` with no list following: rewrite it in place, and the
        // next line (a real key) must not be swallowed.
        let doc = "---\ntype: Task\nstatus: new\ntags:\ntitle: \"A\"\n---\n";
        let out = set_fields(doc, &[("tags", Some("[x1]"))]).unwrap();
        assert!(out.contains("tags: [x1]\n"));
        assert!(out.contains("title: \"A\"\n"));
    }

    #[test]
    fn set_fields_body_bullets_are_never_consumed() {
        // Removing an inline-valued key must not touch `- ` bullet lines in the
        // body — consumption applies only to a block list directly under an
        // empty-valued matched key inside the frontmatter.
        let doc = "---\ntype: Task\nstatus: new\ndue: 2026-07-10\n---\n- a body bullet\n";
        let out = set_fields(doc, &[("due", None)]).unwrap();
        assert!(out.contains("- a body bullet\n"));
        assert!(!out.contains("due:"));
    }

    #[test]
    fn set_fields_consumes_nested_mapping_items_without_orphans() {
        // Regression: a block item that is a mapping has indented continuation
        // lines ("  role: owner") that don't start with "- " — the consumption
        // must take them too or the removal leaves orphaned lines that corrupt
        // the frontmatter structure.
        let doc = "---\ntype: Task\nstatus: new\ntags:\n- name: Alice\n  role: owner\n- name: Bob\ntitle: \"A\"\n---\n";
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert!(!out.contains("Alice"));
        assert!(!out.contains("role: owner"));
        assert!(!out.contains("Bob"));
        assert!(out.contains("title: \"A\"\n")); // next top-level key survives
        assert!(out.contains("status: new\n"));
    }

    #[test]
    fn set_fields_consumed_block_still_lets_a_later_key_be_rewritten() {
        // A key matched AFTER a consumed block must still be rewritable in the
        // same call (the flag must not swallow or skip it).
        let doc = "---\ntype: Task\nstatus: new\ntags:\n- work\ndue: 2026-07-10\n---\n";
        let out = set_fields(doc, &[("tags", None), ("due", Some("2026-08-01"))]).unwrap();
        assert!(!out.contains("- work"));
        assert!(!out.contains("tags"));
        assert!(out.contains("due: 2026-08-01\n"));
    }

    #[test]
    fn set_fields_retires_the_tag_alias_alongside_a_tags_write() {
        // The shell's update_task pushes ("tag", None) with every tags write: on
        // an alias-authored file, writing tags: without removing tag: would leave
        // dual keys (Obsidian reads the union), and clearing tags: alone would be
        // a silent no-op that un-shadows the stale alias on the next read.
        let doc = "---\ntype: Task\nstatus: new\ntag: work\ntitle: \"A\"\n---\n";
        let out = set_fields(doc, &[("tags", Some("[home]")), ("tag", None)]).unwrap();
        assert!(out.contains("tags: [home]\n"));
        assert!(!out.contains("tag: work"));
        // Clearing: both keys removed, nothing resurrected.
        let cleared = set_fields(doc, &[("tags", None), ("tag", None)]).unwrap();
        assert!(!cleared.contains("tag"));
        assert!(cleared.contains("title: \"A\"\n"));
    }

    #[test]
    fn set_fields_preserves_a_comment_trailing_a_block_list() {
        // Codex review, PR #46: a standalone comment AFTER the last item
        // (`tags:` / `- work` / `# keep this note` / `due: …`) is the user's
        // frontmatter, not part of the replaced value — the consumption loop
        // was deleting it. Interior comments (followed by more items) still
        // ride with the block; only trailing ones survive.
        let doc =
            "---\ntype: Task\nstatus: new\ntags:\n- work\n# keep this note\ndue: 2026-07-10\n---\n";
        let out = set_fields(doc, &[("tags", Some("[crafts]"))]).unwrap();
        assert!(out.contains("tags: [crafts]\n"));
        assert!(!out.contains("- work"));
        assert!(out.contains("# keep this note\n"));
        assert!(out.contains("due: 2026-07-10\n"));
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert_eq!(
            out,
            "---\ntype: Task\nstatus: new\n# keep this note\ndue: 2026-07-10\n---\n"
        );
    }

    #[test]
    fn set_fields_consumes_comment_lines_inside_a_block_list() {
        // Writer mirror of the reader rule above: rewriting/removing a block
        // key must consume the block's comment-only lines along with its
        // items, or they'd orphan into the frontmatter after the edit.
        let doc = "---\ntype: Task\nstatus: new\ntags:\n# areas\n- work\n  # midway\n- home\ntitle: \"A\"\n---\nbody\n";
        let out = set_fields(doc, &[("tags", Some("[crafts]"))]).unwrap();
        assert!(out.contains("tags: [crafts]\n"));
        assert!(!out.contains("- work"));
        assert!(!out.contains("# areas"));
        assert!(!out.contains("# midway"));
        assert!(out.contains("title: \"A\"\n"));
        let out = set_fields(doc, &[("tags", None)]).unwrap();
        assert_eq!(
            out,
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\nbody\n"
        );
    }
}
