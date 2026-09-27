<script setup lang="ts">
/**
 * The Layout tab's "Frame & crop" disclosure (visual-parity Task 14; concept
 * spec §5, `webcam.js: videoLayoutHTML`): the frame shape, how the picture
 * fits its frame, the crop zoom and focus (only when it fills the frame —
 * they mean nothing otherwise), mirror and opacity. Each is one `setLayout`
 * through the section's `send`, over its whole selection.
 */
import { computed } from "vue";

import { percentField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import type { LayoutClip, LayoutPatch } from "../../../editor/layoutPresets";
import { shapePatch } from "../../../editor/layoutPresets";
import type { Size } from "../../../editor/previewGeometry";
import { shapeOf } from "../../../editor/previewTransform";
import type { Clip, Fit, FrameShape } from "../../../editorTypes";
import InspectorButton from "./InspectorButton.vue";
import InspectorNumberInput from "./InspectorNumberInput.vue";
import InspectorRange from "./InspectorRange.vue";
import PrecisionDisclosure from "./PrecisionDisclosure.vue";

type FrameClip = LayoutClip & Pick<Clip, "opacity"> & Partial<Pick<Clip, "fit" | "crop_zoom" | "crop_x" | "crop_y">>;

const props = defineProps<{
  clip: FrameClip;
  canvas: Size;
  cam: boolean;
  storeKey: string;
  send: (patch: LayoutPatch) => Promise<boolean>;
}>();

const SHAPES: { id: FrameShape; label: string }[] = [
  { id: "rounded", label: "Rounded" },
  { id: "circle", label: "Circle" },
  { id: "rectangle", label: "Rectangle" },
];

const shape = computed(() => shapeOf(props.clip));
const fit = computed<Fit>(() => props.clip.fit ?? "contain");
const whole = { min: () => 0, max: () => 1 };
const focusX = useInspectorDraft(
  percentField({ value: () => props.clip.crop_x ?? 0.5, label: "Focus X", ...whole }),
  (cropX) => props.send({ cropX }),
);
const focusY = useInspectorDraft(
  percentField({ value: () => props.clip.crop_y ?? 0.5, label: "Focus Y", ...whole }),
  (cropY) => props.send({ cropY }),
);

function onFit(event: Event): void {
  void props.send({ fit: (event.target as HTMLSelectElement).value as Fit });
}
function onMirror(event: Event): void {
  void props.send({ mirror: (event.target as HTMLInputElement).checked });
}
</script>

<template>
  <PrecisionDisclosure
    title="Frame & crop"
    :store-key="storeKey"
    testid="layout-frame-crop"
  >
    <div
      class="flex flex-wrap gap-[5px]"
      role="group"
      aria-label="Frame shape"
    >
      <InspectorButton
        v-for="s in SHAPES"
        :key="s.id"
        preset
        :pressed="s.id === shape"
        :data-testid="`layout-shape-${s.id}`"
        @click="send(shapePatch(s.id, clip, canvas))"
      >
        {{ s.label }}
      </InspectorButton>
    </div>
    <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
      Image fitting
      <select
        data-testid="layout-section-fit"
        class="text-[11px] text-fg"
        :value="fit"
        @change="onFit"
      >
        <option value="cover">Fill · crop to frame</option>
        <option value="contain">Fit · show entire image</option>
      </select>
    </label>
    <template v-if="fit === 'cover'">
      <InspectorRange
        label="Crop zoom"
        testid="layout-section-crop-zoom"
        :value="clip.crop_zoom ?? 1"
        :min="1"
        :max="3"
        :step="0.05"
        suffix="×"
        :commit="(cropZoom) => send({ cropZoom })"
      />
      <div class="grid grid-cols-2 gap-[9px]">
        <InspectorNumberInput
          :field="focusX"
          label="Focus X (%)"
          testid="layout-section-crop-x"
          :step="1"
        />
        <InspectorNumberInput
          :field="focusY"
          label="Focus Y (%)"
          testid="layout-section-crop-y"
          :step="1"
        />
      </div>
    </template>
    <label class="flex items-center gap-2 text-[11px] text-fg">
      <input
        data-testid="layout-section-mirror"
        type="checkbox"
        :checked="clip.mirror ?? false"
        @change="onMirror"
      >
      Mirror this video
    </label>
    <InspectorRange
      label="Opacity"
      testid="layout-section-opacity"
      :value="Math.round(clip.opacity * 100)"
      :min="0"
      :max="100"
      :step="1"
      suffix="%"
      :commit="(pct) => send({ opacity: pct / 100 })"
    />
    <p
      data-testid="layout-frame-help"
      class="text-[10px] leading-[1.6] text-fg-muted"
    >
      Mirroring and crop affect preview and renders, never the original.<template v-if="cam">
        The webcam stays on its own editable video track.
      </template>
    </p>
  </PrecisionDisclosure>
</template>
