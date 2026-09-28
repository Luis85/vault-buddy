<script setup lang="ts">
/**
 * Switches the editor shell's `library` slot between `MediaLibrary`,
 * `TitlesLibrary`, and (Task 36) `CaptionsLibrary`/`ChaptersLibrary` -- the
 * library that "owns Media/Titles/Captions/Chapters" (SCREENS-AND-
 * INTERACTIONS.md) (Task 33 fix round 1; review finding, Important #1):
 * `TitlesLibrary.vue` was fully built and tested in isolation but never
 * mounted anywhere, so F-37 — inserting a title card — was unreachable
 * from the running editor. `EditorRoot.vue` now fills its `library` slot
 * with THIS component instead of `MediaLibrary` directly.
 *
 * A four-tab `role="tablist"`: `aria-selected` + a `tabindex="0"` only on
 * the active tab, only the active tab's panel mounted ("don't pay for a
 * hidden tab" — the `InspectorPanel.vue` precedent), and roving-tabindex
 * keyboard behavior via the shared `useRovingTablist` composable
 * (`InspectorPanel.vue`'s own copy of the same handler was extracted into
 * it once `check:quality`'s clone-group gate caught the two as duplicates).
 *
 * **Which tab is open is `editorWorkspace.libraryTab`** (the persisted
 * `library_tab` field, like `InspectorPanel`'s `propertyTab`) — Task 54
 * moved it there from a local ref because a before-you-share finding must
 * be able to OPEN a tab (Reconnect and Webcam live on Media, caption
 * findings on Captions: `checkReveal.ts`). An unset or unknown stored
 * value falls back to Media.
 *
 * **The Project section (visual-parity Task 10; design D9; concept spec
 * §3.6)** replaced Task 47's fifth "Products" tab: the tabs are the
 * concept's four (§3.1), and the rendered products live in
 * `ProjectSection` — not a tab but what `libraryTab === "project"` shows,
 * with no tab selected (Media keeps the tab stop so the row stays
 * reachable). It opens on the `projectSection` reveal
 * (`revealWorkspaceProducts`: the Project menu, the status bar, the Render
 * dialog's completion) and closes through its own "Back to media" or any
 * tab.
 *
 * **Guide targets (Task 55):** only the open tab's panel is mounted, so each
 * tab is the FALLBACK route to the lessons its panel owns — the guide points
 * at Captions while the Captions library is closed, and at the library
 * itself once it is open (the panels bind their own keys).
 */
import { computed } from "vue";

import { useGuideTabTargets } from "../../../composables/useGuideTarget";
import { useRovingTablist } from "../../../composables/useRovingTablist";
import { onReveal } from "../../../editor/revealBus";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import CaptionsLibrary from "./CaptionsLibrary.vue";
import ChaptersLibrary from "./ChaptersLibrary.vue";
import MediaLibrary from "./MediaLibrary.vue";
import ProjectSection from "./ProjectSection.vue";
import TitlesLibrary from "./TitlesLibrary.vue";

type LibraryTab = "media" | "titles" | "captions" | "chapters";
/** What the library shows: a tab, or the Project section. */
type LibraryView = LibraryTab | "project";

const TABS: { id: LibraryTab; label: string }[] = [
  { id: "media", label: "Media" },
  { id: "titles", label: "Titles" },
  { id: "captions", label: "Captions" },
  { id: "chapters", label: "Chapters" },
];

const workspace = useEditorWorkspaceStore();
const view = computed<LibraryView>(() => {
  const saved = workspace.libraryTab;
  if (saved === "project") return "project";
  return TABS.some((t) => t.id === saved) ? (saved as LibraryTab) : "media";
});
/** The tab that holds the tab stop: the open one, else Media. */
const focusTab = computed<LibraryTab>(() => (view.value === "project" ? "media" : view.value));
function choose(tab: LibraryTab): void {
  workspace.setLibraryTab(tab);
}

onReveal("projectSection", () => workspace.setLibraryTab("project"));

const { setTabRef, onKeydown: onTablistKeydown } = useRovingTablist(
  () => TABS.length,
  () => TABS.findIndex((t) => t.id === focusTab.value),
  (i) => choose(TABS[i].id),
);

const bindTabTarget = useGuideTabTargets<LibraryTab>({
  media: ["library.import", "library.webcam"],
  captions: ["library.captions"],
  chapters: ["library.chapters"],
});
function setTab(i: number, el: Element | null): void {
  setTabRef(i, el);
  bindTabTarget(TABS[i].id, el);
}
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
      class="flex h-12 shrink-0 flex-nowrap gap-0 border-b border-line px-1.5 py-[5px]"
      @keydown="onTablistKeydown"
    >
      <button
        v-for="(tab, i) in TABS"
        :id="`library-tab-${tab.id}`"
        :key="tab.id"
        :ref="(el) => setTab(i, el as Element | null)"
        type="button"
        role="tab"
        :data-testid="`library-tab-${tab.id}`"
        :aria-selected="tab.id === view"
        :aria-controls="tab.id === view ? `library-tabpanel-${tab.id}` : undefined"
        :tabindex="tab.id === focusTab ? 0 : -1"
        class="min-h-8 flex-1 cursor-pointer rounded-md px-1 py-1.5 text-micro transition-colors hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="tab.id === view ? 'bg-accent-bg text-accent-ink' : 'text-fg-subtle'"
        @click="choose(tab.id)"
      >
        {{ tab.label }}
      </button>
    </div>

    <section
      v-if="view === 'project'"
      aria-label="Workspace and rendered products"
      data-testid="library-body"
      class="min-h-0 flex-1"
    >
      <ProjectSection />
    </section>
    <div
      v-else
      :id="`library-tabpanel-${view}`"
      role="tabpanel"
      :aria-labelledby="`library-tab-${view}`"
      data-testid="library-body"
      class="min-h-0 flex-1"
    >
      <MediaLibrary v-if="view === 'media'" />
      <TitlesLibrary v-else-if="view === 'titles'" />
      <CaptionsLibrary v-else-if="view === 'captions'" />
      <ChaptersLibrary v-else />
    </div>
  </div>
</template>
