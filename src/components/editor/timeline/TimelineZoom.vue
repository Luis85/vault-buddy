<script setup lang="ts">
/**
 * The timeline toolbar's zoom group (visual-parity Task 16; concept spec
 * §6.2 item 11): zoom out, a 66px range, zoom in and **Fit**, pushed to the
 * right. Zoom is `editorWorkspace` view state, never an edit.
 *
 * The range is logarithmic (`log2` of the zoom) over the store's own
 * `ZOOM_RANGE`, so 1× sits near the middle and a drag feels the same at
 * both ends. The buttons step ×/÷1.25. At either end of the range the
 * button that could go no further is disabled with its reason (D14: it
 * would otherwise be an enabled control that does nothing), which a press
 * says in a toast. Fit needs the rendered width, which only `TimelineView`
 * knows, so it is emitted.
 */
import { computed } from "vue";

import { useEditorWorkspaceStore, ZOOM_RANGE } from "../../../stores/editorWorkspace";
import { useNotificationsStore } from "../../../stores/notifications";
import EditorIcon from "../icons/EditorIcon.vue";

const emit = defineEmits<(e: "fit") => void>();

const workspace = useEditorWorkspaceStore();
const notifications = useNotificationsStore();

const ZOOM_STEP = 1.25;
const [MIN_ZOOM, MAX_ZOOM] = ZOOM_RANGE;
const LOG_MIN = Math.log2(MIN_ZOOM);
const LOG_MAX = Math.log2(MAX_ZOOM);
const EPSILON = 1e-9;

const outReason = computed(() => (workspace.timelineZoom <= MIN_ZOOM + EPSILON ? "Zoomed all the way out" : null));
const inReason = computed(() => (workspace.timelineZoom >= MAX_ZOOM - EPSILON ? "Zoomed all the way in" : null));

function step(factor: number, reason: string | null): void {
  if (reason) {
    notifications.info(reason);
    return;
  }
  workspace.setZoom(workspace.timelineZoom * factor);
}

function onRange(event: Event): void {
  workspace.setZoom(2 ** Number((event.target as HTMLInputElement).value));
}

const ICON_BUTTON = "inline-flex h-8 w-8 shrink-0 items-center justify-center p-1.5 text-fg-muted";
const DISABLED = "cursor-not-allowed opacity-40";
</script>

<template>
  <div
    data-testid="timeline-zoom"
    class="ml-auto flex shrink-0 items-center gap-0.5"
  >
    <button
      type="button"
      data-testid="timeline-toolbar-zoom-out"
      aria-label="Zoom timeline out"
      :aria-disabled="outReason !== null"
      :title="outReason ?? 'Zoom timeline out'"
      :class="[ICON_BUTTON, outReason ? DISABLED : '']"
      @click="step(1 / ZOOM_STEP, outReason)"
    >
      <EditorIcon name="zoomOut" />
    </button>
    <input
      type="range"
      data-testid="timeline-toolbar-zoom-range"
      aria-label="Timeline zoom"
      :aria-valuetext="`${workspace.timelineZoom.toFixed(2)}×`"
      :min="LOG_MIN"
      :max="LOG_MAX"
      step="0.01"
      :value="Math.log2(workspace.timelineZoom)"
      class="w-[66px] max-[1000px]:w-12"
      @input="onRange"
    >
    <button
      type="button"
      data-testid="timeline-toolbar-zoom-in"
      aria-label="Zoom timeline in"
      :aria-disabled="inReason !== null"
      :title="inReason ?? 'Zoom timeline in'"
      :class="[ICON_BUTTON, inReason ? DISABLED : '']"
      @click="step(ZOOM_STEP, inReason)"
    >
      <EditorIcon name="zoomIn" />
    </button>
    <button
      type="button"
      data-testid="timeline-toolbar-fit"
      title="Fit the whole edit"
      class="min-h-[30px] shrink-0 px-2.5 text-[11px] text-fg-secondary"
      @click="emit('fit')"
    >
      Fit
    </button>
  </div>
</template>
