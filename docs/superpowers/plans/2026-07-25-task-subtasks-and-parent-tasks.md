# Subtasks & Parent Tasks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a Task an optional parent Task, so Tasks form hierarchies — managed from the Task Detail surface, with a light touch in the main list.

**Architecture:** Two additive frontmatter keys on the child: `parent-id` (the parent's stable Task ID — authoritative for all resolution) and `parent` (an Obsidian link — navigation only). All hierarchy logic is pure and lives in `core::tasks` (`hierarchy.rs`, `parent_link.rs`, the readers); the service layer owns validation ordering and the one config side-effect; the shell only threads IPC. The frontend addresses a parent by **path**, never by id.

**Tech Stack:** Rust (`vault_buddy_core`, Tauri shell), Vue 3 + Pinia + TypeScript + Tailwind 4, Vitest, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-07-25-task-subtasks-and-parent-tasks-design.md` — read §1–§7 before Task 1. Where this plan and the spec disagree, the spec wins; stop and flag it.

## Global Constraints

- **Frontmatter keys are exactly** `parent-id` and `parent` (kebab-case, lowercase).
- **`parent-id` is authoritative.** Every resolution (children, ancestors, cycles) reads only `parent-id`. The `parent` link is never parsed for meaning.
- **The pair is written and cleared together** — never one without the other.
- **A parent is addressed by PATH across every boundary** (IPC, service). Ids are an internal detail; `TaskItem.id` is `None` whenever the vault has IDs disabled, so no caller may require one.
- **Validation precedes every side effect.** In `set_task_parent`: validate → enable IDs → write. No config write and no id stamp may occur before the cycle gate.
- **The validation index reads the id property unconditionally**, even when generation is disabled — hand-authored Tasks can already carry ids. This is internal only; `list_tasks`' surfaced `id` stays gated on `task_id_enabled` (unchanged).
- **An id carried by more than one task is unresolvable** — omitted from the index, and refused as a parent.
- **Additive:** a Task with no parent keys must read and write byte-for-byte as today. Pin this with a regression test.
- **Both keys are `RESERVED_TASK_KEYS`** (`tasks/mod.rs`, single-sourced).
- **Never loosen a baseline** (`scripts/loc-baseline.json`, `scripts/quality-baseline.json`, coverage floors in `vite.config.ts`) to make a gate pass.
- **Rust:** run `cargo fmt` before every commit; `cargo clippy --all-targets -- -D warnings` must be clean.
- **Commit style:** Conventional Commits (`feat(core):`, `fix(ui):`, `test(core):`, `docs(tasks):`).

## File Structure

| File | Responsibility |
| --- | --- |
| `src-tauri/core/src/tasks/parent.rs` **NEW** | Lenient readers for `parent-id` / `parent` |
| `src-tauri/core/src/tasks/parent_link.rs` **NEW** | Compose the Obsidian link (wikilink vs. escaped markdown fallback) |
| `src-tauri/core/src/tasks/hierarchy.rs` **NEW** | `ParentIndex`, `parent_index`, `ambiguous_ids`, `ancestors`, `would_create_cycle` |
| `src-tauri/core/src/tasks/mod.rs` | Add both keys to `RESERVED_TASK_KEYS`; declare + re-export the new modules |
| `src-tauri/core/src/tasks/list.rs` | `TaskItem.parent_id` / `parent_link`; read them in `collect_task_file` |
| `src-tauri/core/src/tasks/create.rs` | `render_task` / `create_task` write an optional parent pair |
| `src-tauri/core/src/uri.rs` | Expose `encode` as `pub(crate)` for the markdown fallback |
| `src-tauri/core/src/services/tasks/mod.rs` | `set_task_parent` (3 phases); `TaskDto` fields; `add_task` parent |
| `src-tauri/src/task_commands.rs` | `TaskPatchDto.parent_path`/`clear_parent`; `add_task` parent; `update_task` return; the id-config lock |
| `src/composables/useTaskHierarchy.ts` **NEW** | Index, children, progress, and the parent verbs |
| `src/components/TaskParentPicker.vue` **NEW** | Searchable, cycle-aware parent picker (emits a path) |
| `src/components/TaskDetail.vue` | Parent row + Subtasks section |
| `src/components/TaskRow.vue` | Subtask badge + parent chip |
| `src/types.ts` | `parentId` / `parentLink` on `TaskItem`; `parentPath` / `clearParent` on `TaskPatch` |

---

### Task 1: Parent keys — lenient read, reserved, surfaced

**Files:**
- Create: `src-tauri/core/src/tasks/parent.rs`
- Modify: `src-tauri/core/src/tasks/mod.rs` (module decl + `RESERVED_TASK_KEYS`)
- Modify: `src-tauri/core/src/tasks/list.rs` (`TaskItem` + `collect_task_file`)
- Modify: `src-tauri/core/src/services/tasks/mod.rs` (`TaskDto` + `from_item`)
- Modify: `src/types.ts`

**Interfaces:**
- Consumes: `capture_note::raw_scalar_field`, `tasks::description::decode_scalar_lenient` (make it `pub(super)` if it is not already).
- Produces: `tasks::parent::{parent_id_field, parent_link_field}`; `TaskItem.parent_id` / `TaskItem.parent_link`; `TaskDto.parent_id` / `parent_link` (camelCase over IPC).

- [ ] **Step 1: Write the failing reader tests**

In `src-tauri/core/src/tasks/parent.rs`:

```rust
//! Lenient readers for the two parent keys. `parent-id` is authoritative for
//! hierarchy resolution; `parent` is an Obsidian link carried for navigation
//! only and is never parsed for meaning.

/// The raw `parent-id` scalar, or `None` when absent/empty/non-scalar. Lenient
/// like every other widened field: a block (`|`/`>`) or flow (`[..]`/`{..}`)
/// value degrades to None rather than surfacing a partial value.
pub(super) fn parent_id_field(content: &str) -> Option<String> {
    scalar(content, "parent-id")
}

/// The raw `parent` link scalar. Carried through to the DTO verbatim — the app
/// never interprets it.
pub(super) fn parent_link_field(content: &str) -> Option<String> {
    scalar(content, "parent")
}

fn scalar(content: &str, key: &str) -> Option<String> {
    let raw = crate::capture_note::raw_scalar_field(content, key)?;
    if raw.is_empty() || raw.starts_with(['[', '{', '|', '>']) && !raw.starts_with("[[") {
        return None;
    }
    let decoded = super::description::decode_scalar_lenient(raw);
    (!decoded.trim().is_empty()).then_some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plain_and_quoted_values() {
        let c = "---\ntype: Task\nparent-id: ab12cd34\nparent: \"[[Tasks/Work/p]]\"\n---\n";
        assert_eq!(parent_id_field(c), Some("ab12cd34".to_string()));
        assert_eq!(parent_link_field(c), Some("[[Tasks/Work/p]]".to_string()));
    }

    #[test]
    fn absent_empty_and_non_scalar_read_as_none() {
        assert_eq!(parent_id_field("---\ntype: Task\n---\n"), None);
        assert_eq!(parent_id_field("---\ntype: Task\nparent-id:\n---\n"), None);
        // A block or flow value is the user's own frontmatter, not our scalar.
        assert_eq!(parent_id_field("---\ntype: Task\nparent-id:\n  a: b\n---\n"), None);
        assert_eq!(parent_id_field("---\ntype: Task\nparent-id: {a: b}\n---\n"), None);
    }

    #[test]
    fn an_unquoted_wikilink_still_reads_as_a_link() {
        // Hand-authored `parent: [[X]]` is a YAML flow sequence, but it is the
        // form users type; read it rather than dropping it. It is never parsed
        // for meaning, so a lenient read costs nothing.
        let c = "---\ntype: Task\nparent: [[Tasks/p]]\n---\n";
        assert_eq!(parent_link_field(c), Some("[[Tasks/p]]".to_string()));
    }
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cd src-tauri/core && cargo test --lib tasks::parent`
Expected: FAIL — `parent` module not declared.

- [ ] **Step 3: Declare the module and reserve the keys**

In `src-tauri/core/src/tasks/mod.rs`, add `mod parent;` beside the other module declarations, and add both keys to the shared set (after `"description",`):

```rust
    "description",
    "parent-id",
    "parent",
];
```

If `description::decode_scalar_lenient` is not visible from `parent.rs`, widen it to `pub(super)`.

- [ ] **Step 4: Run the tests — expect PASS**

Run: `cd src-tauri/core && cargo test --lib tasks::parent`

- [ ] **Step 5: Write the failing surfacing test**

Add to `src-tauri/core/src/tasks/list.rs` tests:

```rust
    #[test]
    fn list_tasks_surfaces_the_parent_pair() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "c.md", "---\ntype: Task\nstatus: new\ntitle: \"C\"\ncreated: 2026-07-25\nparent-id: ab12cd34\nparent: \"[[Tasks/p]]\"\n---\n");
        let out = list_tasks(root, None);
        assert_eq!(out[0].parent_id.as_deref(), Some("ab12cd34"));
        assert_eq!(out[0].parent_link.as_deref(), Some("[[Tasks/p]]"));
    }

    #[test]
    fn parent_id_is_surfaced_even_when_ids_are_disabled() {
        // `id_property = None` (feature off) must NOT suppress parent-id: the
        // service validates against dormant ids, and the row still needs to
        // know it has a parent. Only the task's OWN id is gated.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "c.md", "---\ntype: Task\nstatus: new\ntitle: \"C\"\ncreated: 2026-07-25\ntask-id: aaa11111\nparent-id: ab12cd34\n---\n");
        let out = list_tasks(root, None);
        assert_eq!(out[0].id, None); // own id gated, as today
        assert_eq!(out[0].parent_id.as_deref(), Some("ab12cd34")); // parent NOT gated
    }
