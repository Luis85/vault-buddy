# Task Detail Surface (description + delete + duplicate) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each Task a full-height in-panel "detail" home — an editable multi-line description plus all its metadata in one place — and add the two missing lifecycle verbs, permanent delete and duplicate.

**Architecture:** A new `taskDetail` panel view (a sibling of the tasks list in the fixed one-parent-per-view tree) rendered from the row's own `AggTask`. Description is a new `description:` frontmatter field stored as an escaped single-line double-quoted scalar so it rides the existing surgical writer (`set_fields`); delete/duplicate are two new async IPC commands over new core functions that reuse the canonical-containment + collision-safe machinery. The list re-fetches when you return, so no cross-view state sync is needed.

**Tech Stack:** Rust (Tauri v2 shell + pure `vault_buddy_core` crate), Vue 3 + Pinia + Tailwind 4, Vitest (happy-dom + `mockIPC`), `cargo test`.

## Global Constraints

- **`Tasks.vue` must NOT grow** — it is grandfathered over the 500-nonblank-LOC frontend cap at 521 (GAP-65, shrink-only). All new logic lives in new files (`TaskDetail.vue`, `useTaskDetail.ts`, additions to `utils/taskFields.ts`). Per-function ESLint limits also apply everywhere: `max-lines-per-function` 200, `complexity` 25, `max-params` 6, `max-depth` 5.
- **LOC / quality / coverage baselines are shrink-only.** If a gate fails because a metric IMPROVED, re-run with `--update` and commit the baseline in the same task. If a new file legitimately needs headroom, add its allowlist entry in the same commit with a one-line justification.
- **`description` is reserved ONLY in the id-property set** (`tasks/id.rs::RESERVED_TASK_KEYS`), NOT the template set (`tasks/disk.rs::RESERVED_TASK_KEYS`): `render_task` never emits `description`, so a template can't duplicate a managed line and a vault may legitimately seed it — the two `const`s now deliberately diverge on this one key (Codex P2, PR #76), and their comments cross-reference the divergence.
- **camelCase across the Rust↔TS boundary** — DTO fields use `#[serde(rename_all = "camelCase")]`; TS types match.
- **Writes stay never-clobber + canonically contained.** Duplicate uses the collision-safe non-replacing writer (`write_note_collision_safe`); delete asserts canonical containment before `remove_file`. Permanent delete is the app's first destructive vault write and is documented as a bounded departure in `docs/Gaps.md`.
- **Rust gates:** `cd src-tauri && cargo fmt --check`; `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test`. Shell changes compile-gate on Linux: `npm run setup:linux` (once) then `npx tauri build --no-bundle`.
- **Frontend gates (in this order):** `npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage`.
- **Do NOT modify `yaml_quote`** (template.rs:16) — it deliberately flattens newlines to spaces and is shared by every managed-field renderer. Description gets its own `yaml_quote_multiline` / `yaml_unquote_multiline` pair.

---

## Phase 1 — Core (Rust): description model + verbs

### Task 1: Multi-line YAML scalar quote/unquote pair

**Files:**
- Modify: `src-tauri/core/src/template.rs` (add two fns after `yaml_quote`, ~line 22)
- Test: same file's `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `pub fn yaml_quote_multiline(value: &str) -> String` — a valid single-line double-quoted YAML scalar with `\`, `"`, newline, tab escaped (CR dropped). `pub fn yaml_unquote_multiline(value: &str) -> String` — its inverse (single-pass), returning an unquoted input verbatim.

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/core/src/template.rs`'s test module (find `#[cfg(test)] mod tests` — if none exists in this file yet, add it at end of file: `#[cfg(test)] mod tests { use super::*; }`):

```rust
#[test]
fn yaml_quote_multiline_roundtrips_newlines_quotes_backslashes() {
    let s = "line one\nline \"two\"\twith a \\ backslash";
    let quoted = yaml_quote_multiline(s);
    // Single physical line, double-quoted, no raw newline.
    assert!(quoted.starts_with('"') && quoted.ends_with('"'));
    assert!(!quoted.contains('\n'));
    assert_eq!(yaml_unquote_multiline(&quoted), s);
}

#[test]
fn yaml_unquote_multiline_passes_through_unquoted_and_handles_literal_backslash_n() {
    // Hand-authored unquoted scalar → verbatim.
    assert_eq!(yaml_unquote_multiline("hello # not a comment"), "hello # not a comment");
    // A user who literally typed backslash-n must NOT get a newline: the
    // single-pass decoder consumes `\\` before it can see `n`.
    let s = "a\\nb"; // the three chars: a, backslash, n, b — wait: a \ n b
    let quoted = yaml_quote_multiline(s);
    assert_eq!(yaml_unquote_multiline(&quoted), s);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri/core && cargo test template::tests::yaml_quote_multiline -- --nocapture`
Expected: FAIL — `cannot find function yaml_quote_multiline`.

- [ ] **Step 3: Write minimal implementation**

Insert after `yaml_quote` (after line 22) in `src-tauri/core/src/template.rs`:

```rust
/// Double-quote a scalar PRESERVING newlines as `\n` escapes (unlike
/// `yaml_quote`, which flattens them to spaces for single-line managed
/// fields). Produces a valid one-physical-line YAML double-quoted scalar so a
/// multi-line value (the task `description`) rides the line-oriented surgical
/// writer untouched. Escapes `\` and `"`, encodes newline as `\n` and tab as
/// `\t`, and drops CR (newlines normalize to `\n`).
pub fn yaml_quote_multiline(value: &str) -> String {
    let mut inner = String::with_capacity(value.len() + 2);
    for c in value.chars() {
        match c {
            '\\' => inner.push_str("\\\\"),
            '"' => inner.push_str("\\\""),
            '\n' => inner.push_str("\\n"),
            '\t' => inner.push_str("\\t"),
            '\r' => {} // CR dropped; newlines normalize to \n
            other => inner.push(other),
        }
    }
    format!("\"{inner}\"")
}

/// Inverse of `yaml_quote_multiline`. A double-quoted value is unescaped in a
/// SINGLE left-to-right pass (so `\\` consumes both chars before an `n` could
/// be misread as a newline). An unquoted value (hand-authored / older file) is
/// returned verbatim — the defensive-read posture of the rest of the vault
/// domain.
pub fn yaml_unquote_multiline(value: &str) -> String {
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri/core && cargo test template::tests::yaml_quote_multiline && cargo test template::tests::yaml_unquote_multiline`
Expected: PASS (both).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/template.rs
git commit -m "feat(core): add multi-line YAML scalar quote/unquote pair

The task description is free text that may span lines and contain '#'; the
existing yaml_quote flattens newlines and the readers strip inline comments,
so description needs a dedicated escaped-single-line encode/decode pair."
```

---

### Task 2: `description` read path, DTO field, and reserved key

**Files:**
- Modify: `src-tauri/core/src/tasks/parse.rs` (add `description_field`, near `scalar_field` ~line 158)
- Modify: `src-tauri/core/src/tasks/list.rs` (`TaskItem` struct ~line 36; `collect_task_file` ~line 160)
- Modify: `src-tauri/core/src/tasks/disk.rs` (`RESERVED_TASK_KEYS` line 48-59)
- Modify: `src-tauri/core/src/tasks/id.rs` (`RESERVED_TASK_KEYS` line 10-21; tests line 99-142)
- Modify: `src-tauri/core/src/services/tasks/mod.rs` (`TaskDto` line 14-34; `from_item` line 36-52; `add_task` literal line 236-252)
- Test: `parse.rs` + `id.rs` test modules

**Interfaces:**
- Consumes: `template::yaml_unquote_multiline` (Task 1).
- Produces: `parse::description_field(content: &str) -> Option<String>` (`pub(super)`); `TaskItem.description: Option<String>`; `TaskDto.description: Option<String>`.

- [ ] **Step 1: Write the failing tests**

Add to `src-tauri/core/src/tasks/parse.rs` test module:

```rust
#[test]
fn description_field_decodes_a_multiline_scalar_and_ignores_comment_hash() {
    let content = "---\ntype: Task\ndescription: \"fix bug #42\\nsee notes\"\n---\n\nbody\n";
    assert_eq!(
        super::description_field(content),
        Some("fix bug #42\nsee notes".to_string())
    );
}

#[test]
fn description_field_is_none_when_absent_or_empty() {
    assert_eq!(super::description_field("---\ntype: Task\n---\n"), None);
    assert_eq!(super::description_field("---\ntype: Task\ndescription: \"\"\n---\n"), None);
}
```

Add to `src-tauri/core/src/tasks/id.rs` — extend the two existing reserved lists in the tests (line ~106 and ~116-127) to assert `description`:

```rust
// inside id_property_for_generation_gates_on_enabled_and_validity:
assert_eq!(id_property_for_generation(true, "description"), None); // reserved (detail)
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test tasks::parse::tests::description_field`
Expected: FAIL — `cannot find function description_field`.

- [ ] **Step 3: Write minimal implementation**

In `src-tauri/core/src/tasks/parse.rs`, add after `scalar_field` (after line 158):

```rust
/// Read the top-level `description:` free-text field, decoded via
/// `yaml_unquote_multiline`. UNLIKE `scalar_field`, it does NOT strip an inline
/// `#` comment (a description may legitimately contain `#`) and it unescapes
/// `\n` so a multi-line description round-trips. Returns `None` when absent or
/// empty. Top-level only (an indented `  description:` never matches), stops at
/// the closing fence, mirroring `note_field`.
pub(super) fn description_field(content: &str) -> Option<String> {
    let mut lines = content.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    for line in lines {
        if line.trim_end() == "---" {
            break; // end of frontmatter — never scan the body
        }
        if let Some(rest) = line.strip_prefix("description:") {
            let decoded = crate::template::yaml_unquote_multiline(rest.trim());
            return (!decoded.is_empty()).then_some(decoded);
        }
    }
    None
}
```

In `src-tauri/core/src/tasks/list.rs`, add the field to `TaskItem` (after `id`, line 35):

```rust
    pub id: Option<String>,
    /// Free-text detail, decoded from the `description:` frontmatter scalar
    /// (multi-line, `#`-tolerant). `None` when absent/empty.
    pub description: Option<String>,
```

In the same file, in `collect_task_file`'s `out.push(TaskItem { ... })` (line 160-173), add:

```rust
        id,
        description: super::parse::description_field(&content),
    });
