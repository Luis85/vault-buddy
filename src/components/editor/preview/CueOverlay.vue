<script setup lang="ts">
/**
 * The teaching cues as the viewer sees them (Task 35; F-27–F-33): one SVG
 * laid exactly over the letterboxed canvas, in CANVAS pixels (`viewBox` =
 * the project's canvas), drawing every cue whose output span holds the
 * playhead (`cueGeometry.activeCues`) with `cueShapes.cueShapes`.
 *
 * **Part of the picture.** `PreviewSurface.vue` mounts this INSIDE its stage,
 * above the media layers — what is drawn here is what the render burns in.
 * Selection outlines, grab handles and the zoom's focal marker are NOT: they
 * live in the sibling `CueHandles.vue`, outside the stage like Task 31's
 * `LayoutHandles`, so nothing an editor needs to see ever reads as output.
 * The whole layer ignores the pointer; `CueHandles` is what you grab.
 *
 * A zoom cue scales this layer with the media (the reference compositor
 * draws cues under the same camera), via the same `ZoomTransform`, so an
 * arrow keeps pointing at the detail it was drawn on while zoomed in.
 * `draft` is a drag's transient copy of one cue (R14: the store is never
 * touched until the one `updateEffect` on release).
 */
import { computed } from "vue";

import type { ZoomTransform } from "../../../editor/cueGeometry";
import { activeCues, svgZoomTransform } from "../../../editor/cueGeometry";
import type { CueShape } from "../../../editor/cueShapes";
import { cueShapes } from "../../../editor/cueShapes";
import type { Box } from "../../../editor/previewGeometry";
import { boxStyle } from "../../../editor/previewGeometry";
import type { Effect, Project } from "../../../editorTypes";

const props = defineProps<{
  project: Project | null;
  /** Output time, ms. */
  timeMs: number;
  /** The canvas's letterboxed box inside the stage, stage px. */
  frame: Box;
  zoom: ZoomTransform;
  draft?: Effect | null;
}>();

const canvas = computed(() => props.project?.canvas ?? { width: 16, height: 9 });

const drawn = computed(() =>
  activeCues(props.project, props.timeMs)
    .map(({ effect }) => (props.draft?.id === effect.id ? props.draft : effect))
    .map((effect) => ({ id: effect.id, kind: effect.kind, shapes: cueShapes(effect, canvas.value) }))
    .filter((cue) => cue.shapes.length > 0),
);

const frameStyle = computed(() => boxStyle(props.frame));

const zoomAttr = computed(() => svgZoomTransform(props.zoom, canvas.value));

/** Line spacing of a multi-line text shape. */
function lineStep(shape: CueShape): number {
  return Number(shape.attrs["font-size"]) * 1.25;
}
</script>

<template>
  <svg
    data-testid="cue-overlay"
    class="pointer-events-none absolute"
    :style="frameStyle"
    :viewBox="`0 0 ${canvas.width} ${canvas.height}`"
    preserveAspectRatio="none"
    aria-hidden="true"
  >
    <g :transform="zoomAttr">
      <g
        v-for="cue in drawn"
        :key="cue.id"
        :data-testid="`cue-${cue.id}`"
        :data-kind="cue.kind"
      >
        <template
          v-for="(shape, i) in cue.shapes"
          :key="i"
        >
          <text
            v-if="shape.tag === 'text'"
            :data-part="shape.part"
            v-bind="shape.attrs"
          ><tspan
            v-for="(line, j) in shape.lines"
            :key="j"
            :x="shape.attrs.x"
            :dy="j === 0 ? 0 : lineStep(shape)"
          >{{ line }}</tspan></text>
          <component
            :is="shape.tag"
            v-else
            :data-part="shape.part"
            v-bind="shape.attrs"
          />
        </template>
      </g>
    </g>
  </svg>
</template>
