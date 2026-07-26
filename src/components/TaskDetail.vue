<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, nextTick, onMounted, ref, toRef } from "vue";

import { useTaskDetail } from "../composables/useTaskDetail";
import { useTaskDetailTaskSet } from "../composables/useTaskDetailTaskSet";
import { useTaskHierarchy } from "../composables/useTaskHierarchy";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AddTaskResult, AggTask, TaskEditorPatch, TasksConfig } from "../types";
import { buildTaskPatch, dueOf, scheduledOf } from "../utils/taskFields";
import { TASK_IDS_ENABLED_MESSAGE } from "../utils/taskHierarchy";
import { reflectStampedId } from "../utils/taskMutations";
import { archivedMatcher, orderLists } from "../utils/taskSections";
import TaskListPicker from "./TaskListPicker.vue";
import TaskParentRow from "./TaskParentRow.vue";
import TaskSubtasks from "./TaskSubtasks.vue";

// The full-height detail surface: a roomy home for one task. It holds its own
// draft (seeded from the passed task, which carries its own vaultId so writes
// target the right vault in both per-vault and aggregate modes), edits through
// useTaskDetail, and offers the lifecycle verbs. The list re-fetches when the
// user goes back, so this surface never syncs to the list's in-memory array.
const props = defineProps<{ task: AggTask }>();
const taskRef = toRef(props, "task");
const { busy, save, remove, duplicate, openInObsidian } = useTaskDetail(taskRef);
const vaults = useVaultsStore();

// Parent/subtask hierarchy (Task 8): resolved from the vault's own task set,
// loaded independently of Tasks.vue's — that view is UNMOUNTED while this one
// shows (ActionPanel's view switch is one-at-a-time), so there is no shared
// list to read from here. `reload` is what useTaskHierarchy calls INSTEAD OF
// its cheap two-row patch when a write turns Task IDs on for the vault: the
// set was loaded id-suppressed, so EVERY cached id is null, not just the two
// rows that write touched (Codex P2, PR #77).
// Declared HERE, above its first consumer, rather than beside the sibling
// list refs below: `pickerCandidates` reads it to drop archived-list tasks
// from the parent options (GAP-91), and a `const` used before its declaration
// is a TDZ error, not merely a style question. `loadLists` (below) fills it.
const archivedLists = ref<string[]>([]);
const {
  allTasks,
  pickerCandidates,
  reload: reloadTaskSet,
  invalidParentPaths,
} = useTaskDetailTaskSet(taskRef, archivedLists);
const { parent, children, progress, setParent } = useTaskHierarchy(taskRef, allTasks, busy, reloadTaskSet);
const notifications = useNotificationsStore();
const subtasksRef = ref<InstanceType<typeof TaskSubtasks> | null>(null);

// The title trigger in the Tasks list unmounts when this surface opens, so
// keyboard focus would fall back to <body> and a screen-reader user would get
// no signal of the view change. Focus the labelled region on mount (NOT the
// title input — that invites an accidental edit) so focus stays in-panel and
// the surface is announced. Mirrors Search.vue's on-mount focus.
const rootEl = ref<HTMLElement | null>(null);

const normPriority = (p: string | null) => (p === "high" || p === "low" ? p : "normal");

const draftTitle = ref(props.task.title);
const draftDescription = ref(props.task.description ?? "");
const draftDue = ref(dueOf(props.task) ?? "");
const draftScheduled = ref(scheduledOf(props.task) ?? "");
const draftPriority = ref(normPriority(props.task.priority));
const draftTags = ref(props.task.tags.join(", "));
const draftList = ref(props.task.list);

