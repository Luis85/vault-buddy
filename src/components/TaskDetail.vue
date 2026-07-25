<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref, toRef } from "vue";

import { useTaskDetail } from "../composables/useTaskDetail";
import { logWarning } from "../logging";
import type { AggTask, TaskEditorPatch, TasksConfig } from "../types";
import { buildTaskPatch, dueOf, scheduledOf } from "../utils/taskFields";
import { archivedMatcher, orderLists } from "../utils/taskSections";
import TaskListPicker from "./TaskListPicker.vue";

// The full-height detail surface: a roomy home for one task. It holds its own
// draft (seeded from the passed task, which carries its own vaultId so writes
// target the right vault in both per-vault and aggregate modes), edits through
// useTaskDetail, and offers the lifecycle verbs. The list re-fetches when the
// user goes back, so this surface never syncs to the list's in-memory array.
const props = defineProps<{ task: AggTask }>();
const taskRef = toRef(props, "task");
const { busy, save, remove, duplicate, openInObsidian } = useTaskDetail(taskRef);

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
const archivedLists = ref<string[]>([]);
onMounted(async () => {
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
async function openConfirm() {
  confirming.value = true;
  await new Promise((r) => setTimeout(r));
  confirmBtn.value?.focus();
}
function onDeleteKeydown(e: KeyboardEvent) {
  // Only swallow Escape while the confirm is OPEN. When it is closed, let Escape
  // bubble to PanelRoot's window handler so it closes the panel like every other
  // view — an unconditional stopPropagation() would make Escape a dead end on
  // this page (reviewer + Codex P2, PR #76).
  if (e.key === "Escape" && confirming.value) {
    e.stopPropagation();
    confirming.value = false;
  }
}
</script>

<template>
  <div
    class="flex flex-col gap-3 text-fg"
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

    <div class="flex items-center gap-2 pt-1">
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
        class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-3 py-1 text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
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
      <div class="ml-auto flex items-center gap-1">
        <template v-if="confirming">
          <span class="text-micro text-fg-muted">Delete permanently?</span>
          <button
            type="button"
            data-testid="task-detail-delete-cancel"
            :disabled="busy"
            class="cursor-pointer rounded-control px-2 py-1 text-xs text-fg-muted hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
            @click="confirming = false"
          >
            Cancel
          </button>
          <button
            ref="confirmBtn"
            type="button"
            data-testid="task-detail-delete-confirm"
            :disabled="busy"
            class="cursor-pointer rounded-control bg-danger/80 px-2 py-1 text-xs font-semibold text-danger-fg hover:bg-danger focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:opacity-50"
            @click="remove"
          >
            Delete
          </button>
        </template>
        <button
          v-else
          type="button"
          data-testid="task-detail-delete"
          :disabled="busy"
          class="cursor-pointer rounded-control border border-danger/40 px-3 py-1 text-xs text-danger-fg hover:bg-danger/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
          @click="openConfirm"
        >
          Delete
        </button>
      </div>
    </div>
  </div>
</template>
