# Tasks Feature Improvements — Lists-first, drag-default, and per-vault Task IDs

- **Date:** 2026-07-15
- **Status:** Approved
- **Source:** User request to further improve the tasks feature: make Lists
  the first (and default) grouping, make it easy to add a new list, make
  drag-and-drop the standard sort, and add a per-vault setting that stamps a
  generated ID onto tasks under a configurable frontmatter property (default
  `task-id`). Builds directly on the
  [task-lists / sorting / ordering increment](2026-07-11-task-lists-sorting-ordering-design.md).

## Goals

1. **Lists first, and active on open** — the `Lists | Dates | Tags` grouping
   toggle leads with Lists, and the tasks view opens grouped by Lists.
2. **Easy list creation** — a discoverable "New list" control in the Lists
   grouping view (per-vault), on top of the existing composer flow.
3. **Drag-and-drop is the standard sort** — Manual becomes the default sort
   preference, so unconfigured views open with drag handles visible.
4. **Per-vault Task IDs** — a Vault-settings toggle that generates a short
   random ID for a task and writes it under a configurable frontmatter
   property (default `task-id`), assigned to **new** tasks and stamped onto
   an existing task the next time it is created/edited/reordered.

Non-goals: surfacing the ID in the task rows (frontmatter only this slice),
a bulk backfill of existing tasks, sequential/human-referenceable IDs,
cross-vault list ordering, and persisting the grouping choice across opens.

## A. Lists-first grouping + open-on-Lists

Purely frontend, no new state shape:

- `TaskViewControls.vue`: reorder `GROUPINGS` to `[lists, dates, tags]` so
  Lists renders as the first radio.
- `Tasks.vue`: the component-local grouping default flips
  `ref<"dates" | "tags" | "lists">("dates")` → `ref(... )("lists")`. Grouping
  is deliberately **not** persisted (unchanged), so every panel visit opens
  on Lists.
- Everything downstream is unchanged: `buckets` already branches on
  `grouping === "lists"` into `listSections(...)` with `includeEmpty` in
  per-vault non-filtering mode and skip-empty in the aggregate; the
  `hasDisplayableLists` fallback that keeps the control reachable for a vault
  with only empty lists still applies.

## B. Drag-and-drop as the default sort

One constant in `utils/taskSort.ts`:

- `DEFAULT_PREF` `{ key: "default", dir: "asc" }` → `{ key: "manual", dir: "asc" }`.

Consequences, all already handled by existing machinery:

- Sort is persisted **per view** (`vault-buddy:task-sort`, keyed vault id /
  `"all"`). `loadSortPref` returns a stored pref when present, else
  `DEFAULT_PREF`. So a user who explicitly chose a sort keeps it; an
  unconfigured view now resolves to Manual.
- Manual + no active filter makes `reorderView` true → the grip handles and
  keyboard reorder show on open. This is the intended "drag-and-drop is
  standard" experience, and it pairs with the Lists default so each list
  section is independently reorderable.
- The Manual comparator sorts unranked tasks (`order === null`) last and
  falls through to the historical Default chain, so a never-dragged list is
  byte-order-identical to what Default produced — the change is invisible
  until the user drags.
- `directionApplies("manual")` stays `false`, so the direction toggle is
  correctly disabled by default.

## C. Easy list creation in the Lists view

The existing creation path (the composer's `TaskListPicker` "New list…"
option) stays. This adds a **second, more discoverable** entry point:

- A "New list" affordance rendered **only when** `grouping === "lists"` **and
  not** aggregate mode (`isAggregate` false). The aggregate has no single
  target vault, so list creation there is out of scope for this slice.
- Interaction mirrors `TaskListPicker`'s inline create: a "＋ New list"
  button reveals a text input + confirm/cancel; Enter confirms (IME-guarded,
  `isComposing` early-out; Escape `stopPropagation` so it doesn't close the
  panel — the GAP-27/GAP-31 precedents).
- It lives in `TaskViewControls.vue` (which already owns the grouping row),
  emitting a `create-list` event with the trimmed name; `Tasks.vue` wires it
  to `useTaskLists.createList`, which is `composerVaultId ?? vaultId` scoped
  (= the current vault in per-vault mode) and folds the landed name into
  `vaultLists`. Because `knownLists` derives from `vaultLists`, the new
  (empty) list immediately appears as its own section via `includeEmpty`.
