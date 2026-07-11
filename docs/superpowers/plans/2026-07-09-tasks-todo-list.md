# Tasks Todo-List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drive the per-vault task list to a proper todo list: click-to-open in Obsidian, `due`/`priority` frontmatter, date-bucket grouping, a title filter, an inline row editor, and add-row due/priority controls.

**Architecture:** Core gains a multi-key surgical frontmatter writer (`set_fields`, generalizing `set_status`), due/priority on `TaskItem`/`render_task`, and a clock-free sort extension. The shell adds `open_task` (read-only `obsidian://` handoff) and `update_task` (validated single-write patch) and extends `add_task`. The frontend reworks `Tasks.vue`: buckets computed client-side from the local date, row chips/dot, inline editor, add options, filter.

**Tech Stack:** Rust (`vault_buddy_core` + Tauri shell), Vue 3 + Pinia + Tailwind 4, Vitest (`mockIPC`, fake `Date`), `cargo test`.

## Global Constraints

- **Frontmatter schema:** `due: YYYY-MM-DD` (plain, no time); `priority: high|normal|low` with absent = normal — `normal` is NEVER written (set → remove the line). Clearing a due date removes the line.
- **Graceful reads:** unparseable `due` buckets/sorts as "no date"; unknown `priority` renders/sorts as normal. Never an error.
- **Surgical writes only:** every frontmatter edit goes through `set_fields` → atomic temp+fsync+REPLACING rename; only the named lines change, everything else byte-for-byte (CRLF included). No new write path; `open_task` is read-only and `uri::launch`-logged.
- **Core stays clock-free:** sort must not need "today"; bucketing (needs the local date) lives in the frontend. Frontend dates are LOCAL (`new Date()` fields, never UTC/ISO slicing).
- **Rust↔TS contract:** `open_task {id, path}`; `update_task {id, path, patch: {title?, due?, clearDue?, priority?}}`; `add_task {id, title, due?, priority?}`; `TaskDto`/`TaskItem` gain `due: string|null`, `priority: string|null` (camelCase).
- **The shell crate (`src-tauri/src/*.rs`) does not compile on Linux.** Mirror existing command patterns, run `cd src-tauri && cargo fmt --check`, rely on CI's `windows-app`/`linux-app` jobs.
- **Commits:** Conventional Commits (`feat(tasks)`, `feat(ui)`). Git author `Claude <noreply@anthropic.com>` (`git config user.email noreply@anthropic.com && git config user.name Claude`).
- **TDD:** failing test first. Regression tests name their failure mode in a comment.
- Spec: `docs/superpowers/specs/2026-07-09-tasks-todo-list-design.md`.

---

### Task 1: Core — `set_fields` multi-key surgical writer + `update_task_fields`

**Files:**
- Modify: `src-tauri/core/src/tasks.rs` (replace `set_status` body ~line 188; extract disk write from `set_task_status` ~line 248)
- Modify: `src-tauri/core/src/capture_note.rs` (`pub(crate) fn yaml_quote` → `pub fn yaml_quote`, ~line 39)
- Test: `src-tauri/core/src/tasks.rs` (`mod tests`)

