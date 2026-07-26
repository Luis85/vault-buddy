<script setup lang="ts">
import { computed, ref } from "vue";

import type { AggTask } from "../types";

// Presentational parent picker: a search-filtered list of candidate parent
// tasks. No IPC, no open/close state of its own (TaskDetail.vue owns the
// disclosure — the TaskListPicker.vue split: this renders options and emits
// the pick, the caller runs the write) and no `busy` prop — the Change
// trigger that reveals this is already disabled while busy, so this never
// mounts mid-write. `invalidPaths` (self + descendants, computed by the
// caller from the frontend hierarchy index) renders those options disabled
// with a "would create a loop" note. This is a UI HINT ONLY: a stale or
// IDs-off-empty frontend index can only under-disable, never let an actual
// cycle through — core re-validates on write and remains the authority.
const props = defineProps<{
  tasks: AggTask[];
  currentPath: string | null;
  invalidPaths: string[];
}>();
const emit = defineEmits<{ (e: "select", path: string | null): void }>();

const filterInput = ref<HTMLInputElement | null>(null);
defineExpose({ focus: () => filterInput.value?.focus() });

const filter = ref("");
const invalid = computed(() => new Set(props.invalidPaths));
// Alphabetical by title — there is no manual order to fall back to here
// (unlike TaskListPicker's listOrder-then-alphabetical), so title is the
// whole rule.
const filtered = computed(() => {
  const q = filter.value.trim().toLowerCase();
  const list = q ? props.tasks.filter((t) => t.title.toLowerCase().includes(q)) : props.tasks;
  return [...list].sort((a, b) => a.title.localeCompare(b.title));
});

const itemClass =
  "w-full cursor-pointer rounded px-1.5 py-1 text-left text-xs text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-40";
</script>

<template>
  <div class="flex min-w-0 flex-col gap-1">
    <input
      ref="filterInput"
      v-model="filter"
      type="text"
      data-testid="task-parent-picker-filter"
      placeholder="Search tasks…"
      aria-label="Search tasks to set as parent"
      class="min-w-0 rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
    >
    <ul class="panel-scroll flex max-h-40 flex-col gap-0.5 overflow-y-auto">
      <li>
        <button
          type="button"
          data-testid="task-parent-picker-option-none"
          :class="[itemClass, currentPath === null ? 'bg-accent/20 text-fg' : '']"
          @click="emit('select', null)"
        >
          No parent
        </button>
      </li>
      <li
        v-for="t in filtered"
        :key="t.path"
      >
        <button
          type="button"
          :data-testid="`task-parent-picker-option-${t.path}`"
          :disabled="invalid.has(t.path)"
          :title="invalid.has(t.path) ? 'Would create a loop' : undefined"
          :class="[itemClass, currentPath === t.path ? 'bg-accent/20 text-fg' : '']"
          @click="emit('select', t.path)"
        >
          {{ t.title }}
          <span
            v-if="invalid.has(t.path)"
            class="text-fg-subtle"
          > (would create a loop)</span>
        </button>
      </li>
      <li
        v-if="filtered.length === 0"
        class="px-1.5 py-1 text-xs text-fg-subtle"
      >
        No matching tasks.
      </li>
    </ul>
  </div>
</template>
