<script setup lang="ts">
import type { SearchHit } from "../types";
import type { SearchResultGroup } from "../utils/searchResults";
import SearchHitRow from "./SearchHitRow.vue";

// The search view's result listbox: vault group headers with their collapse
// chevrons and count chips, and the hit rows beneath them. Split out of
// Search.vue at 497/500 nonblank lines — the listbox was over a quarter of
// that file and carries none of its state. Presentational only: Search.vue
// still owns the query, the debounce/ticket, the kind filter, the collapse
// set and the keyboard selection, and reacts to what this emits. The row ids
// (SearchHitRow.vue) come from searchHitId, the same function the
// input's aria-activedescendant reads, so the two cannot drift.
defineProps<{
  groups: SearchResultGroup[];
  selected: number;
  query: string;
}>();
defineEmits<{
  (e: "open", hit: SearchHit, keepOpen: boolean): void;
  (e: "hover", i: number): void;
  (e: "toggle", vaultId: string): void;
}>();
</script>

<template>
  <div
    id="search-results"
    role="listbox"
    aria-label="Search results"
    class="flex flex-col gap-2"
  >
    <div
      v-for="group in groups"
      :key="group.vaultId"
      class="flex flex-col gap-1"
    >
      <div class="flex items-center gap-1">
        <button
          type="button"
          data-testid="group-toggle"
          :aria-expanded="!group.collapsed"
          :aria-controls="`search-group-${group.vaultId}`"
          :aria-label="`${group.collapsed ? 'Expand' : 'Collapse'} ${group.vaultName}`"
          class="cursor-pointer rounded p-0.5 text-fg-muted transition-colors hover:bg-white/10 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="$emit('toggle', group.vaultId)"
        >
          <svg
            width="12"
            height="12"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            class="transition-transform"
            :class="group.collapsed ? '-rotate-90' : ''"
          >
            <path d="m6 9 6 6 6-6" />
          </svg>
        </button>
        <h2
          class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-fg-muted"
        >
          {{ group.vaultName }}
          <span
            data-testid="group-count"
            class="rounded-full bg-white/10 px-1.5 py-0.5 text-micro font-normal normal-case text-fg-muted"
          >{{ group.count }}</span>
        </h2>
      </div>
      <div
        :id="`search-group-${group.vaultId}`"
        class="flex flex-col gap-1"
      >
        <SearchHitRow
          v-for="row in group.rows"
          :key="row.hit.file + (row.hit.isNote ? ':n' : ':a')"
          :hit="row.hit"
          :index="row.i"
          :selected="row.i === selected"
          :query="query"
          @open="$emit('open', row.hit, $event)"
          @hover="$emit('hover', row.i)"
        />
      </div>
    </div>
  </div>
</template>
