import { invoke } from "@tauri-apps/api/core";
import { computed, type Ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AddTaskResult, AggTask } from "../types";
import { subtaskCreateDisabledReason, TASK_IDS_ENABLED_MESSAGE } from "../utils/taskHierarchy";
import { reflectStampedId } from "../utils/taskMutations";
import { archivedMatcher } from "../utils/taskSections";

/**
 * The Subtasks section's verbs on the Task Detail surface: create a child
 * (Add subtask), toggle a child's status, and drill into a child. Split out of
 * TaskDetail.vue at 489/500 nonblank lines (the frontend cap), along the seam
 * the surface already draws: TaskSubtasks.vue is the section's presentational
 * half, useTaskHierarchy owns the Parent row's write (setParent), and this is
 * the Subtasks section's write half — the create-path twin of setParent.
 *
 * It owns NO guard of its own. Every verb here rides the ONE `busy` ref the
 * caller passes in (useTaskDetail's), so a subtask create or toggle can never
 * race a Save/Duplicate/Delete or a parent change on the same surface. It also
 * owns no state: the archived-list readiness it gates creation on is loaded by
 * TaskDetail (the Parent row's Change button defers to the same state), and
 * `resetInput` is the caller's handle on TaskSubtasks' own input.
 */
export function useTaskDetailSubtasks(opts: {
  task: Ref<AggTask>;
  busy: Ref<boolean>;
  allTasks: Ref<AggTask[]>;
  reloadTaskSet: () => Promise<void>;
  archivedLists: Ref<string[]>;
  archivedListsResolved: Ref<boolean>;
  archivedListsError: Ref<string | null>;
  resetInput: () => void;
}) {
  const { task, busy, allTasks, reloadTaskSet, archivedLists, archivedListsResolved, archivedListsError } = opts;
  const notifications = useNotificationsStore();
  const vaults = useVaultsStore();

  // GAP-90's UI hint (archived task) plus the archived-list config's own
  // readiness (Codex P2, PR #78 follow-up) — see subtaskCreateDisabledReason's
  // own doc comment for why creation must defer to the same
  // archivedListsResolved/archivedListsError state the Parent row's Change
  // button already does.
  const addSubtaskDisabledReason = computed(() =>
    subtaskCreateDisabledReason(task.value.status, archivedListsResolved.value, archivedListsError.value),
  );

  // Add subtask (Task 9): the create-path twin of useTaskHierarchy's setParent
  // — same shared `busy` guard, same reload-vs-patch branch on `idsEnabled`,
  // and the same TASK_IDS_ENABLED_MESSAGE disclosure, since Add subtask is
  // often a vault's FIRST hierarchy operation (design spec §2).
  async function onAddSubtask(title: string) {
    // Defensive: the input is already :disabled="busy || Boolean(disabledReason)"
    // (a disabled input can't dispatch the Enter that reaches this), so this only
    // matters if that ever drifts — same posture as TaskParentRow.open()'s
    // re-check of canAssign. It matters more here than there: unlike an archived
    // PARENT (core's reject_archived_parent is the backstop), core has no
    // authority at all over archived LISTS, so a create that slipped past would
    // silently write with the disclosure below evaluated against an unknown set.
    if (busy.value || addSubtaskDisabledReason.value) return;
    busy.value = true;
    try {
      const result = await invoke<AddTaskResult>("add_task", {
        id: task.value.vaultId,
        title,
        parentPath: task.value.path,
        list: task.value.list,
      });
      const { idsEnabled, ...fields } = result;
      // The child's parentId IS the parent's effective id — possibly stamped by
      // THIS call (a hand-authored parent that never had one, or the vault's
      // first-ever hierarchy op — core's add_subtask stamps the parent
      // unconditionally, not only when it also enables ids). Copy it onto the
      // cached row BEFORE anything re-resolves: buildParentIndex links
      // child->parent by matching ids, so a stale null id here would leave the
      // just-created child unresolved — the create-path twin of the parent-row
      // patch setParent already applies (Codex P1, PR #77, missed once already
      // in this exact spot).
      reflectStampedId(task.value, fields.parentId);
      if (idsEnabled) {
        // The whole cached set was loaded id-suppressed — a cheap push would
        // reveal only THIS relationship while any pre-existing dormant
        // hierarchy stays orphaned on screen (setParent applies the identical
        // rule).
        await reloadTaskSet();
        notifications.notify("success", TASK_IDS_ENABLED_MESSAGE, {});
      } else {
        allTasks.value.push({ ...fields, vaultId: task.value.vaultId, vaultName: task.value.vaultName });
      }
      opts.resetInput();
      // GAP-92: the child correctly INHERITS the parent's list, keeping it
      // beside its parent — but an archived list is hidden from the Lists view
      // and excluded from count_open_tasks the instant the task is created. The
      // task is not lost (it renders in this section and under Plan/Tags
      // grouping), so the defect was the SILENCE: disclose it rather than
      // rerouting the child away from its parent, which would break the
      // inheritance design to solve a visibility problem.
      if (archivedMatcher(archivedLists.value)(task.value.list)) {
        notifications.notify(
          "info",
          `Added to "${task.value.list}", an archived list hidden from the Lists view.`,
          {},
        );
      }
    } catch (e) {
      notifications.error(String(e));
      logWarning(`add_task (subtask) failed: ${String(e)}`);
    } finally {
      busy.value = false;
    }
  }

  // Toggling a child's status is a plain field write on a DIFFERENT document —
  // still serialized through the ONE shared guard (every write on this surface
  // does), and it mutates the exact object `children` was filtered from, so the
  // progress line updates without a reload.
  async function onToggleSubtask(child: AggTask) {
    if (busy.value) return;
    busy.value = true;
    const prevStatus = child.status;
    const done = !child.done;
    child.done = done;
    child.status = done ? "done" : "new";
    try {
      await invoke("set_task_status", { id: child.vaultId, path: child.path, status: child.status });
      void vaults.refreshTaskCount(child.vaultId);
    } catch (e) {
      child.status = prevStatus;
      child.done = prevStatus === "done";
      notifications.error(String(e));
      logWarning(`set_task_status (subtask) failed: ${String(e)}`);
    } finally {
      busy.value = false;
    }
  }

  // Mirrors TaskDetail's openParentDetail: a plain navigation to a different
  // document, still gated on busy so it can't be clicked away from mid-write.
  function openSubtaskDetail(t: AggTask) {
    if (busy.value) return;
    vaults.openTaskDetail(t);
  }

  return { addSubtaskDisabledReason, onAddSubtask, onToggleSubtask, openSubtaskDetail };
}