// The vault's lists for the picker, loaded once and kept as RAW inputs (not a
// one-time filtered array) so the visible options can be derived reactively
// from the task's CURRENT list — see `lists` below. A load failure logs rather
// than silently rendering a blank picker (Codex P2, PR #76).
const allLists = ref<string[]>([]);
const listOrder = ref<string[]>([]);
async function loadLists(): Promise<void> {
  try {
    const [all, cfg] = await Promise.all([
      invoke<string[]>("list_task_lists", { id: props.task.vaultId }),
      invoke<TasksConfig>("get_tasks_config", { id: props.task.vaultId }),
    ]);
    allLists.value = all;
    listOrder.value = cfg.listOrder ?? [];
    archivedLists.value = cfg.archivedLists ?? [];
  } catch (e) {
    logWarning(`task detail: could not load task lists: ${String(e)}`);
  }
}
onMounted(async () => {
  rootEl.value?.focus();
  // Independent loads with their own catch (reloadTaskSet never throws): a
  // failed task-set read must not also blank the lists picker, and vice versa.
  await Promise.all([loadLists(), reloadTaskSet()]);
});
// Options ordered by the vault's listOrder-then-alphabetical (matching
// useTaskLists.listsForVault), dropping archived lists EXCEPT the task's own
// current one. Derived reactively from `props.task.list`, so after a move OUT of
// an archived list that list drops from the options — a one-time filter would
// leave it selectable and let the user move back into a hidden list (Codex P2,
// PR #76).
const lists = computed(() => {
  const isArchived = archivedMatcher(archivedLists.value);
  return orderLists(allLists.value, listOrder.value).filter(
    (l) => l === props.task.list || !isArchived(l),
  );
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
  // Description lives only here — augment the shared patch. A whitespace-only
  // draft is equivalent to no description (same as an absent one), so after a
  // whitespace-clear save the draft and the now-null task agree and `dirty`
  // returns to false instead of emitting clearDescription forever, which kept
  // Save enabled for repeated no-op writes (Codex P2, PR #76).
  const draftDesc = draftDescription.value.trim() === "" ? "" : draftDescription.value;
  if (draftDesc !== (props.task.description ?? "")) {
    if (draftDesc === "") patch.clearDescription = true;
    else patch.description = draftDesc;
  }
  return patch;
}

const dirty = computed(() => Object.keys(currentPatch()).length > 0);

async function onSave() {
  if (!titleValid.value || busy.value) return;
  await save(currentPatch());
}

// Two-step permanent-delete confirm (GAP-27 class: focus the confirm on open;
// Escape steps back one level ONLY while the confirm is open — a closed-confirm
// Escape bubbles to the panel's own close handler like every other view).
const confirming = ref(false);
const confirmBtn = ref<HTMLButtonElement | null>(null);
const deleteBtn = ref<HTMLButtonElement | null>(null);
async function openConfirm() {
  confirming.value = true;
  await new Promise((r) => setTimeout(r));
  confirmBtn.value?.focus();
}
// Closing the confirm unmounts the confirm cluster; return focus to the Delete
// trigger it came from rather than dropping it to <body> (the GAP-27 pattern
// TaskScheduleMenu follows). Shared by Cancel and the Escape path.
async function cancelConfirm() {
  confirming.value = false;
  await nextTick();
  deleteBtn.value?.focus();
}
function onDeleteKeydown(e: KeyboardEvent) {
  // Only swallow Escape while the confirm is OPEN. When it is closed, let Escape
  // bubble to PanelRoot's window handler so it closes the panel like every other
  // view — an unconditional stopPropagation() would make Escape a dead end on
  // this page (reviewer + Codex P2, PR #76). TaskParentRow's own picker is a
  // separate, self-contained Escape scope (it stops propagation at ITS OWN
  // root before an event could ever reach here), so this handler only ever
  // needs to know about the delete confirm.
  if (e.key === "Escape" && confirming.value) {
    e.stopPropagation();
    // Don't cancel the warning mid-delete: the unlink is already in flight and
    // can't be undone, so keep the confirmation up. The disabled Cancel button
    // blocks the mouse path; this blocks the keyboard path (Codex P2, PR #76).
    if (!busy.value) void cancelConfirm();
  }
}
// A plain navigation to a DIFFERENT document — not the same-file race
// useTaskActions.onOpenTask guards against — but still gated on `busy` so the
// row can't be clicked away from while every other control here is disabled.
function openParentDetail() {
  if (busy.value || !parent.value) return;
  vaults.openTaskDetail(parent.value);
}

// GAP-90's UI hint. An archived task's own detail IS reachable — an active
// child's Parent chip opens it with no gate — and its Add-subtask input was
// gated only on the write lock, so it offered an assignment core refuses.
// A HINT only: reject_archived_parent remains the authority.
const addSubtaskDisabledReason = computed(() =>
  props.task.status === "archived"
    ? "This task is archived. Unarchive it to add subtasks."
    : null,
);

// Add subtask (Task 9): the create-path twin of useTaskHierarchy's setParent
// above — same shared `busy` guard, same reload-vs-patch branch on
// `idsEnabled`, and the same TASK_IDS_ENABLED_MESSAGE disclosure, since Add
// subtask is often a vault's FIRST hierarchy operation (design spec §2).
async function onAddSubtask(title: string) {
  if (busy.value) return;
  busy.value = true;
  try {
    const result = await invoke<AddTaskResult>("add_task", {
      id: props.task.vaultId,
      title,
      parentPath: props.task.path,
      list: props.task.list,
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
    reflectStampedId(props.task, fields.parentId);
    if (idsEnabled) {
      // The whole cached set was loaded id-suppressed — a cheap push would
      // reveal only THIS relationship while any pre-existing dormant
      // hierarchy stays orphaned on screen (setParent applies the identical
      // rule above).
      await reloadTaskSet();
      notifications.notify("success", TASK_IDS_ENABLED_MESSAGE, {});
    } else {
      allTasks.value.push({ ...fields, vaultId: props.task.vaultId, vaultName: props.task.vaultName });
    }
    subtasksRef.value?.reset();
    // GAP-92: the child correctly INHERITS the parent's list, keeping it
    // beside its parent — but an archived list is hidden from the Lists view
    // and excluded from count_open_tasks the instant the task is created. The
    // task is not lost (it renders in this section and under Plan/Tags
    // grouping), so the defect was the SILENCE: disclose it rather than
    // rerouting the child away from its parent, which would break the
    // inheritance design to solve a visibility problem.
    if (archivedMatcher(archivedLists.value)(props.task.list)) {
      notifications.notify(
        "info",
        `Added to "${props.task.list}", an archived list hidden from the Lists view.`,
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

// Mirrors openParentDetail: a plain navigation to a different document, still
// gated on busy so it can't be clicked away from mid-write.
function openSubtaskDetail(t: AggTask) {
  if (busy.value) return;
  vaults.openTaskDetail(t);
}
</script>

<template>
  <div
    ref="rootEl"
    role="region"
    aria-label="Task detail"
    tabindex="-1"
    class="flex flex-col gap-3 text-fg focus:outline-none"
    @keydown="onDeleteKeydown"
  >
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
      <input
        v-model="draftDue"
        data-testid="task-detail-due"
        type="date"
        aria-label="Due date"
        class="min-w-0 flex-1 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg focus:border-focus focus:outline-none"
      >
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">Do</span>
      <input
        v-model="draftScheduled"
        data-testid="task-detail-scheduled"
        type="date"
        aria-label="Do date"
        class="min-w-0 flex-1 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg focus:border-focus focus:outline-none"
      >
    </div>
    <div
      class="flex items-center gap-1"
      role="radiogroup"
      aria-label="Priority"
    >
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">Priority</span>
      <button
        v-for="p in ['high', 'normal', 'low']"
        :key="p"
        type="button"
        role="radio"
        :data-testid="`task-detail-priority-${p}`"
        :aria-checked="draftPriority === p"
        class="cursor-pointer rounded-control border px-2 py-0.5 text-xs capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="draftPriority === p ? 'border-violet-400 bg-accent/20 text-fg' : 'border-white/10 bg-white/5 text-fg-secondary hover:bg-white/10'"
        @click="draftPriority = p"
      >
        {{ p }}
      </button>
    </div>
    <input
      v-model="draftTags"
      data-testid="task-detail-tags"
      type="text"
      placeholder="#tags"
      aria-label="Tags"
      class="rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
    >
    <div class="flex items-center gap-1">
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">List</span>
      <TaskListPicker
        v-model="draftList"
        :lists="lists"
        :allow-create="false"
        aria-label="Task list"
        data-testid="task-detail-list"
      />
    </div>

    <!-- Parent row (Task 8), above the Subtasks section (Task 9). -->
    <TaskParentRow
      :parent="parent"
      :busy="busy"
      :all-tasks="pickerCandidates"
      :invalid-paths="invalidParentPaths"
      @open-parent="openParentDetail"
      @select="setParent"
    />

    <TaskSubtasks
      ref="subtasksRef"
      :children="children"
      :progress="progress"
      :busy="busy"
      :disabled-reason="addSubtaskDisabledReason"
      @add="onAddSubtask"
      @toggle="onToggleSubtask"
      @open="openSubtaskDetail"
    />

    <!-- While confirming a permanent delete the whole row BECOMES the confirm:
         Save/Open/Duplicate are hidden so the only choices are Cancel and the
         irreversible Delete — a focused confirm, not one buried among still-
         clickable verbs, and no six-control row to crowd/wrap on the compact
         panel (frontend review, PR #76). -->
    <div class="flex items-center gap-2 pt-1">
      <template v-if="confirming">
        <span
          id="task-detail-delete-prompt"
          class="text-xs text-danger-fg"
        >Delete permanently? This can't be undone.</span>
        <div class="ml-auto flex items-center gap-1">
          <button
            type="button"
            data-testid="task-detail-delete-cancel"
            :disabled="busy"
            class="cursor-pointer rounded-control px-2 py-1 text-xs text-fg-muted hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
            @click="cancelConfirm"
          >
            Cancel
          </button>
          <button
            ref="confirmBtn"
            type="button"
            data-testid="task-detail-delete-confirm"
            :disabled="busy"
            aria-label="Delete permanently"
            aria-describedby="task-detail-delete-prompt"
            class="cursor-pointer rounded-control bg-danger/80 px-2 py-1 text-xs font-semibold text-danger-fg hover:bg-danger focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-50"
            @click="remove"
          >
            {{ busy ? "Deleting…" : "Delete" }}
          </button>
        </div>
      </template>
      <template v-else>
        <button
          type="button"
          data-testid="task-detail-save"
          :disabled="!titleValid || !dirty || busy"
          class="cursor-pointer rounded-control bg-accent-strong/80 px-3 py-1 text-xs font-semibold text-white hover:bg-accent-strong focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
          @click="onSave"
        >
          Save
        </button>
        <button
          type="button"
          data-testid="task-detail-open"
          :disabled="busy"
          class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-3 py-1 text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
          @click="openInObsidian"
        >
          Open in Obsidian
        </button>
        <button
          type="button"
          data-testid="task-detail-duplicate"
          :disabled="busy"
          class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-3 py-1 text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
          @click="duplicate"
        >
          Duplicate
        </button>
        <div class="ml-auto">
          <button
            ref="deleteBtn"
            type="button"
            data-testid="task-detail-delete"
            :disabled="busy"
            class="cursor-pointer rounded-control border border-danger/40 px-3 py-1 text-xs text-danger-fg hover:bg-danger/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
            @click="openConfirm"
          >
            Delete
          </button>
        </div>
      </template>
    </div>
  </div>
</template>