```

- [ ] **Step 6: Run — expect FAIL (no such fields)**

Run: `cd src-tauri/core && cargo test --lib tasks::list`

- [ ] **Step 7: Add the fields and read them**

In `TaskItem` (after `description`):

```rust
    /// The parent Task's stable id, read from `parent-id`. Authoritative for
    /// hierarchy resolution. NOT gated on the vault's id feature — a task's own
    /// id is (it is read under the configured property), but this is a plain
    /// key that always means the same thing.
    pub parent_id: Option<String>,
    /// The parent's Obsidian link (`parent`), carried verbatim for navigation.
    /// Never parsed for meaning.
    pub parent_link: Option<String>,
```

In `collect_task_file`, before `out.push(...)`:

```rust
    let parent_id = super::parent::parent_id_field(&content);
    let parent_link = super::parent::parent_link_field(&content);
```

and add `parent_id, parent_link,` to the `TaskItem { .. }` literal.

- [ ] **Step 8: Mirror onto the DTO**

In `src-tauri/core/src/services/tasks/mod.rs`, add to `TaskDto` (after `description`):

```rust
    /// The parent Task's stable id (`parent-id`); `None` when the Task has no
    /// parent. Additive for the frontend and MCP `list_tasks` alike.
    pub parent_id: Option<String>,
    /// The parent's Obsidian link, for display/navigation only.
    pub parent_link: Option<String>,
```

and map both in `from_item`.

In `src/types.ts`, add to the `TaskItem` interface:

```ts
  parentId: string | null;
  parentLink: string | null;
```

- [ ] **Step 9: Add an UNFILTERED walk for structural scans**

`list_tasks` drops `status: archived` at `list.rs:128` — it is a *presentation*
function. But an archived Task's file still carries its `parent-id`, so every
hierarchy scan (the cycle index in Task 5, the settings guard in Task 6) must see
them: otherwise a cycle routed `A → B(archived) → C` is invisible, and the guard
lets an ID-settings change through that orphans archived links.

Write the failing test:

```rust
    #[test]
    fn list_tasks_including_archived_keeps_archived_rows() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "a.md", "---\ntype: Task\nstatus: new\ntitle: \"Open\"\n---\n");
        write(root, "b.md", "---\ntype: Task\nstatus: archived\ntitle: \"Arch\"\nparent-id: x\n---\n");
        assert_eq!(list_tasks(root, None).len(), 1); // presentation: archived hidden
        let all = list_tasks_including_archived(root, None);
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|t| t.parent_id.as_deref() == Some("x")));
    }
```

Implement by extracting the archived filter into a flag: keep `list_tasks(root,
id_property)` as-is (delegating with `include_archived: false`) and add
`list_tasks_including_archived(root, id_property)`. One walk implementation, two
entry points — do not copy the walk.

- [ ] **Step 10: Run the full core suite + gates**

Run: `cd src-tauri && cargo fmt && cd core && cargo test --lib && cargo clippy --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 11: Commit**

```bash
git add -A && git commit -m "feat(core): read the parent-id/parent keys and surface them on TaskItem/TaskDto"
```

---

### Task 2: Compose the parent link (wikilink, with an escaped markdown fallback)

**Files:**
- Create: `src-tauri/core/src/tasks/parent_link.rs`
- Modify: `src-tauri/core/src/uri.rs` (expose `encode`)
- Modify: `src-tauri/core/src/tasks/mod.rs` (module decl)

**Interfaces:**
- Consumes: `uri::vault_relative_no_ext`, `uri::encode` (to be `pub(crate)`).
- Produces: `tasks::parent_link::compose(parent_path: &Path, vault_root: &Path, parent_title: &str) -> Option<String>` — the *unquoted* link text. Callers YAML-quote it.

- [ ] **Step 1: Write the failing tests**

```rust
//! Compose the `parent` link. A wikilink is used by default; a List folder can
//! legally contain wikilink metacharacters (`is_valid_list_name` rejects only
//! empty / `/` / `\` / a leading dot, and hand-created folders skip it), and
//! wikilinks have no escape for them — so those paths fall back to a
//! percent-encoded markdown link whose LABEL is also escaped.

use std::path::Path;

/// Characters that change a wikilink's meaning: `#` starts a heading target,
/// `|` an alias, `[`/`]` can terminate it, `^` a block ref.
const WIKILINK_UNSAFE: [char; 5] = ['#', '|', '[', ']', '^'];

pub fn compose(parent_path: &Path, vault_root: &Path, parent_title: &str) -> Option<String> {
    let rel_no_ext = crate::uri::vault_relative_no_ext(parent_path, vault_root)?;
    if !rel_no_ext.contains(WIKILINK_UNSAFE) {
        return Some(format!("[[{rel_no_ext}]]"));
    }
    // Fallback: percent-encoded target (keeping `/` as the path separator) and a
    // markdown-escaped label.
    let encoded = rel_no_ext
        .split('/')
        .map(crate::uri::encode)
        .collect::<Vec<_>>()
        .join("/");
    Some(format!("[{}]({encoded}.md)", escape_label(parent_title)))
}

