# Task Aggregation Design — the cross-vault "All tasks" view

- **Date:** 2026-07-10
- **Status:** Approved
- **Source:** First sub-project of the
  [Aggregated Task Dashboard & Lists](../../use-cases/aggregated-task-dashboard-and-lists.md)
  use case. One merged task view across every configured vault, reusing the
  complete per-vault Tasks machinery (buckets, tag grouping, filters, inline
  editor, toggle/archive/open). User-defined lists, the Quick Task modal,
  bulk operations, and full-text search are separate later increments.

## Goals

1. **One "All tasks" view** merging every vault's tasks, with the full
   existing interactivity — nothing is read-only.
2. **Entry point:** an "All tasks" bar above the vault list showing the
   summed open count.
3. **Vault attribution:** each row shows which vault the task lives in.
4. **Add from the aggregate view** via a vault picker on the add row.

## Architecture: frontend fan-out (no Rust changes)

Aggregation happens client-side over the existing IPC surface. Every row
action (`set_task_status`, `update_task`, `open_task`, `add_task`) already
takes a vault `id` per call, so the interactive surface needs no new
commands; `list_tasks` per vault in parallel is the same pattern
`loadTaskCounts` already uses on every panel open. The shell, core crate,
and IPC registrations are untouched. (A `list_all_tasks` command or a Rust
aggregation service is deliberately rejected for this slice: the shell
would only loop the same core function, and the panel already refreshes
per open — no cache or watcher to justify.)

## Component model (`Tasks.vue`)

- **Prop becomes `vaultId: string | null`** — `null` is aggregate mode.
- **One uniform internal task shape for BOTH modes:**
  `AggTask = TaskItem & { vaultId: string; vaultName: string }`.
  - Per-vault mode: enrich each loaded task with
    `{ vaultId: props.vaultId, vaultName: "" }` (the name is only used by
    the chip, which per-vault mode never renders).
  - Aggregate mode: `list_vaults` first, then `list_tasks` for every vault
    in parallel; enrich each task with its vault's `id`/`name`; merge.
- **Every action reads the ROW's vault** — `task.vaultId` replaces
  `props.vaultId` in `toggle`/`archive`/`saveEdit`/`openInObsidian`
  invokes. No mode branches inside the actions.
- **Load errors are best-effort per vault (aggregate mode):** a vault whose
  `list_tasks` rejects contributes nothing; one error toast names the
  failed vault(s) (`logWarning` too); the other vaults' tasks render. Only
  a failure of `list_vaults` itself (or every vault failing) shows the
  blocking load-error banner. Per-vault mode keeps its current behavior.
- **Sort:** the existing mirror comparator gains a final
  `vaultName` → `path` tiebreak so equal tasks from different vaults order
  stably. (The core sort is unchanged — the tiebreak only matters in the
  merged list, which only the frontend sees.)
- **Everything downstream is unchanged by construction:** date buckets, the
  Dates | Tags grouping toggle, tag/title filters, progress bar, the
  section-scoped `editingKey`, and the per-path `busy` guard all operate on
  the merged list. Task paths are absolute and therefore unique across
  vaults, so row keys and the guard stay collision-free.

## Entry point (`ActionPanel.vue` + `vaults` store)

- **"All tasks" bar** above the vault list, on the list view, rendered when
  at least one vault exists: checklist icon, the label **All tasks**, and a
  count badge showing the sum of the store's `taskCounts` values (already
  loaded on every panel refresh; badge hidden at 0, same rule as the
  per-row badge).
- Clicking it calls **`vaults.openAllTasks()`** — sets `view: "tasks"`,
  `tasksVaultId: null`. (`tasksVaultId` is already `string | null`;
  `openTasks(id)` keeps its signature.)
- The panel **header title reads "All tasks"** when `view === "tasks"` and
  `tasksVaultId === null`, "Tasks" otherwise. Back returns to the list,
  unchanged.

## Vault attribution (aggregate mode only)

Each row renders a small vault chip before the priority dot: the vault
name's first letter in the vault list's avatar style (violet rounded
square, small), with `title` = the full vault name for hover. Per-vault
mode renders no chip and is visually unchanged.

## Add row with a vault picker (aggregate mode only)

- The add row gains a compact vault `SelectMenu` (the existing component,
  as used in CaptureSettings) listing all vaults, defaulting to the first
  vault in the loaded order (alphabetical, matching the vault list).
  Component-local state; no persistence across opens.
- `add_task` is invoked with the picked vault's id; the returned task is
  enriched with that vault's id/name and merged into the list in sort
  order. The due/priority/tags options row works unchanged.
- Per-vault mode's add row is untouched (no picker).

## Error handling summary

- Aggregate load: partial results + toast naming failed vaults; blocking
  banner only when nothing could load.
- Row actions and add: unchanged — optimistic with revert + toast, per-row
  serialization via the path-keyed `busy` Set.

## Testing

- **Fan-out merge:** two vaults' tasks interleave per the global sort; the
  vault-name tiebreak orders an otherwise-equal pair stably.
- **Partial failure:** vault B's `list_tasks` throws → vault A's tasks
  render, one error toast names B, no blocking banner; all-fail → banner.
- **Actions carry the row's vault:** toggling/editing/archiving/opening a
  vault-B task from the merged list sends `id: "vaultB"`.
- **Add routes to the picked vault** and the created task appears merged.
- **Chip renders only in aggregate mode**; per-vault snapshots unchanged.
- **Entry bar:** shows the summed open badge (hidden at 0), opens the
  aggregate view, header reads "All tasks".
- **All existing per-vault tests pass unchanged** (the per-vault mode's
  behavior is bit-identical; only the internal enrichment is new).

## Out of scope (YAGNI)

User-defined lists (Inbox/Next/…), the Quick Task modal, bulk operations,
full-text search, dashboard widget rows (Recently Created, Completed Today,
…), live file-watching/caching, cross-vault dedupe, persisting the add-row
vault picker across panel opens, per-vault grouping mode in the aggregate
view (date/tag grouping already cover triage; a vault section mode can come
with the dashboard increment).
