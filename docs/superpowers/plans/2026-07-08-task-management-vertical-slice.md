# Task Management Vertical Slice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every registered vault a per-vault Tasks view — a simple todo list of `type: Task` markdown documents in a configurable tasks folder, with an inline add box and checkboxes to mark tasks done.

**Architecture:** All task logic lives in the pure `vault_buddy_core` crate (`core/src/tasks.rs`, unit-tested on Linux); the Tauri shell adds thin IPC commands that resolve a vault + tasks root and delegate to core. A per-vault `tasks_folder` extends the existing app-side `config.json`. The frontend adds a `tasks` panel view (store + `ActionPanel` slot), a Tasks button on each `VaultList` row, and a self-contained `Tasks.vue`.

**Tech Stack:** Rust (core crate + Tauri shell), Vue 3 + Pinia + Tailwind 4, Vitest (happy-dom + `mockIPC`), `cargo test`.

## Global Constraints

- **Never clobber a vault.** Creating a task uses the collision-safe atomic writer (`capture_note::write_note_collision_safe`); toggling status is a *surgical* read-modify-write that changes only the `status:` line and preserves all other frontmatter + the entire body, written via temp (`create_new`) + fsync + replacing rename. A file that is not `type: Task` is never written. See `docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md`.
- **Path safety before any read/write:** resolve a configured folder with `capture_paths::safe_recording_root(vault_path, folder)` (lexical), then canonicalize with `capture_paths::assert_root_inside_vault` before **both** reads (skip+warn→empty on escape) and writes (error on escape) — the lexical check can't see through a symlink/junction planted at the tasks folder.
- **Degrade, never error, on reads:** unknown vault / missing folder / unreadable file → empty list, matching `list_recordings`.
- **The shell crate (`src-tauri/src/*.rs`) does not compile on Linux** (no webkit2gtk). Mirror existing patterns exactly, run `cd src-tauri && cargo fmt --check`, and let CI's `windows-app` job be the compile gate. Core-crate tasks build and test locally.
- **Frontmatter format:** `type: Task` (unquoted), `status: new`/`status: done` (unquoted), `created: YYYY-MM-DD` (unquoted), `title: "…"` (quoted via `capture_note::yaml_quote`).
- **Completed = `status: done`.** Any other status renders unchecked. Checking sets `done`; unchecking sets `new`.
- **Default tasks folder:** `Tasks`.
- **Commits:** Conventional Commits (`feat(core)`, `feat(tasks)`, `feat(ui)`, `test(...)`). Imperative subject; body explains the *why*. Author must be `Claude <noreply@anthropic.com>`.
- **TDD:** failing test first, then the minimal implementation. Regression tests name their failure mode in a comment.

---

### Task 1: Per-vault `tasks_folder` config field

**Files:**
- Modify: `src-tauri/core/src/capture_config.rs` (struct ~52-68, `Default` ~70-86, `vault_entry` ~137-188, `serialize_config` ~220-259, add `tasks_root` in the `impl VaultCaptureConfig` block ~88-113)
- Test: `src-tauri/core/src/capture_config.rs` (the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `VaultCaptureConfig.tasks_folder: Option<String>`; `VaultCaptureConfig::tasks_root(&self) -> &str` (returns the configured folder or `"Tasks"`). JSON key `tasksFolder`, omitted when `None`.

- [ ] **Step 1: Write the failing round-trip test**

Add to the `mod tests` block in `capture_config.rs`:

```rust
#[test]
fn tasks_folder_round_trips_and_defaults() {
    // Regression: a per-vault tasks folder must survive serialize→parse and
    // default to "Tasks" when absent, exactly like the recording folder does.
    let mut cfg = AppConfig::default();
    let mut v = VaultCaptureConfig::default();
    assert_eq!(v.tasks_root(), "Tasks"); // None → default
    v.tasks_folder = Some("Inbox/Tasks".to_string());
    assert_eq!(v.tasks_root(), "Inbox/Tasks");
    cfg.vaults.insert("v1".to_string(), v);

    let json = serialize_config(&cfg);
    assert!(json.contains("\"tasksFolder\": \"Inbox/Tasks\""));
    let parsed = parse_config(&json);
    assert_eq!(
        parsed.vaults["v1"].tasks_folder.as_deref(),
        Some("Inbox/Tasks")
    );

    // A None tasks_folder is omitted from the serialized entry.
    let mut cfg2 = AppConfig::default();
    cfg2.vaults
        .insert("v2".to_string(), VaultCaptureConfig::default());
    assert!(!serialize_config(&cfg2).contains("tasksFolder"));
}
```

- [ ] **Step 2: Run it and confirm it fails**

Run: `cd src-tauri/core && cargo test tasks_folder_round_trips_and_defaults`
Expected: FAIL — `no field tasks_folder on type VaultCaptureConfig`.

- [ ] **Step 3: Add the field, default, parse, serialize, and helper**

In the struct (after `follow_up_template: bool,`):

```rust
    /// Vault-relative folder holding this vault's task documents.
    /// None → the default "Tasks".
    pub tasks_folder: Option<String>,
```

In `Default` (after `follow_up_template: true,`): `tasks_folder: None,`

In `vault_entry` (after the `follow_up_template` field): 

```rust
        tasks_folder: entry
            .get("tasksFolder")
            .and_then(|v| v.as_str())
            .map(str::to_string),
```

In `serialize_config`, after the `followUpTemplate` insert:

```rust
        if let Some(folder) = &v.tasks_folder {
            entry.insert("tasksFolder".to_string(), json!(folder));
        }
```

In `impl VaultCaptureConfig` (near `recording_roots`):

```rust
    /// The vault-relative folder holding this vault's task documents.
    pub fn tasks_root(&self) -> &str {
        self.tasks_folder.as_deref().unwrap_or("Tasks")
    }
```

- [ ] **Step 4: Run tests + fmt**

Run: `cd src-tauri/core && cargo test && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/capture_config.rs
git commit -m "feat(core): add per-vault tasks_folder config field"
```

---

### Task 2: Core — task filename + document rendering (pure)

**Files:**
- Create: `src-tauri/core/src/tasks.rs`
- Modify: `src-tauri/core/src/lib.rs` (module list at the top — add `pub mod tasks;`)

**Interfaces:**
- Produces:
  - `tasks::task_basename(title: &str, today: &str) -> String` → e.g. `"2026-07-08-buy-milk"` (no extension); empty slug falls back to `"task"`.
  - `tasks::render_task(title: &str, created: &str) -> String` → full `type: Task` document (empty body).

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/core/src/tasks.rs`:

```rust
//! Task documents: `type: Task` markdown files under a vault's tasks folder.
//! Pure filename/render/parse logic + the two sanctioned vault writes
//! (collision-safe create; surgical `status:` flip). Same never-clobber
//! discipline as the capture note and transcript sidecar. See
//! docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md.

use crate::capture_note::yaml_quote;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_slugifies_title_with_date() {
        assert_eq!(task_basename("Buy milk", "2026-07-08"), "2026-07-08-buy-milk");
        assert_eq!(
            task_basename("  Prepare  Release: cutover!! ", "2026-07-08"),
            "2026-07-08-prepare-release-cutover"
        );
    }

    #[test]
    fn basename_empty_slug_falls_back_to_task() {
        // A title of only punctuation must still yield a usable filename.
        assert_eq!(task_basename("!!!", "2026-07-08"), "2026-07-08-task");
    }

    #[test]
    fn basename_caps_long_slug_for_filesystem_limits() {
        // A very long title must not overflow a Windows path component (255)
        // and blow the ~260-char default MAX_PATH. Slug is capped; the full
        // title still lives in frontmatter (render_task, not the filename).
        let base = task_basename(&"a".repeat(300), "2026-07-08");
        let slug = base.strip_prefix("2026-07-08-").unwrap();
        assert!(slug.len() <= 80, "slug should be capped, got {}", slug.len());
        assert!(slug.chars().all(|c| c == 'a'));
    }

    #[test]
    fn render_writes_type_task_status_new_quoted_title() {
        let doc = render_task("Buy milk", "2026-07-08");
        assert_eq!(
            doc,
            "---\ntype: Task\nstatus: new\ntitle: \"Buy milk\"\ncreated: 2026-07-08\n---\n\n"
        );
    }

    #[test]
    fn render_quotes_a_colon_title() {
        // A colon in the title would break unquoted YAML — must be quoted.
        let doc = render_task("Ship: v1", "2026-07-08");
        assert!(doc.contains("title: \"Ship: v1\"\n"));
    }
}
```

- [ ] **Step 2: Run and confirm failure**

Add `pub mod tasks;` to `src-tauri/core/src/lib.rs` alongside the other `pub mod` lines, then:
Run: `cd src-tauri/core && cargo test tasks::`
Expected: FAIL — `cannot find function task_basename`.

- [ ] **Step 3: Implement the pure helpers**

Add above the `#[cfg(test)]` block:

