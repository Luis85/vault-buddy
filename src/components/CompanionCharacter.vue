<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";

defineProps<{ working: boolean }>();
const emit = defineEmits<{ (e: "toggle"): void }>();

// The buddy is both the click target (toggle panel) and the drag handle
// (move window). A tauri drag region would swallow clicks, so distinguish
// the gestures ourselves: a press that travels past the threshold becomes a
// native window drag, a press that doesn't is a click.
const DRAG_THRESHOLD_PX = 5;
let pressedAt: { x: number; y: number } | null = null;
let dragged = false;

function onPointerDown(e: PointerEvent) {
  if (e.button !== 0) return;
  // Capture the pointer: the buddy is only 64px, so a fast flick can leave
  // the button before the first pointermove fires. Without capture those
  // moves go to whatever is under the cursor, the drag never starts, and
  // the release can even register as a click.
  (e.currentTarget as HTMLElement | null)?.setPointerCapture?.(e.pointerId);
  pressedAt = { x: e.screenX, y: e.screenY };
  dragged = false;
}

function onPointerMove(e: PointerEvent) {
  if (!pressedAt) return;
  const moved = Math.hypot(e.screenX - pressedAt.x, e.screenY - pressedAt.y);
  if (moved < DRAG_THRESHOLD_PX) return;
  pressedAt = null;
  dragged = true;
  // The OS move loop takes the mouse from here; let go of the capture.
  (e.currentTarget as HTMLElement | null)?.releasePointerCapture?.(
    e.pointerId,
  );
  void getCurrentWindow().startDragging();
}

function onPointerEnd(e: PointerEvent) {
  (e.currentTarget as HTMLElement | null)?.releasePointerCapture?.(
    e.pointerId,
  );
  pressedAt = null;
}

function onClick() {
  if (dragged) {
    // trailing click of a drag gesture — not an intent to open the panel
    dragged = false;
    return;
  }
  emit("toggle");
}
</script>

<template>
  <div class="flex flex-col items-center">
    <button
      type="button"
      class="buddy block cursor-grab focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      :class="{ working }"
      aria-label="Vault Buddy — click to open the panel, drag to move"
      title="Click to open · drag to move"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerEnd"
      @pointercancel="onPointerEnd"
      @click="onClick"
    >
      <svg width="64" height="64" viewBox="0 0 96 96" aria-hidden="true">
        <ellipse cx="48" cy="52" rx="34" ry="32" fill="#7c5cff" />
        <circle class="eye" cx="38" cy="46" r="5" fill="#fff" />
        <circle class="eye" cx="58" cy="46" r="5" fill="#fff" />
        <path
          d="M40 62 Q48 70 56 62"
          stroke="#fff"
          stroke-width="3"
          fill="none"
          stroke-linecap="round"
        />
      </svg>
    </button>
  </div>
</template>

<style scoped>
/* idle */
.buddy {
  animation: bob 3s ease-in-out infinite;
}
/* greeting */
.buddy:hover:not(.working) {
  animation: wiggle 0.6s ease-in-out infinite;
}
/* working */
.buddy.working {
  animation: pulse 0.9s ease-in-out infinite;
}
.buddy .eye {
  animation: blink 4s infinite;
  transform-origin: center;
  transform-box: fill-box;
}
@keyframes bob {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-4px);
  }
}
@keyframes wiggle {
  0%,
  100% {
    transform: rotate(-4deg);
  }
  50% {
    transform: rotate(4deg);
  }
}
@keyframes pulse {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(0.94);
  }
}
@keyframes blink {
  0%,
  92%,
  100% {
    transform: scaleY(1);
  }
  96% {
    transform: scaleY(0.1);
  }
}
</style>
