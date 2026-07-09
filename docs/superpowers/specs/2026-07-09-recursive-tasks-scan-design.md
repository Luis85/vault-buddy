# Recursive Tasks-Folder Scan Design

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** Follow-up to the task-management vertical slice (v0.5.0). The
  per-vault tasks list (`core::tasks::list_tasks`) currently scans only the top
  level of the configured tasks folder. Users organize tasks into subfolders,
  so the list must find every task **anywhere under** the configured folder.

## Goal

Change `list_tasks` from a flat scan of the configured tasks folder to a
**recursive walk of that folder's subtree**, surfacing every `type: Task`
document at any depth. The configured per-vault tasks folder (default `Tasks`)
is unchanged and remains the recursion root — the walk never leaves it, and the
vault outside the tasks folder is never scanned.

## Scope

- **Only `core::tasks::list_tasks` changes.** The tasks folder config, the
  `list_tasks` IPC command (`task_commands.rs`), the frontend (`Tasks.vue`), the
  `TaskItem`/`TaskDto` shape, and every other command are untouched. The IPC
  contract is identical (still `TaskDto[]`), so no frontend change is needed.

## Behavior

`list_tasks(root)` walks `root` recursively (`root` is the resolved configured
tasks folder, exactly as today):

- **Descend only into real directories** — entries whose `dir_entries` file type
  `is_dir()`. `dir_entries` reads the dirent *without following symlinks*, so a
  symlinked/junction directory reports as a symlink (not a dir) and is skipped.
  The walk therefore **cannot escape** the tasks folder, preserving the same
  no-symlink-follow escape-safety the flat scan and the shell command's
  `assert_root_inside_vault` already rely on. No new path check is added inside
  the walk.
- **Skip dot-directories** — any directory whose name starts with `.`
  (`.obsidian`, `.git`, `.trash`, …). Config dirs aren't walked and trashed
  tasks aren't surfaced. (Inside a dedicated `Tasks/` folder this rarely
  applies; it is a safe default.)
- **Unlimited depth.** No cycle risk: symlinks aren't followed, so the real
  directory tree is finite and acyclic.
- **Unchanged file matching and output.** Still only `.md` files whose
  frontmatter is `type: Task` (via the existing `is_task` closed-fence check).
  `TaskItem` is unchanged; `path` is absolute, so two files with the same name
  in different subfolders remain distinct entries. The result is one **flat
  sorted list** — open first (`status != "done"`), newest `created` first, then
  title — across the whole subtree. No per-subfolder grouping (a separate,
  future concern).
- **Best-effort, degrade silently.** An unreadable directory or file is skipped;
  a missing root yields an empty list — same as today.

## Implementation shape

Extract the per-file parse into a small private recursive helper and have
`list_tasks` drive it, then sort (sorting stays in `list_tasks` so it runs once
over the full result, not per directory):

```rust
pub fn list_tasks(root: &Path) -> Vec<TaskItem> {
    let mut out = Vec::new();
    collect_tasks(root, &mut out);
    out.sort_by(|a, b| {
        a.done.cmp(&b.done).then(b.created.cmp(&a.created)).then(a.title.cmp(&b.title))
    });
    out
}

/// Recursively collect `type: Task` files under `dir`, best-effort. Descends
/// only into real subdirectories — `dir_entries` never follows symlinks, so a
/// symlinked/junction dir is skipped and the walk can't leave the tasks folder
/// — and skips dot-directories (.obsidian/.trash/.git). Unreadable dirs/files
/// degrade silently.
fn collect_tasks(dir: &Path, out: &mut Vec<TaskItem>) {
    for (path, ft, name) in dir_entries(dir) {
        if ft.is_dir() {
            if !name.starts_with('.') {
                collect_tasks(&path, out);
            }
            continue;
        }
        if !ft.is_file() || !name.ends_with(".md") {
            continue;
        }
        // (unchanged) read → is_task → note_field(title/status/created) → push
    }
}
```

## Testing (core crate, on Linux)

- Tasks nested at multiple depths (`root/a.md`, `root/work/b.md`,
  `root/work/q3/c.md`) are all found and sorted correctly across depths.
- A `type: Task` file inside a dot-directory (`root/.trash/x.md`) is **not**
  surfaced.
- `#[cfg(unix)]`: a symlinked subdirectory pointing outside the tasks folder,
  containing a `type: Task` file, is **not** followed (the file is absent from
  the result).
- A flat root (no subfolders) still returns exactly what it did before.
- Existing `list_tasks` tests (flat, sort order, type-filtering, missing root,
  unterminated-frontmatter skip) continue to pass unchanged.

No frontend or IPC change, so the Vitest suite and shell command are unaffected.