```rust
/// Lower-case, collapse every run of non-alphanumeric chars to a single
/// hyphen, cap the length (so the filename component stays inside Windows'
/// 255-char segment / ~260-char MAX_PATH limits — the full title survives in
/// frontmatter), trim leading/trailing hyphens. Empty result → "task".
fn slugify(title: &str) -> String {
    const MAX_SLUG: usize = 80;
    let mut slug = String::new();
    let mut prev_hyphen = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.extend(ch.to_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    // slug is ASCII (alnum + '-'), so truncating by byte index is char-safe.
    slug.truncate(MAX_SLUG);
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "task".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `YYYY-MM-DD-<slug>` (no extension). `today` is supplied by the shell so
/// the core stays clock-free and testable.
pub fn task_basename(title: &str, today: &str) -> String {
    format!("{today}-{}", slugify(title))
}

/// A `type: Task` document with an empty body. `type`/`status`/`created` are
/// simple unquoted scalars; the user-supplied title is quoted so a colon or
/// quote can't break the frontmatter (read back by `capture_note::note_field`).
pub fn render_task(title: &str, created: &str) -> String {
    format!(
        "---\ntype: Task\nstatus: new\ntitle: {}\ncreated: {created}\n---\n\n",
        yaml_quote(title)
    )
}
```

Note: `yaml_quote` is currently `pub(crate)` in `capture_note.rs` — it is reachable from `tasks.rs` (same crate). No visibility change needed.

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cd src-tauri/core && cargo test tasks:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks.rs src-tauri/core/src/lib.rs
git commit -m "feat(tasks): render task documents and generate filenames"
```

---

### Task 3: Core — create a task file (collision-safe I/O)

**Files:**
- Modify: `src-tauri/core/src/tasks.rs`
- Test: `src-tauri/core/src/tasks.rs` (`mod tests`)

**Interfaces:**
- Consumes: `task_basename`, `render_task` (Task 2); `capture_note::write_note_collision_safe`.
- Produces: `tasks::create_task(root: &Path, title: &str, today: &str) -> std::io::Result<std::path::PathBuf>` — creates `root` if missing, writes `<basename>.md` (suffixing on collision), returns the created absolute path.

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
#[test]
fn create_task_writes_file_and_never_clobbers() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");

    let p1 = create_task(&root, "Buy milk", "2026-07-08").unwrap();
    assert_eq!(p1.file_name().unwrap(), "2026-07-08-buy-milk.md");
    let body = std::fs::read_to_string(&p1).unwrap();
    assert!(body.contains("type: Task"));
    assert!(body.contains("status: new"));
    assert!(body.contains("title: \"Buy milk\""));

    // Same title again → suffixed, original untouched (collision-safe).
    let p2 = create_task(&root, "Buy milk", "2026-07-08").unwrap();
    assert_ne!(p1, p2);
    assert_eq!(p2.file_name().unwrap(), "2026-07-08-buy-milk (2).md");
    assert!(p1.exists() && p2.exists());
}
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri/core && cargo test tasks::tests::create_task_writes_file_and_never_clobbers`
Expected: FAIL — `cannot find function create_task`. (`tempfile` is already a dev-dependency of the core crate — confirm with `grep tempfile src-tauri/core/Cargo.toml`; if absent, add it under `[dev-dependencies]`.)

- [ ] **Step 3: Implement `create_task`**

Add to `tasks.rs`:

```rust
/// Create a new task file under `root` (creating `root` if needed). Uses the
/// collision-safe atomic writer shared with the capture note, so it can never
/// overwrite an existing file — a name clash takes the ` (N)` suffix instead.
pub fn create_task(root: &Path, title: &str, today: &str) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(root)?;
    let target = root.join(format!("{}.md", task_basename(title, today)));
    crate::capture_note::write_note_collision_safe(&target, &render_task(title, today))
}
```

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cd src-tauri/core && cargo test tasks:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks.rs src-tauri/core/Cargo.toml
git commit -m "feat(tasks): create task files collision-safely"
```

---

### Task 4: Core — list `type: Task` documents

**Files:**
- Modify: `src-tauri/core/src/tasks.rs`
- Test: `src-tauri/core/src/tasks.rs` (`mod tests`)

**Interfaces:**
- Consumes: `capture_note::note_field`; `transcript::dir_entries` (folder enumeration used by `recordings.rs`).
- Produces:
  - `pub struct TaskItem { pub path: PathBuf, pub title: String, pub status: String, pub created: String, pub done: bool }`
  - `tasks::is_task(content: &str) -> bool`
  - `tasks::list_tasks(root: &Path) -> Vec<TaskItem>` — flat scan of `root` for `*.md` files whose frontmatter `type` is `Task`; open first (newest `created`, then title), completed after; degrades to empty on a missing/unreadable root.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
fn write(root: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join(name), body).unwrap();
}

#[test]
fn list_tasks_returns_only_type_task_files_sorted_open_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "2026-07-06-a.md", "---\ntype: Task\nstatus: done\ntitle: \"A done\"\ncreated: 2026-07-06\n---\n");
    write(root, "2026-07-08-b.md", "---\ntype: Task\nstatus: new\ntitle: \"B open\"\ncreated: 2026-07-08\n---\n");
    write(root, "2026-07-07-c.md", "---\ntype: Task\nstatus: new\ntitle: \"C open\"\ncreated: 2026-07-07\n---\n");
    // Not a task — must be ignored even though it lives in the folder.
    write(root, "note.md", "---\ntype: Meeting\ntitle: \"Nope\"\n---\n");
    // No frontmatter — ignored.
    write(root, "plain.md", "just text\n");

    let items = list_tasks(root);
    let titles: Vec<&str> = items.iter().map(|t| t.title.as_str()).collect();
    // Open tasks first, newest created first; the done task last.
    assert_eq!(titles, vec!["B open", "C open", "A done"]);
    assert!(!items[0].done);
    assert!(items[2].done);
    assert_eq!(items[0].status, "new");
    assert_eq!(items[2].created, "2026-07-06");
}

#[test]
fn list_tasks_missing_root_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(list_tasks(&dir.path().join("nope")).is_empty());
}

#[test]
fn is_task_only_true_for_type_task() {
    assert!(is_task("---\ntype: Task\nstatus: new\n---\n"));
    assert!(is_task("---\ntype: \"Task\"\n---\n")); // quoted also fine
    assert!(!is_task("---\ntype: Meeting\n---\n"));
    assert!(!is_task("no frontmatter"));
}
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri/core && cargo test tasks::tests::list_tasks_returns_only_type_task_files_sorted_open_first`
Expected: FAIL — `cannot find type TaskItem` / `cannot find function list_tasks`.

- [ ] **Step 3: Implement `TaskItem`, `is_task`, `list_tasks`**

Add to `tasks.rs` (add `use std::path::PathBuf;` to the existing `use std::path::Path;` line → `use std::path::{Path, PathBuf};`):

```rust
use crate::capture_note::note_field;
use crate::transcript::dir_entries;

/// One task surfaced in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskItem {
    pub path: PathBuf,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
}

/// True iff the file's leading frontmatter declares `type: Task`.
pub fn is_task(content: &str) -> bool {
    note_field(content, "type").as_deref() == Some("Task")
}

/// Every `type: Task` file directly under `root`, best-effort. Open tasks
/// (status != "done") first, newest `created` first with title as tiebreaker;
/// completed tasks after. A missing/unreadable root or file degrades silently.
pub fn list_tasks(root: &Path) -> Vec<TaskItem> {
    let mut out = Vec::new();
    for (path, ft, name) in dir_entries(root) {
        if !ft.is_file() || !name.ends_with(".md") {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !is_task(&content) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
        let title = note_field(&content, "title").unwrap_or(stem);
        let status = note_field(&content, "status").unwrap_or_else(|| "new".to_string());
        let created = note_field(&content, "created").unwrap_or_default();
        let done = status == "done";
        out.push(TaskItem { path, title, status, created, done });
    }
    // Open first; within each group newest created first, then title.
    out.sort_by(|a, b| {
        a.done
            .cmp(&b.done)
            .then(b.created.cmp(&a.created))
            .then(a.title.cmp(&b.title))
    });
    out
}
```

