<script setup lang="ts">
import { computed, ref } from "vue";

import type { AggTask, TaskEditorPatch, TaskItem } from "../types";
import { copyToClipboard } from "../utils/clipboard";
import { dueOf, parseTagsInput } from "../utils/taskFields";
import TaskListPicker from "./TaskListPicker.vue";

// Presentational inline editor: owns its own draft field state (seeded from
// the task on mount — the container remounts it per edit via v-if, so no
// re-sync is needed), computes the changed-fields patch, and reports up.
// The container keeps editingKey and the optimistic mutation/revert. `lists`
// is the row's vault's lists; the picker offers existing lists only
// (creation lives in the composer's flow).
const props = defineProps<{ task: AggTask; busy: boolean; lists: string[] }>();
const emit = defineEmits<{
  (e: "save", patch: TaskEditorPatch): void;
  (e: "cancel"): void;
}>();

const normalizedPriority = (t: TaskItem) =>
  t.priority === "high" || t.priority === "low" ? t.priority : "normal";

const editTitle = ref(props.task.title);
const editDue = ref(dueOf(props.task) ?? "");
const editPriority = ref<string>(normalizedPriority(props.task));
const editTags = ref(props.task.tags.join(", "));
const editList = ref(props.task.list);

// A task must have a title (identity + display). A blank one must block the
// save entirely — not just get dropped from the changed-fields patch, which
// would silently retain the old title while still writing any due/priority/
// tags change (Codex, PR #46). Mirrors the add-task composer's disabled Add.
const titleValid = computed(() => editTitle.value.trim().length > 0);

function buildPatch(): TaskEditorPatch {
  const patch: TaskEditorPatch = {};
  const title = editTitle.value.trim();
  if (title && title !== props.task.title) patch.title = title;
  if (editDue.value !== (dueOf(props.task) ?? "")) {
    if (editDue.value === "") patch.clearDue = true;
    else patch.due = editDue.value;
  }
  if (editPriority.value !== normalizedPriority(props.task)) patch.priority = editPriority.value;
  const parsedTags = parseTagsInput(editTags.value);
  if (parsedTags.join(" ") !== props.task.tags.join(" ")) patch.tags = parsedTags;
  // Changed-fields rule, same as everything above: the move rides the patch
  // only when the pick differs from where the task lives.
  if (editList.value !== props.task.list) patch.list = editList.value;
  return patch;
}

function save() {
  // Ignore a save while this row's write is still pending, or with a blank
  // title (belt-and-suspenders with the disabled Save button) — a second write
  // could land out of order, and a blank title must never reach the backend as
  // a silent no-op that keeps the old title while writing the other fields.
  if (props.busy || !titleValid.value) return;
  emit("save", buildPatch());
}

function copyId() {
  // Display-only: no editing, no new write path — just read the generated id
  // (Task 4) and put it on the clipboard via the shared helper.
  if (!props.task.id) return;
  copyToClipboard(props.task.id, "task editor");
}

function onTitleEnter(e: KeyboardEvent) {
  // Mirrors the add-task input (GAP-31): an IME candidate commit fires Enter
  // with isComposing=true, which must select the candidate, not save/close the
  // editor with a half-composed title.
  if (e.isComposing) return;
  // preventDefault lives HERE, after the guard — the template's `.prevent`
  // modifier ran before this handler and cancelled the candidate-commit
  // Enter's default, breaking IME selection (Codex, PR #46). A real Enter
  // still suppresses any form/default action before saving.
  e.preventDefault();
  save();
}

function onEditorEsc(e: KeyboardEvent) {
  // Bound on the editor ROOT so Escape from ANY field (title, due, tags,
  // priority buttons) is caught here as it bubbles — not just the title
  // input (Codex, PR #46): otherwise Escape focused in due/tags/priority
  // bubbles past to PanelRoot's window-level handler and closes the WHOLE
  // panel instead of dismissing the edit (same class as GAP-27's SelectMenu
  // Escape). During IME composition, Escape cancels the CANDIDATE, not the
  // edit, so the guard preserves the in-progress edit.
  if (e.isComposing) return;
  e.stopPropagation();
  emit("cancel");
}
</script>

<template>
  <div
    class="flex min-w-0 flex-1 flex-col gap-1 py-0.5"
    @keydown.esc="onEditorEsc"
  >
    <input
      v-model="editTitle"
      data-testid="task-edit-title"
      type="text"
      aria-label="Task title"
      class="min-w-0 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-sm text-fg focus:border-focus focus:outline-none"
      @keydown.enter="onTitleEnter"
    >
    <div class="flex items-center gap-1">
      <input
        v-model="editDue"
        data-testid="task-edit-due"
        type="date"
        aria-label="Due date"
        class="min-w-0 flex-1 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg focus:border-focus focus:outline-none"
      >
      <div
        class="flex gap-0.5"
        role="radiogroup"
        aria-label="Priority"
      >
        <button
          v-for="p in ['high', 'normal', 'low']"
          :key="p"
          type="button"
          role="radio"
          :data-testid="`task-edit-priority-${p}`"
          :aria-checked="editPriority === p"
          class="cursor-pointer rounded-control border px-1.5 py-0.5 text-micro capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          :class="
            editPriority === p
              ? 'border-violet-400 bg-accent/20 text-fg'
              : 'border-white/10 bg-white/5 text-fg-secondary hover:bg-white/10'
          "
          @click="editPriority = p"
        >
          {{ p }}
        </button>
      </div>
    </div>
    <input
      v-model="editTags"
      data-testid="task-edit-tags"
      type="text"
      placeholder="#tags"
      aria-label="Tags"
      class="min-w-0 flex-1 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
    >
    <div class="flex items-center gap-1">
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">List</span>
      <TaskListPicker
        v-model="editList"
        :lists="lists"
        :allow-create="false"
        aria-label="Task list"
        data-testid="task-edit-list"
      />
    </div>
    <div
      v-if="task.id"
      class="flex items-center gap-1"
    >
      <span class="shrink-0 text-micro uppercase tracking-wider text-fg-subtle">ID</span>
      <code
        data-testid="task-edit-id"
        class="min-w-0 flex-1 truncate rounded bg-white/5 px-1.5 py-0.5 text-micro text-fg-secondary"
      >{{ task.id }}</code>
      <button
        type="button"
        data-testid="task-edit-id-copy"
        aria-label="Copy task ID"
        title="Copy ID"
        class="shrink-0 cursor-pointer rounded-control border border-white/10 bg-white/5 px-1.5 py-0.5 text-micro text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="copyId"
      >
        Copy
      </button>
    </div>
    <div class="flex items-center justify-end gap-1">
      <button
        type="button"
        data-testid="task-edit-cancel"
        class="cursor-pointer rounded-control px-2 py-0.5 text-xs text-fg-muted transition-colors hover:bg-white/10 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="$emit('cancel')"
      >
        Cancel
      </button>
      <button
        type="button"
        data-testid="task-edit-save"
        :disabled="busy || !titleValid"
        class="cursor-pointer rounded-control bg-accent-strong/80 px-2 py-0.5 text-xs font-semibold text-white hover:bg-accent-strong focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
        @click="save"
      >
        Save
      </button>
    </div>
  </div>
</template>