**Interfaces:**
- Consumes: existing `is_task`, `write_atomic_replacing`.
- Produces:
  - `tasks::set_fields(content: &str, updates: &[(&str, Option<&str>)]) -> Option<String>` — `Some(v)` rewrites/inserts `key: v`; `None` removes the line; refuses non-task/unclosed fence.
  - `tasks::update_task_fields(root: &Path, path: &Path, updates: &[(&str, Option<&str>)]) -> Result<(), String>` — canonicalize+containment+read+`set_fields`+atomic replace.
  - `tasks::set_status` / `tasks::set_task_status` unchanged signatures, now thin wrappers.
  - `capture_note::yaml_quote(value: &str) -> String` now `pub` (shell quotes the title patch with it).

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src-tauri/core/src/tasks.rs`:

```rust
#[test]
fn set_fields_updates_multiple_keys_in_one_pass() {
    let doc = "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-08\ndue: 2026-07-10\n---\n\nbody\n";
    let out = set_fields(
        doc,
        &[("title", Some("\"B\"")), ("due", Some("2026-07-20")), ("priority", Some("high"))],
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
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri/core && cargo test tasks::tests::set_fields`
Expected: FAIL — `set_fields` not found.

- [ ] **Step 3: Implement**

In `src-tauri/core/src/capture_note.rs` change `pub(crate) fn yaml_quote` to `pub fn yaml_quote` (the shell's `update_task` quotes the title patch with it).

In `src-tauri/core/src/tasks.rs`, replace the body of `set_status` and add `set_fields` above it:

```rust
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
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
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
                match value {
                    Some(v) => {
                        let ending = &line[trimmed.len()..]; // "\r\n", "\n", or ""
                        out.push_str(&format!("{key}: {v}{ending}"));
                    }
                    None => {} // drop the line (its newline goes with it)
                }
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
```

Delete the old `set_status` body (the whole hand-rolled loop). Then extract the disk write: replace `set_task_status` with:

```rust
/// Apply a surgical frontmatter patch to a task file on disk. Canonicalizes
/// `root` and `path` and requires containment — a lexical check can't see
/// through a symlink at the file or folder — then reads, applies `set_fields`,
/// and writes atomically (hidden `create_new` temp + fsync + REPLACING
/// rename). Replacing is correct here: the target is the `type: Task` file we
/// just read and are editing in place, touching only the named lines.
pub fn update_task_fields(
    root: &Path,
    path: &Path,
    updates: &[(&str, Option<&str>)],
) -> Result<(), String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    let updated = set_fields(&content, updates).ok_or(
        "Task frontmatter could not be updated (not a type: Task document, or its frontmatter is malformed)",
    )?;
    crate::capture_note::write_atomic_replacing(&canon_path, &updated)
        .map_err(|e| format!("Cannot save task: {e}"))
}

/// Set a task's `status:` frontmatter on disk (see `update_task_fields`).
pub fn set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String> {
    update_task_fields(root, path, &[("status", Some(new_status))])
}
```

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cd src-tauri/core && cargo test tasks:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS — including EVERY pre-existing `set_status_*` and `set_task_status_*` test (they now exercise the wrappers; they are the regression harness for the rewrite).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks.rs src-tauri/core/src/capture_note.rs
git commit -m "feat(tasks): multi-key surgical frontmatter writer (set_fields)"
```

---

### Task 2: Core — due/priority fields, render/create, clock-free sort

**Files:**
- Modify: `src-tauri/core/src/tasks.rs` (`TaskItem` ~line 66; `render_task` ~line 49; `create_task` ~line 59; `collect_tasks` field reads; `list_tasks` sort)
- Test: `src-tauri/core/src/tasks.rs` (`mod tests`)

**Interfaces:**
- Produces:
  - `TaskItem` gains `pub due: Option<String>`, `pub priority: Option<String>`.
  - `render_task(title: &str, created: &str, due: Option<&str>, priority: Option<&str>) -> String`
  - `create_task(root: &Path, title: &str, today: &str, due: Option<&str>, priority: Option<&str>) -> std::io::Result<PathBuf>`
  - `pub fn is_valid_due(s: &str) -> bool` — exactly `YYYY-MM-DD` digits/hyphens (no calendar check).
  - `pub fn priority_rank(p: Option<&str>) -> u8` — high=0, normal/unknown/absent=1, low=2.
  - New `list_tasks` order: open first → valid due asc (no/invalid due last) → priority rank → newest `created` → title; done tasks by newest `created` → title.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn render_includes_due_and_priority_only_when_present() {
    let plain = render_task("A", "2026-07-09", None, None);
    assert_eq!(
        plain,
        "---\ntype: Task\nstatus: new\ntitle: \"A\"\ncreated: 2026-07-09\n---\n\n"
    ); // byte-identical to the pre-due/priority output
    let full = render_task("A", "2026-07-09", Some("2026-07-15"), Some("high"));
    assert!(full.contains("created: 2026-07-09\ndue: 2026-07-15\npriority: high\n---\n"));
}

#[test]
fn list_tasks_reads_due_and_priority() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "t.md",
        "---\ntype: Task\nstatus: new\ntitle: \"T\"\ncreated: 2026-07-08\ndue: 2026-07-15\npriority: high\n---\n",
    );
    let items = list_tasks(root);
    assert_eq!(items[0].due.as_deref(), Some("2026-07-15"));
    assert_eq!(items[0].priority.as_deref(), Some("high"));
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
fn list_tasks_sorts_by_due_then_priority_then_created() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mk = |name: &str, extra: &str, title: &str, created: &str| {
        write(
            root,
            name,
            &format!("---\ntype: Task\nstatus: new\ntitle: \"{title}\"\ncreated: {created}\n{extra}---\n"),
        )
    };
    mk("a.md", "", "NoDue", "2026-07-09");
    mk("b.md", "due: 2026-07-20\n", "Later", "2026-07-01");
    mk("c.md", "due: 2026-07-10\n", "Sooner", "2026-07-01");
    mk("d.md", "due: 2026-07-10\npriority: high\n", "SoonerHigh", "2026-07-01");
    mk("e.md", "due: tomorrow\n", "BadDue", "2026-07-08"); // unparseable → no-date
    write(
        root,
        "z.md",
        "---\ntype: Task\nstatus: done\ntitle: \"Done\"\ncreated: 2026-07-09\ndue: 2026-07-01\n---\n",
    );
    let titles: Vec<String> = list_tasks(root).into_iter().map(|t| t.title).collect();
    // dated (due asc, high before normal) → no-date (created desc) → done last
    // (done ignores its overdue due — done sorts by created).
    assert_eq!(
        titles,
        vec!["SoonerHigh", "Sooner", "Later", "NoDue", "BadDue", "Done"]
    );
}
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri/core && cargo test tasks::`
Expected: FAIL — `render_task` arity, missing `TaskItem` fields, `is_valid_due` not found.

- [ ] **Step 3: Implement**

`render_task` and `create_task`:

```rust
/// A `type: Task` document with an empty body. `type`/`status`/`created` (and
/// the optional `due`/`priority`) are simple unquoted scalars; the
/// user-supplied title is quoted so a colon or quote can't break the
/// frontmatter. `due`/`priority` lines are written only when present — absent
/// priority means normal, and a bare `due:` is never emitted.
pub fn render_task(
    title: &str,
    created: &str,
    due: Option<&str>,
    priority: Option<&str>,
) -> String {
    let mut extra = String::new();
    if let Some(d) = due {
        extra.push_str(&format!("due: {d}\n"));
    }
    if let Some(p) = priority {
        extra.push_str(&format!("priority: {p}\n"));
    }
    format!(
        "---\ntype: Task\nstatus: new\ntitle: {}\ncreated: {created}\n{extra}---\n\n",
        yaml_quote(title)
    )
}

pub fn create_task(
    root: &Path,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    crate::capture_note::write_note_collision_safe(&target, &render_task(title, today, due, priority))
}
```

`TaskItem` + `collect_tasks` field reads (after the archived skip, before the push):

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct TaskItem {
    pub path: PathBuf,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
    pub due: Option<String>,
    pub priority: Option<String>,
}
```

```rust
        let created = note_field(&content, "created").unwrap_or_default();
        let due = note_field(&content, "due");
        let priority = note_field(&content, "priority");
        let done = status == "done";
        out.push(TaskItem {
            path,
            title,
            status,
            created,
            done,
            due,
            priority,
        });
```

Validation/rank helpers + the new sort in `list_tasks`:

```rust
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

/// Sort tier for a priority value: high first, low last, anything else
/// (normal, absent, hand-authored unknown) in the middle.
pub fn priority_rank(p: Option<&str>) -> u8 {
    match p {
        Some("high") => 0,
        Some("low") => 2,
        _ => 1,
    }
}

/// (has-no-valid-due, due) — tuple compare puts valid dues first, ascending;
/// an unparseable hand-authored due sorts with the undated.
fn due_key(t: &TaskItem) -> (bool, &str) {
    match t.due.as_deref().filter(|d| is_valid_due(d)) {
        Some(d) => (false, d),
        None => (true, ""),
    }
}
```

Replace the `out.sort_by` in `list_tasks`:

