<script setup lang="ts">
/**
 * The timeline's Teaching layers row (visual-parity Task 19; concept spec
 * §6.4 `.track-row.fx`, design D11, D12): every teaching cue in the
 * project, above the video tracks, as a chip (`CueChip`) on the OUTPUT
 * timeline, packed into rows 22 px apart (`cueLanes.packCues`) — the row is
 * `max(44, lanes × 22 + 5)` px tall, so two overlapping cues stack and the
 * row grows to hold them.
 *
 * **The label cell is pinned** exactly like a track header (`TrackLane`,
 * D12): `position: sticky; left: 0` in the one scroll container, the label
 * column's width at this window width, above the lane's cues and under the
 * sticky ruler, so the playhead (z-12) and a snap guide slide beneath it.
 * It reads the concept's: the text icon on accent, "Teaching layers",
 * "Attached to video" — every cue lives in its clip's time and moves with
 * it.
 *
 * The lane draws the tracks' grid (`rulerTicks.laneGridStyle`) over the
 * row's faint accent wash. `cues` are the packed cues to draw (`TimelineView`
 * keeps only those within a screen of the viewport, as it does clips);
 * `lanes` is the whole project's lane count, so scrolling never changes the
 * row's height.
 */
import { computed } from "vue";

import { lockedReason } from "../../../editor/actionMeta";
import { lockedTrackName } from "../../../editor/actionTargets";
import type { TeachingCue } from "../../../editor/cueLanes";
import { teachingRowHeight } from "../../../editor/cueLanes";
import { laneGridStyle } from "../../../editor/rulerTicks";
import { pxPerMs } from "../../../editor/timelineLayout";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import CueChip from "./CueChip.vue";

const props = defineProps<{
  cues: TeachingCue[];
  lanes: number;
  zoom: number;
  widthPx: number;
  /** The label column at this window width (`editorWorkspace.trackLabelWidth`). */
  labelWidth: number;
}>();
const emit = defineEmits<{
  (e: "context-menu", payload: { effectId: string; clientX: number; clientY: number }): void;
}>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const height = computed(() => teachingRowHeight(props.lanes));
const laneStyle = computed(() => laneGridStyle(pxPerMs(props.zoom) * 1000, props.widthPx));
const selectedId = computed(() => (workspace.selected?.type === "effect" ? workspace.selected.id : null));

/** Why a cue cannot change: its clip's track is locked. */
function lockOf(cue: TeachingCue): string | null {
  const name = lockedTrackName(editorProject.project, [cue.clip.id]);
  return name ? lockedReason(name) : null;
}
</script>

<template>
  <div
    data-testid="teaching-layers-row"
    class="flex w-max min-w-full border-b border-line bg-teaching-row"
    :style="{ height: `${height}px` }"
  >
    <div
      data-testid="teaching-layers-label"
      class="sticky left-0 z-[15] flex shrink-0 items-center gap-2 border-r border-line bg-panel px-2.5 py-2"
      :style="{ width: `${labelWidth}px` }"
    >
      <span
        data-testid="teaching-layers-badge"
        aria-hidden="true"
        class="grid h-7 w-7 shrink-0 place-items-center rounded-[6px] bg-accent-bg text-accent-ink"
      >
        <EditorIcon
          name="text"
          :size="15"
        />
      </span>
      <div class="min-w-0 flex-1">
        <div class="truncate text-[11px] font-[550] text-fg">
          Teaching layers
        </div>
        <span
          data-testid="teaching-layers-sub"
          class="block truncate text-[10px] text-fg-muted"
        >Attached to video</span>
      </div>
    </div>

    <div
      data-testid="teaching-layers-lane"
      role="listbox"
      aria-label="Teaching layers"
      aria-orientation="horizontal"
      class="relative"
      :style="laneStyle"
    >
      <CueChip
        v-for="cue in cues"
        :key="cue.effect.id"
        :cue="cue"
        :selected="cue.effect.id === selectedId"
        :zoom="zoom"
        :locked-reason="lockOf(cue)"
        @context-menu="emit('context-menu', $event)"
      />
    </div>
  </div>
</template>
