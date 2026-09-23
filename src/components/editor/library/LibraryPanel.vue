<script setup lang="ts">
/**
 * Switches the editor shell's `library` slot between `MediaLibrary` and
 * `TitlesLibrary` (Task 33 fix round 1; review finding, Important #1):
 * `TitlesLibrary.vue` was fully built and tested in isolation but never
 * mounted anywhere, so F-37 — inserting a title card — was unreachable
 * from the running editor. `EditorRoot.vue` now fills its `library` slot
 * with THIS component instead of `MediaLibrary` directly.
 *
 * A two-tab `role="tablist"`: `aria-selected` + a `tabindex="0"` only on
 * the active tab, only the active tab's panel mounted ("don't pay for a
 * hidden tab" — the `InspectorPanel.vue` precedent), and roving-tabindex
 * keyboard behavior via the shared `useRovingTablist` composable
 * (`InspectorPanel.vue`'s own copy of the same handler was extracted into
 * it once `check:quality`'s clone-group gate caught the two as duplicates).
 *
 * **Which tab is open is local, unpersisted view state** — unlike
 * `InspectorPanel`'s `propertyTab` (a `editorWorkspace` field saved to
 * `workspace.json`), nothing else in the app reads or needs to restore
 * which library tab was last open, so there is no store field to add for
 * it.
 */
import { ref } from "vue";

import { useRovingTablist } from "../../../composables/useRovingTablist";
import MediaLibrary from "./MediaLibrary.vue";
import TitlesLibrary from "./TitlesLibrary.vue";

type LibraryTab = "media" | "titles";

const TABS: { id: LibraryTab; label: string }[] = [
  { id: "media", label: "Media" },
  { id: "titles", label: "Titles" },
];

const activeTab = ref<LibraryTab>("media");

const { setTabRef, onKeydown: onTablistKeydown } = useRovingTablist(
  () => TABS.length,
  () => TABS.findIndex((t) => t.id === activeTab.value),
  (i) => {
    activeTab.value = TABS[i].id;
  },
);
</script>

<template>
  <div
    data-testid="library-panel"
    class="flex h-full flex-col gap-2"
  >
    <div
      role="tablist"
      aria-label="Library"
      data-testid="library-tablist"
      class="flex shrink-0 gap-1"
      @keydown="onTablistKeydown"
    >
      <button
        v-for="(tab, i) in TABS"
        :id="`library-tab-${tab.id}`"
        :key="tab.id"
        :ref="(el) => setTabRef(i, el as Element | null)"
        type="button"
        role="tab"
        :data-testid="`library-tab-${tab.id}`"
        :aria-selected="tab.id === activeTab"
        :aria-controls="`library-tabpanel-${tab.id}`"
        :tabindex="tab.id === activeTab ? 0 : -1"
        class="cursor-pointer rounded px-1.5 py-0.5 text-micro transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="tab.id === activeTab ? 'bg-accent/20 text-accent-fg' : 'text-fg-subtle'"
        @click="activeTab = tab.id"
      >
        {{ tab.label }}
      </button>
    </div>

    <div
      :id="`library-tabpanel-${activeTab}`"
      role="tabpanel"
      :aria-labelledby="`library-tab-${activeTab}`"
      data-testid="library-body"
      class="min-h-0 flex-1"
    >
      <MediaLibrary v-if="activeTab === 'media'" />
      <TitlesLibrary v-else />
    </div>
  </div>
</template>