```rust
    // Open first. Open tasks: due ascending (no/invalid due last), then
    // priority tier, then newest created, then title. Done tasks ignore due —
    // newest created first, then title. Clock-free: "overdue"/"today" need a
    // clock, so bucketing is the frontend's job, not the sort's.
    out.sort_by(|a, b| {
        a.done.cmp(&b.done).then_with(|| {
            if a.done {
                b.created.cmp(&a.created).then_with(|| a.title.cmp(&b.title))
            } else {
                due_key(a)
                    .cmp(&due_key(b))
                    .then_with(|| {
                        priority_rank(a.priority.as_deref())
                            .cmp(&priority_rank(b.priority.as_deref()))
                    })
                    .then_with(|| b.created.cmp(&a.created))
                    .then_with(|| a.title.cmp(&b.title))
            }
        })
    });
```

Update existing call sites in this file's tests: every `create_task(&root, "Buy milk", "2026-07-08")` becomes `create_task(&root, "Buy milk", "2026-07-08", None, None)`; `render_task("Buy milk", "2026-07-08")` etc. become `render_task(..., None, None)` (there are ~6 call sites — `cargo test` finds them all as compile errors).

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cd src-tauri/core && cargo test tasks:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS (all existing sort tests still pass — undated open tasks keep the created-desc/title order).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks.rs
git commit -m "feat(tasks): due/priority fields with clock-free due-then-priority sort"
```

---

### Task 3: Shell — `open_task`, `update_task`, extended `add_task`

**Files:**
- Modify: `src-tauri/src/task_commands.rs` (`TaskDto` ~line 60; `add_task` ~line 119; add `open_task` + `update_task`)
- Modify: `src-tauri/src/lib.rs` (register `open_task`, `update_task` after `task_commands::count_open_tasks,` ~line 293)

**Interfaces:**
- Consumes: `tasks::{create_task, update_task_fields, is_valid_due}`, `capture_note::yaml_quote`, `uri::{launch, open_file_uri, vault_relative_no_ext}` (all from Task 1/2 + existing core).
- Produces (IPC): `open_task(id, path) -> Result<(), String>`; `update_task(id, path, patch: TaskPatchDto) -> Result<(), String>` with `TaskPatchDto { title?: String, due?: String, clear_due: bool (camelCase clearDue), priority?: String }`; `add_task(id, title, due: Option<String>, priority: Option<String>) -> Result<TaskDto, String>`; `TaskDto` gains `due: Option<String>`, `priority: Option<String>`.

*Shell crate: not compilable on Linux — mirror the existing command idioms; `cargo fmt --check` is the local gate.*

- [ ] **Step 1: Widen `TaskDto`**

```rust
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub path: String,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
    pub due: Option<String>,
    pub priority: Option<String>,
}

impl TaskDto {
    fn from_item(t: tasks::TaskItem) -> Self {
        Self {
            path: t.path.to_string_lossy().into_owned(),
            title: t.title,
            status: t.status,
            created: t.created,
            done: t.done,
            due: t.due,
            priority: t.priority,
        }
    }
}
```

- [ ] **Step 2: Add shared validation + extend `add_task`**

Add near the top of the command section:

```rust
/// Validate an optional due date for a write. Ok(None) when absent.
fn validated_due(due: Option<String>) -> Result<Option<String>, String> {
    match due {
        Some(d) if !tasks::is_valid_due(&d) => {
            Err(format!("Due date must be YYYY-MM-DD, got: {d}"))
        }
        other => Ok(other),
    }
}

/// Validate an optional priority for a write. `normal` normalizes to None —
/// absent means normal, and a `priority: normal` line is never written.
fn validated_priority(priority: Option<String>) -> Result<Option<String>, String> {
    match priority.as_deref() {
        None | Some("normal") => Ok(None),
        Some("high") | Some("low") => Ok(priority),
        Some(other) => Err(format!("Unknown task priority: {other}")),
    }
}
```

Replace `add_task`'s signature and the `create_task` call + returned DTO:

```rust
#[tauri::command]
pub fn add_task(
    id: String,
    title: String,
    due: Option<String>,
    priority: Option<String>,
) -> Result<TaskDto, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A task needs a title.".to_string());
    }
    let due = validated_due(due)?;
    let priority = validated_priority(priority)?;
    // ... (existing vault_path/root resolution, is_dir guard,
    //      assert_path_inside_vault, create_dir_all, `today` — unchanged) ...
    let path = tasks::create_task(&root, title, &today, due.as_deref(), priority.as_deref())
        .map_err(|e| format!("Could not create task: {e}"))?;
    Ok(TaskDto {
        path: path.to_string_lossy().into_owned(),
        title: title.to_string(),
        status: "new".to_string(),
        created: today,
        done: false,
        due,
        priority,
    })
}
```

- [ ] **Step 3: Add `update_task` and `open_task`**

```rust
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatchDto {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub clear_due: bool,
    #[serde(default)]
    pub priority: Option<String>,
}

/// Apply an inline-editor patch to a task: rename, set/clear the due date,
/// set the priority — validated up front, then ONE surgical multi-key
/// frontmatter write (title quoted here; `priority: normal` and a cleared due
/// remove their lines). An empty patch is a no-op Ok.
#[tauri::command]
pub fn update_task(id: String, path: String, patch: TaskPatchDto) -> Result<(), String> {
    let mut updates: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(title) = &patch.title {
        let t = title.trim();
        if t.is_empty() {
            return Err("A task needs a title.".to_string());
        }
        updates.push(("title", Some(capture_note::yaml_quote(t))));
    }
    if patch.clear_due {
        updates.push(("due", None));
    } else if patch.due.is_some() {
        updates.push(("due", validated_due(patch.due.clone())?));
    }
    if patch.priority.is_some() {
        updates.push(("priority", validated_priority(patch.priority.clone())?));
    }
    if updates.is_empty() {
        return Ok(());
    }
    let (vault_path, root) = tasks_root_for(&id)?;
    if root.exists() {
        capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    }
    let refs: Vec<(&str, Option<&str>)> =
        updates.iter().map(|(k, v)| (*k, v.as_deref())).collect();
    tasks::update_task_fields(&root, Path::new(&path), &refs)
}

