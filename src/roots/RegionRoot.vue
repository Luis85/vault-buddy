<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
 *
 * This root deliberately wires NO Pinia store, and that is not an omission:
 * AGENTS.md's per-window `init()` rule exists because a store that mirrors
 * Rust state is dead in any webview that never subscribed. This root mirrors
 * no Rust state — it PRODUCES one answer per selection, via
 * `resolve_region_selection` — so there is nothing to wire per window. Its
 * one subscription, `region:begin`, carries no state: it is the edge that
 * re-arms this component for a new selection, because the overlay window is
 * hidden and reused rather than reloaded. See `beginSelection`.
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
/** One-shot PER SELECTION: the Rust side takes its sender out of the state on
 * the first answer, so a second call is a silent no-op that would hide a
 * double-fire rather than being harmless. Re-armed by `region:begin` — see
 * `beginSelection` — because this window is hidden and reused, never
 * reloaded, so the latch would otherwise outlive the selection it belongs to
 * and make every later one inert. */
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
  // Primary button only. A right- or middle-press is a menu gesture, not a
  // selection; AGENTS.md documents that the buddy drag re-checks the logical
  // primary button for the same reason.
  if (e.button !== 0) return;
  // After the one answer has gone, a fresh press would paint a live rubber
  // band that can never report — a band that lies for as long as Rust takes
  // to hide the overlay.
  if (resolvedOnce.value) return;
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

/** The pointer was taken away from us (an OS gesture, a lost capture). Without
 * this the band stays painted and `dragging` stays true with nothing ever
 * resolving, so the overlay sits there until Rust's own bounded wait expires.
 * Gated on `dragging` so a stray cancel outside a drag cannot answer for the
 * user. */
function onCancel() {
  if (!dragging.value) return;
  report(null);
}

function onKeydown(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  report(null);
}

/**
 * A NEW selection is starting. Rust emits `region:begin` from the same
 * main-thread closure that shows this window, immediately BEFORE the show.
 *
 * The overlay window is hidden and reused rather than reloaded, so this
 * component — and `resolvedOnce` with it — survives from one selection to
 * the next. Without this reset the second selection of an app run paints a
 * scrim that drops every pointerdown and every Escape, for the full length
 * of Rust's bounded wait, and then reports a cancel the user never made.
 *
 * It cannot lose a race with the user's first press: a hidden window
 * receives no pointer input, so no pointerdown for this selection can exist
 * before the show, and the event that runs this was already queued on the
 * same FIFO task queue before the show made input possible.
 *
 * `dragging` is reset too. A selection that ended via Escape mid-drag leaves
 * a band painted on a window that is about to be hidden; re-arming without
 * clearing it would show the previous selection's rectangle for the first
 * frame of the next one.
 */
function beginSelection() {
  resolvedOnce.value = false;
  dragging.value = false;
}

let unlistenBegin: (() => void) | undefined;

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  unlistenBegin = await listen("region:begin", beginSelection);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  unlistenBegin?.();
});
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
    @pointercancel="onCancel"
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
        <!-- ASCII `x`, not the multiplication sign spec 5.2 spells the
             readout with: `list_capture_sources` already renders a monitor's
             size as `{width}x{height}` (screen/src/source.rs) and Task 7's
             `regionLabel.ts` matches that. One spelling across the feature
             beats matching the spec's prose in this one place. -->
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
