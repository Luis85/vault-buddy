<script setup lang="ts">
/**
 * The tutorial editor's inspector (Task 19; F-48/F-49; visual-parity
 * Task 13, concept spec §5): a 48px heading naming what is selected, with
 * the ✕ that hides the panel (`InspectorHeading`), over a scrolling body
 * that shows one of five states (`inspectorState.ts`):
 *
 *   - nothing selected: what to do next and the project's facts
 *     (`EmptyInspector`) — never a panel of disabled controls (SCREENS §02);
 *   - one clip (`ClipInspector`): its card, then its categories as a 2×3
 *     grid bound to the persisted `editorWorkspace.propertyTab` — Clip,
 *     Layout, Fades, Audio, Speed, Color, or four for a sound clip — and the
 *     active category's named slot (`clip`/`layout`/…), scoped with
 *     `clipIds`; the other slots stay unmounted;
 *   - several clips: the shared actions (`MultiInspector`), then the Layout
 *     and Color categories over the whole selection (`AdjustAllSection`);
 *   - a track (`editorWorkspace.selectedTrackId`): its own controls
 *     (`TrackInspector`);
 *   - a teaching cue (`cueActions.selectedEffectOf`, Task 35): the `#effect`
 *     slot in place of the clip categories, with a way back to them.
 */
import { computed } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { selectedEffectOf } from "../../../editor/cueActions";
import { INSPECTOR_TITLES, inspectorMode } from "../../../editor/inspectorState";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import AdjustAllSection from "./AdjustAllSection.vue";
import ClipInspector from "./ClipInspector.vue";
import EmptyInspector from "./EmptyInspector.vue";
import InspectorHeading from "./InspectorHeading.vue";
import MultiInspector from "./MultiInspector.vue";
import TrackInspector from "./TrackInspector.vue";

const workspace = useEditorWorkspaceStore();
const editorProject = useEditorProjectStore();

const selectedEffectId = computed(
  () => selectedEffectOf(editorProject.project, workspace.selected, workspace.selectionClipIds)?.id ?? null,
);
const track = computed(
  () => editorProject.project?.tracks.find((t) => t.id === workspace.selectedTrackId) ?? null,
);
const mode = computed(() =>
  inspectorMode(track.value?.id ?? null, selectedEffectId.value, workspace.selectionClipIds.length),
);

// The guide's `inspector` target (Task 55) is the whole panel.
const panelTarget = useGuideTarget("inspector");
</script>

<template>
  <div
    :ref="panelTarget"
    data-testid="inspector-panel"
    class="flex h-full min-h-0 flex-col"
  >
    <InspectorHeading :title="INSPECTOR_TITLES[mode]" />
    <div class="min-h-0 flex-1 overflow-y-auto p-3.5 text-micro text-fg-subtle">
      <!-- The states in `inspectorMode`'s order: a track, a cue, several
           clips, nothing, one clip. -->
      <TrackInspector
        v-if="track"
        :key="track.id"
        :track="track"
      />
      <div
        v-else-if="selectedEffectId"
        data-testid="inspector-effect"
        class="flex flex-col gap-2"
      >
        <button
          type="button"
          data-testid="inspector-effect-back"
          class="cursor-pointer self-start rounded px-1.5 py-0.5 text-fg-secondary hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="workspace.setSelected(null)"
        >
          ← Clip settings
        </button>
        <slot
          name="effect"
          :effect-id="selectedEffectId"
        />
      </div>
      <template v-else-if="mode === 'multi'">
        <MultiInspector />
        <AdjustAllSection>
          <template #default="{ tab, clipIds }">
            <slot
              :name="tab"
              :clip-ids="clipIds"
            />
          </template>
        </AdjustAllSection>
      </template>
      <EmptyInspector v-else-if="mode === 'none'" />
      <ClipInspector v-else>
        <template #default="{ tab, clipIds }">
          <slot
            :name="tab"
            :clip-ids="clipIds"
          />
        </template>
      </ClipInspector>
    </div>
  </div>
</template>
