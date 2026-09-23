<script setup lang="ts">
/**
 * The Inspector's Layout category (Task 31; F-21, F-23, F-38;
 * SCREENS-AND-INTERACTIONS.md §04: "Numeric entry is the alternative to
 * dragging"). Position and size in percent of the canvas, the four corner
 * presets, the frame shape, fit, crop zoom and focus, rotation, mirror,
 * vertical flip and opacity — every one a `setLayout` over the WHOLE
 * selection (`clipIds`), so a multi-selection gets identical values and
 * Rust applies them atomically or not at all. The fields show the first
 * selected clip's values; `InspectorPanel` already states the scope. A clip
 * on a locked track disables everything and says why (R20).
 *
 * Position and size are bounded by the frame the way the preview handles
 * are (`layoutGeometry`): X may go only as far as the width leaves room
 * for, and so on — refused inline with the correction, never silently
 * clamped. A corner preset and the circle shape both go through
 * `cornerPreset`/`aspectHeight`, so a circle is square in PIXELS on every
 * canvas (F-38). The crop controls only mean something when the picture
 * fills its frame (`cover`), so they show only then, as in the reference.
 */
import { computed } from "vue";

import { numberField, percentField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { useSelectedClips } from "../../../composables/useSelectedClips";
import type { EditorCommand } from "../../../editor/editorCommandTypes";
import type { Corner } from "../../../editor/layoutGeometry";
import { aspectHeight, clampBox, cornerPreset, PIP_SIZE, roundBox } from "../../../editor/layoutGeometry";
import { shapeOf } from "../../../editor/previewTransform";
import type { Clip, Fit, FrameShape, Rotation } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import InspectorNumberInput from "./InspectorNumberInput.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
const { clips, lockReason } = useSelectedClips(() => props.clipIds);

const FULL: Pick<Clip, "x" | "y" | "w" | "h" | "opacity"> = { x: 0, y: 0, w: 1, h: 1, opacity: 1 };
const first = computed(() => clips.value[0]);
/** The first clip, or a full-frame stand-in the fields can read safely. */
const c = (): Pick<Clip, "x" | "y" | "w" | "h" | "opacity"> & Partial<Clip> => first.value ?? FULL;

/** Only video-track clips have a layout: anything else gets the note. */
const ready = computed(() => {
  const tracks = editorProject.project?.tracks ?? [];
  const visual = (trackId: string) => tracks.find((t) => t.id === trackId)?.kind === "video";
  return clips.value.length > 0 && clips.value.every((clip) => visual(clip.track_id));
});
const canvas = computed(() => editorProject.project?.canvas ?? { width: 1280, height: 720 });
const shape = computed<FrameShape>(() => shapeOf(c()));
const fit = computed<Fit>(() => c().fit ?? "contain");
const rotation = computed(() => String(c().rotation ?? 0));
const mirror = computed(() => c().mirror ?? false);
const flipY = computed(() => c().flip_y ?? false);

type LayoutPatch = Omit<Extract<EditorCommand, { kind: "setLayout" }>, "kind" | "clipIds">;

function send(patch: LayoutPatch): Promise<boolean> {
  if (lockReason.value || clips.value.length === 0) return Promise.resolve(false);
  return editorProject.execute({ kind: "setLayout", clipIds: clips.value.map((clip) => clip.id), ...patch });
}

const whole = { min: () => 0, max: () => 1 };
const fields = {
  x: useInspectorDraft(percentField({ value: () => c().x, label: "X", min: () => 0, max: () => 1 - c().w }), (x) => send({ x })),
  y: useInspectorDraft(percentField({ value: () => c().y, label: "Y", min: () => 0, max: () => 1 - c().h }), (y) => send({ y })),
  w: useInspectorDraft(percentField({ value: () => c().w, label: "Width", min: () => 0.1, max: () => 1 - c().x }), (w) => send({ w })),
  h: useInspectorDraft(percentField({ value: () => c().h, label: "Height", min: () => 0.1, max: () => 1 - c().y }), (h) => send({ h })),
  opacity: useInspectorDraft(percentField({ value: () => c().opacity, label: "Opacity", ...whole }), (opacity) => send({ opacity })),
  cropX: useInspectorDraft(percentField({ value: () => c().crop_x ?? 0.5, label: "Focus X", ...whole }), (cropX) => send({ cropX })),
  cropY: useInspectorDraft(percentField({ value: () => c().crop_y ?? 0.5, label: "Focus Y", ...whole }), (cropY) => send({ cropY })),
  // Mirrors `validate::check_clip`'s `crop_zoom` range `[1, 3]`.
  cropZoom: useInspectorDraft(
    numberField({ value: () => c().crop_zoom ?? 1, label: "Crop zoom", min: 1, max: 3, rangeLabel: "1× and 3×" }),
    (cropZoom) => send({ cropZoom }),
  ),
};

const CORNERS: { id: Corner; label: string }[] = [
  { id: "tl", label: "Top left" },
  { id: "tr", label: "Top right" },
  { id: "bl", label: "Bottom left" },
  { id: "br", label: "Bottom right" },
];
const ROTATIONS: Rotation[] = [0, 90, 180, 270];

/** A picture already in a box keeps its width; a full-frame one becomes the
 * default picture-in-picture size. */
function onCorner(corner: Corner): void {
  const asset = editorProject.project?.assets.find((a) => a.id === c().asset_id);
  const size = c().w < 0.98 ? c().w : PIP_SIZE;
  const source = { width: asset?.width, height: asset?.height, circle: shape.value === "circle" };
  void send(roundBox(cornerPreset(corner, canvas.value, size, source)));
}

/** A circle becomes square in pixels (and fills its frame), the reference's
 * `setVideoShape`; a box that would then be taller than the canvas narrows
 * instead, so it stays a circle. Leaving a circle keeps the box as it is. */
function onShape(event: Event): void {
  const next = (event.target as HTMLSelectElement).value as FrameShape;
  if (next !== "circle") {
    void send({ frameShape: next });
    return;
  }
  const b = c();
  const tall = aspectHeight(b.w, canvas.value, 1);
  const w = tall > 1 ? b.w / tall : b.w;
  void send({ frameShape: "circle", fit: "cover", ...roundBox(clampBox({ x: b.x, y: b.y, w, h: Math.min(tall, 1) })) });
}

function onFit(event: Event): void {
  void send({ fit: (event.target as HTMLSelectElement).value as Fit });
}
function onRotation(event: Event): void {
  void send({ rotation: Number((event.target as HTMLSelectElement).value) as Rotation });
}
function onMirror(event: Event): void {
  void send({ mirror: (event.target as HTMLInputElement).checked });
}
function onFlip(event: Event): void {
  void send({ flipY: (event.target as HTMLInputElement).checked });
}
</script>

<template>
  <div
    v-if="ready"
    data-testid="layout-section"
    class="flex flex-col gap-2"
  >
    <p
      v-if="lockReason"
      data-testid="layout-section-locked"
    >
      {{ lockReason }} — unlock it to change this layout.
    </p>
    <fieldset
      :disabled="lockReason !== null"
      class="flex flex-col gap-2 disabled:opacity-50"
    >
      <div class="grid grid-cols-2 gap-2">
        <InspectorNumberInput
          :field="fields.x"
          label="X (%)"
          testid="layout-section-x"
        />
        <InspectorNumberInput
          :field="fields.y"
          label="Y (%)"
          testid="layout-section-y"
        />
        <InspectorNumberInput
          :field="fields.w"
          label="Width (%)"
          testid="layout-section-w"
        />
        <InspectorNumberInput
          :field="fields.h"
          label="Height (%)"
          testid="layout-section-h"
        />
      </div>
      <div
        class="grid grid-cols-2 gap-1"
        role="group"
        aria-label="Place in a corner"
      >
        <button
          v-for="corner in CORNERS"
          :key="corner.id"
          type="button"
          :data-testid="`layout-corner-${corner.id}`"
          class="cursor-pointer rounded border border-line px-1.5 py-0.5 text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="onCorner(corner.id)"
        >
          {{ corner.label }}
        </button>
      </div>
      <label class="flex flex-col gap-0.5">
        <span class="text-fg-subtle">Shape</span>
        <select
          data-testid="layout-section-shape"
          class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
          :value="shape"
          @change="onShape"
        >
          <option value="rectangle">Rectangle</option>
          <option value="rounded">Rounded</option>
          <option value="circle">Circle</option>
        </select>
      </label>
      <label class="flex flex-col gap-0.5">
        <span class="text-fg-subtle">Fit</span>
        <select
          data-testid="layout-section-fit"
          class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
          :value="fit"
          @change="onFit"
        >
          <option value="contain">Fit · show the whole picture</option>
          <option value="cover">Fill · crop to the frame</option>
        </select>
      </label>
      <template v-if="fit === 'cover'">
        <InspectorNumberInput
          :field="fields.cropZoom"
          label="Crop zoom (×)"
          testid="layout-section-crop-zoom"
        />
        <div class="grid grid-cols-2 gap-2">
          <InspectorNumberInput
            :field="fields.cropX"
            label="Focus X (%)"
            testid="layout-section-crop-x"
          />
          <InspectorNumberInput
            :field="fields.cropY"
            label="Focus Y (%)"
            testid="layout-section-crop-y"
          />
        </div>
      </template>
      <label class="flex flex-col gap-0.5">
        <span class="text-fg-subtle">Rotation</span>
        <select
          data-testid="layout-section-rotation"
          class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
          :value="rotation"
          @change="onRotation"
        >
          <option
            v-for="deg in ROTATIONS"
            :key="deg"
            :value="String(deg)"
          >
            {{ deg }}°
          </option>
        </select>
      </label>
      <label class="flex items-center gap-1.5">
        <input
          data-testid="layout-section-mirror"
          type="checkbox"
          :checked="mirror"
          @change="onMirror"
        >
        <span>Mirror</span>
      </label>
      <label class="flex items-center gap-1.5">
        <input
          data-testid="layout-section-flip"
          type="checkbox"
          :checked="flipY"
          @change="onFlip"
        >
        <span>Flip vertically</span>
      </label>
      <InspectorNumberInput
        :field="fields.opacity"
        label="Opacity (%)"
        testid="layout-section-opacity"
      />
    </fieldset>
  </div>
  <p
    v-else
    data-testid="layout-section-audio"
  >
    Layout applies to video and image clips. Select only those to place them.
  </p>
</template>