Note: `dir_entries` is `pub` in `transcript.rs` (used by `recordings.rs`) — confirm with `grep "pub fn dir_entries" src-tauri/core/src/transcript.rs`; if it is not `pub`, make it so (a one-word change, mirroring `recordings.rs`'s reuse).

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cd src-tauri/core && cargo test tasks:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks.rs src-tauri/core/src/transcript.rs
git commit -m "feat(tasks): list type:Task documents, open first"
```

---

### Task 5: Core — surgical `status:` flip (pure + I/O)

**Files:**
- Modify: `src-tauri/core/src/tasks.rs`
- Test: `src-tauri/core/src/tasks.rs` (`mod tests`)

**Interfaces:**
- Consumes: `is_task` (Task 4); `capture_note::NOTE_TMP_SUFFIX`.
- Produces:
  - `tasks::set_status(content: &str, new_status: &str) -> Option<String>` — returns `content` with the frontmatter `status:` line set to `new_status`; **inserts** a `status:` line at the top of the frontmatter when none exists (so a hand-authored task stays toggleable); `None` only if not `type: Task`.
  - `tasks::set_task_status(root: &Path, path: &Path, done: bool) -> Result<(), String>` — **canonicalizes** `root` and `path` and requires containment (a symlinked file/folder can't carry the write outside the vault), reads, applies `set_status(_, if done {"done"} else {"new"})`, writes atomically (temp `create_new` + fsync + replacing rename). Refuses (Err) a path that resolves outside the root or a non-`type: Task` file.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
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
fn set_task_status_writes_and_rejects_escape() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    let p = create_task(&root, "Buy milk", "2026-07-08").unwrap();

    set_task_status(&root, &p, true).unwrap();
    assert!(std::fs::read_to_string(&p).unwrap().contains("status: done\n"));
    set_task_status(&root, &p, false).unwrap();
    assert!(std::fs::read_to_string(&p).unwrap().contains("status: new\n"));

    // A path outside the root is refused.
    let outside = dir.path().join("outside.md");
    std::fs::write(&outside, "---\ntype: Task\nstatus: new\n---\n").unwrap();
    assert!(set_task_status(&root, &outside, true).is_err());
}

#[cfg(unix)]
#[test]
fn set_task_status_rejects_symlinked_file_escaping_root() {
    // Canonicalization (not a lexical starts_with) must catch a task file that
    // is a symlink pointing outside the tasks root — the write would otherwise
    // land outside the vault.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Tasks");
    std::fs::create_dir_all(&root).unwrap();
    let real = dir.path().join("elsewhere.md");
    std::fs::write(&real, "---\ntype: Task\nstatus: new\n---\n").unwrap();
    let link = root.join("2026-07-08-linked.md");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert!(set_task_status(&root, &link, true).is_err());
}
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri/core && cargo test tasks::tests::set_status_flips_only_the_status_line_preserving_body`
Expected: FAIL — `cannot find function set_status`.

- [ ] **Step 3: Implement `set_status` and `set_task_status`**

Add to `tasks.rs`:

```rust
/// Return `content` with the frontmatter `status:` line set to `new_status`,
/// preserving every other line and its exact ending. If the frontmatter has no
/// `status:` line, insert one at the closing fence (a hand-authored `type:
/// Task` file the list surfaces must stay toggleable). `None` only if the file
/// is not `type: Task` — then the caller skips + warns.
pub fn set_status(content: &str, new_status: &str) -> Option<String> {
    if !is_task(content) {
        return None;
    }
    // Split keeping line endings so CRLF is preserved verbatim.
    let mut out = String::with_capacity(content.len() + 16);
    let mut in_frontmatter = false;
    let mut seen_open = false;
    let mut done = false;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if !seen_open {
            // First line is the opening `---` (guaranteed by is_task).
            seen_open = true;
            in_frontmatter = true;
            out.push_str(line);
            continue;
        }
        if in_frontmatter && trimmed == "---" {
            // Closing fence: if no status line was found, insert one now,
            // matching this fence line's ending so CRLF stays CRLF.
            if !done {
                let ending = &line[trimmed.len()..]; // "\r\n", "\n", or ""
                out.push_str(&format!("status: {new_status}{ending}"));
                done = true;
            }
            in_frontmatter = false;
            out.push_str(line);
            continue;
        }
        if in_frontmatter && !done && trimmed.starts_with("status:") {
            let ending = &line[trimmed.len()..]; // "\r\n", "\n", or ""
            out.push_str(&format!("status: {new_status}{ending}"));
            done = true;
            continue;
        }
        out.push_str(line);
    }
    done.then_some(out)
}

/// Flip a task's completion status on disk. Canonicalizes `root` and `path`
/// and requires containment — a lexical check can't see through a symlink at
/// the file or folder — then reads, applies `set_status`, and writes atomically
/// (hidden `create_new` temp + fsync + REPLACING rename). Replacing is correct
/// here: the target is the `type: Task` file we just read and are editing in
/// place, and we touch only its status line (see the spec's surgical-write rule).
pub fn set_task_status(root: &Path, path: &Path, done: bool) -> Result<(), String> {
    // Canonical containment: resolve both and require the file under the root.
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    let updated = set_status(&content, if done { "done" } else { "new" })
        .ok_or("Not a Vault Buddy task (not a type: Task document)")?;

    let dir = canon_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = canon_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Owned temp with suffix-retry — a crash between create_new and rename can
    // strand a predictable temp; without retry every future toggle of the same
    // task would fail AlreadyExists on that stranded temp and be permanently
    // stuck. Mirrors write_note_atomic's numbered-temp loop; the marker suffix
    // lets recovery clean strays.
    use std::io::Write;
    let (tmp, mut f) = {
        let mut attempt = 0u32;
        loop {
            let candidate = if attempt == 0 {
                dir.join(format!(".{file_name}{}", crate::capture_note::NOTE_TMP_SUFFIX))
            } else {
                dir.join(format!(
                    ".{file_name}.{attempt}{}",
                    crate::capture_note::NOTE_TMP_SUFFIX
                ))
            };
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(f) => break (candidate, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
                Err(e) => return Err(format!("Cannot stage task write: {e}")),
            }
        }
    };
    f.write_all(updated.as_bytes())
        .map_err(|e| format!("Cannot write task: {e}"))?;
    f.sync_all().map_err(|e| format!("Cannot flush task: {e}"))?;
    drop(f);
    let result = std::fs::rename(&tmp, &canon_path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(|e| format!("Cannot save task: {e}"))
}
```

Note: `NOTE_TMP_SUFFIX` is `pub` in `capture_note.rs`. The replacing `std::fs::rename` is intentional and matches the spec's documented surgical-write rule.

- [ ] **Step 4: Run tests + fmt + clippy**

Run: `cd src-tauri/core && cargo test tasks:: && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/tasks.rs
git commit -m "feat(tasks): flip task status with a surgical atomic write"
```

---

### Task 6: Shell — tasks config IPC commands

**Files:**
- Create: `src-tauri/src/task_commands.rs`
- Modify: `src-tauri/src/capture_commands.rs` (`set_capture_config` — preserve `tasks_folder`)
- Modify: `src-tauri/src/lib.rs` (add `mod task_commands;` near the other `mod` lines ~top; register commands in `generate_handler!` ~256-287)

**Interfaces:**
- Consumes: `capture_config`, `capture_paths`, `discovery`, `tasks` (core); `ConfigWriteLock` (from `capture_commands`); `lock_ignoring_poison`.
- Produces (IPC): `get_tasks_config(id) -> TasksConfigDto { tasksFolder }`; `set_tasks_config(lock, id, tasksFolder) -> Result<(), String>`.

*This crate does not compile on Linux — mirror `capture_commands::get_capture_config`/`set_capture_config` exactly, run `cargo fmt --check`, and rely on CI's Windows job. Its behavior is exercised from the frontend via `mockIPC` in Task 11.*

- [ ] **Step 1: Preserve `tasks_folder` when saving capture settings**

Task 1 added `tasks_folder` to `VaultCaptureConfig`, but `set_capture_config`
(`src-tauri/src/capture_commands.rs`) rebuilds the whole struct from
`CaptureConfigDto` — which has no tasks field. After Task 1 that struct literal
won't compile (missing field), and naively defaulting it to `None` would wipe a
configured tasks folder every time the user saves Capture Settings. Load the
existing value and carry it across. In `set_capture_config`, immediately after
the `let _guard = lock_ignoring_poison(&lock.0);` line and before
`let value = capture_config::VaultCaptureConfig {`, add:

```rust
    // Preserve fields CaptureConfigDto doesn't carry (tasks are configured on
    // their own surface) so saving capture settings can't reset them.
    let existing = capture_config::vault_config(&capture_config::load_config(), &id);
```

and add this field inside the `VaultCaptureConfig { … }` literal (e.g. after
`follow_up_template: cfg.follow_up_template,`):

```rust
        tasks_folder: existing.tasks_folder,
```

- [ ] **Step 2: Create the module**

Create `src-tauri/src/task_commands.rs`:

```rust
use std::path::Path;
use tauri::Manager;
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths, discovery, tasks};

use crate::capture_commands::ConfigWriteLock;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksConfigDto {
    pub tasks_folder: Option<String>,
}

/// The vault's configured tasks folder (or None → the frontend shows the
/// default "Tasks"). Unknown vaults return None — never an error.
#[tauri::command]
pub fn get_tasks_config(id: String) -> TasksConfigDto {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    TasksConfigDto {
        tasks_folder: cfg.tasks_folder,
    }
}

/// Persist the vault's tasks folder. Validates the folder stays inside the
/// vault BEFORE writing (an invalid folder is an inline error, nothing is
/// saved), serialized behind ConfigWriteLock so a concurrent per-vault write
/// isn't lost. Read-modify-write preserves the vault's other config.
#[tauri::command]
pub fn set_tasks_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    tasks_folder: Option<String>,
) -> Result<(), String> {
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let folder = tasks_folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string);
    if let Some(folder) = &folder {
        capture_paths::safe_recording_root(Path::new(&vault.path), folder)?;
    }
    let _guard = lock_ignoring_poison(&lock.0);
    let mut value = capture_config::vault_config(&capture_config::load_config(), &id);
    value.tasks_folder = folder;
    capture_config::update_vault_config(&id, value)
}
```

- [ ] **Step 3: Register the module + commands**

In `src-tauri/src/lib.rs`, add `mod task_commands;` beside the other `mod` declarations, and add to the `generate_handler!` list (after `capture_commands::rename_capture`, adding a comma to that line):

```rust
            capture_commands::rename_capture,
            task_commands::get_tasks_config,
            task_commands::set_tasks_config,
