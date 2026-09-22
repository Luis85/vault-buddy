<script setup lang="ts">
/**
 * The tutorial editor's inspector shell (Task 19; F-48/F-49;
 * ARCHITECTURE-AND-STACK.md: "`Inspector` and six panels | Clip, Layout,
 * Fades, Audio, Speed, Color"; SCREENS-AND-INTERACTIONS.md §04: "Inspector
 * categories are Clip, Layout, Fades, Audio, Speed and Color. A control's
 * scope is the selected object or an explicitly indicated multi-selection.";
 * §02: "Unselected states teach what to do next rather than filling the
 * inspector with disabled controls.").
 *
 * The six category tabs are always shown (a `role="tablist"` bound to
 * `editorWorkspace.propertyTab`, the persisted `property_tab` field —
 * choosing a tab is a view preference independent of what's selected right
 * now, so it survives across selections and a reopen). What renders BELOW
 * the tabs depends on `editorWorkspace.selectionClipIds`:
 *
 *   - empty: teaching copy, never a disabled control — there is nothing to
 *     scope a control to yet, and greying out six panels of inputs would
 *     bury the one actionable instruction ("select a clip") under noise.
 *   - exactly one clip: the active category's slot renders scoped to that
 *     one id, no extra banner (a single selection IS the scope).
 *   - more than one: a "`N` clips selected" statement states the scope
 *     explicitly before the slot, so a later section filling that slot
 *     never has to re-derive or restate what it's editing.
 *
 * Each category is a named slot (`clip`/`layout`/`fades`/`audio`/`speed`/
 * `color`), scoped with `clipIds` — the exact selection to act on — so a
 * later task's real section (Task 20 onward) can be dropped in without this
 * shell changing. Only the ACTIVE category's slot is rendered; the other
 * five stay unmounted, the same "don't pay for a hidden tab" posture
 * `ScreenSourcePicker`'s `<TabGroup>` already uses elsewhere in this repo.
 */
import { computed, nextTick, ref } from "vue";

import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

type CategoryId = "clip" | "layout" | "fades" | "audio" | "speed" | "color";

const CATEGORIES: { id: CategoryId; label: string }[] = [
  { id: "clip", label: "Clip" },
  { id: "layout", label: "Layout" },
  { id: "fades", label: "Fades" },
  { id: "audio", label: "Audio" },
  { id: "speed", label: "Speed" },
  { id: "color", label: "Color" },
];
const CATEGORY_IDS = CATEGORIES.map((c) => c.id);

const workspace = useEditorWorkspaceStore();

/** Falls back to the first category when the persisted tab is unset or
 * names something this shell no longer recognizes (a stale `workspace.json`
 * from a future build, or the never-populated initial `null`). */
const activeTab = computed<CategoryId>(() => {
  const saved = workspace.propertyTab;
  return (CATEGORY_IDS as string[]).includes(saved ?? "") ? (saved as CategoryId) : "clip";
});

function selectTab(id: CategoryId): void {
  workspace.setPropertyTab(id);
}

const selectionCount = computed(() => workspace.selectionClipIds.length);
const hasSelection = computed(() => selectionCount.value > 0);
const isMultiSelection = computed(() => selectionCount.value > 1);

// ---- roving tabindex over the tablist (the PreviewToolbar/ContextMenu
// precedent: arrow keys move focus, Home/End jump to the ends). ------------
const tabEls = ref<(HTMLElement | null)[]>([]);
function setTabRef(i: number, el: Element | null): void {
  tabEls.value[i] = el as HTMLElement | null;
}
async function onTablistKeydown(event: KeyboardEvent): Promise<void> {
  const n = CATEGORIES.length;
  const current = CATEGORY_IDS.indexOf(activeTab.value);
  let target: number;
  if (event.key === "ArrowRight") target = (current + 1) % n;
  else if (event.key === "ArrowLeft") target = (current - 1 + n) % n;
  else if (event.key === "Home") target = 0;
  else if (event.key === "End") target = n - 1;
  else return;
  event.preventDefault();
  selectTab(CATEGORIES[target].id);
  await nextTick();
  tabEls.value[target]?.focus();
}
</script>

<template>
  <div
    data-testid="inspector-panel"
    class="flex h-full flex-col gap-2"
  >
    <div
      role="tablist"
      aria-label="Inspector categories"
      data-testid="inspector-tablist"
      class="flex flex-wrap gap-1"
      @keydown="onTablistKeydown"
    >
      <button
        v-for="(cat, i) in CATEGORIES"
        :id="`inspector-tab-${cat.id}`"
        :key="cat.id"
        :ref="(el) => setTabRef(i, el as Element | null)"
        type="button"
        role="tab"
        :data-testid="`inspector-tab-${cat.id}`"
        :aria-selected="cat.id === activeTab"
        :aria-controls="`inspector-tabpanel-${cat.id}`"
        :tabindex="cat.id === activeTab ? 0 : -1"
        class="cursor-pointer rounded px-1.5 py-0.5 text-micro transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="cat.id === activeTab ? 'bg-accent/20 text-accent-fg' : 'text-fg-subtle'"
        @click="selectTab(cat.id)"
      >
        {{ cat.label }}
      </button>
    </div>

    <div
      :id="`inspector-tabpanel-${activeTab}`"
      role="tabpanel"
      :aria-labelledby="`inspector-tab-${activeTab}`"
      data-testid="inspector-body"
      class="flex-1 text-micro text-fg-subtle"
    >
      <p
        v-if="!hasSelection"
        data-testid="inspector-empty"
      >
        Select a clip to adjust it…
      </p>
      <template v-else>
        <p
          v-if="isMultiSelection"
          data-testid="inspector-scope"
          class="mb-2 text-fg-muted"
        >
          {{ selectionCount }} clips selected
        </p>
        <slot
          :name="activeTab"
          :clip-ids="workspace.selectionClipIds"
        >
          This section arrives in a later task.
        </slot>
      </template>
    </div>
  </div>
</template>