```

Do NOT add `"description"` to `src-tauri/core/src/tasks/disk.rs`'s `RESERVED_TASK_KEYS` (the TEMPLATE filter). `render_task` never emits `description`, so a template key can't duplicate a managed line, and a vault may legitimately seed a task's description from its template — reserving it there would silently drop that content on upgrade (Codex P2, PR #76). Instead, update disk.rs's sync comment (the `// keep in sync with id.rs::RESERVED_TASK_KEYS` line above the array) to note the intentional divergence:

```rust
// Matches id.rs::RESERVED_TASK_KEYS except `description` (reserved there as an id property only; render_task never emits it, so a template may seed it — Codex PR #76).
```

`description` IS reserved as an id property. In `src-tauri/core/src/tasks/id.rs`, add `"description"` to its `RESERVED_TASK_KEYS` (line 10-21) after `"order",`, update its sync comment to note the same divergence, AND add it to the reserved-iteration loop in `is_valid_id_property_charset_and_reserved` (line 116-127):

```rust
    "order",
    "description",
];
```
```rust
        for reserved in [
            "type", "status", "title", "created", "due", "scheduled",
            "priority", "tags", "tag", "order", "description",
        ] {
```

In `src-tauri/core/src/services/tasks/mod.rs`, add to `TaskDto` (after `id`, line 33):

```rust
    pub id: Option<String>,
    /// Free-text detail (the `description:` frontmatter field). `None` when
    /// absent. Additive for the frontend and MCP `list_tasks` alike.
    pub description: Option<String>,
```

Add to `from_item` (after `id: t.id,`, line 50):

```rust
            id: t.id,
            description: t.description,
```

Add to `add_task`'s returned literal (after `id: generated_id,`, line 251):

```rust
        id: generated_id,
        // A newly created task has no description; it is added later in the
        // detail view.
        description: None,
    })
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test tasks::parse::tests::description_field && cargo test tasks::id::tests && cargo test --lib`
Expected: PASS. (The full `--lib` run confirms `TaskItem`/`TaskDto` still compile everywhere they're constructed — any other constructor site the compiler flags must get `description: None`.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks/parse.rs src-tauri/core/src/tasks/list.rs src-tauri/core/src/tasks/disk.rs src-tauri/core/src/tasks/id.rs src-tauri/core/src/services/tasks/mod.rs
git commit -m "feat(core): read a task description and reserve the key

description flows through list_tasks like every other field (decoded with
its own #-tolerant, newline-unescaping reader) and is reserved in both the
template and id-property key-sets so it can't be redefined or aliased."
```

---

### Task 3: `delete_task` core function

**Files:**
- Modify: `src-tauri/core/src/tasks/disk.rs` (add fn after `update_task_fields`, ~line 264)
- Test: `disk.rs` test module

**Interfaces:**
- Produces: `pub fn delete_task(root: &Path, path: &Path) -> Result<(), String>`.

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/core/src/tasks/disk.rs` test module:

```rust
#[test]
fn delete_task_removes_a_task_refuses_outside_and_refuses_a_non_task() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    let p = root.join("t.md");
    std::fs::write(&p, "---\ntype: Task\nstatus: new\ntitle: X\n---\n").unwrap();
    // Happy path: a real task is removed.
    assert!(delete_task(&root, &p).is_ok());
    assert!(!p.exists());
    // A path outside the tasks root is refused (write a sibling file to delete).
    let outside = dir.path().join("outside.md");
    std::fs::write(&outside, "x").unwrap();
    assert!(delete_task(&root, &outside).is_err());
    assert!(outside.exists());
    // A FOREIGN (non-task) file INSIDE the tasks root is refused — task folders
    // may legitimately hold non-task files, and this first destructive write
    // must never remove one (Codex P1, PR #76).
    let foreign = root.join("notes.md");
    std::fs::write(&foreign, "# just some notes, not a task\n").unwrap();
    assert!(delete_task(&root, &foreign).is_err());
    assert!(foreign.exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri/core && cargo test tasks::disk::tests::delete_task_removes`
Expected: FAIL — `cannot find function delete_task`.

- [ ] **Step 3: Write minimal implementation**

Add after `update_task_fields` (after line 264) in `src-tauri/core/src/tasks/disk.rs`:

```rust
/// Permanently delete a task file — the app's ONLY destructive vault write.
/// Canonicalizes `root` and `path` and requires containment (a symlink at the
/// file or folder can't be seen lexically), THEN re-reads the file and requires
/// it to be a `type: Task` document before removing. Task folders may
/// legitimately hold foreign files, and a listed row could be swapped for a
/// non-task file at the same path before the confirm lands — so identity is
/// re-validated immediately before this irreversible write, the same posture
/// the move/field writers get from `set_fields`' `type: Task` precondition
/// (Codex P1, PR #76). A missing file surfaces as an error (the row the user
/// clicked should exist), never a silent success.
pub fn delete_task(root: &Path, path: &Path) -> Result<(), String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    if !super::doc::is_task(&content) {
        return Err("Refusing to delete: not a type: Task document".to_string());
    }
    std::fs::remove_file(&canon_path).map_err(|e| format!("Cannot delete task: {e}"))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri/core && cargo test tasks::disk::tests::delete_task_removes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks/disk.rs
git commit -m "feat(core): add delete_task (canonical-containment-gated remove)"
```

---

### Task 4: `duplicate_task` core function

**Files:**
- Modify: `src-tauri/core/src/tasks/disk.rs` (add fn after `delete_task`)
- Test: `disk.rs` test module

**Interfaces:**
- Consumes: `set_fields` (writer), `task_basename` (private, disk.rs), `super::id::new_task_id`, `capture_note::{note_field, write_note_collision_safe}`, `template::yaml_quote`.
- Produces: `pub fn duplicate_task(root: &Path, path: &Path, today: &str, id_property: Option<&str>, ids_enabled: bool) -> Result<PathBuf, String>`. `id_property` is `Some(name)` only when the configured name is a valid, non-reserved id property (never touch a foreign/reserved key); `ids_enabled` decides regenerate vs. strip.

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/core/src/tasks/disk.rs` test module:

```rust
#[test]
fn duplicate_task_copies_body_and_resets_identity_with_fresh_id() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    let src = root.join("orig.md");
    std::fs::write(
        &src,
        "---\ntype: Task\nstatus: done\ntitle: \"Buy milk\"\ncreated: 2026-07-01\ntask-id: aaa11111\ndue: 2026-07-10\n---\n\nThe body stays.\n",
    )
    .unwrap();
    let new = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
    assert!(new.exists() && new != src);
    let out = std::fs::read_to_string(&new).unwrap();
    assert!(out.contains("title: \"Buy milk (copy)\""));
    assert!(out.contains("status: new")); // reset
    assert!(out.contains("The body stays.")); // body preserved
    assert!(out.contains("due: 2026-07-10")); // other fields preserved
    assert!(!out.contains("task-id: aaa11111")); // id regenerated, not shared
    assert!(out.contains("task-id: ")); // a fresh id is present
    // IDs off → the configured id property is STRIPPED, not inherited: leaving
    // the source id on the copy would collide with the original if IDs are
    // later re-enabled, and ensure-id never overwrites an existing value
    // (Codex P2, PR #76).
    let new2 = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), false).unwrap();
    let out2 = std::fs::read_to_string(&new2).unwrap();
    assert!(!out2.contains("task-id")); // stripped when ids are off
    assert!(out2.contains("title: \"Buy milk (copy)\""));
}

