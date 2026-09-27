<script setup lang="ts">
/**
 * The Inspector's Layout category (Task 31; F-21, F-23, F-38; visual-parity
 * Task 14, concept spec §5 "Layout", screens 02/03). "Webcam overlay" (or
 * "Video layout" for any other picture): Full frame / Picture-in-picture,
 * Horizontal / Vertical in percent of the canvas, a Size range that keeps
 * the frame's proportions, the four corner presets and the help line; then
 * the "Frame & crop" and "Transform source" disclosures, each remembering
 * whether it is open per selection (view state). Every control is one
 * `setLayout` over the WHOLE selection (`clipIds`), so a multi-selection
 * gets identical values and Rust applies them atomically or not at all; the
 * fields show the first selected clip's values. A clip on a locked track
 * disables everything; the inspector's frame says why (one lock note,
 * visual-parity Task 13), and the fieldset carries the reason as its
 * tooltip.
 *
 * Position is bounded by the frame the way the preview handles are
 * (`layoutGeometry`): Horizontal may go only as far as the width leaves room
 * for — refused inline with the correction, never silently clamped. A
 * corner preset and the circle shape keep a circle square in PIXELS on every
 * canvas (F-38).
 */
import { computed } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { percentField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { useSelectedClips } from "../../../composables/useSelectedClips";
import type { Corner } from "../../../editor/layoutGeometry";
import { maxProportionalWidth, minProportionalWidth, roundBox, sizeBox } from "../../../editor/layoutGeometry";
import type { LayoutPatch } from "../../../editor/layoutPresets";
import { cornerPatch, FULL_FRAME, isWebcamAsset, pipPatch } from "../../../editor/layoutPresets";
import { allOnVideoTracks } from "../../../editor/visualTargets";
import type { Clip } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import CornerPresets from "./CornerPresets.vue";
import FrameCropDisclosure from "./FrameCropDisclosure.vue";
import InspectorButton from "./InspectorButton.vue";
import InspectorNumberInput from "./InspectorNumberInput.vue";
import InspectorRange from "./InspectorRange.vue";
import InspectorSection from "./InspectorSection.vue";
import TransformSource from "./TransformSource.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
/** The guide's `inspector.layout` (Task 55): the open section wins over its tab. */
const sectionTarget = useGuideTarget("inspector.layout");
const { clips, lockReason } = useSelectedClips(() => props.clipIds);

const FULL: Pick<Clip, "x" | "y" | "w" | "h" | "opacity"> = { x: 0, y: 0, w: 1, h: 1, opacity: 1 };
/** The first clip, or a full-frame stand-in the fields can read safely. */
const c = computed(() => clips.value[0] ?? FULL);
const asset = computed(() => editorProject.project?.assets.find((a) => a.id === c.value.asset_id));

/** Only video-track clips have a layout (`visualTargets.ts`, Rust's
 * `check_targets`): anything else gets the note. */
const ready = computed(() =>
  allOnVideoTracks(
    editorProject.project,
    clips.value.map((clip) => clip.id),
  ),
);
const canvas = computed(() => editorProject.project?.canvas ?? { width: 1280, height: 720 });
const cam = computed(() => isWebcamAsset(asset.value));
/** The disclosures remember their state per selection. */
const selectionKey = computed(() => props.clipIds.join(","));

function send(patch: LayoutPatch): Promise<boolean> {
  if (lockReason.value || clips.value.length === 0) return Promise.resolve(false);
  return editorProject.execute({ kind: "setLayout", clipIds: clips.value.map((clip) => clip.id), ...patch });
}

const fields = {
  x: useInspectorDraft(
    percentField({ value: () => c.value.x, label: "Horizontal", min: () => 0, max: () => 1 - c.value.w }),
    (x) => send({ x }),
  ),
  y: useInspectorDraft(
    percentField({ value: () => c.value.y, label: "Vertical", min: () => 0, max: () => 1 - c.value.h }),
    (y) => send({ y }),
  ),
};

/** The Size range in percent, a tenth inside the widths Rust accepts (the
 * epsilon keeps a float like 100.00000000000003 from rounding a step away). */
const TENTHS_EPSILON = 1e-9;
const sizeMin = computed(() => Math.ceil(minProportionalWidth(c.value) * 1000 - TENTHS_EPSILON) / 10);
const sizeMax = computed(() => Math.floor(maxProportionalWidth(c.value) * 1000 + TENTHS_EPSILON) / 10);
/** One clip keeps its box inside the frame; a selection gets only the size,
 * never the first clip's place (each keeps its own). */
function onSize(pct: number): Promise<boolean> {
  const box = roundBox(sizeBox(c.value, pct / 100));
  return send(clips.value.length === 1 ? box : { w: box.w, h: box.h });
}
function onCorner(corner: Corner): void {
  void send(cornerPatch(corner, c.value, canvas.value, asset.value));
}
</script>

<template>
  <fieldset
    v-if="ready"
    :ref="sectionTarget"
    data-testid="layout-section"
    :disabled="lockReason !== null"
    :title="lockReason ?? undefined"
    class="flex min-w-0 flex-col disabled:opacity-50"
  >
    <InspectorSection :title="cam ? 'Webcam overlay' : 'Video layout'">
      <div class="flex flex-wrap gap-[5px]">
        <InspectorButton
          preset
          data-testid="layout-preset-full"
          @click="send(FULL_FRAME)"
        >
          Full frame
        </InspectorButton>
        <InspectorButton
          preset
          data-testid="layout-preset-pip"
          @click="send(pipPatch(canvas, asset))"
        >
          Picture-in-picture
        </InspectorButton>
      </div>
      <div class="grid grid-cols-2 gap-[9px]">
        <InspectorNumberInput
          :field="fields.x"
          label="Horizontal (%)"
          testid="layout-section-x"
          :step="0.5"
        />
        <InspectorNumberInput
          :field="fields.y"
          label="Vertical (%)"
          testid="layout-section-y"
          :step="0.5"
        />
      </div>
      <InspectorRange
        label="Size"
        testid="layout-section-size"
        :value="Math.round(c.w * 1000) / 10"
        :min="sizeMin"
        :max="sizeMax"
        :step="0.5"
        suffix="%"
        :commit="onSize"
      />
      <CornerPresets @corner="onCorner" />
      <p
        data-testid="layout-section-help"
        class="text-[10px] leading-[1.6] text-fg-muted"
      >
        Drag to move. Drag any corner to resize. Size keeps the frame’s proportions; these controls work without dragging.
      </p>
    </InspectorSection>
    <FrameCropDisclosure
      :clip="c"
      :canvas="canvas"
      :cam="cam"
      :store-key="`${selectionKey}:frame`"
      :send="send"
    />
    <TransformSource
      :clip="c"
      :store-key="`${selectionKey}:transform`"
      :send="send"
    />
  </fieldset>
  <p
    v-else
    :ref="sectionTarget"
    data-testid="layout-section-audio"
  >
    Layout applies to video and image clips. Select only those to place them.
  </p>
</template>