```

- [ ] **Step 4: Format check**

Run: `cd src-tauri && cargo fmt --check`
Expected: PASS (no diff). Full compile is verified by CI's `windows-app` job.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/task_commands.rs src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "feat(tasks): add tasks config commands, preserve folder on capture save"
```

---

### Task 7: Shell — task list/add/toggle IPC commands

**Files:**
- Modify: `src-tauri/src/task_commands.rs`
- Modify: `src-tauri/src/lib.rs` (register three more commands)

**Interfaces:**
- Consumes: everything from Task 6 plus `tasks::{list_tasks, create_task, set_task_status}`.
- Produces (IPC):
  - `list_tasks(id) -> Vec<TaskDto>` (read-only; degrades to empty)
  - `add_task(id, title) -> Result<TaskDto, String>`
  - `set_task_status(id, path, done) -> Result<(), String>`
  - `TaskDto { path, title, status, created, done }` (camelCase).

*Same Linux-compile caveat as Task 6.*

- [ ] **Step 1: Add the DTO + helper**

Add to `task_commands.rs`:

```rust
use std::path::PathBuf;
use vault_buddy_core::capture_config::VaultCaptureConfig;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub path: String,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
}

impl TaskDto {
    fn from_item(t: tasks::TaskItem) -> Self {
        Self {
            path: t.path.to_string_lossy().into_owned(),
            title: t.title,
            status: t.status,
            created: t.created,
            done: t.done,
        }
    }
}

/// Resolve a vault id to (vault path, lexically-safe tasks root). Shared by
/// list/add/toggle so folder resolution lives in one place; the canonical
/// escape check is applied per-command (skip-on-read, error-on-write) since
/// it needs the folder to exist.
fn tasks_root_for(id: &str) -> Result<(PathBuf, PathBuf), String> {
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let cfg: VaultCaptureConfig =
        capture_config::vault_config(&capture_config::load_config(), id);
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), cfg.tasks_root())?;
    Ok((PathBuf::from(&vault.path), root))
}
```

- [ ] **Step 2: Add the three commands**

```rust
/// Read-only list of a vault's tasks. Unknown vault / unsafe folder / missing
/// folder → empty list, never an error (mirrors list_recordings). Never writes.
#[tauri::command]
pub fn list_tasks(id: String) -> Vec<TaskDto> {
    let Ok((vault_path, root)) = tasks_root_for(&id) else {
        return Vec::new();
    };
    // Canonicalize before scanning: a symlinked tasks folder could otherwise
    // enumerate/read frontmatter outside the vault. A merely missing folder
    // degrades quietly (list_tasks returns empty); an escape is warned.
    if root.exists() {
        if let Err(e) = capture_paths::assert_root_inside_vault(&vault_path, &root) {
            log::warn!("list_tasks: tasks folder resolves outside the vault: {e}");
            return Vec::new();
        }
    }
    tasks::list_tasks(&root)
        .into_iter()
        .map(TaskDto::from_item)
        .collect()
}

/// Create a task from a title (creating the tasks folder if needed). Rejects
/// an empty title; returns the created task so the UI can prepend it.
#[tauri::command]
pub fn add_task(id: String, title: String) -> Result<TaskDto, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A task needs a title.".to_string());
    }
    let (vault_path, root) = tasks_root_for(&id)?;
    // Create the folder, THEN canonicalize-verify it stays inside the vault
    // before any task file is written — the exact create-then-assert order the
    // capture recording folder uses (capture_commands.rs). No vault DATA is
    // written before the assert passes; a symlinked folder can at worst create
    // a stray empty dir, then this errors out (create_task never runs).
    std::fs::create_dir_all(&root).map_err(|e| format!("Could not create tasks folder: {e}"))?;
    capture_paths::assert_root_inside_vault(&vault_path, &root)?;
    // Local calendar date (YYYY-MM-DD), matching every other date-sensitive
    // path in the app (capture uses chrono::Local::now().date_naive()). A UTC
    // date would name a task with tomorrow's/yesterday's date near local
    // midnight. Passed into the clock-free core so core stays testable.
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
    let path = tasks::create_task(&root, title, &today)
        .map_err(|e| format!("Could not create task: {e}"))?;
    Ok(TaskDto {
        path: path.to_string_lossy().into_owned(),
        title: title.to_string(),
        status: "new".to_string(),
        created: today,
        done: false,
    })
}

/// Flip a task's completion status. The path (from list_tasks) is re-validated
/// inside the vault's tasks root by `tasks::set_task_status`.
#[tauri::command]
pub fn set_task_status(id: String, path: String, done: bool) -> Result<(), String> {
    let (_vault_path, root) = tasks_root_for(&id)?;
    // Core canonicalizes root + path and requires containment before writing.
    tasks::set_task_status(&root, Path::new(&path), done)
}
```