/// Open a task document in Obsidian from its list row. Read-only: canonical
/// containment inside the vault's tasks root (list_tasks hands out canonical
/// paths, so the vault-relative part is computed against the CANONICAL vault
/// path or strip_prefix would fail on Windows' \\?\ form), then an
/// `obsidian://open` launch, logged by `uri::launch` like every vault open.
#[tauri::command]
pub fn open_task(id: String, path: String) -> Result<(), String> {
    let (vault_path, root) = tasks_root_for(&id)?;
    let canon_root = std::fs::canonicalize(&root)
        .map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path = std::fs::canonicalize(Path::new(&path))
        .map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let canon_vault = std::fs::canonicalize(&vault_path)
        .map_err(|e| format!("Cannot resolve vault folder: {e}"))?;
    let rel = uri::vault_relative_no_ext(&canon_path, &canon_vault)
        .ok_or_else(|| format!("task is outside its vault: {path}"))?;
    uri::launch(&uri::open_file_uri(&id, &rel))
}
```

Add the needed imports at the top of `task_commands.rs`: extend the existing `use vault_buddy_core::{capture_config, capture_paths, discovery, tasks};` to `use vault_buddy_core::{capture_config, capture_note, capture_paths, discovery, tasks, uri};`.

In `src-tauri/src/lib.rs` after `task_commands::count_open_tasks,`:

```rust
            task_commands::count_open_tasks,
            task_commands::open_task,
            task_commands::update_task,
        ])
```

- [ ] **Step 4: Format check + commit**

Run: `cd src-tauri && cargo fmt --check`
Expected: PASS (no diff). Compile is verified by CI's `windows-app`/`linux-app` jobs.

```bash
git add src-tauri/src/task_commands.rs src-tauri/src/lib.rs
git commit -m "feat(tasks): open_task, update_task patch command, add_task due/priority"
```

---

### Task 4: Frontend — types, row display (due chip, priority dot), open on title click

**Files:**
- Modify: `src/types.ts` (`TaskItem` ~line 123; add `TaskPatch`)
- Modify: `src/components/Tasks.vue`
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes (IPC): `open_task {id, path}` (Task 3).
- Produces: `TaskItem` TS shape `{path, title, status, created, done, due: string|null, priority: string|null}`; `TaskPatch {title?: string; due?: string; clearDue?: boolean; priority?: string}` (used in Task 6); `Tasks.vue` helpers `dueOf`, `localToday`, `dueLabel` (used by Task 5's buckets).

- [ ] **Step 1: Update types and the test fixture, write the failing tests**

In `src/types.ts`:

```ts
export interface TaskItem {
  path: string;
  title: string;
  status: string;
  created: string;
  done: boolean;
  due: string | null;
  priority: string | null;
}

/** Patch for the update_task command; only present fields are written. */
export interface TaskPatch {
  title?: string;
  due?: string;
  clearDue?: boolean;
  priority?: string;
}
```

In `tests/tasks.test.ts`, widen the sample (existing tests keep passing):

```ts
const sample: TaskItem[] = [
  { path: "C:/v/Tasks/2026-07-08-b.md", title: "B open", status: "new", created: "2026-07-08", done: false, due: null, priority: null },
  { path: "C:/v/Tasks/2026-07-06-a.md", title: "A done", status: "done", created: "2026-07-06", done: true, due: null, priority: null },
];
```

Add tests:

```ts
it("opens a task in Obsidian when its title is clicked", async () => {
  const { wrapper, calls } = mountView();
  await flushPromises();
  await wrapper.get('[data-testid="task-open"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "open_task")).toEqual({
    cmd: "open_task",
    args: { id: "v1", path: "C:/v/Tasks/2026-07-08-b.md" },
  });
});

it("toasts and keeps the panel state when open_task fails", async () => {
  const notifications = useNotificationsStore();
  const { wrapper } = mountView({
    open_task: () => {
      throw new Error("no vault");
    },
  });
  await flushPromises();
  await wrapper.get('[data-testid="task-open"]').trigger("click");
  await flushPromises();
  expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
});

it("renders a due chip and priority dot from the task fields", async () => {
  const { wrapper } = mountView({
    list_tasks: () => [
      { path: "C:/v/Tasks/p.md", title: "P", status: "new", created: "2026-07-08", done: false, due: "2026-07-15", priority: "high" },
    ],
  });
  await flushPromises();
  expect(wrapper.get('[data-testid="task-due"]').text()).toBe("Jul 15");
  expect(wrapper.find('[data-testid="task-priority"]').exists()).toBe(true);
});

