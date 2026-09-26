<script setup lang="ts">
/**
 * The splitter between the workspace and the timeline (visual-parity Task
 * 4; concept spec §6.1): the frame's own 8px grid row, panel-coloured with
 * a `line` border above and below and a 42×2 muted grip. It replaces the
 * handle `TimelineView` used to draw inside itself (Task 20, fix round 1's
 * finding 4 put that one at the timeline's top edge for the same reason
 * this row sits there: a drag up grows the timeline).
 *
 * A focusable `role="separator"`: ArrowUp/ArrowDown step ±25px from the
 * height ON SCREEN (the stored one, clamped to this window), and a drag
 * moves the height with the pointer. `editorWorkspace.setTimelineHeight`
 * clamps every value to 170 … min(540, innerHeight − 370) and persists it.
 */
import { computed, onBeforeUnmount } from "vue";

import { TIMELINE_KEY_STEP, timelineHeightRange } from "../../../editor/panelLayout";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const workspace = useEditorWorkspaceStore();
const range = computed(() => timelineHeightRange(workspace.viewportHeight));

function onKeydown(event: KeyboardEvent) {
  if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
  event.preventDefault();
  const step = event.key === "ArrowUp" ? TIMELINE_KEY_STEP : -TIMELINE_KEY_STEP;
  workspace.setTimelineHeight(workspace.timelineHeightPx + step);
}

let startY = 0;
let startHeight = 0;
function onPointerMove(event: PointerEvent) {
  workspace.setTimelineHeight(startHeight + (startY - event.clientY));
}
function stopDrag() {
  window.removeEventListener("pointermove", onPointerMove);
  window.removeEventListener("pointerup", stopDrag);
}
function onPointerDown(event: PointerEvent) {
  startY = event.clientY;
  startHeight = workspace.timelineHeightPx;
  window.addEventListener("pointermove", onPointerMove);
  window.addEventListener("pointerup", stopDrag);
}
onBeforeUnmount(stopDrag);
</script>

<template>
  <div
    data-testid="editor-splitter"
    role="separator"
    tabindex="0"
    aria-label="Resize the timeline"
    aria-orientation="horizontal"
    :aria-valuenow="workspace.timelineHeightPx"
    :aria-valuemin="range[0]"
    :aria-valuemax="range[1]"
    class="relative cursor-ns-resize touch-none border-y border-line bg-panel"
    @keydown="onKeydown"
    @pointerdown="onPointerDown"
  >
    <span
      aria-hidden="true"
      class="absolute top-0.5 left-[calc(50%-21px)] h-0.5 w-[42px] rounded-sm bg-fg-muted opacity-50"
    />
  </div>
</template>
