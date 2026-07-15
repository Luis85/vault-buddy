<script setup lang="ts">
import { ref } from "vue";

import {
  directionApplies,
  SORT_OPTIONS,
  type SortKey,
  type TaskSortPref,
} from "../utils/taskSort";
import SelectMenu from "./SelectMenu.vue";

// The tasks view's controls row: the grouping radios and the sort selector +
// direction toggle. Presentational — the container owns grouping/sort state
// and persistence; extracted so the container template stays under the
// complexity threshold.
const props = defineProps<{
  grouping: "dates" | "tags" | "lists";
  sortPref: TaskSortPref;
  isAggregate: boolean;
  creatingList: boolean;
}>();
const emit = defineEmits<{
  (e: "update:grouping", value: "dates" | "tags" | "lists"): void;
  (e: "setSortKey", key: SortKey): void;
  (e: "flipSortDir"): void;
  (e: "createList", name: string): void;
}>();

const GROUPINGS = [
  { key: "lists", label: "Lists" },
  { key: "dates", label: "Dates" },
  { key: "tags", label: "Tags" },
] as const;

// Inline "New list" create — shown only in per-vault Lists grouping (the
// aggregate has no single target vault). Mirrors TaskListPicker's create UX:
// IME-guarded Enter, Escape stops propagation so it doesn't close the panel.
const newMode = ref(false);
const newName = ref("");
function openNew() {
  newMode.value = true;
  newName.value = "";
}
function confirmNew() {
  const name = newName.value.trim();
  if (!name || props.creatingList) return;
  emit("createList", name);
  newMode.value = false;
  newName.value = "";
}
function onNewEnter(e: KeyboardEvent) {
  if (e.isComposing) return;
  e.preventDefault();
  confirmNew();
}
function onNewEscape(e: KeyboardEvent) {
  if (e.isComposing) return;
  e.stopPropagation();
  newMode.value = false;
}
</script>

<template>
  <div class="flex items-center gap-0.5">
    <div
      class="flex gap-0.5"
      role="radiogroup"
      aria-label="Group tasks by"
    >
      <button
        v-for="g in GROUPINGS"
        :key="g.key"
        type="button"
        role="radio"
        :data-testid="`task-grouping-${g.key}`"
        :aria-checked="grouping === g.key"
        class="cursor-pointer rounded-lg border px-1.5 py-0.5 text-[10px] transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :class="
          grouping === g.key
            ? 'border-violet-400 bg-violet-500/20 text-slate-100'
            : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
        "
        @click="$emit('update:grouping', g.key)"
      >
        {{ g.label }}
      </button>
    </div>
    <div
      v-if="grouping === 'lists' && !isAggregate"
      class="flex items-center gap-1"
    >
      <button
        v-if="!newMode"
        type="button"
        data-testid="task-newlist"
        aria-label="New list"
        title="New list"
        class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="openNew"
      >
        ＋ List
      </button>
      <template v-else>
        <input
          v-model="newName"
          data-testid="task-newlist-input"
          type="text"
          placeholder="List name"
          aria-label="New list name"
          class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-[10px] text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
          @keydown.enter="onNewEnter"
          @keydown.esc="onNewEscape"
        >
        <button
          type="button"
          data-testid="task-newlist-confirm"
          :disabled="creatingList || newName.trim() === ''"
          aria-label="Create list"
          title="Create list"
          class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
          @click="confirmNew"
        >
          ✓
        </button>
        <button
          type="button"
          data-testid="task-newlist-cancel"
          aria-label="Cancel new list"
          title="Cancel"
          class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          @click="newMode = false"
        >
          ✕
        </button>
      </template>
    </div>
    <div class="ml-auto flex items-center gap-1">
      <SelectMenu
        :model-value="sortPref.key"
        :options="SORT_OPTIONS"
        aria-label="Sort tasks"
        data-testid="task-sort"
        @update:model-value="$emit('setSortKey', $event as SortKey)"
      />
      <button
        type="button"
        data-testid="task-sort-dir"
        :disabled="!directionApplies(sortPref.key)"
        :aria-label="`Sort direction: ${sortPref.dir === 'asc' ? 'ascending' : 'descending'}`"
        :title="sortPref.dir === 'asc' ? 'Ascending' : 'Descending'"
        class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-1.5 py-0.5 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
        @click="$emit('flipSortDir')"
      >
        {{ sortPref.dir === "asc" ? "↑" : "↓" }}
      </button>
    </div>
  </div>
</template>
