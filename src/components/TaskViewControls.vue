<script setup lang="ts">
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
defineProps<{
  grouping: "dates" | "tags" | "lists";
  sortPref: TaskSortPref;
}>();
defineEmits<{
  (e: "update:grouping", value: "dates" | "tags" | "lists"): void;
  (e: "setSortKey", key: SortKey): void;
  (e: "flipSortDir"): void;
}>();

const GROUPINGS = [
  { key: "lists", label: "Lists" },
  { key: "dates", label: "Dates" },
  { key: "tags", label: "Tags" },
] as const;
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