it("shows no due chip or dot for a plain task, and no dot for normal", async () => {
  const { wrapper } = mountView();
  await flushPromises();
  expect(wrapper.find('[data-testid="task-due"]').exists()).toBe(false);
  expect(wrapper.find('[data-testid="task-priority"]').exists()).toBe(false);
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — no `task-open`/`task-due`/`task-priority` elements.

- [ ] **Step 3: Implement in `Tasks.vue`**

Script additions (below the existing refs):

```ts
// A due only counts when it's a plain YYYY-MM-DD — a hand-authored value like
// "tomorrow" degrades to no-date instead of erroring (defensive read).
const DUE_RE = /^\d{4}-\d{2}-\d{2}$/;
const dueOf = (t: TaskItem): string | null =>
  t.due && DUE_RE.test(t.due) ? t.due : null;

// LOCAL calendar date — never UTC/ISO slicing, matching add_task's local-date
// rule; near midnight UTC-derived "today" would mis-bucket by a day.
function localToday(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// Deterministic short label (no locale dependence): "Jul 15".
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
function dueLabel(d: string): string {
  const [, m, day] = d.split("-");
  return `${MONTHS[Number(m) - 1]} ${Number(day)}`;
}
const isOverdue = (t: TaskItem): boolean => {
  const d = dueOf(t);
  return d !== null && !t.done && d < localToday();
};

async function openInObsidian(task: TaskItem) {
  try {
    await invoke("open_task", { id: props.vaultId, path: task.path });
  } catch (e) {
    notifications.error(String(e));
    logWarning(`open_task failed: ${String(e)}`);
  }
}
```

Template — replace the title `<span>` inside the row `<li>` with a click target plus chips (checkbox and archive button stay as-is around it):

```html
        <button
          type="button"
          data-testid="task-open"
          class="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :aria-label="`Open ${task.title} in Obsidian`"
          :title="`Open ${task.title} in Obsidian`"
          @click="openInObsidian(task)"
        >
          <span
            v-if="task.priority === 'high' || task.priority === 'low'"
            data-testid="task-priority"
            class="h-1.5 w-1.5 shrink-0 rounded-full"
            :class="task.priority === 'high' ? 'bg-red-400' : 'bg-slate-500'"
            :title="task.priority === 'high' ? 'High priority' : 'Low priority'"
            aria-hidden="true"
          ></span>
          <span
            class="min-w-0 flex-1 truncate text-sm"
            :class="task.done ? 'text-slate-500 line-through' : 'text-slate-100'"
          >
            {{ task.title }}
          </span>
          <span
            v-if="dueOf(task)"
            data-testid="task-due"
            class="shrink-0 text-[10px] tabular-nums"
            :class="isOverdue(task) ? 'font-semibold text-red-300' : 'text-slate-400'"
          >{{ dueLabel(dueOf(task)!) }}</span>
        </button>
```

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS (`vue-tsc` clean — every `TaskItem` literal in tests now carries `due`/`priority`).

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): open task in Obsidian on click, due chip + priority dot"
```

---

### Task 5: Frontend — sort mirror + date buckets

**Files:**
- Modify: `src/components/Tasks.vue` (`sortInPlace`; list template)
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes: `dueOf`/`localToday` (Task 4); backend sort order (Task 2).
- Produces: `buckets` computed consumed by the template; `filteredTasks` placeholder is NOT yet present (Task 8 adds the filter; buckets read `tasks` directly here and Task 8 swaps the source).

- [ ] **Step 1: Write the failing tests**

```ts
it("groups tasks into date buckets with headers", async () => {
  vi.useFakeTimers({ now: new Date(2026, 6, 9, 12, 0, 0), toFake: ["Date"] }); // 2026-07-09 local
  try {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/o.md", title: "Old", status: "new", created: "2026-07-01", done: false, due: "2026-07-08", priority: null },
        { path: "C:/v/Tasks/t.md", title: "Now", status: "new", created: "2026-07-01", done: false, due: "2026-07-09", priority: null },
        { path: "C:/v/Tasks/u.md", title: "Soon", status: "new", created: "2026-07-01", done: false, due: "2026-07-10", priority: null },
        { path: "C:/v/Tasks/n.md", title: "Someday", status: "new", created: "2026-07-01", done: false, due: null, priority: null },
        { path: "C:/v/Tasks/d.md", title: "Finished", status: "done", created: "2026-07-01", done: true, due: null, priority: null },
      ],
    });
    await flushPromises();
    const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
    expect(headers).toEqual(["Overdue", "Today", "Upcoming", "No date", "Done"]);
  } finally {
    vi.useRealTimers();
  }
});

it("shows no bucket headers when no open task has a parseable due date", async () => {
  // The pre-due-date flat list must stay visually unchanged — headers appear
  // only once dated open tasks exist.
  const { wrapper } = mountView(); // sample: one undated open + one done
  await flushPromises();
  expect(wrapper.findAll('[data-testid="task-bucket-header"]')).toHaveLength(0);
});

it("buckets an unparseable hand-authored due under No date", async () => {
  vi.useFakeTimers({ now: new Date(2026, 6, 9, 12, 0, 0), toFake: ["Date"] });
  try {
    const { wrapper } = mountView({
      list_tasks: () => [
        { path: "C:/v/Tasks/x.md", title: "Bad", status: "new", created: "2026-07-01", done: false, due: "tomorrow", priority: null },
        { path: "C:/v/Tasks/y.md", title: "Dated", status: "new", created: "2026-07-01", done: false, due: "2026-07-10", priority: null },
      ],
    });
    await flushPromises();
    const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
    expect(headers).toEqual(["Upcoming", "No date"]);
  } finally {
    vi.useRealTimers();
  }
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — no `task-bucket-header` elements.

- [ ] **Step 3: Implement**

Update `sortInPlace` to mirror the backend order (Task 2):

```ts
const PRIORITY_RANK: Record<string, number> = { high: 0, low: 2 };
const rank = (t: TaskItem) => PRIORITY_RANK[t.priority ?? ""] ?? 1;
// "0<date>" < "1" makes valid dues sort ascending ahead of undated.
const dueKey = (t: TaskItem) => {
  const d = dueOf(t);
  return d ? `0${d}` : "1";
};

function sortInPlace() {
  // Mirrors core::tasks::list_tasks so an optimistic insert/edit lands where
  // a refetch would put it: open first (due asc → priority → newest created
  // → title); done by newest created → title.
  tasks.value.sort(
    (a, b) =>
      Number(a.done) - Number(b.done) ||
      (a.done
        ? b.created.localeCompare(a.created) || a.title.localeCompare(b.title)
        : dueKey(a).localeCompare(dueKey(b)) ||
          rank(a) - rank(b) ||
          b.created.localeCompare(a.created) ||
          a.title.localeCompare(b.title)),
  );
}
```

Add the buckets computed:

```ts
type Bucket = { key: string; label: string | null; tasks: TaskItem[] };

const buckets = computed<Bucket[]>(() => {
  const today = localToday();
  const groups: Record<string, TaskItem[]> = { overdue: [], today: [], upcoming: [], nodate: [], done: [] };
  for (const t of tasks.value) {
    if (t.done) groups.done.push(t);
    else {
      const d = dueOf(t);
      if (!d) groups.nodate.push(t);
      else if (d < today) groups.overdue.push(t);
      else if (d === today) groups.today.push(t);
      else groups.upcoming.push(t);
    }
  }
  // Headers only once a dated open task exists — a vault that never uses due
  // dates keeps the flat list it had before this feature.
  const showHeaders =
    groups.overdue.length + groups.today.length + groups.upcoming.length > 0;
  return [
    { key: "overdue", label: "Overdue" },
    { key: "today", label: "Today" },
    { key: "upcoming", label: "Upcoming" },
    { key: "nodate", label: "No date" },
    { key: "done", label: "Done" },
  ]
    .map(({ key, label }) => ({ key, label: showHeaders ? label : null, tasks: groups[key] }))
    .filter((b) => b.tasks.length > 0);
});
```

Template — replace the single `<ul v-else ...>` with bucketed lists (the `<li>` row content is unchanged from Task 4):

```html
    <template v-else>
      <div v-for="bucket in buckets" :key="bucket.key" class="mt-1 first:mt-0">
        <h3
          v-if="bucket.label"
          data-testid="task-bucket-header"
          class="mb-1 px-1 text-[10px] font-semibold uppercase tracking-wider"
          :class="bucket.key === 'overdue' ? 'text-red-300' : 'text-slate-500'"
        >
          {{ bucket.label }}
        </h3>
        <ul class="flex flex-col gap-1">
          <li
            v-for="task in bucket.tasks"
            :key="task.path"
            data-testid="task-row"
            class="flex items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1"
          >
            <!-- row content exactly as after Task 4 -->
          </li>
        </ul>
      </div>
    </template>
```

(The preceding `v-else-if="tasks.length === 0"` empty state stays; buckets render only when tasks exist.)

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS — including the pre-existing "renders open tasks before done ones" test (undated sample → flat, no headers).

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): date buckets (Overdue/Today/Upcoming/No date/Done) in Tasks view"
```

---

### Task 6: Frontend — add-row due/priority controls

**Files:**
- Modify: `src/components/Tasks.vue` (add row + `add()`)
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes (IPC): `add_task {id, title, due?, priority?}` (Task 3).

- [ ] **Step 1: Write the failing tests**

```ts
it("adds a task with due and priority from the options row", async () => {
  const { wrapper, calls } = mountView();
  await flushPromises();
  await wrapper.get('[data-testid="task-add-options"]').trigger("click");
  await wrapper.get('[data-testid="task-add-due"]').setValue("2026-07-20");
  await wrapper.get('[data-testid="task-add-priority-high"]').trigger("click");
  await wrapper.get('[data-testid="task-input"]').setValue("Big one");
  await wrapper.get('[data-testid="task-add"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "add_task")).toEqual({
    cmd: "add_task",
    args: { id: "v1", title: "Big one", due: "2026-07-20", priority: "high" },
  });
});

