<script setup lang="ts">
import type { SearchHit } from "../types";
import { searchHitId } from "../utils/searchResults";
import HighlightText from "./HighlightText.vue";

// One search hit: kind icon, highlighted name, folder and snippet. Its own
// component (the TaskRow.vue precedent) because the row's own branching —
// note vs file icon, the optional folder and snippet lines, the selected
// treatment — is what pushed the result list's template past the fallow
// complexity threshold once the list left Search.vue (split at 497/500
// nonblank). Presentational only: whether this row is selected, and what
// opening it does, belong to Search.vue.
defineProps<{
  hit: SearchHit;
  /** Index into the visible rows — the keyboard selection's coordinate. */
  index: number;
  selected: boolean;
  query: string;
}>();
defineEmits<{
  (e: "open", keepOpen: boolean): void;
  (e: "hover"): void;
}>();
</script>

<template>
  <button
    :id="searchHitId(index)"
    type="button"
    data-testid="search-hit"
    role="option"
    :aria-selected="selected"
    class="flex w-full cursor-pointer flex-col items-start gap-0.5 rounded-control border px-2 py-1 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    :class="
      selected
        ? 'border-violet-400/60 bg-white/10'
        : 'border-white/10 bg-white/5'
    "
    @click="$emit('open', $event.ctrlKey || $event.metaKey)"
    @mousemove="$emit('hover')"
  >
    <span class="flex w-full min-w-0 items-center gap-1.5">
      <svg
        v-if="hit.isNote"
        data-testid="hit-icon-note"
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
        class="shrink-0 text-fg-muted"
      >
        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
        <path d="M14 2v6h6M16 13H8M16 17H8M10 9H8" />
      </svg>
      <svg
        v-else
        data-testid="hit-icon-file"
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
        class="shrink-0 text-fg-muted"
      >
        <path
          d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"
        />
      </svg>
      <span
        class="min-w-0 flex-1 truncate text-sm text-fg"
        :title="hit.name"
      >
        <HighlightText
          :text="hit.name"
          :query="query"
        />
      </span>
    </span>
    <span
      v-if="hit.folder"
      class="w-full truncate text-xs text-fg-subtle"
    >
      {{ hit.folder }}
    </span>
    <span
      v-if="hit.snippet"
      class="w-full truncate text-xs text-fg-muted"
    >
      <HighlightText
        :text="hit.snippet"
        :query="query"
      />
    </span>
  </button>
</template>
