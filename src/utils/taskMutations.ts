import type { AggTask } from "../types";

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