*`chrono` is already a shell dependency (`chrono = { version = "0.4", … features = ["clock"] }`) and `chrono::Local::now().date_naive()` is the app's established local-date idiom (see `capture_commands.rs`'s dated recording folder). No new dependency, no hand-rolled UTC date.*

- [ ] **Step 3: Register the commands**

In `lib.rs` `generate_handler!`, after `task_commands::set_tasks_config,`:

```rust
            task_commands::list_tasks,
            task_commands::add_task,
            task_commands::set_task_status,
```

- [ ] **Step 4: Format check + commit**

Run: `cd src-tauri && cargo fmt --check`
Expected: PASS.

```bash
git add src-tauri/src/task_commands.rs src-tauri/src/lib.rs
git commit -m "feat(tasks): add list_tasks/add_task/set_task_status commands"
```

---

### Task 8: Frontend — types + vaults store `tasks` view

**Files:**
- Modify: `src/types.ts`
- Modify: `src/stores/vaults.ts`
- Test: `tests/vaults-store.test.ts`

**Interfaces:**
- Produces:
  - `TaskItem { path: string; title: string; status: string; created: string; done: boolean }` and `TasksConfig { tasksFolder: string | null }` in `types.ts`.
  - Store: `view` union gains `"tasks"`; `tasksVaultId: string | null`; `openTasks(vaultId)`; `showList()` clears `tasksVaultId`; `back()` from `tasks` → `showList()`.

- [ ] **Step 1: Write the failing store tests**

Add to `tests/vaults-store.test.ts` (mirror the existing describe/it style there):

```ts
it("openTasks sets the tasks view and vault id", () => {
  const store = useVaultsStore();
  store.openTasks("v1");
  expect(store.view).toBe("tasks");
  expect(store.tasksVaultId).toBe("v1");
});

it("back() from tasks returns to the list and clears the vault id", () => {
  const store = useVaultsStore();
  store.openTasks("v1");
  store.back();
  expect(store.view).toBe("list");
  expect(store.tasksVaultId).toBeNull();
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/vaults-store.test.ts`
Expected: FAIL — `openTasks is not a function`.

- [ ] **Step 3: Implement the store + types changes**

In `src/types.ts` append:

```ts
export interface TaskItem {
  path: string;
  title: string;
  status: string;
  created: string;
  done: boolean;
}

export interface TasksConfig {
  tasksFolder: string | null;
}
```

In `src/stores/vaults.ts`:
- Add `"tasks"` to the `view` union (after `"transcriptions"`).
- Add state field after `recordModeVaultId`: `tasksVaultId: null as string | null,`
- In `showList()` add: `this.tasksVaultId = null;`
- Add action beside `openRecordMode`:

```ts
    openTasks(vaultId: string) {
      this.view = "tasks";
      this.tasksVaultId = vaultId;
    },
```

