<script setup lang="ts">
/**
 * "Adjust all" (visual-parity Task 13 fix round 1, ruling T13-1): under the
 * §5 Selection properties, the categories that act on a whole
 * multi-selection — Layout and Color, each one atomic `setLayout` /
 * `setAdjustments` over every selected clip (F-23). Offered only when every
 * selected clip is on a video track, since Rust refuses both otherwise
 * (`visualTargets.ts`). What fills the panel is the caller's scoped default
 * slot (`{ tab, clipIds }`), the `ClipInspector` seam, so the root's own
 * Layout and Color sections render here with the whole selection.
 */
import { computed } from "vue";

import { activeTabOf, BATCH_TABS } from "../../../editor/inspectorState";
import { allOnVideoTracks } from "../../../editor/visualTargets";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import InspectorSection from "./InspectorSection.vue";
import InspectorTabs from "./InspectorTabs.vue";

const workspace = useEditorWorkspaceStore();
const editorProject = useEditorProjectStore();

const visual = computed(() => allOnVideoTracks(editorProject.project, workspace.selectionClipIds));
const activeTab = computed(() => activeTabOf(workspace.propertyTab, BATCH_TABS));
</script>

<template>
  <InspectorSection
    v-if="visual"
    title="Adjust all"
  >
    <InspectorTabs
      :tabs="BATCH_TABS"
      :active="activeTab"
      @select="workspace.setPropertyTab"
    />
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
  </InspectorSection>
</template>
