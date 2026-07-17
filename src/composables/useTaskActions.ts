import { invoke } from "@tauri-apps/api/core";
import { type Ref,ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AggTask, TaskEditorPatch, TaskPatch } from "../types";
import { applyMovedTask, type MovedTask, reflectStampedId } from "../utils/taskMutations";

// The per-row write actions of the Tasks view (toggle / archive / open /
// inline-editor save) plus the busy guard and editing-row state they share.
// Split out of Tasks.vue for the LOC cap — state + IPC, no rendering; every
// action reads the ROW's vaultId (aggregate-safe, no mode branches).
export function useTaskActions(opts: {
  tasks: Ref<AggTask[]>;
  sortInPlace: () => void;
}) {
  const notifications = useNotificationsStore();
  const vaultsStore = useVaultsStore();
  const { tasks, sortInPlace } = opts;

  // Task paths whose write is in flight. A second action on the same row
  // while its write is pending would race the first (on a slow disk the two
  // writes can land out of order, leaving the file disagreeing with the UI),
  // so the row's controls are disabled and re-entrant actions are ignored
  // until it resolves — a toggle and an archive for the same task can't
  // race. A reactive Set so the template's :disabled tracks add/delete.
  // Keyed by path alone: task paths are unique across vaults (two vaults
  // would have to contain the same absolute file), and the aggregation spec
  // documents that assumption — this comment is its code-side anchor.
  const busy = ref(new Set<string>());
  const isBusy = (path: string) => busy.value.has(path);

  async function toggle(task: AggTask) {
    if (busy.value.has(task.path)) return;
    // GAP-32: captured BEFORE the optimistic flip so a failed write can
    // restore the task's actual original status (e.g. "in-progress") instead
    // of forging "new" — the old revert derived the restored value from the
    // just-flipped `done` boolean, which only ever knows "done"/"new".
    const prevStatus = task.status;
    const done = !task.done;
    // Optimistic: flip locally, revert + notify on failure.
    task.done = done;
    task.status = done ? "done" : "new";
    sortInPlace();
    busy.value.add(task.path);
    try {
      await invoke("set_task_status", { id: task.vaultId, path: task.path, status: task.status });
      // Badge refresh (GAP-32 / Codex PR #46): fire-and-forget right after
      // the write resolves — colocated with the success branch rather than a
      // `finally` that also runs on failure.
      void vaultsStore.refreshTaskCount(task.vaultId);
    } catch (e) {
      task.status = prevStatus;
      task.done = prevStatus === "done";
      sortInPlace();
      notifications.error(String(e));
      logWarning(`set_task_status failed: ${String(e)}`);
    } finally {
      busy.value.delete(task.path);
    }
  }

  async function archive(task: AggTask) {
    if (busy.value.has(task.path)) return;
    busy.value.add(task.path);
    // Optimistic: remove from the list; on failure push back + re-sort rather
    // than re-inserting at a captured index (GAP-32: the index goes stale —
    // one slot off — if a concurrent add landed while this write was in
    // flight; recomputing placement via sortInPlace is always correct).
    const index = tasks.value.findIndex((t) => t.path === task.path);
    const removed = tasks.value.splice(index, 1)[0];
    try {
      await invoke("set_task_status", { id: task.vaultId, path: task.path, status: "archived" });
      void vaultsStore.refreshTaskCount(task.vaultId);
    } catch (e) {
      tasks.value.push(removed);
      sortInPlace();
      notifications.error(String(e));
      logWarning(`archive failed: ${String(e)}`);
    } finally {
      busy.value.delete(task.path);
    }
  }

  async function openInObsidian(task: AggTask) {
    try {
      await invoke("open_task", { id: task.vaultId, path: task.path });
      // Obsidian takes over — get the panel out of the way. Panel visibility
      // is owned by Rust (close_panel), best-effort, mirroring the vault-open
      // and recording-open flows. A failed launch falls through to the catch
      // and keeps the panel up so the error toast is visible.
      void invoke("close_panel").catch(() => {});
    } catch (e) {
      notifications.error(String(e));
      logWarning(`open_task failed: ${String(e)}`);
    }
  }

  // Inline editor: one row at a time; opening another row discards unsaved
  // edits in the first (the file is the source of truth, edits are cheap).
  // Keyed on `${bucketKey}:${path}` (not a bare path) so a task rendered in
  // two tag sections opens its editor on only the clicked row. The draft
  // field state and its IME-guarded key handlers live in TaskEditor.
  const editingKey = ref<string | null>(null);
  const rowKey = (bucketKey: string, task: AggTask) => `${bucketKey}:${task.path}`;
  const startEdit = (task: AggTask, bucketKey: string) => {
    editingKey.value = rowKey(bucketKey, task);
  };
  const cancelEdit = () => {
    editingKey.value = null;
  };

  // Optimistic field save: apply locally (re-sort/re-bucket live), revert +
  // toast on failure. Returns whether the write landed.
  async function applyFieldPatch(task: AggTask, patch: TaskPatch): Promise<boolean> {
    const before = { title: task.title, due: task.due, priority: task.priority, tags: task.tags };
    if (patch.title) task.title = patch.title;
    if (patch.clearDue) task.due = null;
    else if (patch.due) task.due = patch.due;
    if (patch.priority) task.priority = patch.priority === "normal" ? null : patch.priority;
    if (patch.tags !== undefined) task.tags = patch.tags;
    sortInPlace();
    try {
      // update_task returns the task's current ID (freshly stamped when the
      // vault opts in and it lacked one, or the existing value; null when IDs
      // are off), so the row can reveal its copy-ID affordance immediately
      // rather than only after a view reload (Codex, PR #59).
      reflectStampedId(
        task,
        await invoke<string | null>("update_task", { id: task.vaultId, path: task.path, patch }),
      );
      return true;
    } catch (e) {
      Object.assign(task, before);
      sortInPlace();
      notifications.error(String(e));
      logWarning(`update_task failed: ${String(e)}`);
      return false;
    }
  }

  // The list move. NOT optimistic: the landed path (which may carry a
  // collision suffix) only exists in the command's answer, so the row adopts
  // it on success. A failure keeps any just-saved fields (never silently
  // half-reverted) — the toast names exactly what failed.
  async function moveToList(task: AggTask, targetList: string, fieldsSaved: boolean) {
    try {
      const moved = await invoke<MovedTask>("move_task_to_list", {
        id: task.vaultId,
        path: task.path,
        list: targetList,
      });
      // Shared adoption (landed path + any freshly-stamped id — a list-only
      // editor save can be the first edit on a legacy task), same helper as
      // the drag mover so the two can't drift (Codex, PR #59).
      applyMovedTask(task, moved);
      task.list = targetList;
      sortInPlace();
    } catch (e) {
      const prefix = fieldsSaved ? "Saved fields, but couldn't move" : "Couldn't move";
      notifications.error(`${prefix} to "${targetList || "No list"}": ${String(e)}`);
      logWarning(`move_task_to_list failed: ${String(e)}`);
    }
  }

  async function onEditorSave(task: AggTask, editorPatch: TaskEditorPatch) {
    editingKey.value = null;
    // The list move is not a frontmatter write — strip it off the field patch
    // and run it as its own step AFTER the fields land (the fields write
    // targets the OLD path; the move changes it).
    const { list: targetList, ...patch } = editorPatch;
    const hasFields = Object.keys(patch).length > 0;
    if (!hasFields && targetList === undefined) return;
    const oldPath = task.path;
    busy.value.add(oldPath);
    try {
      // A failed field write aborts the move — don't compound the situation.
      if (hasFields && !(await applyFieldPatch(task, patch))) return;
      if (targetList !== undefined) await moveToList(task, targetList, hasFields);
    } finally {
      busy.value.delete(oldPath);
    }
  }

  return {
    busy,
    isBusy,
    toggle,
    archive,
    openInObsidian,
    editingKey,
    rowKey,
    startEdit,
    cancelEdit,
    onEditorSave,
  };
}
