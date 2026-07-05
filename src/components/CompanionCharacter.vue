<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import BuddyAvatar from "./BuddyAvatar.vue";
import { logWarning } from "../logging";

const props = withDefaults(
  defineProps<{
    working: boolean;
    animated?: boolean;
    character?: string;
    draggable?: boolean;
    facing?: "right" | "left";
    recording?: boolean;
    paused?: boolean;
  }>(),
  {
    animated: true,
    character: "classic",
    draggable: true,
    facing: "right",
    recording: false,
    paused: false,
  },
);
const emit = defineEmits<{
  (e: "toggle"): void;
  (e: "drag-start"): void;
  (e: "drag-cancelled"): void;
}>();

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
  // Dragging is disabled in the settings — the buddy stays pinned and the
  // whole press/release stays a plain click, however far the pointer moves.
  if (!props.draggable) return;
  if (!pressedAt) {
    // A hover move with no press means the native drag is over and any
    // trailing click has already been dispatched. Windows sometimes
    // consumes the release without a trailing click at all
    // (tauri-apps/tauri#10767) — without this reset the suppression
    // would eat the user's next deliberate click.
    dragged = false;
    return;
  }
  const moved = Math.hypot(e.screenX - pressedAt.x, e.screenY - pressedAt.y);
  if (moved < DRAG_THRESHOLD_PX) return;
  // Past the threshold the press is a drag gesture, not a click — consume it
  // (swallow the trailing click) whatever happens next.
  pressedAt = null;
  dragged = true;
  // A fast flick can queue a pointermove that is only dispatched after the
  // button was released. Starting the native drag from it would hand
  // Windows a buttonless WM_NCLBUTTONDOWN — a "sticky" move loop that glues
  // the buddy to the cursor and eats the next real press. The button is
  // already visibly up here, so drop the drag but keep the gesture consumed.
  if ((e.buttons & 1) === 0) return;
  // The OS move loop takes the mouse from here; let go of the capture.
  (e.currentTarget as HTMLElement | null)?.releasePointerCapture?.(
    e.pointerId,
  );
  // Arm App.vue's blur suppression BEFORE the move loop starts, so the
  // focus loss it causes is recognised. Emitted synchronously (the OS blur
  // arrives on a later turn) even though the command may still drop the
  // request — a drop is retracted below.
  emit("drag-start");
  // Rust-side chokepoint, not window.startDragging(): the command re-checks
  // the primary button on the main thread right before entering the OS move
  // loop, dropping requests that went stale in IPC transit. It reports
  // whether the drag actually started; the pointer type lets it skip the
  // mouse-only button re-check for touch/pen. `pointerType` can be absent on
  // synthetic events — default to mouse so the guard still applies.
  invoke<boolean>("start_buddy_drag", {
    pointerType: e.pointerType || "mouse",
  })
    .then((started) => {
      // Dropped in transit: no move loop began, so no blur will consume the
      // suppression we just armed — retract it, or a later desktop click is
      // wrongly swallowed and the panel stays open over the desktop.
      if (!started) emit("drag-cancelled");
    })
    .catch((e) => {
      // A genuine command failure (not just the no-Tauri unit-test path,
      // which logWarning no-ops) still means no drag started.
      logWarning(`start_buddy_drag failed: ${String(e)}`);
      emit("drag-cancelled");
    });
}

function onPointerEnd(e: PointerEvent) {
  (e.currentTarget as HTMLElement | null)?.releasePointerCapture?.(
    e.pointerId,
  );
  pressedAt = null;
}

function onClick(e: MouseEvent) {
  // detail 0 = keyboard activation (Enter/Space) — never a drag's
  // trailing click, so it must not be suppressed.
  if (dragged && e.detail !== 0) {
    // trailing click of a drag gesture — not an intent to open the panel
    dragged = false;
    return;
  }
  dragged = false;
  emit("toggle");
}

function onContextMenu() {
  // Native OS popup — the collapsed window is far too small to host an
  // HTML menu, and the OS menu matches the tray menu's look. The current
  // animation/dragging states drive the menu's checkmarks.
  void invoke("show_buddy_menu", {
    animated: props.animated,
    dragging: props.draggable,
  }).catch(() => {
    // not running under Tauri (unit tests)
  });
}
</script>

<template>
  <div class="flex flex-col items-center">
    <button
      type="button"
      class="buddy block focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      :class="[
        draggable ? 'cursor-grab' : 'cursor-pointer',
        { working, still: !animated, recording, paused },
      ]"
      :aria-label="
        draggable
          ? 'Vault Buddy — click to open the panel, drag to move'
          : 'Vault Buddy — click to open the panel'
      "
      :title="draggable ? 'Click to open · drag to move' : 'Click to open'"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerEnd"
      @pointercancel="onPointerEnd"
      @click="onClick"
      @contextmenu.prevent="onContextMenu"
    >
      <span class="relative inline-block">
        <BuddyAvatar
          :character-id="character"
          :working="working"
          :animated="animated"
          :facing="facing"
        />
        <span
          v-if="recording"
          class="rec-dot absolute -right-1 -top-1 h-3 w-3 rounded-full ring-2 ring-slate-900"
          :class="paused ? 'bg-amber-400' : 'bg-red-500'"
          aria-hidden="true"
        ></span>
      </span>
    </button>
  </div>
</template>

<style scoped>
.buddy.recording:not(.still) .rec-dot {
  animation: rec-blink 1.2s ease-in-out infinite;
}

.buddy.recording.paused .rec-dot {
  animation: none;
}

@keyframes rec-blink {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.35;
  }
}
</style>