#[test]
fn duplicate_task_uses_the_filename_stem_when_the_source_has_no_title() {
    // An untitled hand-authored task lists under its filename stem, so the copy
    // must too — not an empty " (copy)" (Codex P2, PR #76).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    let src = root.join("My hand note.md");
    std::fs::write(&src, "---\ntype: Task\nstatus: new\n---\n\nbody\n").unwrap();
    let new = duplicate_task(&root, &src, "2026-07-24", None, false).unwrap();
    let out = std::fs::read_to_string(&new).unwrap();
    assert!(out.contains("title: \"My hand note (copy)\""));
}

#[test]
fn duplicate_task_rewrites_the_on_disk_id_key_casing() {
    // Source key `Task-ID:`, configured property `task-id`: the copy must
    // rewrite/strip THAT on-disk key, never insert a second differently-cased
    // id or miss the strip (Codex P2, PR #76).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    let src = root.join("orig.md");
    std::fs::write(
        &src,
        "---\ntype: Task\nstatus: new\ntitle: X\nTask-ID: aaa11111\n---\n",
    )
    .unwrap();
    // IDs on → the existing `Task-ID:` line is rewritten to a fresh id (no
    // second `task-id:` line, exactly one id key remains).
    let on = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), true).unwrap();
    let out_on = std::fs::read_to_string(&on).unwrap();
    assert!(!out_on.contains("aaa11111")); // old id replaced
    assert_eq!(out_on.matches("Task-ID:").count(), 1); // rewritten in place
    assert!(!out_on.to_lowercase().contains("task-id: aaa")); // no stale value
    assert_eq!(out_on.to_lowercase().matches("task-id:").count(), 1); // exactly one id key
    // IDs off → the `Task-ID:` line is stripped despite the casing mismatch.
    let off = duplicate_task(&root, &src, "2026-07-24", Some("task-id"), false).unwrap();
    let out_off = std::fs::read_to_string(&off).unwrap();
    assert!(!out_off.to_lowercase().contains("task-id"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri/core && cargo test tasks::disk::tests::duplicate_task_copies`
Expected: FAIL — `cannot find function duplicate_task`.

- [ ] **Step 3: Write minimal implementation**

Add after `delete_task` in `src-tauri/core/src/tasks/disk.rs`:

```rust
/// Duplicate a task file into the same folder, faithfully: the source bytes
/// are copied (body, extra frontmatter, description, unknown keys all
/// preserved), then only the identity fields are rewritten surgically via
/// `set_fields` — title → "<title> (copy)", status → new, and the id handled so
/// no two tasks ever share one. `id_property` is the vault's configured id key
/// ONLY when it is a valid, non-reserved property (else `None`, so a
/// foreign/reserved key is never touched). When present, the copy's id is
/// REGENERATED (`ids_enabled == true`) or STRIPPED (`false`): stripping matters
/// because leaving the source id on the copy would collide with the original if
/// the user later re-enables IDs, and the ensure-id path never overwrites an
/// existing value (Codex P2, PR #76). Written through the collision-safe
/// never-clobber writer, so a name clash takes the ` (N)` suffix. `today` names
/// the new file (clock-free core).
pub fn duplicate_task(
    root: &Path,
    path: &Path,
    today: &str,
    id_property: Option<&str>,
    ids_enabled: bool,
) -> Result<PathBuf, String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    // Title fallback matches list_tasks' display: an untitled hand-authored
    // task shows its filename stem, so the copy must too (Codex P2, PR #76).
    let stem = canon_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();
    let title = crate::capture_note::note_field(&content, "title").unwrap_or(stem);
    let new_title = format!("{title} (copy)");
    let quoted = yaml_quote(&new_title);
    // A fresh id when IDs are on; `None` strips the configured property so the
    // copy can never inherit the source id.
    let new_id = if ids_enabled {
        Some(super::id::new_task_id())
    } else {
        None
    };
    // Resolve the id key to its ON-DISK casing (case-insensitive lookup) so the
    // case-sensitive `set_fields` matches a differently-cased existing id — else
    // a strip misses `Task-ID:` and a regenerate inserts a SECOND, differently-
    // cased id (Codex P2, PR #76). With no id line on disk, add one under the
    // configured name ONLY when regenerating.
    let id_key: Option<String> = id_property.and_then(|prop| {
        match super::parse::frontmatter_scalar_ci(&content, prop) {
            Some((on_disk, _)) => Some(on_disk),
            None => new_id.as_ref().map(|_| prop.to_string()),
        }
    });
    let mut updates: Vec<(&str, Option<&str>)> =
        vec![("title", Some(quoted.as_str())), ("status", Some("new"))];
    if let Some(key) = id_key.as_deref() {
        updates.push((key, new_id.as_deref()));
    }
    let rewritten = set_fields(&content, &updates)
        .ok_or("Source is not a valid type: Task document")?;
    let parent = canon_path.parent().unwrap_or(&canon_root);
    let target = parent.join(format!("{}.md", task_basename(&new_title, today)));
    crate::capture_note::write_note_collision_safe(&target, &rewritten)
        .map_err(|e| format!("Cannot write duplicate: {e}"))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri/core && cargo test tasks::disk::tests::duplicate_task_copies`
Expected: PASS.

- [ ] **Step 5: Run the full core gate + commit**

Run: `cd src-tauri/core && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS, no warnings.

```bash
git add src-tauri/core/src/tasks/disk.rs
git commit -m "feat(core): add duplicate_task (faithful collision-safe copy)

Copies the source bytes (body + all frontmatter preserved), resets title to
'(copy)' and status to new, and regenerates the id when IDs are on so a
duplicate never shares an identifier."
```

---

## Phase 2 — Shell (Rust): IPC wiring

### Task 5: `update_task` carries `description` / `clearDescription`

**Files:**
- Modify: `src-tauri/core/src/capture_note.rs:42` (re-export `yaml_quote_multiline`)
- Modify: `src-tauri/src/task_commands.rs` (`TaskPatchDto` line 476-497; `update_task` line 521-569)

**Interfaces:**
- Consumes: `capture_note::yaml_quote_multiline` (Task 1, re-exported here).
- Produces: `TaskPatchDto.description: Option<String>`, `TaskPatchDto.clear_description: bool`; `update_task` writes the `description:` line.

- [ ] **Step 1: Re-export the multiline quoter**

In `src-tauri/core/src/capture_note.rs`, change line 42 from:

```rust
pub use crate::template::yaml_quote;
```
to:
```rust
pub use crate::template::{yaml_quote, yaml_quote_multiline};
```

- [ ] **Step 2: Add the fields + write logic**

In `src-tauri/src/task_commands.rs`, add to `TaskPatchDto` (after `order`, line 496):

```rust
    #[serde(default)]
    pub order: Option<f64>,
    /// Free-text detail written as an escaped single-line scalar via
    /// `yaml_quote_multiline` (multi-line, `#`-safe).
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub clear_description: bool,
```

In `update_task`, after the tags block (after line 569, before `if updates.is_empty()`):

```rust
    if patch.clear_description {
        updates.push(("description", None));
    } else if let Some(desc) = &patch.description {
        updates.push(("description", Some(capture_note::yaml_quote_multiline(desc))));
    }
```

- [ ] **Step 3: Add a core round-trip regression test (proves the DTO wiring is correct at the core boundary)**

Add to `src-tauri/core/src/tasks/disk.rs` test module — this pins the on-disk shape the shell produces via `set_fields`:

```rust
#[test]
fn update_task_fields_sets_rewrites_and_clears_description() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    let p = root.join("t.md");
    std::fs::write(&p, "---\ntype: Task\nstatus: new\ntitle: X\n---\n\nbody\n").unwrap();
    let quoted = crate::template::yaml_quote_multiline("hi\nthere #42");
    update_task_fields(&root, &p, &[("description", Some(quoted.as_str()))], None).unwrap();
    let after = std::fs::read_to_string(&p).unwrap();
    assert_eq!(super::parse::description_field(&after), Some("hi\nthere #42".to_string()));
    assert!(after.contains("\nbody\n")); // body untouched
    update_task_fields(&root, &p, &[("description", None)], None).unwrap();
    assert_eq!(super::parse::description_field(&std::fs::read_to_string(&p).unwrap()), None);
}
```

- [ ] **Step 4: Run tests + compile-gate the shell**

Run: `cd src-tauri/core && cargo test tasks::disk::tests::update_task_fields_sets_rewrites_and_clears_description`
Expected: PASS.

Then compile-gate the shell (first time only: `npm run setup:linux && npm run build`):
Run: `npx tauri build --no-bundle`
Expected: builds cleanly (no type errors from the new `TaskPatchDto` fields).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/capture_note.rs src-tauri/core/src/tasks/disk.rs src-tauri/src/task_commands.rs
git commit -m "feat(shell): update_task writes the task description

description/clearDescription ride the existing surgical writer via the new
escaped-single-line quoter; the note body is never touched."
```

---

### Task 6: `delete_task` + `duplicate_task` commands

**Files:**
- Modify: `src-tauri/src/task_commands.rs` (add two commands after `update_task`, ~line 593)
- Modify: `src-tauri/src/lib.rs` (`generate_handler!` — register both)

**Interfaces:**
- Consumes: `tasks::delete_task`, `tasks::duplicate_task` (Tasks 3-4); `tasks_root_for`, `capture_paths::assert_root_inside_vault`, `tasks::id_property_for_generation`, `chrono::Local`.
- Produces: IPC commands `delete_task(id, path) -> Result<(), String>` and `duplicate_task(id, path) -> Result<String, String>` (the landed path).

- [ ] **Step 1: Add the commands**

In `src-tauri/src/task_commands.rs`, after `update_task` (after line 593):

```rust
/// Permanently delete a task file. The app's first destructive vault write —
/// gated behind a hardened confirm in the detail view; see docs/Gaps.md for
/// the deliberate departure from vault-is-sacred. ASYNC: the fs removal is off
/// the main thread like the other task writes.
#[tauri::command]
pub async fn delete_task(id: String, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (vault_path, root, _cfg) = tasks_root_for(&id)?;
        if root.exists() {
            capture_paths::assert_root_inside_vault(&vault_path, &root)?;
        }
        tasks::delete_task(&root, Path::new(&path))
    })
    .await
    .map_err(|e| format!("delete_task: task failed: {e}"))?
}

/// Duplicate a task file into the same list. Returns the landed (possibly
/// suffixed) path so the detail view's success toast can offer to open it.
/// ASYNC: the read + collision-safe fsync'd write is off the main thread.
#[tauri::command]
pub async fn duplicate_task(id: String, path: String) -> Result<String, String> {
    let today = chrono::Local::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let (vault_path, root, cfg) = tasks_root_for(&id)?;
        if root.exists() {
            capture_paths::assert_root_inside_vault(&vault_path, &root)?;
        }
        // Touch the id property only when the configured name is a valid,
        // non-reserved id key — never a foreign/reserved field. `ids_enabled`
        // then decides regenerate (on) vs. strip (off) inside the core fn.
        // (If `tasks::is_valid_id_property` isn't already re-exported from the
        // `tasks` module the way `id_property_for_generation` is, add
        // `pub use id::is_valid_id_property;` alongside it — it is `pub` in
        // `tasks/id.rs`.)
        let prop_name = cfg.task_id_property_name();
        let id_property = tasks::is_valid_id_property(prop_name).then_some(prop_name);
        let new_path = tasks::duplicate_task(
            &root,
            Path::new(&path),
            &today,
            id_property,
            cfg.task_id_enabled,
        )?;
        Ok(new_path.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("duplicate_task: task failed: {e}"))?
}
```

- [ ] **Step 2: Register the commands**

In `src-tauri/src/lib.rs`, find the `tauri::generate_handler![` list and add (next to `update_task`):

```rust
            task_commands::update_task,
            task_commands::delete_task,
            task_commands::duplicate_task,
```

- [ ] **Step 3: Compile-gate the shell**

Run: `npx tauri build --no-bundle`
Expected: builds cleanly with both commands registered.

- [ ] **Step 4: Run the shell unit tests (if the linux-app env is set up)**

Run: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: PASS (existing tests unaffected).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/task_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): add delete_task and duplicate_task commands

