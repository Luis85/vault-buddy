<script setup lang="ts">
import { ref, watch } from "vue";

import { parseTagsInput } from "../utils/taskFields";
import SelectMenu from "./SelectMenu.vue";

// Presentational add-task composer: owns its own draft field state (title,
// due, priority, tags, showAddOptions, and — in aggregate mode — the selected
// vault). No store/invoke access: it reports a parsed payload up via `submit`
// and the container resolves the real target vault, validates, and writes.
const props = defineProps<{
  isAggregate: boolean;
  vaultOptions: { value: string; label: string }[];
  adding: boolean;
}>();
const emit = defineEmits<{
  (
    e: "submit",
    payload: { title: string; due: string; priority: string; tags: string[]; vaultId: string | null },
  ): void;
}>();

const title = ref("");
const showAddOptions = ref(false);
const addDue = ref("");
const addPriority = ref("normal");
const addTags = ref("");
// Aggregate add: which vault receives the new task. Defaults to the first
// vault once the options arrive (they load async in the container), re-homing
// if the current pick vanishes; component-local, no persistence across opens.
const addVaultId = ref("");
watch(
  () => props.vaultOptions,
  (opts) => {
    if (!opts.some((o) => o.value === addVaultId.value)) addVaultId.value = opts[0]?.value ?? "";
  },
  { immediate: true },
);

function submit() {
  emit("submit", {
    title: title.value,
    due: addDue.value,
    priority: addPriority.value,
    // Client-side lenient parse; the shell strictly validates the charset.
    tags: parseTagsInput(addTags.value),
    // The container uses props.vaultId in single-vault mode; only the aggregate
    // picker's value is meaningful here.
    vaultId: props.isAggregate ? addVaultId.value : null,
  });
}

function onTitleEnter(e: KeyboardEvent) {
  // GAP-31: committing an IME candidate fires Enter with isComposing=true —
  // that must select the candidate, never create a task document (a vault
  // write) from the half-composed title. The Search view's handlers are the
  // precedent. preventDefault lives HERE, after the guard (mirrors TaskEditor),
  // so it never cancels the candidate-commit Enter's default.
  if (e.isComposing) return;
  e.preventDefault();
  submit();
}

// Cleared by the container after a SUCCESSFUL add only — a failed add keeps the
// user's input. The selected vault is deliberately NOT reset, so a burst of
// cross-vault adds stays on the chosen vault.
function reset() {
  title.value = "";
  addDue.value = "";
  addPriority.value = "normal";
  addTags.value = "";
  showAddOptions.value = false;
}
defineExpose({ reset });
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center gap-1">
      <SelectMenu
        v-if="isAggregate"
        v-model="addVaultId"
        :options="vaultOptions"
        aria-label="Vault for the new task"
        data-testid="task-add-vault"
      />
      <input
        v-model="title"
        data-testid="task-input"
        type="text"
        placeholder="Add a task…"
        aria-label="New task title"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        @keydown.enter="onTitleEnter"
      >
      <button
        type="button"
        data-testid="task-add-options"
        :aria-label="showAddOptions ? 'Hide task options' : 'Set due date or priority'"
        :aria-expanded="showAddOptions"
        title="Due date / priority"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :class="showAddOptions ? 'border-violet-400 text-slate-100' : ''"
        @click="showAddOptions = !showAddOptions"
      >
        ⋯
      </button>
      <button
        type="button"
        data-testid="task-add"
        :disabled="adding || title.trim() === ''"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
        @click="submit"
      >
        Add
      </button>
    </div>

    <div
      v-if="showAddOptions"
      class="flex items-center gap-1"
    >
      <input
        v-model="addDue"
        data-testid="task-add-due"
        type="date"
        aria-label="Due date"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
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
          :data-testid="`task-add-priority-${p}`"
          :aria-checked="addPriority === p"
          class="cursor-pointer rounded-lg border px-1.5 py-0.5 text-[10px] capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            addPriority === p
              ? 'border-violet-400 bg-violet-500/20 text-slate-100'
              : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
          "
          @click="addPriority = p"
        >
          {{ p }}
        </button>
      </div>
      <input
        v-model="addTags"
        data-testid="task-add-tags"
        type="text"
        placeholder="#tags"
        aria-label="Tags"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      >
    </div>
  </div>
</template>
