<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { RegionPick } from "../types";

/**
 * The region-selection surface (spec 5.2). This root runs in the `overlay`
 * window, which Rust has already sized and positioned over ONE monitor
 * while it was still hidden — so this viewport's origin IS that monitor's
 * origin, and every coordinate below is monitor-local by construction.
 * That is what makes `core::screen_geometry::to_physical`'s single scale
 * factor correct; see its module doc for why virtual-desktop coordinates
 * would not be.
 *
 * Everything here is reported in LOGICAL (CSS) pixels. The conversion to
 * physical happens in Rust with the monitor's own scale factor, not here:
 * a webview knows its `devicePixelRatio` but not which monitor Rust picked,
 * and two sources of truth for the scale is exactly the bug this feature is
 * most likely to have.
 */

/** Below this, on either axis, a drag is a stray click rather than a
 * selection. A 0x0 region fails `clamp_to_frame` and would surface as an
 * error toast for the crime of clicking. */
const MIN_DRAG_PX = 8;

const startX = ref(0);
const startY = ref(0);
const curX = ref(0);
const curY = ref(0);
const dragging = ref(false);
/** One-shot: the Rust side takes its sender out of the state on the first
 * answer, so a second call is a silent no-op that would hide a double-fire
 * rather than being harmless. */
const resolvedOnce = ref(false);

const box = computed(() => ({
  x: Math.min(startX.value, curX.value),
  y: Math.min(startY.value, curY.value),
  width: Math.abs(curX.value - startX.value),
  height: Math.abs(curY.value - startY.value),
}));

const style = computed(() => ({
  left: `${box.value.x}px`,
  top: `${box.value.y}px`,
  width: `${box.value.width}px`,
  height: `${box.value.height}px`,
}));

function report(rect: RegionPick | null) {
  if (resolvedOnce.value) return;
  resolvedOnce.value = true;
  dragging.value = false;
  invoke("resolve_region_selection", { rect }).catch((e) => {
    // Nothing to retry against: Rust's own bounded wait hides the overlay
    // on expiry, so a lost answer costs a cancelled selection, not a stuck
    // window. Never silent, though.
    logWarning(`resolve_region_selection failed: ${String(e)}`);
  });
}

function onDown(e: PointerEvent) {
  startX.value = e.clientX;
  startY.value = e.clientY;
  curX.value = e.clientX;
  curY.value = e.clientY;
  dragging.value = true;
}

function onMove(e: PointerEvent) {
  if (!dragging.value) return;
  curX.value = e.clientX;
  curY.value = e.clientY;
}

function onUp(e: PointerEvent) {
  if (!dragging.value) return;
  curX.value = e.clientX;
  curY.value = e.clientY;
  const { x, y, width, height } = box.value;
  if (width < MIN_DRAG_PX || height < MIN_DRAG_PX) {
    report(null);
    return;
  }
  report({ x, y, width, height, dpr: window.devicePixelRatio });
}

function onKeydown(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  report(null);
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <!-- The scrim is load-bearing, not decoration: a FULLY transparent
       region of a transparent window does not reliably receive pointer
       events, so the surface that reads the drag must be painted. -->
  <div
    data-testid="region-surface"
    class="fixed inset-0 cursor-crosshair select-none bg-black/35"
    @pointerdown="onDown"
    @pointermove="onMove"
    @pointerup="onUp"
  >
    <div
      v-if="dragging"
      data-testid="region-box"
      class="pointer-events-none absolute border-2 border-violet-400 bg-white/15"
      :style="style"
    >
      <span
        data-testid="region-readout"
        class="absolute left-0 top-full mt-1 rounded bg-black/70 px-1.5 py-0.5 text-xs font-medium text-fg"
      >
        {{ Math.round(box.width) }} x {{ Math.round(box.height) }}
      </span>
    </div>
    <p
      v-if="!dragging"
      data-testid="region-hint"
      class="absolute left-1/2 top-8 -translate-x-1/2 rounded-control bg-black/70 px-3 py-1.5 text-sm text-fg"
    >
      Drag to select a region — Esc to cancel
    </p>
  </div>
</template>