- In `back()` add a branch before the final `else` (tasks' parent is the list):

```ts
      } else if (this.view === "tasks") {
        return this.showList();
```

- [ ] **Step 4: Run tests**

Run: `npx vitest run tests/vaults-store.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src/stores/vaults.ts tests/vaults-store.test.ts
git commit -m "feat(ui): add tasks view to the vaults store"
```

---

### Task 9: Frontend — Tasks button on each vault row

**Files:**
- Modify: `src/components/VaultList.vue` (emits ~13-18; add a button in the row action group ~157-179, before the capture button)
- Test: `tests/vault-list.test.ts`

**Interfaces:**
- Consumes: nothing new.
- Produces: `VaultList` emits `open-tasks` with the vault id when its Tasks button is clicked.

- [ ] **Step 1: Write the failing test**

Add to `tests/vault-list.test.ts` (mirror the existing mount + emit assertions):

```ts
it("emits open-tasks with the vault id", async () => {
  const wrapper = mountList(); // use the file's existing mount helper
  await wrapper.get('[data-testid="open-tasks"]').trigger("click");
  expect(wrapper.emitted("open-tasks")?.[0]).toEqual(["v1"]);
});
```

If the test file has no shared `mountList` helper, mount directly as the other tests in the file do, passing a single vault with `id: "v1"`.

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/vault-list.test.ts`
Expected: FAIL — selector `[data-testid="open-tasks"]` not found.

- [ ] **Step 3: Implement the button**

In `VaultList.vue` `defineEmits`, add:

```ts
  (e: "open-tasks", id: string): void;
```

Insert this button immediately before the capture (mic) button (the `@click="$emit('capture', …)"` one), so row order is open · daily · **tasks** · capture · settings:

```html
        <button
          type="button"
          data-testid="open-tasks"
          class="mr-1 shrink-0 cursor-pointer rounded-lg p-1.5 text-slate-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
          :disabled="busyVaultId !== null"
          :aria-label="`Tasks in ${accessibleName(vault)}`"
          title="Tasks"
          @click="$emit('open-tasks', vault.id)"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path d="M9 11l3 3 8-8" />
            <path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h9" />
          </svg>
        </button>
```

- [ ] **Step 4: Run tests**

Run: `npx vitest run tests/vault-list.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/VaultList.vue tests/vault-list.test.ts
git commit -m "feat(ui): add a Tasks button to each vault row"
```

---

### Task 10: Frontend — ActionPanel wiring for the tasks view

**Files:**
- Modify: `src/components/ActionPanel.vue` (import + header title ~71-83; view switch ~205-219; VaultList wiring ~229-233)
- Test: `tests/action-panel.test.ts`

**Interfaces:**
- Consumes: `Tasks.vue` (Task 11) — a forward reference; this task imports it, so Task 11 must land for the app to build, but the ActionPanel test here stubs it.
- Produces: `tasks` view renders `<Tasks :vault-id>`, header shows "Tasks" + back button; `@open-tasks` on `<VaultList>` calls `store.openTasks`.

- [ ] **Step 1: Write the failing test**

Add to `tests/action-panel.test.ts` (mirror how the file mounts `ActionPanel` and drives `store.view`):

```ts
it("renders the Tasks view with a back button when view is tasks", async () => {
  const store = useVaultsStore();
  store.openTasks("v1");
  const wrapper = mountPanel(); // the file's existing mount helper
  await flushPromises();
  expect(wrapper.find('[data-testid="back-button"]').exists()).toBe(true);
  expect(wrapper.text()).toContain("Tasks");
});
```

Stub the child so the panel test doesn't hit IPC — extend the file's existing
`mount(..., { global: { stubs: { … } } })` with `Tasks: true` (or add a stubs
map if the file mounts without one).

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/action-panel.test.ts`
Expected: FAIL — no back button / "Tasks" title for the `tasks` view (and/or unknown component `Tasks`).

- [ ] **Step 3: Implement the wiring**

In `ActionPanel.vue` `<script setup>`, add the import beside the other view imports:

```ts
import Tasks from "./Tasks.vue";
```

Header title — extend the ternary chain (add before the final `: "Vaults"`):

```
                    : view === "tasks"
                      ? "Tasks"
```

(The `v-else` back button already covers any non-`list` view, so no header-button change is needed.)

View switch — add before the final `<div v-else>` (the VaultList slot):

```html
    <div
      v-else-if="view === 'tasks' && store.tasksVaultId"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <Tasks :key="store.tasksVaultId" :vault-id="store.tasksVaultId" />
    </div>
```

VaultList wiring — add to the `<VaultList>` tag's listeners:

```html
        @open-tasks="store.openTasks($event)"
```

- [ ] **Step 4: Run tests**

Run: `npx vitest run tests/action-panel.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/ActionPanel.vue tests/action-panel.test.ts
git commit -m "feat(ui): route the tasks panel view to Tasks.vue"
```

---

### Task 11: Frontend — `Tasks.vue` component

**Files:**
- Create: `src/components/Tasks.vue`
- Test: `tests/tasks.test.ts`

**Interfaces:**
- Consumes (IPC): `get_tasks_config`, `list_tasks`, `add_task`, `set_task_status`, `set_tasks_config`; notifications store; `TaskItem`/`TasksConfig` types.
- Produces: a self-contained per-vault Tasks view (props `{ vaultId: string }`), mirroring `Recordings.vue`.

- [ ] **Step 1: Write the failing tests**

Create `tests/tasks.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Tasks from "../src/components/Tasks.vue";
import type { TaskItem } from "../src/types";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const sample: TaskItem[] = [
  { path: "C:/v/Tasks/2026-07-08-b.md", title: "B open", status: "new", created: "2026-07-08", done: false },
  { path: "C:/v/Tasks/2026-07-06-a.md", title: "A done", status: "done", created: "2026-07-06", done: true },
];

function mountView(handlers: Partial<Record<string, (args: unknown) => unknown>> = {}) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  let list = [...sample];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "get_tasks_config") return { tasksFolder: null };
    if (cmd === "list_tasks") return list;
    if (cmd === "add_task") {
      const created = { path: "C:/v/Tasks/2026-07-08-new.md", title: (args as { title: string }).title, status: "new", created: "2026-07-08", done: false };
      list = [created, ...list];
      return created;
    }
    if (cmd === "set_task_status") return null;
    if (cmd === "set_tasks_config") return null;
  });
  const wrapper = mount(Tasks, { props: { vaultId: "v1" } });
  return { wrapper, calls };
}

describe("Tasks", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("loads config and tasks for the vault on mount", async () => {
    const { calls } = mountView();
    await flushPromises();
    expect(calls.find((c) => c.cmd === "list_tasks")).toEqual({ cmd: "list_tasks", args: { id: "v1" } });
    expect(calls.find((c) => c.cmd === "get_tasks_config")).toBeTruthy();
  });

  it("renders open tasks before done ones", async () => {
    const { wrapper } = mountView();
    await flushPromises();
    const rows = wrapper.findAll('[data-testid="task-row"]');
    expect(rows[0].text()).toContain("B open");
    expect(rows[1].text()).toContain("A done");
  });

  it("adds a task from the input", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-input"]').setValue("Ship it");
    await wrapper.get('[data-testid="task-add"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "add_task")).toEqual({ cmd: "add_task", args: { id: "v1", title: "Ship it" } });
    expect(wrapper.text()).toContain("Ship it");
  });

  it("toggles a task via set_task_status", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="task-checkbox"]').trigger("change");
    await flushPromises();
    const call = calls.find((c) => c.cmd === "set_task_status");
    expect(call?.args).toMatchObject({ id: "v1", path: "C:/v/Tasks/2026-07-08-b.md", done: true });
  });

  it("saves a new tasks folder", async () => {
    const { wrapper, calls } = mountView();
    await flushPromises();
    await wrapper.get('[data-testid="tasks-folder-input"]').setValue("Inbox/Tasks");
    await wrapper.get('[data-testid="tasks-folder-save"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_tasks_config")).toEqual({
      cmd: "set_tasks_config",
      args: { id: "v1", tasksFolder: "Inbox/Tasks" },
    });
  });
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/tasks.test.ts`
Expected: FAIL — cannot find `src/components/Tasks.vue`.

- [ ] **Step 3: Implement `Tasks.vue`**

Create `src/components/Tasks.vue`:

```vue
<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { TaskItem, TasksConfig } from "../types";

const props = defineProps<{ vaultId: string }>();
const notifications = useNotificationsStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const tasks = ref<TaskItem[]>([]);
const newTitle = ref("");
const folder = ref(""); // empty shows the "Tasks" placeholder
const adding = ref(false);

function sortInPlace() {
  // Open first, newest created first, then title — mirrors the backend order
  // so an optimistic insert lands where a refetch would put it.
  tasks.value.sort(
    (a, b) =>
      Number(a.done) - Number(b.done) ||
      b.created.localeCompare(a.created) ||
      a.title.localeCompare(b.title),
  );
}

async function reload() {
  try {
    tasks.value = await invoke<TaskItem[]>("list_tasks", { id: props.vaultId });
  } catch (e) {
    loadError.value = String(e);
  }
}

onMounted(async () => {
  try {
    const cfg = await invoke<TasksConfig>("get_tasks_config", { id: props.vaultId });
    folder.value = cfg.tasksFolder ?? "";
    await reload();
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});

async function add() {
  const title = newTitle.value.trim();
  if (!title || adding.value) return;
  adding.value = true;
  try {
    const created = await invoke<TaskItem>("add_task", { id: props.vaultId, title });
    tasks.value.unshift(created);
    sortInPlace();
    newTitle.value = "";
  } catch (e) {
    notifications.error(String(e));
    logWarning(`add_task failed: ${String(e)}`);
  } finally {
    adding.value = false;
  }
}

async function toggle(task: TaskItem) {
  const done = !task.done;
  // Optimistic: flip locally, revert + notify on failure.
  task.done = done;
  task.status = done ? "done" : "new";
  sortInPlace();
  try {
    await invoke("set_task_status", { id: props.vaultId, path: task.path, done });
  } catch (e) {
    task.done = !done;
    task.status = done ? "new" : "done";
    sortInPlace();
    notifications.error(String(e));
    logWarning(`set_task_status failed: ${String(e)}`);
  }
}

async function saveFolder() {
  const value = folder.value.trim();
  try {
    await invoke("set_tasks_config", {
      id: props.vaultId,
      tasksFolder: value === "" ? null : value,
    });
    await reload();
  } catch (e) {
    notifications.error(String(e));
    logWarning(`set_tasks_config failed: ${String(e)}`);
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center gap-1">
      <input
        v-model="folder"
        data-testid="tasks-folder-input"
        type="text"
        placeholder="Tasks"
        aria-label="Tasks folder"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        @keydown.enter="saveFolder"
      />
      <button
        type="button"
        data-testid="tasks-folder-save"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="saveFolder"
      >
        Save
      </button>
    </div>

    <div class="flex items-center gap-1">
      <input
        v-model="newTitle"
        data-testid="task-input"
        type="text"
        placeholder="Add a task…"
        aria-label="New task title"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        @keydown.enter="add"
      />
      <button
        type="button"
        data-testid="task-add"
        :disabled="adding || newTitle.trim() === ''"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
        @click="add"
      >
        Add
      </button>
    </div>

    <p v-if="loading" class="text-xs text-slate-400">Loading…</p>
    <p
      v-else-if="loadError"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ loadError }}
    </p>
    <p v-else-if="tasks.length === 0" class="text-xs text-slate-400">
      No tasks yet.
    </p>
    <ul v-else class="flex flex-col gap-1">
      <li
        v-for="task in tasks"
        :key="task.path"
        data-testid="task-row"
        class="flex items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1"
      >
        <input
          type="checkbox"
          data-testid="task-checkbox"
          :checked="task.done"
          :aria-label="`Mark ${task.title} ${task.done ? 'not done' : 'done'}`"
          class="shrink-0 cursor-pointer accent-violet-500"
          @change="toggle(task)"
        />
        <span
          class="min-w-0 flex-1 truncate text-sm"
          :class="task.done ? 'text-slate-500 line-through' : 'text-slate-100'"
          :title="task.title"
        >
          {{ task.title }}
        </span>
      </li>
    </ul>
  </div>
</template>
```

- [ ] **Step 4: Run tests + full suite + typecheck**

Run: `npx vitest run tests/tasks.test.ts && npm test && npm run build`
Expected: PASS (all suites green; `vue-tsc` typecheck clean).

- [ ] **Step 5: Commit**

```bash
git add src/components/Tasks.vue tests/tasks.test.ts
git commit -m "feat(ui): add the per-vault Tasks view"
```

---

## Self-Review

**Spec coverage:**
- Configure per-vault tasks folder → Task 1 (field) + Task 6 (commands) + Task 11 (UI). ✓
- Todo list derived from folder → Task 4 (`list_tasks`) + Task 7 (command) + Task 11. ✓
- Add a task from the vault list view → Task 3/7 (create) + Task 9 (row button) + Task 11 (add box). ✓
- Check tasks off (toggle write) → Task 5 (surgical flip) + Task 7 + Task 11. ✓
- `type: Task` / `status: new`/`done` frontmatter → Task 2 (render) + Global Constraints. ✓
- Never-clobber (create) + surgical replacing write (toggle) + path safety → Tasks 3, 5, 7 + Global Constraints. ✓
- Panel view + row button + header/back → Tasks 8, 9, 10. ✓
- Default folder `Tasks`; completed = `done` → Global Constraints, Tasks 1/5/11. ✓
- Out-of-scope items (due dates, priority, lists, aggregation, Task Tags, inline-Todo scan, templates, quick-add modal, open-in-Obsidian) → intentionally absent. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code. The two "reuse if it already exists" notes (`dir_entries` visibility in Task 4, `today_ymd`/`civil_from_days` in Task 7, `tempfile` dev-dep in Task 3) are concrete verification steps with a grep and a fallback, not deferred work.

**Type consistency:** `TaskItem { path, title, status, created, done }` is identical across Rust (`tasks::TaskItem`, Task 4), the DTO (`TaskDto`, Task 7), and TS (`types.ts`, Task 8). `TasksConfigDto { tasksFolder }` (Task 6) ↔ `TasksConfig { tasksFolder }` (Task 8). Command names/args match between the shell commands (Tasks 6–7) and the frontend `invoke` calls / test mocks (Task 11): `get_tasks_config {id}`, `set_tasks_config {id, tasksFolder}`, `list_tasks {id}`, `add_task {id, title}`, `set_task_status {id, path, done}`. `openTasks`/`tasksVaultId`/`view: "tasks"` consistent across Tasks 8 and 10.
