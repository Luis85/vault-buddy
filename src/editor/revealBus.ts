/**
 * Reveal requests (Task 54): how a before-you-share finding asks a surface
 * that owns its own open state to show itself — the mixer's popover, the
 * media library's Reconnect and Webcam dialogs, the compact shell's
 * drawers, the ratio control's focus, the timeline's scroll, the Render
 * dialog, and (Task 57) the preview toolbar's Review, which Ctrl+E asks
 * for. `requestReveal(surface)` bumps a per-surface serial and
 * `onReveal(surface, fn)` — called from that surface's own setup — runs
 * `fn` once per request it has not handled yet, including one made just
 * before it mounted (the media library mounts only when its tab is chosen,
 * which the same reveal does a moment earlier). A module-level `reactive`,
 * not a Pinia store: nothing here is project or workspace state, the
 * `dialogs.ts` precedent. `checkReveal.ts` decides WHAT to reveal.
 *
 * `checksDialogOpen` is the Checks dialog's one open flag, so the header's
 * button, the Render dialog and the canvas-ratio toast (Task 32) open the
 * same dialog.
 */
import { reactive, ref, watch } from "vue";

export type RevealSurface =
  | "reconnect" | "webcam" | "mixer" | "ratio" | "timeline" | "library" | "inspector" | "render" | "review";

const SURFACES: readonly RevealSurface[] = [
  "reconnect", "webcam", "mixer", "ratio", "timeline", "library", "inspector", "render", "review",
];

const requested = reactive(Object.fromEntries(SURFACES.map((s) => [s, 0])) as Record<RevealSurface, number>);
const handled = Object.fromEntries(SURFACES.map((s) => [s, 0])) as Record<RevealSurface, number>;

/** Where the timeline should scroll to, output ms (`"timeline"`). */
let timelineTarget = 0;

/** The Checks dialog's open flag (header button, canvas toast). */
export const checksDialogOpen = ref(false);

export function openChecks(): void {
  checksDialogOpen.value = true;
}

export function requestReveal(surface: RevealSurface): void {
  requested[surface] += 1;
}

/** Runs `fn` for each request of `surface` not yet handled — at once for
 * one made before the caller mounted. Call from a component's setup. */
export function onReveal(surface: RevealSurface, fn: () => void): void {
  watch(
    () => requested[surface],
    (n) => {
      if (n <= handled[surface]) return;
      handled[surface] = n;
      fn();
    },
    { immediate: true },
  );
}

/** How many reveals of `surface` were ever requested — the tests'
 * observation point. */
export function revealSerial(surface: RevealSurface): number {
  return requested[surface];
}

/** Asks the timeline to scroll output instant `ms` into view. */
export function requestTimelineReveal(ms: number): void {
  timelineTarget = ms;
  requestReveal("timeline");
}

/** The output time the last timeline reveal asked for. */
export function revealedTimelineMs(): number {
  return timelineTarget;
}
