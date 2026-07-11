# Task Tags Design — Obsidian-compatible tags + tag-aware task management

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** Follow-up on the tasks todo-list increment (PR #46). Adds
  Obsidian-compatible tags to task documents and makes the Tasks view use
  them: chips on rows, click-to-filter, tags on add/edit, and a group-by-tag
  view.

## Goals

1. **Tag chips on rows** — a task's tags are visible at a glance.
2. **Filter by tag** — clicking a chip narrows the list to that tag; combines
   with the title filter.
3. **Set tags on add + edit** — a tags input on the add-options row and in
   the inline editor, written via the same surgical frontmatter writer.
4. **Group-by-tag view** — a Dates | Tags grouping toggle; tag mode shows one
   section per tag (a task repeats under each of its tags), plus "No tags"
   and "Done".

## Obsidian compatibility (the contract)

Tags live in **frontmatter only** — the `tags:` property (or its `tag:`
alias), exactly what Obsidian Properties manages. Body `#hashtags` are out of
scope (the scanner stays frontmatter-only, and tag edits could not round-trip
inline tags anyway).

**Read — accept every form Obsidian accepts:**

```yaml
tags: [work, home/errands]        # flow style (canonical)
tags:                             # block style
  - work
  - "home/errands"
tags: work, home/errands          # legacy comma/space string
tag: work                         # singular alias
```

Normalization on read: strip an optional leading `#`, trim surrounding
quotes/whitespace, validate Obsidian's tag charset (letters, digits, `-`,
`_`, `/`; at least one non-digit character), silently drop invalid entries
(defensive read, never an error), dedupe case-insensitively keeping the
first-seen casing. Nested tags (`home/errands`) are kept verbatim as
distinct strings — no hierarchy UI this slice. If both `tags:` and `tag:`
exist, `tags:` wins (the alias is read only when `tags:` is absent).

**Write — always canonical flow style:** `tags: [work, home/errands]`, one
line, written only when the list is non-empty; clearing tags removes the
line (or block) entirely. Tags in our charset never need YAML quoting, and
every tag is validated before any write, so the flow line is always safe.

## Core (`core/src/tasks.rs` + the writer)

- **`note_tags(content) -> Vec<String>`** — the frontmatter tag parser
  described above (flow / block / legacy string / `tag:` alias +
  normalization). Lives beside `note_field`; used by `collect_tasks`.
- **`TaskItem` gains `tags: Vec<String>`** (empty when none).
- **`is_valid_tag(s: &str) -> bool`** — the charset rule (letters, digits,
  `-`, `_`, `/`, ≥1 non-digit). Shared by the read-side normalization and
  the shell's write validation so they cannot disagree.
- **`render_task` gains a `tags: &[String]` parameter** — emits the
  flow-style line after `priority`/`due` only when non-empty. `create_task`
  threads it through.
- **`set_fields` block-list consumption** — the one writer change: when a
  matched key's line carries an EMPTY value (nothing after the colon but
  whitespace) and is followed by YAML list-item lines (indented-or-not
  `- item`), a rewrite or removal of that key consumes those list lines too.
  A rewrite replaces key line + items with the single new flow line; a
  removal drops them all. Everything else stays byte-for-byte (CRLF,
  unknown keys, order, body). Keys whose line has an inline value behave
  exactly as today. This makes a hand-authored block-style `tags:` list
  round-trip to one canonical flow line instead of leaving orphaned
  `- item` lines.
- **Sort is unchanged** — tags do not affect ordering.

## Shell (`task_commands.rs`)

- **`TaskDto` gains `tags: Vec<String>`** (camelCase serde as-is).
- **`add_task` gains `tags: Option<Vec<String>>`** — each tag trimmed,
  leading `#` stripped, then `is_valid_tag`-checked; an invalid tag is an
  inline error naming the offending token (write validation is strict where
  read is lenient). Empty/absent → no line written.
- **`update_task`'s patch gains `tags: Option<Vec<String>>`** — `None`
  leaves tags untouched; `Some([])` clears (removes the line/block);
  `Some(nonempty)` validates then writes the flow line. Same single
  read-modify-write via `update_task_fields`. Every tags write (clear or set)
  also pushes `("tag", None)` — retiring the singular alias — so a clear
  can't silently no-op on an alias-authored file and a set can't leave dual
  `tag:`/`tags:` keys.