/// Backslash-escape the characters that would break a markdown link label.
/// YAML quoting protects the surrounding scalar, not the Markdown parsed after
/// YAML decoding.
fn escape_label(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for c in title.chars() {
        if matches!(c, '\\' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn an_ordinary_path_becomes_a_wikilink() {
        let root = PathBuf::from("/v");
        let p = PathBuf::from("/v/Tasks/Work/2026-07-04-ship.md");
        assert_eq!(
            compose(&p, &root, "Ship it"),
            Some("[[Tasks/Work/2026-07-04-ship]]".to_string())
        );
    }

    #[test]
    fn a_metacharacter_list_falls_back_to_an_encoded_markdown_link() {
        // `Project#1` is a legal List folder; inside [[..]] the `#` would start a
        // heading target and silently point click-through at the wrong note.
        let root = PathBuf::from("/v");
        let p = PathBuf::from("/v/Tasks/Project#1/2026-07-04-ship.md");
        let link = compose(&p, &root, "Ship it").unwrap();
        assert_eq!(link, "[Ship it](Tasks/Project%231/2026-07-04-ship.md)");
        assert!(!link.starts_with("[[")); // not a wikilink
    }

    #[test]
    fn the_fallback_label_escapes_markdown_metacharacters() {
        // A title carrying `]` or `\` would otherwise produce a malformed link
        // even though the target is encoded.
        let root = PathBuf::from("/v");
        let p = PathBuf::from("/v/Tasks/A|B/t.md");
        let link = compose(&p, &root, r#"we [need] this \ now"#).unwrap();
        assert!(link.starts_with(r#"[we \[need\] this \\ now]("#), "got {link}");
    }

    #[test]
    fn a_path_outside_the_vault_yields_none() {
        assert_eq!(compose(Path::new("/other/t.md"), Path::new("/v"), "T"), None);
    }
}
```

- [ ] **Step 2: Run — expect FAIL**

Run: `cd src-tauri/core && cargo test --lib tasks::parent_link`

- [ ] **Step 3: Make `encode` reusable and declare the module**

In `src-tauri/core/src/uri.rs` change `fn encode(` to `pub(crate) fn encode(`. In `tasks/mod.rs` add `mod parent_link;` and `pub use parent_link::compose as compose_parent_link;`.

- [ ] **Step 4: Run — expect PASS**

Run: `cd src-tauri/core && cargo test --lib tasks::parent_link`

- [ ] **Step 5: Commit**

```bash
cd src-tauri && cargo fmt && cd .. && git add -A
git commit -m "feat(core): compose the parent link, falling back to an escaped markdown link"
```

---

### Task 3: Hierarchy resolution — index, ambiguity, ancestors, cycles

**Files:**
- Create: `src-tauri/core/src/tasks/hierarchy.rs`
- Modify: `src-tauri/core/src/tasks/mod.rs`

**Interfaces:**
- Consumes: `TaskItem` (`id`, `parent_id`).
- Produces: `ParentIndex<'a>`, `parent_index`, `ambiguous_ids`, `ancestors`, `would_create_cycle` — re-exported from `tasks`.

- [ ] **Step 1: Write the failing tests**

```rust
//! Pure hierarchy resolution over a loaded task set. Everything is keyed on
//! `parent-id`; the link is never consulted.

use super::TaskItem;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Child PATH -> parent PATH, for ONE vault's tasks, with edges resolved
/// THROUGH ids.
///
/// Keyed on paths, NOT ids: every task has a path, but a task can lack an id
/// while still naming a parent. An id-keyed index would drop such a task's
/// outgoing edge, so `P(no id, parent-id: c)` + `C(id: c)` would let "make P the
/// parent of C" pass and then create a P<->C cycle. Lacking an id only means a
/// task cannot be REFERENCED as a parent (design spec §3).
pub type ParentIndex<'a> = HashMap<&'a Path, &'a Path>;

/// Ids carried by more than one task — ambiguous, so unusable as a parent
/// reference. VB never creates these (duplicate regenerates, ensure_id never
/// overwrites), but a file copied in Explorer, a sync conflict, or a hand edit
/// does, and never-clobber then preserves them.
pub fn ambiguous_ids(tasks: &[TaskItem]) -> HashSet<&str> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for t in tasks {
        if let Some(id) = t.id.as_deref() {
            *seen.entry(id).or_insert(0) += 1;
        }
    }
    seen.into_iter().filter(|(_, n)| *n > 1).map(|(id, _)| id).collect()
}

pub fn parent_index(tasks: &[TaskItem]) -> ParentIndex<'_> {
    let ambiguous = ambiguous_ids(tasks);
    // id -> path, for the UNambiguous ids only.
    let by_id: HashMap<&str, &Path> = tasks
        .iter()
        .filter_map(|t| {
            let id = t.id.as_deref()?;
            (!ambiguous.contains(id)).then_some((id, t.path.as_path()))
        })
        .collect();
    let mut idx = HashMap::new();
    for t in tasks {
        let Some(pid) = t.parent_id.as_deref() else {
            continue;
        };
        // An unresolvable or ambiguous parent-id yields no edge: the child is an
        // orphan, never a guess.
        if let Some(&parent_path) = by_id.get(pid) {
            idx.insert(t.path.as_path(), parent_path);
        }
    }
    idx
}

/// Ancestor paths of `start`, nearest first, EXCLUDING `start`. Bounded by a
/// visited set so a pre-existing hand-authored cycle terminates.
pub fn ancestors<'a>(index: &ParentIndex<'a>, start: &'a Path) -> Vec<&'a Path> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = start;
    while let Some(&parent) = index.get(cur) {
        if !seen.insert(parent) {
            break; // cycle already on disk
        }
        out.push(parent);
        cur = parent;
    }
    out
}

/// True when making `parent` the parent of `child` would create a cycle.
pub fn would_create_cycle(index: &ParentIndex<'_>, child: &Path, parent: &Path) -> bool {
    child == parent || ancestors(index, parent).contains(&child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `name` doubles as the filename stem and the own-id (unless `id` is None).
    fn t(name: &str, id: Option<&str>, parent: Option<&str>) -> TaskItem {
        TaskItem {
            path: PathBuf::from(format!("/v/Tasks/{name}.md")),
            title: name.to_string(),
            status: "new".into(),
            created: "2026-07-25".into(),
            done: false,
            due: None,
            scheduled: None,
            priority: None,
            tags: vec![],
            list: String::new(),
            order: None,
            id: id.map(str::to_string),
            description: None,
            parent_id: parent.map(str::to_string),
            parent_link: None,
        }
    }

    fn p(name: &str) -> PathBuf {
        PathBuf::from(format!("/v/Tasks/{name}.md"))
    }

    #[test]
    fn detects_self_direct_and_transitive_cycles() {
        // c -> b -> a
        let tasks = vec![
            t("a", Some("a"), None),
            t("b", Some("b"), Some("a")),
            t("c", Some("c"), Some("b")),
        ];
        let idx = parent_index(&tasks);
        assert!(would_create_cycle(&idx, &p("a"), &p("a"))); // self
        assert!(would_create_cycle(&idx, &p("a"), &p("b"))); // direct
        assert!(would_create_cycle(&idx, &p("a"), &p("c"))); // transitive
        assert!(!would_create_cycle(&idx, &p("c"), &p("a"))); // c under a is fine
    }

    #[test]
    fn an_id_less_task_still_contributes_its_outgoing_edge() {
        // REGRESSION (design spec §3): P has no id of its own but names C as its
        // parent. An id-keyed index would drop P's edge and let "make P the
        // parent of C" pass, creating a P<->C cycle on the next write.
        let tasks = vec![t("p", None, Some("c")), t("c", Some("c"), None)];
        let idx = parent_index(&tasks);
        assert_eq!(idx.get(p("p").as_path()), Some(&p("c").as_path()));
        assert!(would_create_cycle(&idx, &p("c"), &p("p")));
    }

    #[test]
    fn a_preexisting_on_disk_cycle_terminates() {
        let tasks = vec![t("a", Some("a"), Some("b")), t("b", Some("b"), Some("a"))];
        let idx = parent_index(&tasks);
        let anc = ancestors(&idx, &p("a")); // must not loop forever
        assert!(anc.len() <= 2);
    }

    #[test]
    fn duplicate_ids_are_ambiguous_and_resolve_no_edges() {
        // Two files share id "dup" — a copied file. Neither end can identify a
        // unique task, so no edge is invented.
        let tasks = vec![
            t("x", Some("dup"), None),
            t("y", Some("dup"), None),
            t("z", Some("z"), Some("dup")),
        ];
        assert!(ambiguous_ids(&tasks).contains("dup"));
        assert!(parent_index(&tasks).is_empty());
    }

    #[test]
    fn an_unresolvable_parent_id_yields_no_edge() {
        let tasks = vec![t("orphan", Some("o"), Some("gone"))];
        assert!(parent_index(&tasks).is_empty());
    }
}
```

- [ ] **Step 2: Run — expect FAIL, then declare the module**

Run: `cd src-tauri/core && cargo test --lib tasks::hierarchy`
Then add to `tasks/mod.rs`: `mod hierarchy;` and
`pub use hierarchy::{ambiguous_ids, ancestors, parent_index, would_create_cycle, ParentIndex};`

- [ ] **Step 3: Run — expect PASS**

Run: `cd src-tauri/core && cargo test --lib tasks::hierarchy`

- [ ] **Step 4: Commit**

```bash
cd src-tauri && cargo fmt && cd .. && git add -A
git commit -m "feat(core): add hierarchy resolution with ambiguity and cycle detection"
```

---

### Task 4: Create a Task with a parent

**Files:**
- Modify: `src-tauri/core/src/tasks/create.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: `render_task(..., parent: Option<(&str, &str)>)` and `create_task(..., parent: Option<(&str, &str)>)` — `(parent_id, link)`, both already composed/validated by the caller. Append the parameter **last** on both, mirroring how `scheduled` was added.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn render_task_writes_the_parent_pair_after_created() {
        let out = render_task(
            "Child", "2026-07-25", None, None, &[], None, None, None, None,
            Some(("ab12cd34", "[[Tasks/p]]")),
        );
        assert!(out.contains("parent-id: ab12cd34"));
        assert!(out.contains("parent: \"[[Tasks/p]]\"")); // YAML-quoted
    }

    #[test]
    fn render_task_without_a_parent_is_byte_identical_to_today() {
        // The additive guarantee: a Task with no parent must be unchanged.
        let with_none = render_task(
            "T", "2026-07-25", Some("2026-08-01"), Some("high"), &["a".to_string()],
            Some(("task-id", "aaa11111")), None, None, Some("2026-07-30"), None,
        );
        assert!(!with_none.contains("parent"));
        assert_eq!(
            with_none,
            "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-25\ntask-id: aaa11111\ndue: 2026-08-01\nscheduled: 2026-07-30\npriority: high\ntags: [a]\n---\n"
        );
    }
```

> If the exact expected string differs, run the test once, copy the ACTUAL output into the assertion, and confirm by inspection that it matches today's format — the point is to lock byte-identity, not to invent a format.

- [ ] **Step 2: Run — expect FAIL (arity)**

Run: `cd src-tauri/core && cargo test --lib tasks::create`

- [ ] **Step 3: Thread the parameter**

Add `parent: Option<(&str, &str)>` as the last parameter of both `render_task` and `create_task` (pass it through in `create_task`). In `render_task`, emit immediately after the `task_id` line (so it sits with identity, before `due`):

```rust
    if let Some((pid, link)) = parent {
        out.push_str(&format!("parent-id: {pid}\n"));
        out.push_str(&format!(
            "parent: {}\n",
            crate::yaml_scalar::yaml_quote(link)
        ));
    }
```

Update every existing call site to pass `None` (the compiler lists them).

- [ ] **Step 4: Pin that Duplicate carries the parent pair**

A duplicated Task keeps its parent, landing as a *sibling* — this falls out of
`duplicate_task` resetting identity only, but the spec makes it a chosen
behavior, so pin it. Add to `src-tauri/core/src/tasks/structural.rs` tests:

```rust
    #[test]
    fn duplicate_task_preserves_the_parent_pair() {
        // A copy is a SIBLING: identity (title/status/id) resets, the parent
        // link does not, so the copy stays under the same parent.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        let src = root.join("c.md");
        std::fs::write(
            &src,
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\nparent-id: ab12cd34\nparent: \"[[Tasks/p]]\"\n---\n\nbody\n",
        )
        .unwrap();
        let new = duplicate_task(&root, &src, "2026-07-25", None, false).unwrap();
        let out = std::fs::read_to_string(&new).unwrap();
        assert!(out.contains("parent-id: ab12cd34"));
        assert!(out.contains("parent: \"[[Tasks/p]]\""));
        assert!(out.contains("title: \"C (copy)\"")); // identity did reset
    }
```

- [ ] **Step 5: Run the full core suite — expect PASS**

Run: `cd src-tauri/core && cargo test --lib`

- [ ] **Step 6: Commit**

```bash
cd src-tauri && cargo fmt && cd .. && git add -A
git commit -m "feat(core): render_task/create_task write an optional parent pair"
```

---

### Task 5: `set_task_parent` — validate, enable, write

**Files:**
- Modify: `src-tauri/core/src/services/tasks/mod.rs`

**Interfaces:**
- Consumes: `tasks::{parent_index, would_create_cycle, ambiguous_ids, is_valid_id_property, compose_parent_link, update_task_fields, list_tasks, is_task}`; `capture_config` for the vault config + `ConfigWriteLock`.
- Produces: `services::set_task_parent(paths, vault_id, child_path, parent_path: Option<&Path>) -> Result<ParentSet, String>` where `ParentSet { parent_id: Option<String>, parent_link: Option<String> }`. `parent_path = None` **clears** the pair.

**Read the spec's §2 before writing this — the phase order is the point of the task.**

- [ ] **Step 1: Write the failing tests**

Add to the services tests. These are the four that matter; add the happy path alongside.

```rust
    #[test]
    fn rejects_a_self_parent_without_enabling_ids_or_stamping() {
        // Phase separation: validation precedes EVERY side effect.
        let (paths, vault) = fixture_with_ids_disabled(&["a.md"]);
        let p = tasks_root(&paths, &vault).join("a.md");
        let before = std::fs::read_to_string(&p).unwrap();
        assert!(set_task_parent(&paths, &vault, &p, Some(&p)).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), before); // no stamp
        assert!(!config_for(&vault).task_id_enabled); // still disabled
    }

    #[test]
    fn refuses_a_cycle_through_an_id_less_prospective_parent() {
        // REGRESSION (design spec §3): P has no id but already names C as its
        // parent. The path-keyed graph must see P->C and refuse; an id-keyed one
        // would skip the check and create a P<->C cycle on the next write.
        let (paths, vault) = fixture_with_ids_enabled(&[]);
        let root = tasks_root(&paths, &vault);
        write(&root, "p.md", "---\ntype: Task\nstatus: new\ntitle: \"P\"\nparent-id: c\n---\n");
        write(&root, "c.md", "---\ntype: Task\nstatus: new\ntitle: \"C\"\ntask-id: c\n---\n");
        assert!(set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).is_err());
        // And nothing was stamped onto the id-less parent by the failed attempt.
        assert!(!std::fs::read_to_string(root.join("p.md")).unwrap().contains("task-id:"));
    }

    #[test]
    fn refuses_a_cycle_using_dormant_ids_while_generation_is_disabled() {
        // Hand-authored ids exist even with the feature off; the ordinary
        // list_tasks walk suppresses them, so validation must read the property
        // unconditionally or this passes vacuously and creates a real cycle.
        let (paths, vault) = fixture_with_ids_disabled(&[]);
        let root = tasks_root(&paths, &vault);
        write(&root, "a.md", "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\nparent-id: b\n---\n");
        write(&root, "b.md", "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\n---\n");
        // A already points at B; making A the parent of B closes the loop.
        let err = set_task_parent(&paths, &vault, &root.join("b.md"), Some(&root.join("a.md")));
        assert!(err.is_err(), "a cycle through dormant ids must be refused");
    }

    #[test]
    fn refuses_an_ambiguous_parent_id() {
        let (paths, vault) = fixture_with_ids_enabled(&[]);
        let root = tasks_root(&paths, &vault);
        write(&root, "p1.md", "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: dup\n---\n");
        write(&root, "p2.md", "---\ntype: Task\nstatus: new\ntitle: \"P2\"\ntask-id: dup\n---\n");
        write(&root, "c.md", "---\ntype: Task\nstatus: new\ntitle: \"C\"\n---\n");
        assert!(set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p1.md"))).is_err());
    }

    #[test]
    fn enables_ids_stamps_both_and_writes_a_resolvable_pair() {
        // The bootstrap: IDs off (the default), so no id is surfaced anywhere —
        // the parent is named by PATH and the service does the rest.
        let (paths, vault) = fixture_with_ids_disabled(&["p.md", "c.md"]);
        let root = tasks_root(&paths, &vault);
        let out = set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).unwrap();
        let pid = out.parent_id.unwrap();
        assert!(!pid.is_empty());
        assert!(config_for(&vault).task_id_enabled); // auto-enabled
        assert!(std::fs::read_to_string(root.join("p.md")).unwrap().contains(&format!("task-id: {pid}")));
        let child = std::fs::read_to_string(root.join("c.md")).unwrap();
        assert!(child.contains(&format!("parent-id: {pid}")));
        assert!(child.contains("parent: \"[[")); // a link was written
    }

    #[test]
    fn clearing_removes_both_keys() {
        let (paths, vault) = fixture_with_ids_enabled(&["p.md", "c.md"]);
        let root = tasks_root(&paths, &vault);
        set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).unwrap();
        set_task_parent(&paths, &vault, &root.join("c.md"), None).unwrap();
        let child = std::fs::read_to_string(root.join("c.md")).unwrap();
        assert!(!child.contains("parent-id"));
        assert!(!child.contains("parent:"));
    }
```

> Build the `fixture_with_ids_*` / `tasks_root` / `config_for` / `write` helpers to match the existing services test setup in this file — do not invent a new harness.

- [ ] **Step 2: Run — expect FAIL**

Run: `cd src-tauri/core && cargo test --lib services::tasks`

- [ ] **Step 3: Implement in three phases**

```rust
/// The effective pair written, so the caller can reflect it without a reload.
pub struct ParentSet {
    pub parent_id: Option<String>,
    pub parent_link: Option<String>,
}

/// Set (or clear, with `parent_path: None`) a task's parent.
///
/// THREE STRICTLY SEPARATED PHASES — see the design spec §2. Validation must
/// precede every side effect: a rejected self-parent or cycle leaves IDs
/// disabled and nothing stamped.
pub fn set_task_parent(
    paths: &ServicePaths,
    vault_id: &str,
    child_path: &Path,
    parent_path: Option<&Path>,
) -> Result<ParentSet, String> {
    // ---- Phase 1: validate. No writes, nothing mutated. ----
    let (vault, root) = resolve_vault_and_tasks_root(paths, vault_id)?;
    let child = canonical_task_in_root(&root, child_path)?;

    let Some(parent_path) = parent_path else {
        // Clear: no parent to validate, no ids needed.
        tasks::update_task_fields(&root, &child, &[("parent-id", None), ("parent", None)], None)?;
        return Ok(ParentSet { parent_id: None, parent_link: None });
    };
    let parent = canonical_task_in_root(&root, parent_path)?;
    if parent == child {
        return Err("A task cannot be its own parent.".to_string());
    }

    let cfg = capture_config::vault_config(&capture_config::load_config(), vault_id);
    let prop = cfg.task_id_property_name().to_string();
    if !tasks::is_valid_id_property(&prop) {
        return Err(format!(
            "The vault's task ID property {prop:?} is not a valid frontmatter key; \
             change it in the vault's Task settings first."
        ));
    }

    // Read ids UNCONDITIONALLY — hand-authored tasks carry ids even while
    // generation is off, and an index built from the gated walk would be empty,
    // passing the cycle check vacuously (design spec §2).
    // UNFILTERED: list_tasks hides archived tasks, but their files still carry
    // parent-id, so a cycle through an archived task would be invisible.
    let all = tasks::list_tasks_including_archived(&root, Some(&prop));
    let ambiguous = tasks::ambiguous_ids(&all);
    let parent_existing_id = read_own_id(&parent, &prop);
    if let Some(pid) = parent_existing_id.as_deref() {
        if ambiguous.contains(pid) {
            return Err("Two tasks share that ID, so it can't identify a parent. \
                        Change one of their IDs first."
                .to_string());
        }
    }
    // The graph is keyed on PATHS with edges resolved through ids, so an id-less
    // task still contributes its outgoing edge and the check is never skipped
    // for want of an id (design spec §3).
    let index = tasks::parent_index(&all);
    if tasks::would_create_cycle(&index, &child, &parent) {
        return Err("That would make a task its own ancestor.".to_string());
    }

    // ---- Phases 2+3a: the SHARED resolve path (also used by add_task). ----
    let resolved = resolve_parent_for_write(&vault, &root, &parent, &prop, &cfg, || {
        // Re-validation closure, run only if the config changed under the lock.
        let all = tasks::list_tasks_including_archived(&root, Some(&prop));
        tasks::would_create_cycle(&tasks::parent_index(&all), &child, &parent)
    })?;

    // ---- Phase 3b: write the child's pair. ----
    tasks::update_task_fields(
        &root,
        &child,
        &[
            // ensure_id preserves ANY usable existing value, so an inherited
            // `task-id: "[legacy]"` would otherwise emit a bare flow sequence the
            // reader rejects. quote_id_if_needed keeps generated base36 bare.
            ("parent-id", Some(&quote_id_if_needed(&resolved.parent_id))),
            ("parent", Some(&crate::yaml_scalar::yaml_quote(&resolved.link))),
        ],
        Some(&prop),
    )?;
    Ok(ParentSet { parent_id: Some(resolved.parent_id), parent_link: Some(resolved.link) })
}

/// Validate-under-lock, enable, stamp the parent, compose the link. Returns with
/// the lock RELEASED only after the caller's own write, so callers take the
/// guard from the returned struct (or this takes a closure that performs the
/// child write — pick one shape and keep it consistent; the guard must outlive
/// the child write).
///
/// Shared by `set_task_parent` (writes onto an existing child) and `add_task`
/// (passes the pair into `create_task`). Add-subtask is very often a vault's
/// FIRST hierarchy operation — IDs off, parent unstamped — so the create path
/// must run this WHOLE path, not just the read-only validation (design spec §2).
fn resolve_parent_for_write(
    vault: &Path,
    root: &Path,
    parent: &Path,
    prop: &str,
    phase1_cfg: &VaultCaptureConfig,
    recheck_cycle: impl FnOnce() -> bool,
) -> Result<ResolvedParent, String> {
    // ONE lock across the config re-check, the enable, and the stamp. A
    // concurrent set_task_id_config holds the same lock across its scan AND
    // write, so the two serialize either way (design spec §2).
    let _guard = capture_config::config_write_lock();

    // Phase 1 read the config BEFORE this lock existed, so a settings save may
    // have committed a different property in between; writing under the stale
    // one would orphan the hierarchy immediately. Re-read and re-validate only
    // when it actually changed (design spec §2).
    let fresh = capture_config::vault_config(&capture_config::load_config(), vault_id);
    if fresh.task_id_enabled != phase1_cfg.task_id_enabled
        || fresh.task_id_property_name() != prop
    {
        return Err("The vault's Task ID settings changed while this was in \
                    flight. Try again."
            .to_string());
    }
    if recheck_cycle() {
        return Err("That would make a task its own ancestor.".to_string());
    }

    if !fresh.task_id_enabled {
        // MUST NOT re-acquire the lock — it is not reentrant and a nested
        // acquire self-deadlocks. This is the *_locked variant.
        enable_task_ids_locked(vault_id)?;
    }
    let parent_id = tasks::update_task_fields(root, parent, &[], Some(prop))?
        .ok_or("Could not assign an ID to the parent task.")?;
    let link = tasks::compose_parent_link(parent, vault, &read_title(parent))
        .ok_or("The parent task is outside the vault.")?;
    Ok(ResolvedParent { parent_id, link })
}
```

Add this helper beside the writer (and use it in the `create_task` path too, so
both writers quote identically):

```rust
/// Emit an id bare when it is a plain-safe token, quoted otherwise. Every
/// GENERATED id is base36 and stays bare, matching how the id property itself is
/// written. But `ensure_id` preserves any usable non-empty existing value, so a
/// hand-authored `task-id: "[legacy]"` yields the effective id `[legacy]` —
/// emitting `parent-id: [legacy]` bare would make it a YAML flow SEQUENCE, which
/// the lenient reader rejects, unresolving the link the instant it is written
/// (Codex P2, PR #77).
fn quote_id_if_needed(id: &str) -> String {
    let plain = !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if plain { id.to_string() } else { crate::yaml_scalar::yaml_quote(id) }
}

#[test]
fn quote_id_if_needed_keeps_base36_bare_and_quotes_the_rest() {
    assert_eq!(quote_id_if_needed("k3m9x2qp"), "k3m9x2qp");
    assert_eq!(quote_id_if_needed("[legacy]"), "\"[legacy]\"");
    assert_eq!(quote_id_if_needed("has space"), "\"has space\"");
}
```

> **Shape decision for the implementer:** the `ConfigWriteLock` guard must stay
> alive across the caller's own write (the child's pair, or `create_task`). Either
> return the guard alongside the result, or invert the helper so it takes the
> caller's write as a closure it runs before releasing. Pick one and use it for
> both callers — do NOT let the guard drop at the end of the helper, which would
> reopen the race this closes. Simplest correct choice: pass the write in as a
> closure.
>
> A "settings changed, try again" error is preferable to silently re-running a
> full walk. It is reachable only on a vault's first hierarchy operation, since
> `set_task_id_config` refuses a change once parent links exist (Task 6).

Helpers to add beside it: `canonical_task_in_root` (canonicalize + containment + `is_task`), `read_own_id` (read the file, `tasks::parse::scalar_id_ci`-equivalent — widen visibility if needed), `read_title`, and `enable_task_ids_locked` (the read-modify-write setting `task_id_enabled = true`, **without** taking the lock — the caller holds it).

> **Check the existing lock API first.** `capture_config` already serializes config writes; find how `set_task_id_config` acquires it and reuse that exact mechanism, adding a `*_locked` inner variant rather than inventing a second lock. If the existing helpers only expose a lock-taking form, split them into `foo()` (acquires, delegates) + `foo_locked()` (the body) so both callers share one implementation.

> **Note on quoting:** `update_task_fields` writes the value verbatim, so the link is `yaml_quote`d here while `parent-id` (a bare base36 scalar) is not. Confirm against `set_fields`' behavior and adjust if the writer already quotes.

- [ ] **Step 4: Run — expect PASS**

Run: `cd src-tauri/core && cargo test --lib services::tasks`

- [ ] **Step 5: Full gates + commit**

```bash
cd src-tauri && cargo fmt && cd core && cargo test --lib && cargo clippy --all-targets -- -D warnings
cd ../.. && git add -A
git commit -m "feat(core): add set_task_parent with validate-before-side-effect ordering"
```

---

### Task 6: Lock the ID configuration while hierarchies exist

**Files:**
- Modify: `src-tauri/src/task_commands.rs` (`set_task_id_config`)
- Modify: `src-tauri/core/src/services/tasks/mod.rs` (a `vault_has_parent_links` helper)

**Interfaces:**
- Produces: `services::vault_has_parent_links(paths, vault_id) -> bool`.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn vault_has_parent_links_detects_any_parent_id() {
        let (paths, vault) = fixture_with_ids_enabled(&[]);
        let root = tasks_root(&paths, &vault);
        write(&root, "a.md", "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n");
        assert!(!vault_has_parent_links(&paths, &vault));
        write(&root, "b.md", "---\ntype: Task\nstatus: new\ntitle: \"B\"\nparent-id: x\n---\n");
        assert!(vault_has_parent_links(&paths, &vault));
    }
```

- [ ] **Step 2: Implement the helper**

```rust
/// True when ANY task in the vault carries a `parent-id`. The ID configuration
/// is locked while this holds: changing the property name OR disabling the
/// feature would make every recorded reference unresolvable (design spec §2a).
pub fn vault_has_parent_links(paths: &ServicePaths, vault_id: &str) -> bool {
    let Ok((_, root)) = resolve_vault_and_tasks_root(paths, vault_id) else {
        return false;
    };
    // UNFILTERED: an archived task's file still carries parent-id, so reusing
    // the presentation-filtered list would let a settings change orphan it.
    tasks::list_tasks_including_archived(&root, None)
        .iter()
        .any(|t| t.parent_id.is_some())
}
```

(This relies on Task 1's guarantee that `parent_id` is surfaced even when `id_property` is `None`.)

- [ ] **Step 3: Gate the command**

In `set_task_id_config`, hold the config lock across **both** the scan and the write, so a concurrent `set_task_parent` (which holds the same lock through its phases 2–3) cannot slip a hierarchy in between them:

```rust
    // ONE lock across scan + write: without it, set_task_parent could write a
    // new parent link after this scan sees none and before this save commits,
    // orphaning that hierarchy immediately (design spec §2a).
    let _guard = capture_config::config_write_lock();
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    let changing = cfg.task_id_enabled != enabled || cfg.task_id_property_name() != resolved_property;
    if changing && services::vault_has_parent_links(&ServicePaths::real(), &id) {
        return Err("This vault has tasks with a parent, which reference Task IDs \
                    under the current property. Clear those parent links before \
                    changing the Task ID settings."
            .to_string());
    }
    // …then the existing read-modify-write, in its *_locked form (the lock is
    // NOT reentrant — a nested acquire self-deadlocks).
```

Keep a no-op save (nothing changed) working — the guard must only fire on an actual change.

- [ ] **Step 4: Add a shell test**

In `src-tauri/src/task_commands.rs` tests, assert a change is refused with parent links present and allowed without them.

- [ ] **Step 5: Run + commit**

Run: `cd src-tauri && cargo fmt && cargo test -p vault-buddy --lib && cd core && cargo test --lib`

```bash
git add -A && git commit -m "feat(shell): lock the task ID configuration while parent links exist"
```

---

### Task 7: IPC surface — path-keyed parent on update/add

**Files:**
- Modify: `src-tauri/src/task_commands.rs`
- Modify: `src-tauri/core/src/services/tasks/mod.rs` (`add_task` parent)
- Modify: `src/types.ts`

**Interfaces:**
- Produces: `TaskPatchDto.parent_path: Option<String>` + `clear_parent: bool`; `add_task(..., parent_path: Option<String>)`; `update_task` returns `{ id, parentId, parentLink }`.

- [ ] **Step 1: Extend the patch DTO**

```rust
    /// The parent Task's PATH (never its id — with IDs disabled, the default,
    /// no id is surfaced anywhere, so a path is the only identity the frontend
    /// can supply; design spec §2).
    #[serde(default)]
    pub parent_path: Option<String>,
    #[serde(default)]
    pub clear_parent: bool,
```

- [ ] **Step 2: Write the failing "parent-only patch" test FIRST**

```rust
    #[test]
    fn a_parent_only_patch_is_not_treated_as_empty() {
        // The Parent picker's Change/Clear sends {parentPath} / {clearParent}
        // with NO ordinary field updates. update_task no-ops an empty patch, so
        // unless the relationship fields count toward "is there anything to do",
        // every picker action is a silent no-op (Codex P1, PR #77).
        let patch = TaskPatchDto { parent_path: Some("/v/Tasks/p.md".into()), ..Default::default() };
        assert!(!patch_is_empty(&patch));
        let clearing = TaskPatchDto { clear_parent: true, ..Default::default() };
        assert!(!patch_is_empty(&clearing));
        assert!(patch_is_empty(&TaskPatchDto::default()));
    }
```

Derive `Default` on `TaskPatchDto` if it does not already, and extract the
emptiness decision into a `patch_is_empty` helper so it is testable and has ONE
definition.

- [ ] **Step 3: Handle it in `update_task`**

`parent_path` / `clear_parent` must count toward the emptiness decision AND be dispatched even when no ordinary field changed (clear wins over set, matching `clearDue`).

**Ordering matters for a COMBINED patch** (a title change *and* a parent assignment in one call, which an IPC caller may legally send). Writing the fields first and then failing parent validation commits the title while returning an error — the frontend reverts its whole optimistic patch and reports failure, yet the title is changed on disk (Codex P2, PR #77). So:

1. Run the parent's **read-only validation first** (phase 1 of the shared path) — a rejected parent must write nothing.
2. Then the ordinary field write, if any.
3. Then the parent write.
4. A parent failure at step 3 (an I/O failure, not validation) is a real partial state no ordering removes without a journal — report it in the **fields-saved** form so the caller does not claim total failure:

```rust
    Err(format!("Saved fields, but couldn't set the parent: {e}"))
```

This mirrors `useTaskDetail`'s existing `saveErrorMessage` for a failed list move — reuse that wording shape.

Add the regression:

```rust
    #[test]
    fn a_combined_patch_with_an_invalid_parent_writes_nothing() {
        // Title + a self-parent in one patch: validation runs first, so the
        // title must NOT be committed (Codex P2, PR #77).
        let (paths, vault) = fixture_with_ids_enabled(&["a.md"]);
        let root = tasks_root(&paths, &vault);
        let p = root.join("a.md");
        let before = std::fs::read_to_string(&p).unwrap();
        let patch = TaskPatchDto {
            title: Some("Renamed".into()),
            parent_path: Some(p.to_string_lossy().into_owned()), // self-parent
            ..Default::default()
        };
        assert!(apply_task_patch(&paths, &vault, &p, patch).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), before); // title untouched
    }
```

Fold the parent result into the return. Change the return type from `Option<String>` to a small DTO:

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskWriteResult {
    pub id: Option<String>,
    pub parent_id: Option<String>,
    pub parent_link: Option<String>,
}
```

Update `src/types.ts` and every frontend caller of `update_task` accordingly (`useTaskActions`, `useTaskDetail`, `useTaskReorder` — the compiler and `vue-tsc` will list them).

- [ ] **Step 4: Thread the parent through `add_task` — via the FULL shared path**

Add `parent_path: Option<String>` to the command and to `services::add_task`. In the service, run the **whole** `resolve_parent_for_write` path from Task 5 — validate, lock, re-check, enable IDs, stamp the parent, compose the link — then pass the resulting pair into `tasks::create_task` while the guard is still held. Phase 1's read-only validation alone is **not** enough here.

Write this failing test first:

```rust
    #[test]
    fn add_subtask_bootstraps_ids_when_the_vault_has_none() {
        // Add subtask is very often a vault's FIRST hierarchy operation: IDs are
        // off by default and the parent is unstamped, so validation alone would
        // leave no authoritative parent-id to write (Codex P1, PR #77).
        let (paths, vault) = fixture_with_ids_disabled(&["p.md"]);
        let root = tasks_root(&paths, &vault);
        let child = add_task(
            &paths, &vault, "Child", "2026-07-25", None, None, &[], None, None,
            Some(&root.join("p.md")),
        )
        .unwrap();
        assert!(config_for(&vault).task_id_enabled); // bootstrapped
        let pid = child.parent_id.expect("the child names a parent");
        assert!(!pid.is_empty());
        // …and it RESOLVES: the parent now carries that exact id.
        assert!(std::fs::read_to_string(root.join("p.md")).unwrap().contains(&format!("task-id: {pid}")));
    }
```

- [ ] **Step 5: Verify + commit**

Run: `cd src-tauri && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib`
Run: `npm run build`

```bash
git add -A && git commit -m "feat(shell): path-keyed parent on update_task and add_task"
```

---

### Task 8: `useTaskHierarchy` + the Detail Parent row

**Files:**
- Create: `src/composables/useTaskHierarchy.ts`
- Create: `src/components/TaskParentPicker.vue`
- Modify: `src/components/TaskDetail.vue`
- Test: `tests/task-hierarchy.test.ts` (new), `tests/task-detail.test.ts`

**Interfaces:**
- Produces: `useTaskHierarchy(task, allTasks)` → `{ parent, children, progress, setParent(path|null), busy }`.

- [ ] **Step 1: Write the failing composable tests**

`tests/task-hierarchy.test.ts`:

```ts
it("resolves the parent and children per vault, ignoring cross-vault ids", () => {
  // Ids are only unique within a vault; the aggregate view must never link across.
  const a = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
  const b = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
  const foreign = task({ vaultId: "v2", id: "x", parentId: "p", path: "/v2/x.md" });
  const h = useTaskHierarchy(ref(a), ref([a, b, foreign]));
  expect(h.children.value.map((t) => t.path)).toEqual(["/v1/c.md"]);
});

it("counts progress over children", () => {
  const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
  const c1 = task({ vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md", done: true, status: "done" });
  const c2 = task({ vaultId: "v1", id: "c2", parentId: "p", path: "/v1/c2.md" });
  const h = useTaskHierarchy(ref(p), ref([p, c1, c2]));
  expect(h.progress.value).toEqual({ done: 1, total: 2 });
});

it("resolves no parent when the parent id matches nothing (an orphan)", () => {
  const orphan = task({ vaultId: "v1", id: "c", parentId: "gone", path: "/v1/c.md" });
  const h = useTaskHierarchy(ref(orphan), ref([orphan]));
  expect(h.parent.value).toBeNull();
});

it("sends the parent PATH and clears with clearParent", async () => {
  const calls: any[] = [];
  mockIPC((cmd, args) => { calls.push([cmd, args]); return cmd === "update_task" ? { id: null, parentId: "p", parentLink: null } : undefined; });
  const h = useTaskHierarchy(ref(child), ref([parent, child]));
  await h.setParent("/v1/p.md");
  expect(calls[0][1].patch).toEqual({ parentPath: "/v1/p.md" });
  await h.setParent(null);
  expect(calls[1][1].patch).toEqual({ clearParent: true });
});

it("writes the stamped id onto the PARENT's cached row (IDs-off bootstrap)", async () => {
  // With IDs off — the default — the loaded parent row has id: null. The backend
  // enables IDs and stamps it, returning parentId. If only the child is updated,
  // the parent's cached id stays null and (since resolution compares ids) the
  // relationship the user just made is invisible until a reload (Codex P1, PR #77).
  const parent = task({ vaultId: "v1", id: null, path: "/v1/p.md", title: "Parent" });
  const child = task({ vaultId: "v1", id: null, path: "/v1/c.md", title: "Child" });
  mockIPC((cmd) => (cmd === "update_task" ? { id: "cid", parentId: "pid", parentLink: "[[Tasks/p]]" } : undefined));
  const all = ref([parent, child]);
  const h = useTaskHierarchy(ref(child), all);
  await h.setParent("/v1/p.md");
  expect(all.value.find((t) => t.path === "/v1/p.md")!.id).toBe("pid"); // parent stamped in cache
  expect(child.parentId).toBe("pid");
  expect(h.parent.value?.path).toBe("/v1/p.md"); // resolves WITHOUT a reload
});
```

- [ ] **Step 2: Implement the composable**

Mirror `useTaskDetail`'s discipline exactly: one shared `busy` guard, optimistic update, revert + `notifications.error` on failure, `logWarning`. Scope every lookup by `vaultId` before comparing ids.

**On success, write the response's `parentId` onto the selected PARENT row's cached `id` as well as the child's `parentId`** — in the IDs-off default the parent's cached id is `null` until the backend stamps it, and resolution compares ids, so skipping this leaves the new relationship invisible until a reload.

- [ ] **Step 3: Build `TaskParentPicker.vue`**

Presentational: props `{ tasks, currentPath, invalidPaths }`, emits `select(path | null)`. A search input filters by title; options in `invalidPaths` render `disabled` with a "would create a loop" note. Follow `TaskListPicker.vue` for markup and token usage; do NOT force `Field`/`AppButton` where they would resize the dense controls.

- [ ] **Step 4: Key `TaskDetail` by path so drilling remounts it (P1)**

This increment introduces the first detail→detail navigation. `openTaskDetail`
only swaps `store.taskDetailTask`; the view stays `taskDetail`, `ActionPanel`
renders `<TaskDetail>` **unkeyed** (`ActionPanel.vue:382`), and `TaskDetail.vue`
seeds all seven draft refs once in `setup` (lines 30-36) with **no watcher** on
`props.task`. So drilling would keep the previous task's fields on screen while
`useTaskDetail`'s `toRef` already points at the new path — and Save would write
the old values onto the newly opened task (Codex P1, PR #77).

Write the failing test first, in `tests/task-detail.test.ts`:

```ts
it("re-seeds every draft when drilling from one task's detail to another", async () => {
  // The rendered FIELDS must follow the task, not just the store path.
  const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md", title: "Parent", description: "pd" });
  const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", title: "Child", description: "cd" });
  mockIPC((cmd) => {
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
    if (cmd === "list_tasks") return [parent, child];
    return undefined;
  });
  const { useVaultsStore } = await import("../src/stores/vaults");
  const store = useVaultsStore();
  store.openTaskDetail(parent);
  const ActionPanel = (await import("../src/components/ActionPanel.vue")).default;
  const wrapper = mount(ActionPanel);
  await new Promise((r) => setTimeout(r));
  expect((wrapper.get('[data-testid="task-detail-title"]').element as HTMLInputElement).value).toBe("Parent");
  store.openTaskDetail(child); // drill through
  await new Promise((r) => setTimeout(r));
  expect((wrapper.get('[data-testid="task-detail-title"]').element as HTMLInputElement).value).toBe("Child");
  expect((wrapper.get('[data-testid="task-detail-description"]').element as HTMLTextAreaElement).value).toBe("cd");
});
```

Then fix it in `ActionPanel.vue`:

```html
        <TaskDetail
          v-if="store.taskDetailTask"
          :key="store.taskDetailTask.path"
          :task="store.taskDetailTask"
        />
```

Keying beats a props watcher: it resets *every* piece of local state — drafts,
the list load, the delete-confirm, the on-mount focus — including whatever a
future field adds, with no watcher to keep in sync.

- [ ] **Step 5: Wire the Parent row into `TaskDetail.vue`**

A row above the Subtasks section: the parent's title as a clickable chip (`vaults.openTaskDetail(parent)`), plus Change / Clear. Disable while `busy`. Compute `invalidPaths` from the frontend index (self + descendants); note in a comment that with IDs off the index is empty and nothing is pre-disabled — correctly, since no parent links can exist — and that core remains the authority.

- [ ] **Step 6: Run the frontend gates**

Run: `npx vitest run tests/task-hierarchy.test.ts tests/task-detail.test.ts tests/action-panel.test.ts`
Run: `npm run lint && npm run build`

- [ ] **Step 7: Commit**

```bash
git add -A && git commit -m "feat(ui): add the Task Detail parent row and cycle-aware picker"
```

---

### Task 9: The Subtasks section + Add subtask

**Files:**
- Modify: `src/components/TaskDetail.vue`
- Test: `tests/task-detail.test.ts`

- [ ] **Step 1: Write the failing tests**

```ts
it("lists children with a done/total progress line", async () => {
  const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md", list: "Work" });
  const kids = [
    task({ vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md", title: "One", done: true, status: "done" }),
    task({ vaultId: "v1", id: "c2", parentId: "p", path: "/v1/c2.md", title: "Two" }),
  ];
  mockIPC((cmd) => {
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
    if (cmd === "list_tasks") return [parent, ...kids];
    return undefined;
  });
  const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
  const wrapper = mount(TaskDetail, { props: { task: parent } });
  await new Promise((r) => setTimeout(r));
  expect(wrapper.get('[data-testid="task-detail-subtask-progress"]').text()).toContain("1 / 2");
  expect(wrapper.findAll('[data-testid="task-detail-subtask"]')).toHaveLength(2);
});

it("Add subtask creates a child with this task as the parent and inherits its List", async () => {
  const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md", list: "Work" });
  const calls: any[] = [];
  mockIPC((cmd, args) => {
    calls.push([cmd, args]);
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
    if (cmd === "list_tasks") return [parent];
    if (cmd === "add_task") return task({ vaultId: "v1", id: "n", parentId: "p", path: "/v1/n.md" });
    return undefined;
  });
  const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
  const wrapper = mount(TaskDetail, { props: { task: parent } });
  await new Promise((r) => setTimeout(r));
  await wrapper.get('[data-testid="task-detail-add-subtask"]').setValue("New child");
  await wrapper.get('[data-testid="task-detail-add-subtask"]').trigger("keydown", { key: "Enter" });
  await new Promise((r) => setTimeout(r));
  const add = calls.find((c) => c[0] === "add_task");
  expect(add[1].parentPath).toBe("/v1/p.md");
  expect(add[1].list).toBe("Work"); // inherits the parent's List
});

it("clicking a child's title drills into that child's detail", async () => {
  const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
  const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", title: "Kid" });
  mockIPC((cmd) => {
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], archivedLists: [] };
    if (cmd === "list_tasks") return [parent, child];
    return undefined;
  });
  const { useVaultsStore } = await import("../src/stores/vaults");
  const store = useVaultsStore();
  const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
  const wrapper = mount(TaskDetail, { props: { task: parent } });
  await new Promise((r) => setTimeout(r));
  await wrapper.get('[data-testid="task-detail-subtask-open"]').trigger("click");
  expect(store.taskDetailTask?.path).toBe("/v1/c.md");
});
```

- [ ] **Step 2: Implement**

A `SectionHeader` "Subtasks", the progress line, child rows (status checkbox + title button), and an inline "Add subtask" title input (IME-guarded Enter, Escape that `stopPropagation`s — the `TaskViewControls` create-list flow is the model). Every write goes through the shared `busy` guard.

- [ ] **Step 3: Watch the LOC cap**

Run: `npm run check:loc`. If `TaskDetail.vue` exceeds its cap, extract the Subtasks block into `TaskSubtasks.vue` (presentational, props + emits) rather than raising the baseline.

- [ ] **Step 4: Run gates + commit**

Run: `npx vitest run tests/task-detail.test.ts && npm run lint && npm run build`

```bash
git add -A && git commit -m "feat(ui): add the Subtasks section and Add subtask to Task Detail"
```

---

### Task 10: List affordances — subtask badge and parent chip

**Files:**
- Modify: `src/components/TaskRow.vue`, `src/components/Tasks.vue`
- Test: `tests/tasks.test.ts`

- [ ] **Step 1: Write the failing tests**

```ts
it("shows an open-subtask count badge on a parent, and none when all are done", async () => {
  const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
  const openKid = task({ vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md" });
  const doneKid = task({ vaultId: "v1", id: "c2", parentId: "p", path: "/v1/c2.md", done: true, status: "done" });
  const w = await mountTasks([parent, openKid, doneKid]);
  expect(w.get('[data-testid="task-subtask-count"]').text()).toBe("1"); // open only
  // With every child done the badge disappears entirely.
  const w2 = await mountTasks([parent, doneKid]);
  expect(w2.find('[data-testid="task-subtask-count"]').exists()).toBe(false);
});

it("shows a parent chip on a child that opens the parent's detail", async () => {
  const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md", title: "Big" });
  const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
  const { useVaultsStore } = await import("../src/stores/vaults");
  const store = useVaultsStore();
  const w = await mountTasks([parent, child]);
  const chip = w.get('[data-testid="task-parent-chip"]');
  expect(chip.text()).toContain("Big");
  await chip.trigger("click");
  expect(store.taskDetailTask?.path).toBe("/v1/p.md");
});
it("scopes the index per vault in aggregate mode", async () => {
  // Ids are unique only WITHIN a vault, so the aggregate view must never link a
  // child in one vault to a same-id task in another.
  const p1 = task({ vaultId: "v1", id: "p", path: "/v1/p.md", title: "V1 parent" });
  const foreign = task({ vaultId: "v2", id: "c", parentId: "p", path: "/v2/c.md" });
  const w = await mountTasks([p1, foreign], { vaultId: null });
  expect(w.find('[data-testid="task-parent-chip"]').exists()).toBe(false);
  expect(w.find('[data-testid="task-subtask-count"]').exists()).toBe(false);
});

it("renders a child whose parent id resolves to nothing as an ordinary top-level row", async () => {
  const orphan = task({ vaultId: "v1", id: "c", parentId: "gone", path: "/v1/c.md", title: "Orphan" });
  const w = await mountTasks([orphan]);
  expect(w.find('[data-testid="task-parent-chip"]').exists()).toBe(false);
  expect(w.text()).toContain("Orphan"); // still listed, just parentless
});
```

- [ ] **Step 2: Implement**

Pass a derived per-vault index down from `Tasks.vue`. The badge counts **open** (not-done) children and is hidden at zero. The chip shows the parent's title and calls `openTaskDetail`. Use `CountBadge` / `Chip`; change nothing about sorting, grouping, filtering, or drag-reorder.

- [ ] **Step 3: Run gates + commit**

Run: `npx vitest run tests/tasks.test.ts && npm run lint && npm run build && npm run check:loc && npm run check:quality`

```bash
git add -A && git commit -m "feat(ui): show subtask counts and parent chips in the task list"
```

---

### Task 11: Docs sweep + full verification

**Files:**
- Modify: `AGENTS.md`, `CONTEXT.md`, `docs/prds/task-management.md`, `docs/use-cases/per-vault-task-list.md`, `docs/Gaps.md`

- [ ] **Step 1: AGENTS.md** — in the tasks-domain section, document the two keys, the authoritative-id/navigational-link split, the validate→enable→write ordering, the ID-config lock, and ambiguous-id handling. Update the IPC table (`update_task` return, `add_task` parent) and the sanctioned-writes list (the parent write is the surgical field writer, not a new capability).

- [ ] **Step 2: CONTEXT.md** — add **Parent Task** and **Subtask** to the ubiquitous language, both in the Task-document sense.

- [ ] **Step 3: PRD** — mark `Parent Task` shipped in `docs/prds/task-management.md` (status paragraph, Task Model, Task Editing, and the Version 1 roadmap line).

- [ ] **Step 4: docs/Gaps.md** — add entries for: the parent link staling on a parent's List move (tracked fix: refresh children's links); orphan-on-delete leaving stale keys (tracked fix: best-effort clear, the `delete_task_list` precedent); the reserved-`parent`/`parent-id` id-property edge (the GAP-68/77 pattern); and phases 2–3 not being atomic (benign partial states only).

- [ ] **Step 5: Run every gate, in CI order**

```bash
npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
cd core && cargo test && cd .. && cargo test -p vault-buddy --lib
```

Expected: all green, no baseline loosened. If a ratchet regressed, fix the code — do not bump the baseline without a justification in the PR.

- [ ] **Step 6: Commit**

```bash
git add -A && git commit -m "docs(tasks): document subtasks and parent tasks"
```
