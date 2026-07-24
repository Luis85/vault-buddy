import type { AggTask, TaskPatch } from "../types";

// The shared adoption half of the task write paths — pure row mutations, no
// IPC (the composables own their invoke/optimistic/revert strategies).

/** What `move_task_to_list` answers: the landed path (which may carry a
 * ` (N)` collision suffix) plus the task's id — freshly backfilled when the
 * vault opts in and the file lacked one, `null` when IDs are off. */
export type MovedTask = { path: string; id: string | null };

/** Reflect a freshly-stamped task id (update_task / move_task_to_list's
 * return) onto the row so the editor's copy-id affordance shows without a
 * reload. No-op when ids are off (the command returns null). One helper so
 * the edit, reorder, and both move call sites can't drift (review, PR #59). */
export function reflectStampedId(task: AggTask, id: string | null): void {
  if (id) task.id = id;
}

/** Adopt a move result onto the row: the landed path and any stamped id.
 * Shared by the drag (optimistic) and editor-save (non-optimistic) movers —
 * this PR had to patch both by hand to add the id half, which is exactly the
 * drift one adoption helper prevents. */
export function applyMovedTask(task: AggTask, moved: MovedTask): void {
  task.path = moved.path;
  reflectStampedId(task, moved.id);
}

// due and scheduled apply the identical set/clear shape — single-sourced so
// every reflect-a-patch-locally call site (the list's inline editor, the
// detail view) uses the exact same rule instead of risking a hand-mirrored
// copy drifting (the complexFunctions CRAP gate is what first forced this
// out of a single function, GAP-74; keeping a second copy for the detail
// view would have silently reintroduced the drift risk it was extracted to
// avoid).
function applyDateField(
  task: AggTask,
  patch: TaskPatch,
  field: "due" | "scheduled",
  setKey: "due" | "scheduled",
  clearKey: "clearDue" | "clearScheduled",
): void {
  if (patch[clearKey]) task[field] = null;
  else if (patch[setKey]) task[field] = patch[setKey]!;
}

/** Reflect a TaskPatch's scalar fields (title/due/scheduled/priority/tags)
 * onto a task row. Shared by useTaskActions' inline-editor save and
 * useTaskDetail's save. Deliberately excludes `description`: only the detail
 * view edits it (the list's inline editor has no description field), so each
 * caller augments the result with that field itself rather than this helper
 * carrying a field only one of its two callers ever sets. */
export function applyScalarFields(task: AggTask, patch: TaskPatch): void {
  if (patch.title) task.title = patch.title;
  applyDateField(task, patch, "due", "due", "clearDue");
  applyDateField(task, patch, "scheduled", "scheduled", "clearScheduled");
  if (patch.priority) task.priority = patch.priority === "normal" ? null : patch.priority;
  if (patch.tags !== undefined) task.tags = patch.tags;
}

/** Reflect a saved field patch onto a task row, scalar fields plus
 * `description` — the detail view's save. A thin wrapper over
 * `applyScalarFields` (not folded into it: the list's inline editor never
 * touches description, so that helper stays scoped to the fields both
 * callers share) kept here rather than in useTaskDetail so every pure
 * row-mutation reflector is single-sourced in one place. */
export function applyDetailFields(task: AggTask, patch: TaskPatch): void {
  applyScalarFields(task, patch);
  if (patch.clearDescription) task.description = null;
  else if (patch.description !== undefined) task.description = patch.description;
}