it("omits due/priority when the options are untouched", async () => {
  const { wrapper, calls } = mountView();
  await flushPromises();
  await wrapper.get('[data-testid="task-input"]').setValue("Plain");
  await wrapper.get('[data-testid="task-add"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "add_task")).toEqual({
    cmd: "add_task",
    args: { id: "v1", title: "Plain" },
  });
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — no `task-add-options` element; plain add sends the old shape (first test errors on the missing element before that).

- [ ] **Step 3: Implement**

Script:

```ts
const showAddOptions = ref(false);
const addDue = ref("");
const addPriority = ref("normal");
```

Replace `add()`'s invoke + success reset:

```ts
async function add() {
  const title = newTitle.value.trim();
  if (!title || adding.value) return;
  adding.value = true;
  try {
    const args: Record<string, unknown> = { id: props.vaultId, title };
    if (addDue.value) args.due = addDue.value;
    if (addPriority.value !== "normal") args.priority = addPriority.value;
    const created = await invoke<TaskItem>("add_task", args);
    tasks.value.unshift(created);
    sortInPlace();
    newTitle.value = "";
    addDue.value = "";
    addPriority.value = "normal";
    showAddOptions.value = false;
  } catch (e) {
    notifications.error(String(e));
    logWarning(`add_task failed: ${String(e)}`);
  } finally {
    adding.value = false;
  }
}
```

Template — inside the add-row `<div class="flex items-center gap-1">`, add a toggle button before the Add button, and an options row after the div:

```html
      <button
        type="button"
        data-testid="task-add-options"
        :aria-label="showAddOptions ? 'Hide task options' : 'Set due date or priority'"
        :aria-expanded="showAddOptions"
        title="Due date / priority"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :class="showAddOptions ? 'border-violet-400 text-slate-100' : ''"
        @click="showAddOptions = !showAddOptions"
      >
        ⋯
      </button>
```

```html
    <div v-if="showAddOptions" class="flex items-center gap-1">
      <input
        v-model="addDue"
        data-testid="task-add-due"
        type="date"
        aria-label="Due date"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
      />
      <div class="flex gap-0.5" role="radiogroup" aria-label="Priority">
        <button
          v-for="p in ['high', 'normal', 'low']"
          :key="p"
          type="button"
          role="radio"
          :data-testid="`task-add-priority-${p}`"
          :aria-checked="addPriority === p"
          class="cursor-pointer rounded-lg border px-1.5 py-0.5 text-[10px] capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            addPriority === p
              ? 'border-violet-400 bg-violet-500/20 text-slate-100'
              : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
          "
          @click="addPriority = p"
        >
          {{ p }}
        </button>
      </div>
    </div>
```

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): optional due date and priority on the add-task row"
```

---

### Task 7: Frontend — inline row editor

**Files:**
- Modify: `src/components/Tasks.vue`
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes (IPC): `update_task {id, path, patch}` with `TaskPatch` (Tasks 3/4); the per-row `busy` guard.

- [ ] **Step 1: Write the failing tests**

```ts
it("edits a task inline: sends only the changed fields", async () => {
  const { wrapper, calls } = mountView({
    list_tasks: () => [
      { path: "C:/v/Tasks/e.md", title: "Old name", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null },
    ],
  });
  await flushPromises();
  await wrapper.get('[data-testid="task-edit"]').trigger("click");
  await wrapper.get('[data-testid="task-edit-title"]').setValue("New name");
  await wrapper.get('[data-testid="task-edit-priority-high"]').trigger("click");
  await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "update_task")).toEqual({
    cmd: "update_task",
    args: { id: "v1", path: "C:/v/Tasks/e.md", patch: { title: "New name", priority: "high" } },
  });
  expect(wrapper.text()).toContain("New name"); // optimistic
});

