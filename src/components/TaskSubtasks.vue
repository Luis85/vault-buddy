<script setup lang="ts">
import { ref } from "vue";

import type { AggTask } from "../types";
import SectionHeader from "./ui/SectionHeader.vue";

// The Task Detail Subtasks section (Task 9): the current task's direct
// children with a done/total progress line, plus an inline "Add subtask"
// title input. Presentational, TaskParentRow's precedent — the container
// (TaskDetail.vue) owns the write logic (create / toggle a child's status /
// the id-cache patch that keeps a just-bootstrapped parent id in sync) and
// the one shared `busy` guard; this component only renders and reports
// intent up, keeping TaskDetail's own template under the complexity gate.
//
// `progress` is typed structurally here rather than imported: useTaskHierarchy's
// own TaskProgress interface is deliberately unexported for exactly this — any
// consumer destructures `.done`/`.total` by shape.
defineProps<{
  children: AggTask[];
  progress: { done: number; total: number };
  busy: boolean;
  /** Non-null when Add subtask must be inert, carrying the reason to show.
   * A UI HINT ONLY — core's `reject_archived_parent` is the authority and
   * refuses the write regardless of what this renders. Without the hint the
   * user meets an error toast on submit instead of an affordance (GAP-90). */
  disabledReason?: string | null;
}>();
const emit = defineEmits<{
  (e: "toggle", task: AggTask): void;
  (e: "open", task: AggTask): void;
  (e: "add", title: string): void;
}>();

const newTitle = ref("");

function onEnter(e: KeyboardEvent) {
  // IME guard mirrors TaskComposer/TaskViewControls: committing an IME
  // candidate fires Enter with isComposing=true, which must select the
  // candidate, never submit a half-composed title as a vault write.
  if (e.isComposing) return;
  e.preventDefault();
  const title = newTitle.value.trim();
  if (!title) return;
  emit("add", title);
}
function onEscape(e: KeyboardEvent) {
  // stopPropagation so clearing the draft can't also bubble into the panel's
  // own Escape-closes handler (TaskViewControls' New-list input is the model,
  // and the reason every popover/menu on this page swallows its own Escape).
  if (e.isComposing) return;
  e.stopPropagation();
  newTitle.value = "";
}
// Cleared by the container after a SUCCESSFUL add only — a failed add keeps
// the user's input (TaskComposer's `reset` precedent, called via this same
// exposed-method shape rather than TaskViewControls' resetNonce prop, since
// there is no ephemeral open/closed mode here to also reset).
function reset() {
  newTitle.value = "";
}
defineExpose({ reset });
</script>

<template>
  <div class="flex flex-col gap-1">
    <SectionHeader>Subtasks</SectionHeader>
    <div
      v-if="progress.total > 0"
      data-testid="task-detail-subtask-progress"
      class="px-2 text-micro tabular-nums text-fg-muted"
    >
      {{ progress.done }} / {{ progress.total }} done
    </div>
    <ul
      v-if="children.length > 0"
      class="flex flex-col gap-1"
    >
      <li
        v-for="child in children"
        :key="child.path"
        data-testid="task-detail-subtask"
        class="flex items-center gap-2 rounded-control border border-white/10 bg-white/5 px-2 py-1"
      >
        <input
          type="checkbox"
          data-testid="task-detail-subtask-checkbox"
          :checked="child.done"
          :disabled="busy"
          :aria-label="`Mark ${child.title} ${child.done ? 'not done' : 'done'}`"
          class="shrink-0 cursor-pointer accent-violet-500 disabled:cursor-default disabled:opacity-50"
          @change="emit('toggle', child)"
        >
        <button
          type="button"
          data-testid="task-detail-subtask-open"
          :disabled="busy"
          class="min-w-0 flex-1 cursor-pointer truncate rounded text-left text-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default"
          :class="child.done ? 'text-fg-subtle line-through' : 'text-fg-secondary'"
          @click="emit('open', child)"
        >
          {{ child.title }}
        </button>
      </li>
    </ul>
    <input
      v-model="newTitle"
      data-testid="task-detail-add-subtask"
      type="text"
      placeholder="Add subtask"
      aria-label="Add subtask"
      :disabled="busy || Boolean(disabledReason)"
      class="rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none disabled:cursor-default disabled:opacity-50"
      @keydown.enter="onEnter"
      @keydown.esc="onEscape"
    >
    <p
      v-if="disabledReason"
      class="text-micro text-fg-subtle"
    >
      {{ disabledReason }}
    </p>
  </div>
</template>