Two async IPC commands over the new core fns; delete is containment-gated,
duplicate returns the landed path for the success toast. IPC surface 71->73."
```

---

## Phase 3 — Frontend model

### Task 7: TypeScript types

**Files:**
- Modify: `src/types.ts` (`TaskItem` line 169-189; `TaskPatch` line 197-209)

**Interfaces:**
- Produces: `TaskItem.description: string | null`; `TaskPatch.description?: string`, `TaskPatch.clearDescription?: boolean` (inherited by `TaskEditorPatch`).

- [ ] **Step 1: Add the fields**

In `src/types.ts`, add to `TaskItem` (after `id`, line 188):

```ts
  id: string | null;
  /** Free-text detail (the `description:` frontmatter field); null when unset. */
  description: string | null;
```

Add to `TaskPatch` (after `order`, line 208):

```ts
  order?: number;
  /** Set the free-text description. */
  description?: string;
  /** Clear the description (mirrors clearDue). */
  clearDescription?: boolean;
```

- [ ] **Step 2: Typecheck**

Run: `npm run build`
Expected: `vue-tsc` passes. Any test/fixture constructing a `TaskItem` literal that the compiler now flags needs `description: null` — fix those in this commit (search: `rg "status:\s*\"" tests/ --type ts -l` for fixture builders, or rely on the tsc errors).

- [ ] **Step 3: Commit**

```bash
git add src/types.ts
git commit -m "feat(ui): add description to TaskItem and TaskPatch types"
```

---

### Task 8: Store — the `taskDetail` view

**Files:**
- Modify: `src/stores/vaults.ts` (imports; view union line 26-37; state ~line 45; `showList` line 222-231; add `openTaskDetail`; `back` line 307-323)
- Test: `tests/vaults-store.test.ts` (create if absent, or add to the existing store test file)

**Interfaces:**
- Consumes: `AggTask` type.
- Produces: `store.view === "taskDetail"`; `store.taskDetailTask: AggTask | null`; `store.openTaskDetail(task: AggTask)`; `back()` returns `taskDetail → tasks` preserving `tasksVaultId`.

- [ ] **Step 1: Write the failing test**

Add a test file `tests/task-detail-nav.test.ts`:

```ts
import { setActivePinia, createPinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";
import { useVaultsStore } from "../src/stores/vaults";
import type { AggTask } from "../src/types";

const task = (over: Partial<AggTask> = {}): AggTask => ({
  path: "/v/Tasks/t.md", title: "T", status: "new", created: "2026-07-01",
  done: false, due: null, scheduled: null, priority: null, tags: [], list: "",
  order: null, id: null, description: null, vaultId: "v1", vaultName: "V", ...over,
});

describe("task detail navigation", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("opens detail keeping the aggregate/per-vault mode, and back() restores it", () => {
    const s = useVaultsStore();
    s.openAllTasks(); // aggregate: tasksVaultId = null
    s.openTaskDetail(task());
    expect(s.view).toBe("taskDetail");
    expect(s.taskDetailTask?.path).toBe("/v/Tasks/t.md");
    expect(s.tasksVaultId).toBeNull(); // NOT cleared by openTaskDetail
    s.back();
    expect(s.view).toBe("tasks");
    expect(s.tasksVaultId).toBeNull(); // aggregate mode preserved
  });

  it("back() from a per-vault detail returns to that vault's tasks", () => {
    const s = useVaultsStore();
    s.openTasks("v1");
    s.openTaskDetail(task({ vaultId: "v1" }));
    s.back();
    expect(s.view).toBe("tasks");
    expect(s.tasksVaultId).toBe("v1");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/task-detail-nav.test.ts`
Expected: FAIL — `openTaskDetail is not a function`.

- [ ] **Step 3: Write minimal implementation**

In `src/stores/vaults.ts`:

Add the import near the top (with the other type imports):
```ts
import type { AggTask } from "../types";
```

Add `"taskDetail"` to the view union (after `"update"`, line 37):
```ts
      | "update"
      | "taskDetail",
```

Add state (after `tasksVaultId`, line 45):
```ts
    tasksVaultId: null as string | null,
    // The task whose detail surface is showing (its own vaultId decides which
    // vault every detail-view write targets). Cleared by showList.
    taskDetailTask: null as AggTask | null,
```

In `showList()` (line 222-231), add the clear (after `this.tasksVaultId = null;`):
```ts
      this.tasksVaultId = null;
      this.taskDetailTask = null;
```

Add the action (after `openAllTasks`, line 261):
```ts
    /** Open a task's detail surface. Deliberately does NOT clear tasksVaultId,
     * so back() returns to the same list mode (aggregate null / a vault id). */
    openTaskDetail(task: AggTask) {
      this.view = "taskDetail";
      this.taskDetailTask = task;
    },
```

In `back()` (line 307-323), add a case before the final `else`:
```ts
      } else if (this.view === "taskDetail") {
        // Return to the tasks list in the mode it came from — openTaskDetail
        // left tasksVaultId intact, so just re-show tasks (showList would
        // wrongly clear it). Tasks.vue remounts and re-fetches on the way in,
        // so any edit/delete/duplicate is reflected.
        this.view = "tasks";
      } else {
        this.showList();
      }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/task-detail-nav.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/stores/vaults.ts tests/task-detail-nav.test.ts
git commit -m "feat(ui): add the taskDetail panel view + openTaskDetail

Detail is a sibling of the tasks list; openTaskDetail keeps tasksVaultId so
back() returns to the same aggregate/per-vault mode."
```

---

### Task 9: Extract `buildTaskPatch`, refactor `TaskEditor`

**Files:**
- Modify: `src/utils/taskFields.ts` (add `TaskDraft` + `buildTaskPatch`)
- Modify: `src/components/TaskEditor.vue` (`buildPatch` line 37-56 → thin caller)
- Test: `tests/task-fields.test.ts` (create or extend)

**Interfaces:**
- Consumes: `dueOf`, `scheduledOf`, `parseTagsInput` (already in `taskFields.ts`).
- Produces: `interface TaskDraft { title; due; scheduled; priority; tags; list: string }`; `buildTaskPatch(task: TaskItem, draft: TaskDraft): TaskEditorPatch`.

- [ ] **Step 1: Write the failing test**

Add `tests/task-fields.test.ts` (or extend it):

```ts
import { describe, expect, it } from "vitest";
import { buildTaskPatch, type TaskDraft } from "../src/utils/taskFields";
import type { TaskItem } from "../src/types";

const base: TaskItem = {
  path: "p", title: "Old", status: "new", created: "2026-07-01", done: false,
  due: "2026-07-10", scheduled: null, priority: null, tags: ["a"], list: "Work",
  order: null, id: null, description: null,
};
const draft = (o: Partial<TaskDraft> = {}): TaskDraft => ({
  title: "Old", due: "2026-07-10", scheduled: "", priority: "normal", tags: "a", list: "Work", ...o,
});

describe("buildTaskPatch", () => {
  it("emits only changed fields", () => {
    expect(buildTaskPatch(base, draft())).toEqual({});
    expect(buildTaskPatch(base, draft({ title: "New" }))).toEqual({ title: "New" });
    expect(buildTaskPatch(base, draft({ due: "" }))).toEqual({ clearDue: true });
    expect(buildTaskPatch(base, draft({ scheduled: "2026-07-15" }))).toEqual({ scheduled: "2026-07-15" });
    expect(buildTaskPatch(base, draft({ list: "Home" }))).toEqual({ list: "Home" });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/task-fields.test.ts`
Expected: FAIL — `buildTaskPatch is not exported`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/utils/taskFields.ts` (it already exports `dueOf`, `scheduledOf`, `parseTagsInput`):

```ts
import type { TaskEditorPatch, TaskItem } from "../types";

/** The editable draft the inline editor and the detail view both hold. */
export interface TaskDraft {
  title: string;
  due: string;
  scheduled: string;
  priority: string; // "high" | "normal" | "low"
  tags: string; // comma/space free-text input
  list: string;
}

/** The changed-fields patch shared by the inline editor and the detail view —
 * only keys whose draft differs from the task are emitted (an emptied date →
 * clear*). The detail view augments the result with description separately. */
export function buildTaskPatch(task: TaskItem, draft: TaskDraft): TaskEditorPatch {
  const patch: TaskEditorPatch = {};
  const title = draft.title.trim();
  if (title && title !== task.title) patch.title = title;
  if (draft.due !== (dueOf(task) ?? "")) {
    if (draft.due === "") patch.clearDue = true;
    else patch.due = draft.due;
  }
  if (draft.scheduled !== (scheduledOf(task) ?? "")) {
    if (draft.scheduled === "") patch.clearScheduled = true;
    else patch.scheduled = draft.scheduled;
  }
  const normPriority =
    task.priority === "high" || task.priority === "low" ? task.priority : "normal";
  if (draft.priority !== normPriority) patch.priority = draft.priority;
  const parsedTags = parseTagsInput(draft.tags);
  if (parsedTags.join(" ") !== task.tags.join(" ")) patch.tags = parsedTags;
  if (draft.list !== task.list) patch.list = draft.list;
  return patch;
}
```

(If `taskFields.ts` does not already import these types, add them to its imports; `dueOf`/`scheduledOf`/`parseTagsInput` are defined in the same file.)

Then simplify `TaskEditor.vue`'s `buildPatch` (line 37-56) to reuse it:

```ts
import { buildTaskPatch, dueOf, parseTagsInput, scheduledOf } from "../utils/taskFields";
// ...
function buildPatch(): TaskEditorPatch {
  return buildTaskPatch(props.task, {
    title: editTitle.value,
    due: editDue.value,
    scheduled: editScheduled.value,
    priority: editPriority.value,
    tags: editTags.value,
    list: editList.value,
  });
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/task-fields.test.ts && npx vitest run tests/ -t "editor"`
Expected: PASS — the new unit test AND the existing TaskEditor tests (the refactor is behavior-preserving).

- [ ] **Step 5: Commit**

```bash
git add src/utils/taskFields.ts src/components/TaskEditor.vue tests/task-fields.test.ts
git commit -m "refactor(ui): extract buildTaskPatch shared by editor + detail view"
```

---

## Phase 4 — Frontend surface

### Task 10: `useTaskDetail` composable

**Files:**
- Create: `src/composables/useTaskDetail.ts`
- Test: `tests/task-detail.test.ts`

**Interfaces:**
- Consumes: `invoke` (update_task/move_task_to_list/delete_task/duplicate_task/open_task/close_panel), `reflectStampedId`/`applyMovedTask`/`MovedTask` (`utils/taskMutations`), `useNotificationsStore`, `useVaultsStore`.
- Produces: `useTaskDetail(task: Ref<AggTask>)` → `{ saving, deleting, save(patch), remove(), duplicate(), openInObsidian() }`.

- [ ] **Step 1: Write the failing test**

Add `tests/task-detail.test.ts`:

```ts
import { setActivePinia, createPinia } from "pinia";
import { mockIPC } from "@tauri-apps/api/mocks";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";
import { useTaskDetail } from "../src/composables/useTaskDetail";
import type { AggTask } from "../src/types";

const task = (o: Partial<AggTask> = {}): AggTask => ({
  path: "/v/Tasks/t.md", title: "T", status: "new", created: "2026-07-01",
  done: false, due: null, scheduled: null, priority: null, tags: [], list: "",
  order: null, id: null, description: null, vaultId: "v1", vaultName: "V", ...o,
});

describe("useTaskDetail", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("save sends description and reflects it locally", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => { calls.push([cmd, args]); return cmd === "update_task" ? null : undefined; });
    const t = ref(task());
    const { save } = useTaskDetail(t);
    await save({ description: "notes" });
    expect(calls[0][0]).toBe("update_task");
    expect(calls[0][1].patch.description).toBe("notes");
    expect(t.value.description).toBe("notes");
  });

  it("remove deletes then navigates back", async () => {
    mockIPC((cmd) => (cmd === "delete_task" ? undefined : undefined));
    const t = ref(task());
    const { remove } = useTaskDetail(t);
    const { useVaultsStore } = await import("../src/stores/vaults");
    const back = vi.spyOn(useVaultsStore(), "back");
    await remove();
    expect(back).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/task-detail.test.ts`
Expected: FAIL — cannot resolve `useTaskDetail`.

- [ ] **Step 3: Write minimal implementation**

Create `src/composables/useTaskDetail.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import { ref, type Ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AggTask, TaskEditorPatch } from "../types";
import { applyMovedTask, type MovedTask, reflectStampedId } from "../utils/taskMutations";

/** The single-task write layer for the detail view. Unlike useTaskActions it
 * owns ONE task (no shared list, no re-sort): edits apply to the passed ref,
 * and the tasks list re-fetches when the user goes back. */
export function useTaskDetail(task: Ref<AggTask>) {
  const notifications = useNotificationsStore();
  const vaults = useVaultsStore();
  // One shared in-flight guard serializes every detail WRITE (save / delete /
  // duplicate): a slow save must not leave delete or duplicate clickable, or a
  // delete could race the save's atomic rename and the save would recreate the
  // deleted file (Codex P2, PR #76). Matches the row-write busy invariant.
  const busy = ref(false);

  async function save(patch: TaskEditorPatch): Promise<boolean> {
    const { list: targetList, ...fieldPatch } = patch;
    const hasFields = Object.keys(fieldPatch).length > 0;
    if (!hasFields && targetList === undefined) return true;
    if (busy.value) return false;
    busy.value = true;
    try {
      if (hasFields) {
        const id = await invoke<string | null>("update_task", {
          id: task.value.vaultId,
          path: task.value.path,
          patch: fieldPatch,
        });
        // Reflect the saved fields on the local copy so the surface stays
        // consistent if the user keeps editing.
        if (fieldPatch.title) task.value.title = fieldPatch.title;
        if (fieldPatch.clearDue) task.value.due = null;
        else if (fieldPatch.due) task.value.due = fieldPatch.due;
        if (fieldPatch.clearScheduled) task.value.scheduled = null;
        else if (fieldPatch.scheduled) task.value.scheduled = fieldPatch.scheduled;
        if (fieldPatch.priority)
          task.value.priority = fieldPatch.priority === "normal" ? null : fieldPatch.priority;
        if (fieldPatch.tags !== undefined) task.value.tags = fieldPatch.tags;
        if (fieldPatch.clearDescription) task.value.description = null;
        else if (fieldPatch.description !== undefined) task.value.description = fieldPatch.description;
        reflectStampedId(task.value, id);
      }
      if (targetList !== undefined && targetList !== task.value.list) {
        const moved = await invoke<MovedTask>("move_task_to_list", {
          id: task.value.vaultId,
          path: task.value.path,
          list: targetList,
        });
        applyMovedTask(task.value, moved);
        task.value.list = targetList;
      }
      void vaults.refreshTaskCount(task.value.vaultId);
      return true;
    } catch (e) {
      notifications.error(String(e));
      logWarning(`task detail save failed: ${String(e)}`);
      return false;
    } finally {
      busy.value = false;
    }
  }

  async function remove(): Promise<void> {
    if (busy.value) return;
    busy.value = true;
    try {
      await invoke("delete_task", { id: task.value.vaultId, path: task.value.path });
      void vaults.refreshTaskCount(task.value.vaultId);
      notifications.success(`Deleted "${task.value.title}".`);
      vaults.back(); // to the tasks list (remounts + re-fetches)
    } catch (e) {
      busy.value = false;
      notifications.error(String(e));
      logWarning(`delete_task failed: ${String(e)}`);
    }
  }

  async function duplicate(): Promise<void> {
    if (busy.value) return;
    busy.value = true;
    try {
      const newPath = await invoke<string>("duplicate_task", {
        id: task.value.vaultId,
        path: task.value.path,
      });
      void vaults.refreshTaskCount(task.value.vaultId);
      const vaultId = task.value.vaultId;
      notifications.notify("success", `Duplicated "${task.value.title}".`, {
        action: {
          label: "Open",
          // Mirror openInObsidian: await the launch, close the panel only on
          // success, and surface a failure — never fire-and-forget the close or
          // swallow the launch error (Codex P2, PR #76).
          run: async () => {
            try {
              await invoke("open_task", { id: vaultId, path: newPath });
              void invoke("close_panel").catch(() => {});
            } catch (e) {
              notifications.error(String(e));
              logWarning(`open_task (duplicate) failed: ${String(e)}`);
            }
          },
        },
      });
    } catch (e) {
      notifications.error(String(e));
      logWarning(`duplicate_task failed: ${String(e)}`);
    } finally {
      busy.value = false;
    }
  }

  async function openInObsidian(): Promise<void> {
    try {
      await invoke("open_task", { id: task.value.vaultId, path: task.value.path });
      void invoke("close_panel").catch(() => {});
    } catch (e) {
      notifications.error(String(e));
      logWarning(`open_task failed: ${String(e)}`);
    }
  }

  return { busy, save, remove, duplicate, openInObsidian };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/task-detail.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/composables/useTaskDetail.ts tests/task-detail.test.ts
git commit -m "feat(ui): add useTaskDetail (single-task save/delete/duplicate)"
```

---

### Task 11: `TaskDetail.vue` component

**Files:**
- Create: `src/components/TaskDetail.vue`
- Test: extend `tests/task-detail.test.ts`

**Interfaces:**
- Consumes: `useTaskDetail` (Task 10), `buildTaskPatch`/`TaskDraft` (Task 9), `TaskListPicker`, `AppButton`/`IconButton` primitives, `dueOf`/`scheduledOf` (`taskFields`), `useTaskLists` (for the list options) OR a passed list set. To keep it self-contained, fetch the vault's lists via `list_task_lists`.
- Produces: `<TaskDetail :task="AggTask" />` — a full-height editable surface with Save, Open-in-Obsidian, Duplicate, and Delete (inline confirm).

- [ ] **Step 1: Write the failing test**

Add to `tests/task-detail.test.ts`:

```ts
import { mount } from "@vue/test-utils";
// ... existing imports ...

it("renders the description and gates delete behind a confirm", async () => {
  const calls: any[] = [];
  mockIPC((cmd, args) => {
    calls.push([cmd, args]);
    if (cmd === "list_task_lists") return [];
    if (cmd === "update_task") return null;
    return undefined;
  });
  const TaskDetail = (await import("../src/components/TaskDetail.vue")).default;
  const wrapper = mount(TaskDetail, { props: { task: task({ description: "hello" }) } });
  await new Promise((r) => setTimeout(r));
  expect((wrapper.find('[data-testid="task-detail-description"]').element as HTMLTextAreaElement).value).toBe("hello");
  // First delete click reveals the confirm; the command is not sent yet.
  await wrapper.find('[data-testid="task-detail-delete"]').trigger("click");
  expect(calls.some((c) => c[0] === "delete_task")).toBe(false);
  await wrapper.find('[data-testid="task-detail-delete-confirm"]').trigger("click");
  await new Promise((r) => setTimeout(r));
  expect(calls.some((c) => c[0] === "delete_task")).toBe(true);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/task-detail.test.ts`
Expected: FAIL — cannot resolve `TaskDetail.vue`.

- [ ] **Step 3: Write minimal implementation**

Create `src/components/TaskDetail.vue`:

```vue
<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref, toRef } from "vue";

import { useTaskDetail } from "../composables/useTaskDetail";
import type { AggTask, TaskEditorPatch } from "../types";
import { buildTaskPatch, dueOf, scheduledOf } from "../utils/taskFields";
import TaskListPicker from "./TaskListPicker.vue";

// The full-height detail surface: a roomy home for one task. It holds its own
// draft (seeded from the passed task, which carries its own vaultId so writes
// target the right vault in both per-vault and aggregate modes), edits through
// useTaskDetail, and offers the lifecycle verbs. The list re-fetches when the
// user goes back, so this surface never syncs to the list's in-memory array.
const props = defineProps<{ task: AggTask }>();
const taskRef = toRef(props, "task");
const { busy, save, remove, duplicate, openInObsidian } = useTaskDetail(taskRef);

const normPriority = (p: string | null) => (p === "high" || p === "low" ? p : "normal");

const draftTitle = ref(props.task.title);
const draftDescription = ref(props.task.description ?? "");
const draftDue = ref(dueOf(props.task) ?? "");
const draftScheduled = ref(scheduledOf(props.task) ?? "");
const draftPriority = ref(normPriority(props.task.priority));
const draftTags = ref(props.task.tags.join(", "));
const draftList = ref(props.task.list);

// The vault's lists for the picker (self-contained fetch, empty on failure).
const lists = ref<string[]>([]);
onMounted(async () => {
  try {
    lists.value = await invoke<string[]>("list_task_lists", { id: props.task.vaultId });
  } catch {
    lists.value = [];
  }
});

const titleValid = computed(() => draftTitle.value.trim().length > 0);

function currentPatch(): TaskEditorPatch {
  const patch = buildTaskPatch(props.task, {
    title: draftTitle.value,
    due: draftDue.value,
    scheduled: draftScheduled.value,
    priority: draftPriority.value,
    tags: draftTags.value,
    list: draftList.value,
  });
  // Description lives only here — augment the shared patch.
  if (draftDescription.value !== (props.task.description ?? "")) {
    if (draftDescription.value.trim() === "") patch.clearDescription = true;
    else patch.description = draftDescription.value;
  }
  return patch;
}

const dirty = computed(() => Object.keys(currentPatch()).length > 0);

async function onSave() {
  if (!titleValid.value || busy.value) return;
  await save(currentPatch());
}

// Two-step permanent-delete confirm (GAP-27 class: focus the confirm on open,
// Escape steps back one level and stops propagation so it can't bubble into the
// panel's own close handler).
const confirming = ref(false);
const confirmBtn = ref<HTMLButtonElement | null>(null);
async function openConfirm() {
  confirming.value = true;
  await new Promise((r) => setTimeout(r));
  confirmBtn.value?.focus();
}
function onDeleteKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.stopPropagation();
    confirming.value = false;
  }
}
</script>

<template>
  <div class="flex flex-col gap-3 text-fg" @keydown="onDeleteKeydown">
    <input
      v-model="draftTitle"
      data-testid="task-detail-title"
      type="text"
      aria-label="Task title"
      class="rounded-control border border-white/10 bg-white/5 px-2 py-1.5 text-sm font-semibold text-fg focus:border-focus focus:outline-none"
    >
    <label class="flex flex-col gap-1">
      <span class="text-micro uppercase tracking-wider text-fg-subtle">Description</span>
      <textarea
        v-model="draftDescription"
        data-testid="task-detail-description"
        rows="5"
        aria-label="Description"
        placeholder="Add context, links, or notes…"
        class="resize-y rounded-control border border-white/10 bg-white/5 px-2 py-1.5 text-sm text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
      />
    </label>
    <div class="flex items-center gap-1">
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">Due</span>
      <input v-model="draftDue" data-testid="task-detail-due" type="date" aria-label="Due date"
        class="min-w-0 flex-1 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg focus:border-focus focus:outline-none">
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">Do</span>
      <input v-model="draftScheduled" data-testid="task-detail-scheduled" type="date" aria-label="Do date"
        class="min-w-0 flex-1 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg focus:border-focus focus:outline-none">
    </div>
    <div class="flex items-center gap-1" role="radiogroup" aria-label="Priority">
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">Priority</span>
      <button v-for="p in ['high', 'normal', 'low']" :key="p" type="button" role="radio"
        :data-testid="`task-detail-priority-${p}`" :aria-checked="draftPriority === p"
        class="cursor-pointer rounded-control border px-2 py-0.5 text-xs capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="draftPriority === p ? 'border-violet-400 bg-accent/20 text-fg' : 'border-white/10 bg-white/5 text-fg-secondary hover:bg-white/10'"
        @click="draftPriority = p">{{ p }}</button>
    </div>
    <input v-model="draftTags" data-testid="task-detail-tags" type="text" placeholder="#tags" aria-label="Tags"
      class="rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none">
    <div class="flex items-center gap-1">
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">List</span>
      <TaskListPicker v-model="draftList" :lists="lists" :allow-create="false" aria-label="Task list" data-testid="task-detail-list" />
    </div>

    <div class="flex items-center gap-2 pt-1">
      <button type="button" data-testid="task-detail-save" :disabled="!titleValid || !dirty || busy"
        class="cursor-pointer rounded-control bg-accent-strong/80 px-3 py-1 text-xs font-semibold text-white hover:bg-accent-strong focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
        @click="onSave">Save</button>
      <button type="button" data-testid="task-detail-open"
        class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-3 py-1 text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="openInObsidian">Open in Obsidian</button>
      <button type="button" data-testid="task-detail-duplicate" :disabled="busy"
        class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-3 py-1 text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
        @click="duplicate">Duplicate</button>
      <div class="ml-auto flex items-center gap-1">
        <template v-if="confirming">
          <span class="text-micro text-fg-muted">Delete permanently?</span>
          <button type="button" data-testid="task-detail-delete-cancel"
            class="cursor-pointer rounded-control px-2 py-1 text-xs text-fg-muted hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
            @click="confirming = false">Cancel</button>
          <button ref="confirmBtn" type="button" data-testid="task-detail-delete-confirm" :disabled="busy"
            class="cursor-pointer rounded-control bg-danger/80 px-2 py-1 text-xs font-semibold text-danger-fg hover:bg-danger focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-50"
            @click="remove">Delete</button>
        </template>
        <button v-else type="button" data-testid="task-detail-delete" :disabled="busy"
          class="cursor-pointer rounded-control border border-danger/40 px-3 py-1 text-xs text-danger-fg hover:bg-danger/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
          @click="openConfirm">Delete</button>
      </div>
    </div>
  </div>
</template>
```

(Note: if the `bg-danger`/`text-danger-fg` token classes differ in `src/style.css`, match the exact token names used by the existing `Banner`/danger call sites — grep `text-danger-fg` for the canonical spelling.)

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/task-detail.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/TaskDetail.vue tests/task-detail.test.ts
git commit -m "feat(ui): add the TaskDetail surface (description + verbs)

Full-height task home: editable title/description/metadata, Save, Open in
Obsidian, Duplicate, and a two-step permanent-delete confirm."
```

---

### Task 12: Wire opening — ActionPanel, TaskRow, Tasks.vue

**Files:**
- Modify: `src/components/ActionPanel.vue` (import; `title` computed line 74-78; transition branch before line 372)
- Modify: `src/components/TaskRow.vue` (`open` emit line 37 + line 99)
- Modify: `src/components/Tasks.vue` (add `onOpenTask` handler; `@open` line 515)
- Test: `tests/task-open-routing.test.ts`

**Interfaces:**
- Consumes: `store.openTaskDetail` (Task 8), `openInObsidian` (already destructured in Tasks.vue), `TaskDetail.vue` (Task 11).
- Produces: title-click → detail view; Ctrl/⌘-click → Obsidian; `taskDetail` renders in the panel.

- [ ] **Step 1: Write the failing test**

Add `tests/task-open-routing.test.ts` — a focused test on the row's modifier routing (mount `TaskRow`, assert the emit carries the event; the routing lives in `Tasks.vue` but the emit contract is what changes):

```ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import TaskRow from "../src/components/TaskRow.vue";
import type { AggTask } from "../src/types";

const task = (): AggTask => ({
  path: "p", title: "T", status: "new", created: "2026-07-01", done: false,
  due: null, scheduled: null, priority: null, tags: [], list: "", order: null,
  id: null, description: null, vaultId: "v1", vaultName: "V",
});

describe("TaskRow open emit", () => {
  it("emits open with the MouseEvent so the container can route ctrl/meta", async () => {
    const wrapper = mount(TaskRow, {
      props: { task: task(), busy: false, isAggregate: false, editing: false },
    });
    await wrapper.find('[data-testid="task-open"]').trigger("click");
    const ev = wrapper.emitted("open")?.[0]?.[0];
    expect(ev).toBeInstanceOf(MouseEvent);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/task-open-routing.test.ts`
Expected: FAIL — the emitted payload is `undefined`, not a `MouseEvent`.

- [ ] **Step 3: Write minimal implementation**

In `src/components/TaskRow.vue`, change the `open` emit type (line 37):
```ts
  (e: "open", ev: MouseEvent): void;
```
and the button's click (line 99):
```vue
          @click="$emit('open', $event)"
```

In `src/components/ActionPanel.vue`:
- Import `TaskDetail` with the other view imports:
```ts
import TaskDetail from "./TaskDetail.vue";
```
- Extend the `title` computed (line 74-78):
```ts
const title = computed(() => {
  if (view.value === "taskDetail") return store.taskDetailTask?.title ?? "Task";
  return view.value === "tasks" && store.tasksVaultId === null
    ? "All tasks"
    : (VIEW_TITLES[view.value] ?? "Vaults");
});
```
- Add a transition branch immediately before the final `v-else` list branch (before line 372):
```vue
      <div
        v-else-if="view === 'taskDetail'"
        key="taskDetail"
        class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
      >
        <TaskDetail
          v-if="store.taskDetailTask"
          :task="store.taskDetailTask"
        />
      </div>
```

In `src/components/Tasks.vue`, add the routing handler in `<script setup>` (the store is already `const vaultsStore = useVaultsStore()` at line 31 and `openInObsidian` is destructured at line 105):
```ts
function onOpenTask(task: AggTask, ev: MouseEvent) {
  // Plain click opens the in-panel detail home; Ctrl/⌘-click keeps the old
  // muscle memory and jumps straight to Obsidian.
  if (ev.ctrlKey || ev.metaKey) openInObsidian(task);
  else vaultsStore.openTaskDetail(task);
}
```
and change the row wiring (line 515):
```vue
              @open="onOpenTask(task, $event)"
```

- [ ] **Step 4: Run tests + full frontend gate**

Run: `npx vitest run tests/task-open-routing.test.ts && npm run build`
Expected: PASS + typecheck clean.

Then the full frontend gate:
Run: `npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage`
Expected: PASS. If `check:loc` fails because `Tasks.vue` grew, revert incidental additions there (only `onOpenTask` + the one-line `@open` change are allowed). If a coverage floor rose, re-run `npm run test:coverage -- --update` per the repo's ratchet and stage the baseline.

- [ ] **Step 5: Commit**

```bash
git add src/components/ActionPanel.vue src/components/TaskRow.vue src/components/Tasks.vue tests/task-open-routing.test.ts vite.config.ts scripts/loc-baseline.json scripts/quality-baseline.json
git commit -m "feat(ui): title click opens the Task Detail surface

Plain click opens the in-panel detail home (Obsidian one click away inside
it); Ctrl/Cmd-click preserves the direct-to-Obsidian jump."
```

(Only stage the baseline files if the gate actually updated them.)

---

## Phase 5 — Docs & baselines

### Task 13: Documentation

**Files:**
- Modify: `AGENTS.md` (tasks-domain section; IPC table `task_commands.rs` row + count 71→73; `update_task`/`list_tasks` field notes; frontend-state `view` union)
- Modify: `CONTEXT.md` (add **Description** and **Task Detail** terms)
- Modify: `docs/prds/task-management.md` (mark detail/description/delete/duplicate shipped; Task Editing "delete/duplicate still open" → shipped)
- Modify: `docs/use-cases/per-vault-task-list.md` (detail surface + description + delete/duplicate)
- Modify: `docs/Gaps.md` (new GAP entries)

**Interfaces:** none (docs only).

- [ ] **Step 1: Update AGENTS.md**

In the tasks-domain section, document: the `taskDetail` view (a task's home, opened by title click, Ctrl/⌘-click still → Obsidian); the `description` field (an escaped single-line `description:` scalar via `yaml_quote_multiline`, read `#`-tolerant + newline-decoding via `description_field`, reserved in both key-sets); and delete/duplicate as new sanctioned task writes (delete = the first vault-file removal, containment-gated; duplicate = faithful collision-safe copy, fresh/stripped id). In the IPC table's `task_commands.rs` row add `delete_task` *(async)* and `duplicate_task` *(async)* and note `update_task`'s patch now carries `description`/`clearDescription` and `list_tasks` rows carry `description`. Update the "All 71 commands" line to **73**. In the frontend-state section add `taskDetail` (+ `taskDetailTask`) to the documented `view` union and note it keeps `tasksVaultId` for back().

- [ ] **Step 2: Update CONTEXT.md** — add **Description** (a Task's free-text detail; a frontmatter property of the Task document, distinct from the note **body**, which the app still never edits, and from a **Todo**) and **Task Detail** (the in-panel home surface for a single Task).

- [ ] **Step 3: Update the PRD + use case** — in `docs/prds/task-management.md`, the Status/Roadmap lines: Task Editing gains delete + duplicate (shipped); note the Task Detail surface + `description` field. In the use case, add the detail/description/delete/duplicate flow.

- [ ] **Step 4: Add Gaps entries** — a new GAP for the **permanent-delete departure** (Low; the app's first destructive vault write, hardened by the confirm; recoverable trash named as the future refinement) and a GAP for the **hand-authored block-scalar description** limitation (Low; `description: |`/`>` reads as its raw marker and editing it via the detail view may leave orphaned continuation lines — the app only ever writes escaped single-line scalars; extending `set_fields` to consume block scalars is the future hardening). Also note the **`description` in `list_tasks` payload** trade-off (every row carries its description; a `get_task` command is the fallback if it ever bites).

- [ ] **Step 5: Commit**

```bash
git add AGENTS.md CONTEXT.md docs/prds/task-management.md docs/use-cases/per-vault-task-list.md docs/Gaps.md
git commit -m "docs(tasks): Task Detail surface, description, delete/duplicate

AGENTS tasks-domain + IPC table (71->73), CONTEXT terms (Description, Task
Detail), PRD/use-case shipped status, and Gaps (permanent-delete departure,
block-scalar description limit)."
```

---

## Self-Review

**1. Spec coverage** — every spec section maps to a task:
- Task Detail view + `tasksVaultId` gotcha + open-by-title + Ctrl-click → Tasks 8, 11, 12.
- `description` field (escaped single-line scalar, both key-sets, DTO, defensive read) → Tasks 1, 2, 5.
- Delete (permanent + confirm) → Tasks 3, 6, 11 (confirm UI).
- Duplicate (faithful, fresh/stripped id, collision-safe) → Tasks 4, 6, 10.
- Detail contents (all metadata + description + verbs + Open-in-Obsidian; shared `buildTaskPatch`; `useTaskDetail`) → Tasks 9, 10, 11.
- Architecture footprint (+2 commands, DTO/patch fields, new files, Tasks.vue unchanged in size) → Tasks 5, 6, 7, 12.
- Docs + Gaps → Task 13.

**2. Placeholder scan** — no "TBD/handle appropriately"; every code step shows complete code. The two "match the exact token/idiom" notes (danger token spelling; a fixture that needs `description: null`) are compiler/grep-verifiable, not logic gaps.

**3. Type consistency** — `description: Option<String>` (Rust) ↔ `description: string | null` (TS); `TaskPatchDto.clear_description` ↔ `TaskPatch.clearDescription`; `duplicate_task` returns a path `String` (matches `useTaskDetail.duplicate`'s `invoke<string>`); `yaml_quote_multiline`/`yaml_unquote_multiline` names consistent across Tasks 1/2/5; `buildTaskPatch(task, draft)` signature consistent across Tasks 9/11; `useTaskDetail(task: Ref<AggTask>)` return shape consistent across Tasks 10/11.

**Deviation from spec (noted):** `duplicate_task` returns the landed **path** (not a full `TaskDto`) — the detail view re-fetches on back and only needs the path for the "Open" toast action, avoiding a DTO-from-file helper. Recorded in Task 13's Gaps note.
