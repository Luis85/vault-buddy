<script setup lang="ts">
/**
 * The Layout tab's "Transform source" disclosure (visual-parity Task 14;
 * concept spec §5, `contextual-ui.js`): rotate, flip, centre and fit the
 * source inside its frame — each one `setLayout` computed from the first
 * selected clip (`layoutPresets.transformPatch`), through the section's
 * `send`.
 */
import type { LayoutClip, LayoutPatch, TransformKey } from "../../../editor/layoutPresets";
import { transformPatch } from "../../../editor/layoutPresets";
import type { EditorIconName } from "../icons/conceptIcons";
import InspectorButton from "./InspectorButton.vue";
import PrecisionDisclosure from "./PrecisionDisclosure.vue";

defineProps<{ clip: LayoutClip; storeKey: string; send: (patch: LayoutPatch) => Promise<boolean> }>();

const BUTTONS: { key: TransformKey; label: string; icon: EditorIconName }[] = [
  { key: "rotate", label: "Rotate 90°", icon: "rotate" },
  { key: "flip", label: "Flip horizontal", icon: "arrowRight" },
  { key: "flip-y", label: "Flip vertical", icon: "down" },
  { key: "center", label: "Center", icon: "locate" },
  { key: "fit", label: "Fit source", icon: "zoomOut" },
  { key: "fill", label: "Fill frame", icon: "zoomIn" },
];
</script>

<template>
  <PrecisionDisclosure
    title="Transform source"
    :store-key="storeKey"
    testid="layout-transform"
  >
    <div class="grid grid-cols-2 gap-1.5">
      <InspectorButton
        v-for="b in BUTTONS"
        :key="b.key"
        :icon="b.icon"
        :data-testid="`layout-transform-${b.key}`"
        class="min-w-0"
        @click="send(transformPatch(b.key, clip))"
      >
        <span class="truncate">{{ b.label }}</span>
      </InspectorButton>
    </div>
    <p class="text-[10px] leading-[1.6] text-fg-muted">
      Rotate or flip the source inside its frame. Review callout positions after changing its orientation.
    </p>
  </PrecisionDisclosure>
</template>
