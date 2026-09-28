<script setup lang="ts">
/**
 * One selected clip (visual-parity Task 13; concept spec §5): its card —
 * the asset's glyph, the clip's name, "{track} · {d.d}s" — then its
 * categories (`InspectorTabs`, bound to the persisted
 * `editorWorkspace.propertyTab`), a warning when its track is locked, and
 * the active category's tab panel. What fills the panel is the caller's
 * scoped default slot (`{ tab, clipIds }`), so `InspectorPanel` can hand
 * each category to the root's named slots; the other categories stay
 * unmounted.
 */
import { computed } from "vue";

import { activeTabOf, assetKindOf, clipCardDetail, tabsFor } from "../../../editor/inspectorState";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import InspectorTabs from "./InspectorTabs.vue";
import SelectionCard from "./SelectionCard.vue";

const workspace = useEditorWorkspaceStore();
const editorProject = useEditorProjectStore();

const clip = computed(() => editorProject.clipById(workspace.selectionClipIds[0] ?? "") ?? null);
const kind = computed(() => assetKindOf(editorProject.project, clip.value));
const tabs = computed(() => tabsFor(kind.value));
const activeTab = computed(() => activeTabOf(workspace.propertyTab, tabs.value));
const card = computed(() => {
  const c = clip.value;
  if (!c) return null;
  return {
    icon: kind.value === "audio" ? ("music" as const) : ("video" as const),
    name: c.name,
    detail: clipCardDetail(editorProject.project, c),
  };
});
const locked = computed(() => {
  const c = clip.value;
  return Boolean(c && editorProject.project?.tracks.find((t) => t.id === c.track_id)?.locked);
});
</script>

<template>
  <SelectionCard
    v-if="card"
    v-bind="card"
  />
  <InspectorTabs
    :tabs="tabs"
    :active="activeTab"
    @select="workspace.setPropertyTab"
  />
  <p
    v-if="locked"
    data-testid="inspector-locked"
    class="mb-3 rounded-md bg-gold-bg p-[9px] text-[10px] leading-[1.6] text-gold"
  >
    Track locked. Unlock it using the padlock below.
  </p>
  <div
    :id="`inspector-tabpanel-${activeTab}`"
    role="tabpanel"
    :aria-labelledby="`inspector-tab-${activeTab}`"
    data-testid="inspector-body"
  >
    <slot
      :tab="activeTab"
      :clip-ids="workspace.selectionClipIds"
    />
  </div>
</template>
