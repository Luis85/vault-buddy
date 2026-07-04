<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import BuddyAvatar from "./BuddyAvatar.vue";

const props = withDefaults(
  defineProps<{
    working: boolean;
    animated?: boolean;
    character?: string;
    draggable?: boolean;
  }>(),
  { animated: true, character: "classic", draggable: true },
);
const emit = defineEmits<{
  (e: "toggle"): void;
  (e: "drag-start"): void;
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
  pressedAt = null;
  dragged = true;
  // The OS move loop takes the mouse from here; let go of the capture.
  (e.currentTarget as HTMLElement | null)?.releasePointerCapture?.(
    e.pointerId,
  );
  emit("drag-start");
  void getCurrentWindow().startDragging();
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
  // animation state drives the menu's checkmark.
  void invoke("show_buddy_menu", { animated: props.animated }).catch(() => {
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
        { working, still: !animated },
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
      <BuddyAvatar
        :character-id="character"
        :working="working"
        :animated="animated"
      />
    </button>
  </div>
</template>