- `creatingList` (already exposed by `useTaskLists`) disables the confirm
  while the create is in flight; a failure is toasted by the composable and
  the input stays open for retry (no value change to close it, matching the
  picker's `resetNonce` discipline — here a simple local reset on success).

`TaskViewControls.vue` gains two props (`isAggregate: boolean`,
`creatingList: boolean`) and the `create-list` emit. It stays presentational
(local input state only); `Tasks.vue` owns the IPC.

## D. Per-vault Task ID generation

### D.1 Config (`core/src/vault_config.rs`)

`VaultCaptureConfig` gains two fields beside the existing tasks settings:

- `task_id_enabled: bool` — default `false` (opt-in, like `transcribe`).
- `task_id_property: Option<String>` — default `None`; resolves to the
  default `"task-id"`.

A resolver `task_id_property_name(&self) -> &str` returns
`self.task_id_property.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("task-id")`
— the single source of the effective property name (mirrors `tasks_root()`).

- `vault_entry` parses both per-field defensively (a quoted bool must not
  enable; a non-string property defaults only itself), matching every other
  field in that function.
- `serialize_vault_entry` writes `taskIdEnabled` only when `true` and
  `taskIdProperty` only when `Some` and non-default — the hand-editable file
  stays minimal (the `recordingDateFolders`/`documentDateFolders` precedent).
- Round-trip + defensive-parse unit tests, alongside the existing lists tests.

JSON keys: `taskIdEnabled`, `taskIdProperty`. Reserved-key rejection and
validation live in the write path (D.4), not the parser (reads stay lenient).

### D.2 ID generation (`core/src/tasks`)

- `new_task_id() -> String` — 8 characters of base36 (`0-9a-z`) drawn from a
  CSPRNG. No vault scan, collision-safe across synced devices/git. Unit-tested
  by **format** (length 8, charset) and weak uniqueness across many calls, not
  a fixed value. The randomness comes from a CSPRNG already reachable in the
  `core` crate's dependency graph (e.g. `getrandom`, pulled transitively);
  the exact source is confirmed against `cargo machete`/`cargo deny` during
  planning so no unused/dis­allowed dependency is added.

### D.3 Write paths

**Create** (`services::add_task`, shared by IPC + MCP):

- After loading `cfg`, when `cfg.task_id_enabled`, compute
  `(cfg.task_id_property_name(), tasks::new_task_id())` and thread it through
  `tasks::create_task(...) → tasks::render_task(...)`.
- `render_task` gains a `task_id: Option<(&str, &str)>` parameter and, when
  `Some`, emits a `{property}: {id}` line. Placement: immediately after
  `created:` (before `due`/`priority`/`tags`), a plain unquoted scalar (the
  value is charset-safe base36; the property was validated on save). When
  `None`, output is byte-identical to today (pinned by the existing
  render tests).
- `create_task` gains the same `Option<(&str, &str)>` passthrough.

**Stamp on edit** (`update_task` command):

- `tasks::update_task_fields` gains an `ensure_absent: &[(&str, &str)]`
  parameter. After it reads the file content, for each `(key, val)` in
  `ensure_absent` whose `key` is **absent** (`scalar_field(content, key)` is
  `None`), it appends `(key, Some(val))` to the effective updates before
  `set_fields`. Present keys are left untouched — an existing/hand-authored
  ID is never overwritten, so IDs are stable.
- `set_task_status` (core + services) passes `&[]`: a checkbox toggle/archive
  is not an "edit" and does not stamp.
- The shell `update_task` command, inside its blocking closure, loads the
  vault config and — when `task_id_enabled` — builds
  `ensure_absent = [(property, new_task_id())]`, passed to
  `update_task_fields`. Per the reorder decision, **any** `update_task` write
  (field edit, tags, or an `order`-only reorder) stamps a missing ID; a drag
  that materializes ranks across a section may therefore stamp several tasks
  at once (bounded to the visible section, additive, only on unranked rows).
- Existing `update_task_fields`/`set_task_status` callers pass `&[]`; the
  signature change is mechanical and compile-checked.

No change to `TaskItem`/`TaskDto`: the ID is frontmatter-only and the
absence check is server-side, so nothing needs to cross IPC.

### D.4 Settings command + UI

- New command `set_task_id_config(id, enabled, property)` (async, fsync'd
  config write, `ConfigWriteLock`), the independent field-save pattern of
  `set_task_lists_config`. It read-modify-writes the vault entry, preserving
  every other field. Write-strict validation of `property`:
  - trim; empty → store `None` (resolves to the default).
  - reject anything outside `[A-Za-z0-9_-]` or matching a **reserved** task
    key (`type`, `status`, `title`, `created`, `due`, `priority`, `tags`,
    `tag`, `order`) with an inline error naming the token — so the ID writer
    can never clobber a structured field, and the surgical writer's
    `key:`-prefix match stays unambiguous.
- `get_tasks_config` / `TasksConfigDto` gain `task_id_enabled: bool` and
  `task_id_property: String` (the **resolved** name, so the UI shows
  `task-id` when unset). `set_capture_config` already round-trips the whole
  entry; a regression test pins that a capture save preserves the two fields.
- `TasksConfigTab.vue` gains a self-contained "Task IDs" card: a toggle
  ("Generate an ID for each task") and, when on, a "Property name" text input
  (placeholder `task-id`), auto-saved via `useAutosave` → `set_task_id_config`
  (the `TaskListSettings` autosave precedent, including the `saving-change`
  fence if it shares the tab's in-flight coordination). An invalid property
  shows the command's inline error.
- Frontend `TasksConfig` type gains `taskIdEnabled: boolean` and
  `taskIdProperty: string`.

## Error handling

- Config reads never fail: a malformed/absent field defaults itself; an
  invalid stored property resolves to `task-id`. Only the **write** command
  rejects (inline error), matching every other settings command.
- ID stamping is best-effort within the existing write: it rides the same
  atomic temp+fsync+replace as the edit itself. A failed write reverts the
  whole optimistic edit (existing behavior) — the ID isn't a separate write.
- `new_task_id` cannot fail (CSPRNG); if the platform RNG ever errored, the
  chosen source's `expect` is acceptable at this single call site (documented
  in planning).

## Testing

TDD, failing test first. Rust (Linux):

- `vault_config`: default off / `None`; round-trip with values; defensive
  parse (quoted bool, non-string property); `task_id_property_name()`
  resolution; `serialize` omits defaults.
- `tasks::new_task_id`: length/charset; N-call uniqueness (weak).
- `tasks::render_task` / `create_task`: id line present when `Some`, absent
  and byte-identical when `None`, placed after `created`.
- `tasks::update_task_fields`: `ensure_absent` inserts when the key is
  absent, no-ops when present (no overwrite), and preserves CRLF/body;
  `set_task_status` still passes `&[]` (no stamp on toggle).
- Property validation: reserved keys and bad charset rejected; empty → default.
- `services::add_task`: enabled writes `{property}: <8 base36>`; disabled
  writes nothing; custom property honored; MCP path covered by the shared fn.
- Shell `task_commands`: `set_task_id_config` validation; `get_tasks_config`
  returns the fields; `set_capture_config` preserves them; `update_task`
  stamps when enabled & absent.

Frontend (Vitest + happy-dom + mockIPC):

- `TaskViewControls`: grouping order is Lists-first; `create-list` emit and
  its inline input flow; hidden in aggregate.
- `taskSort`: `loadSortPref` with no stored value returns `manual`.
- `Tasks.vue`: opens grouped by Lists; the New-list control calls
  `create_task_list` and the empty section appears.
- `TasksConfigTab`: the Task IDs card renders; toggle + property input
  auto-save via `set_task_id_config`; invalid property surfaces the error.

Coverage floors (`vite.config.ts`) and LOC baselines
(`scripts/loc-baseline.json`) are ratcheted in the same PR if the additions
move them; any Rust coverage floor likewise.

## Docs to update

- `AGENTS.md` — the tasks domain section: the new default grouping/sort, the
  Lists-view create control, and the Task ID config + write paths (the
  seventh field the surgical writer may touch; note it is stamp-if-absent,
  never overwrite).
- `CONTEXT.md` — add **Task ID** to the ubiquitous language (a generated,
  stable frontmatter identifier; distinct from the file path and the manual
  `order` rank).
- `docs/Gaps.md` — record any residual (e.g. changing the property name
  later leaves old-property IDs in place by design; aggregate-mode list
  creation is intentionally omitted).

## Rollout / compatibility

Every change is additive and defaulted-off or default-preserving: the ID
setting starts disabled, existing config files parse unchanged, and the sort
default only affects views with no stored preference. No migration, no mass
vault write.