it("clearing the due date sends clearDue", async () => {
  const { wrapper, calls } = mountView({
    list_tasks: () => [
      { path: "C:/v/Tasks/e.md", title: "T", status: "new", created: "2026-07-08", done: false, due: "2026-07-10", priority: null },
    ],
  });
  await flushPromises();
  await wrapper.get('[data-testid="task-edit"]').trigger("click");
  await wrapper.get('[data-testid="task-edit-due"]').setValue("");
  await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "update_task")?.args).toMatchObject({
    patch: { clearDue: true },
  });
});

it("reverts the row and notifies when the edit save fails", async () => {
  const notifications = useNotificationsStore();
  const { wrapper } = mountView({
    update_task: () => {
      throw new Error("disk full");
    },
  });
  await flushPromises();
  await wrapper.get('[data-testid="task-edit"]').trigger("click");
  await wrapper.get('[data-testid="task-edit-title"]').setValue("Broken");
  await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
  await flushPromises();
  expect(wrapper.text()).toContain("B open"); // reverted
  expect(wrapper.text()).not.toContain("Broken");
  expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
});

it("cancel closes the editor without a write", async () => {
  const { wrapper, calls } = mountView();
  await flushPromises();
  await wrapper.get('[data-testid="task-edit"]').trigger("click");
  await wrapper.get('[data-testid="task-edit-title"]').setValue("Nope");
  await wrapper.get('[data-testid="task-edit-cancel"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
  expect(wrapper.text()).toContain("B open");
});

it("saving with nothing changed is a no-op close", async () => {
  const { wrapper, calls } = mountView();
  await flushPromises();
  await wrapper.get('[data-testid="task-edit"]').trigger("click");
  await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
  await flushPromises();
  expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
  expect(wrapper.find('[data-testid="task-edit-title"]').exists()).toBe(false);
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — no `task-edit` element.

- [ ] **Step 3: Implement**

Script:

```ts
import type { TaskItem, TaskPatch } from "../types";
```

```ts
// Inline editor: one row at a time; opening another row discards unsaved
// edits in the first (the file is the source of truth, edits are cheap).
const editingPath = ref<string | null>(null);
const editTitle = ref("");
const editDue = ref("");
const editPriority = ref("normal");

const normalizedPriority = (t: TaskItem) =>
  t.priority === "high" || t.priority === "low" ? t.priority : "normal";

function startEdit(task: TaskItem) {
  editingPath.value = task.path;
  editTitle.value = task.title;
  editDue.value = dueOf(task) ?? "";
  editPriority.value = normalizedPriority(task);
}

function cancelEdit() {
  editingPath.value = null;
}

async function saveEdit(task: TaskItem) {
  if (busy.value.has(task.path)) return;
  const patch: TaskPatch = {};
  const title = editTitle.value.trim();
  if (title && title !== task.title) patch.title = title;
  if (editDue.value !== (dueOf(task) ?? "")) {
    if (editDue.value === "") patch.clearDue = true;
    else patch.due = editDue.value;
  }
  if (editPriority.value !== normalizedPriority(task)) patch.priority = editPriority.value;
  editingPath.value = null;
  if (Object.keys(patch).length === 0) return;
  // Optimistic: apply locally (re-sort/re-bucket live), revert on failure.
  const before = { title: task.title, due: task.due, priority: task.priority };
  if (patch.title) task.title = patch.title;
  if (patch.clearDue) task.due = null;
  else if (patch.due) task.due = patch.due;
  if (patch.priority) task.priority = patch.priority === "normal" ? null : patch.priority;
  sortInPlace();
  busy.value.add(task.path);
  try {
    await invoke("update_task", { id: props.vaultId, path: task.path, patch });
  } catch (e) {
    Object.assign(task, before);
    sortInPlace();
    notifications.error(String(e));
    logWarning(`update_task failed: ${String(e)}`);
  } finally {
    busy.value.delete(task.path);
  }
}
```

Template — inside the `<li>`, wrap the Task 4 row content in `v-if="editingPath !== task.path"` (a `<template>` wrapper around checkbox + open button + edit + archive buttons) and add the editor branch; also add the pencil button between the open button and the archive button:

```html
        <button
          type="button"
          data-testid="task-edit"
          :disabled="isBusy(task.path)"
          :aria-label="`Edit ${task.title}`"
          title="Edit"
          class="shrink-0 cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
          @click="startEdit(task)"
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
          </svg>
        </button>
```

Editor branch (replaces the whole row content when editing):

```html
        <div v-if="editingPath === task.path" class="flex min-w-0 flex-1 flex-col gap-1 py-0.5">
          <input
            v-model="editTitle"
            data-testid="task-edit-title"
            type="text"
            aria-label="Task title"
            class="min-w-0 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 focus:border-violet-400 focus:outline-none"
            @keydown.enter.prevent="saveEdit(task)"
            @keydown.esc="cancelEdit"
          />
          <div class="flex items-center gap-1">
            <input
              v-model="editDue"
              data-testid="task-edit-due"
              type="date"
              aria-label="Due date"
              class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
            />
            <div class="flex gap-0.5" role="radiogroup" aria-label="Priority">
              <button
                v-for="p in ['high', 'normal', 'low']"
                :key="p"
                type="button"
                role="radio"
                :data-testid="`task-edit-priority-${p}`"
                :aria-checked="editPriority === p"
                class="cursor-pointer rounded-lg border px-1.5 py-0.5 text-[10px] capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
                :class="
                  editPriority === p
                    ? 'border-violet-400 bg-violet-500/20 text-slate-100'
                    : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
                "
                @click="editPriority = p"
              >
                {{ p }}
              </button>
            </div>
          </div>
          <div class="flex items-center justify-end gap-1">
            <button
              type="button"
              data-testid="task-edit-cancel"
              class="cursor-pointer rounded-lg px-2 py-0.5 text-xs text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
              @click="cancelEdit"
            >
              Cancel
            </button>
            <button
              type="button"
              data-testid="task-edit-save"
              :disabled="isBusy(task.path)"
              class="cursor-pointer rounded-lg bg-violet-600/80 px-2 py-0.5 text-xs font-semibold text-white hover:bg-violet-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
              @click="saveEdit(task)"
            >
              Save
            </button>
          </div>
        </div>
        <template v-else>
          <!-- checkbox, task-open button, task-edit button, task-archive button — as before -->
        </template>
```

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): inline task editor (rename, due date, priority)"
```

---

### Task 8: Frontend — title filter

**Files:**
- Modify: `src/components/Tasks.vue`
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes: `buckets` (Task 5) — its source switches from `tasks` to `filteredTasks`.

- [ ] **Step 1: Write the failing tests**

```ts
const many = (n: number): TaskItem[] =>
  Array.from({ length: n }, (_, i) => ({
    path: `C:/v/Tasks/${i}.md`,
    title: `Task ${i}`,
    status: "new",
    created: "2026-07-08",
    done: false,
    due: null,
    priority: null,
  }));

it("shows the filter only above 5 tasks and narrows by title", async () => {
  const { wrapper } = mountView({ list_tasks: () => many(6) });
  await flushPromises();
  const input = wrapper.get('[data-testid="task-filter"]');
  await input.setValue("Task 3");
  expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(1);
  expect(wrapper.text()).toContain("Task 3");
});

it("hides the filter for short lists", async () => {
  const { wrapper } = mountView(); // 2 tasks
  await flushPromises();
  expect(wrapper.find('[data-testid="task-filter"]').exists()).toBe(false);
});

it("keeps the progress bar counting the unfiltered list", async () => {
  const { wrapper } = mountView({
    list_tasks: () => [
      ...many(6),
      { path: "C:/v/Tasks/d.md", title: "Done one", status: "done", created: "2026-07-01", done: true, due: null, priority: null },
    ],
  });
  await flushPromises();
  await wrapper.get('[data-testid="task-filter"]').setValue("Task 3");
  expect(wrapper.get('[data-testid="task-progress"]').text()).toContain("1 / 7");
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — no `task-filter` element.

- [ ] **Step 3: Implement**

Script:

```ts
const filter = ref("");
// Same threshold as the vault list: a filter only earns its row above 5.
const showFilter = computed(() => tasks.value.length > 5);
const filteredTasks = computed(() => {
  const q = filter.value.trim().toLowerCase();
  if (!q) return tasks.value;
  return tasks.value.filter((t) => t.title.toLowerCase().includes(q));
});
```

In the `buckets` computed, change `for (const t of tasks.value)` to `for (const t of filteredTasks.value)`. (`progress` keeps reading `tasks.value` — the bar reflects the vault, not the filter.)

Template — insert between the progress bar and the add row:

```html
    <input
      v-if="showFilter"
      v-model="filter"
      data-testid="task-filter"
      type="search"
      placeholder="Filter tasks…"
      aria-label="Filter tasks"
      class="rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
    />
```

And add a no-matches state before the buckets `<template v-else>` chain — change the empty-state chain to:

```html
    <p v-else-if="tasks.length === 0" class="text-xs text-slate-400">
      No tasks yet.
    </p>
    <p v-else-if="filteredTasks.length === 0" class="text-xs text-slate-400">
      No tasks match "{{ filter }}".
    </p>
    <template v-else>
      <!-- buckets as in Task 5 -->
```

- [ ] **Step 4: Run tests + full suite + build**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): task title filter above five tasks"
```

---

### Task 9: Docs — AGENTS.md tasks domain

**Files:**
- Modify: `AGENTS.md` (tasks-domain section + IPC surface list)

- [ ] **Step 1: Update the docs**

In the IPC surface paragraph, extend the tasks command list to: `get_tasks_config`, `set_tasks_config`, `list_tasks`, `add_task`, `set_task_status`, `count_open_tasks`, `open_task`, `update_task`.

In "The tasks domain" section: document the widened frontmatter (`due: YYYY-MM-DD`, `priority: high|normal|low`, absent = normal and never written; unparseable values degrade to no-date/normal), that `set_fields` is the generalized surgical writer behind both `set_task_status` and `update_task` (multi-key, byte-preserving, refuses non-Task/unclosed fences), the clock-free sort (due asc → priority → created desc; buckets are frontend-only because they need "today"), and `open_task` as a read-only `uri::launch`-logged handoff. Note the Tasks view now has date buckets, an inline editor, add-row due/priority, and a >5 filter.

- [ ] **Step 2: Full verification sweep + commit**

Run: `npm test && npm run build && cd src-tauri && cargo fmt --check && cd core && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all PASS.

```bash
git add AGENTS.md
git commit -m "docs(agents): document the tasks todo-list increment"
```

---

## Self-Review

**Spec coverage:**
- Open in Obsidian on title click → Task 3 (`open_task`) + Task 4 (row button). ✓
- `due`/`priority` schema incl. normal-never-written, clear-removes-line → Tasks 1–3. ✓
- Date buckets + header-visibility rule → Task 5. ✓
- Filter (>5, title, progress unfiltered) → Task 8. ✓
- Inline editor (only changed fields, clearDue, optimistic+revert, busy guard, one row) → Task 7. ✓
- Add-row due/priority → Task 6. ✓
- Clock-free core sort / frontend-local-date bucketing → Tasks 2 & 5. ✓
- Graceful bad-due/unknown-priority reads → Task 2 (`is_valid_due` filter in sort), Task 4 (`dueOf`), Task 5 (bucket test). ✓
- Surgical write discipline & no new write path → Task 1 (`set_fields` + wrappers), `open_task` read-only. ✓
- Docs → Task 9. ✓

**Placeholder scan:** none. The two "as before / as in Task N" template comments refer to markup fully shown in Tasks 4/5 of THIS plan and mark unchanged regions, not deferred work.

**Type consistency:** `set_fields(&str, &[(&str, Option<&str>)])` used identically in Tasks 1–3; `update_task_fields(root, path, updates)` Task 1↔3; `render_task`/`create_task` 4-arg/5-arg forms Task 2↔3; `TaskPatchDto`/`TaskPatch` field names (`title`, `due`, `clearDue`, `priority`) Task 3↔4↔7; `TaskItem.due/priority: string|null` Tasks 4–8 fixtures; `is_valid_due`/`priority_rank` names Task 2↔3; test ids consistent (`task-open`, `task-due`, `task-priority`, `task-edit*`, `task-add-*`, `task-filter`, `task-bucket-header`).