- No new commands; `list_tasks` carries tags out through the widened DTO.

## Frontend

### `types.ts`
`TaskItem` gains `tags: string[]`; `TaskPatch` gains `tags?: string[]`.

### Tasks view (`Tasks.vue`)

- **Chips.** Each row renders ALL of its tags as small chips between the
  title and the due chip (the title keeps layout priority via truncation;
  no chip-truncation logic this slice). Each chip is a click target that
  activates the tag filter for that tag (and does not trigger the row's
  open action).
- **Tag filter.** One active tag at a time, held in component state. While
  active it renders as a dismissible chip (with ✕) in the filter area,
  independent of the >5 title-filter threshold — it can only be activated
  by clicking an existing chip, and the ✕ is always visible while active,
  so it can never strand the user. It combines AND with the title filter
  (both feed `filteredTasks`); matching is case-insensitive and exact per
  tag (no prefix matching of nested tags this slice). The progress bar
  keeps counting the unfiltered list.
- **Tags on add + edit.** A free-text tags input (comma/space separated,
  leading `#` optional per token) on the add-options row and in the inline
  editor. The client splits and trims into an array; the shell validates.
  Editor semantics follow the changed-fields rule: the patch includes
  `tags` only when the parsed array differs from the task's current tags
  (order-sensitive compare is fine); emptying the input sends `tags: []`
  (clear). Optimistic apply + revert covers tags like the other fields.
- **Group-by-tag view.** A small `Dates | Tags` segmented toggle above the
  list (component-local state; dates is the default every visit). Tag mode
  partitions the SAME filtered, globally-sorted list into: one section per
  distinct tag (alphabetical, case-insensitive), where a task appears under
  EACH of its tags; then **No tags** (open, untagged); then **Done** (all
  done tasks, regardless of tags). Section headers always render in tag
  mode. Because a task can render more than once, row `:key` becomes
  `` `${section}:${task.path}` `` and the inline editor tracks the clicked
  row (section + path), so opening the editor expands only that one row;
  the per-path `busy` guard still serializes writes for the task across
  all its rendered rows. Toggle/archive from any duplicate row acts on the
  one underlying task.

### Unchanged
Sort order, date buckets (default mode), counter badge, progress semantics,
archive/toggle mechanics, the >5 title-filter threshold and its
stale-query gating.

## Sanctioned writes (unchanged discipline)

Still the same two write paths — collision-safe create (one more optional
line) and the surgical field write (one more key, plus the block-list
consumption rule). Atomicity, containment, and never-clobber guarantees are
untouched; no new write path.

## Testing

- **Core:** `note_tags` on flow / block / legacy-string / `tag:` alias /
  quoted items / `#`-prefixed items / invalid-charset drops /
  case-insensitive dedupe / `tags:` beating `tag:`; `is_valid_tag` accept +
  reject table (incl. all-numeric rejection); `render_task` with and
  without tags (without = byte-identical to today); `set_fields`
  block-consumption: block→flow rewrite, block removal, CRLF block,
  indented items, block followed by another key, empty-value key with NO
  list following (plain rewrite, nothing extra consumed).
- **Shell contract (via Vitest `mockIPC`):** `add_task` carries `tags`;
  `update_task` patch carries `tags` / `[]` clear semantics.
- **Vitest:** chips render from `tags`; chip click activates the tag filter
  (list narrows, dismiss chip appears, ✕ clears); tag + title filters
  combine; add/editor tags inputs parse `#work, home` into `["work",
  "home"]` and send them; editor omits `tags` when unchanged and sends
  `[]` when emptied; tag view sections alphabetical with a multi-tag task
  repeating, No tags + Done sections, editor opens on only the clicked
  duplicate row; grouping toggle defaults back to dates on remount.

## Out of scope (YAGNI)

Body `#hashtags`; tag autocomplete/suggestions; tag renaming across files;
nested-tag hierarchy UI (each nested string is its own section); persisting
the grouping toggle; multiple simultaneous tag filters.
