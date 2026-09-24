/**
 * The tutorial editor's timeline layout math (Task 20; F-04, F-14, F-26).
 * Deliberately pure and Vue-free, the `timeMap.ts` precedent: a component
 * that has to stay smooth over 600 clips (`MAX_CLIPS`, the global
 * constraints' contract reference) cannot afford this logic hiding inside a
 * `computed()` where a mutation slips past the one place performance and
 * correctness both live. `TimelineView.vue`/`TrackLane.vue`/`ClipItem.vue`/
 * `TimelineRuler.vue` consume this module; none of them re-derive any of it.
 *
 * All px values here are CONTENT-space unless a function explicitly takes a
 * `scrollLeft` — `msToX`/`xToMs` default `scrollLeft` to 0 for exactly that
 * reason: a clip or ruler tick's own left position is placed in content
 * space (the browser's native horizontal scroll does the panning), while
 * converting a pointer's viewport-relative `clientX` into a time, or turning
 * a viewport's visible px range into an ms range for virtualization, needs
 * the real `scrollLeft` to undo.
 */
import type { Clip, ClipSpan, Marker, Project } from "../editorTypes";
import { clipOutputEnd, outputAt } from "./timeMap";

function clipSpanOf(clip: Clip): ClipSpan {
  return { start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed: clip.speed ?? 1 };
}

/** The track-label column's width — mirrors `--editor-label` (`style.css`,
 * 196px, the CONTRACT's own value). `TrackLane.vue`/`TimelineRuler.vue` both
 * lay out a matching label-column-then-content-track row, so this lives here
 * (their one shared layout constant) rather than being read from CSS at
 * runtime or hand-copied twice. */
export const TRACK_LABEL_WIDTH_PX = 196;

/** One track lane's rendered height (`TrackLane.vue`'s own `LANE_HEIGHT_PX`,
 * moved here in Task 21 so `useTimelineDrag.ts`'s cross-track drop hit-test
 * shares the exact same number `TrackLane` lays lanes out with, rather than
 * a second copy of the magic number). */
export const LANE_HEIGHT_PX = 56;

/** Pixels per millisecond at zoom 1 — 50px per second. Chosen so a typical
 * few-minute tutorial spans a scrollable-but-not-absurd content width; there
 * is no contract value for this, so it is this module's own constant and
 * every other function here is defined against it. */
export const BASE_PX_PER_MS = 0.05;

/** `zoom` is `editorWorkspace.timelineZoom` (clamped to `[0.1, 20]` there);
 * a negative zoom (never produced by that store, but this function must
 * still not invert the axis for a caller that hands it one) floors at 0. */
export function pxPerMs(zoom: number): number {
  return BASE_PX_PER_MS * Math.max(zoom, 0);
}

/** An output ms instant's CONTENT-space x, or viewport-space when a real
 * `scrollLeft` is passed (see the module doc). */
export function msToX(ms: number, zoom: number, scrollLeft = 0): number {
  return ms * pxPerMs(zoom) - scrollLeft;
}

/** The inverse of `msToX` for the same `(zoom, scrollLeft)` pair. `ppm <= 0`
 * (a degenerate zoom) reads as "everything is at time 0" rather than
 * dividing by zero into `Infinity`/`NaN`. */
export function xToMs(x: number, zoom: number, scrollLeft = 0): number {
  const ppm = pxPerMs(zoom);
  if (ppm <= 0) return 0;
  return (x + scrollLeft) / ppm;
}

/**
 * The zoom that fits the whole edit into `viewportPx` — `editorWorkspace.
 * fit()`'s own doc names this as the "later task that DOES know the
 * timeline's rendered width" call; `TimelineView.vue` is that task. Exact
 * fit (`pxPerMs(zoom) * durationMs === viewportPx`), not a value that then
 * gets clamped against `editorWorkspace`'s own `ZOOM_RANGE` here — clamping
 * INSIDE this function could zoom a very-long edit in PAST what fits (a
 * floor clamp fighting the very property "fit" promises); the caller
 * (`setZoom`) applies that store-level clamp on the way in, the same as it
 * already does for every other zoom value.
 *
 * A non-positive `durationMs`/`viewportPx` has nothing to fit or nowhere to
 * fit it — returns `1` (the store's own default zoom) rather than `Infinity`
 * or a divide-by-zero `NaN`.
 */
export function fitZoom(durationMs: number, viewportPx: number): number {
  if (durationMs <= 0 || viewportPx <= 0) return 1;
  return viewportPx / (BASE_PX_PER_MS * durationMs);
}

/** Clip edges (start and, via the shared time-mapping table, the speed-
 * aware output end), marker output positions and the playhead itself —
 * exactly the Behavior section's own list. A marker's `source_ms` is SOURCE
 * time (like every clip-linked cue, `editorTypes.ts`'s own doc); `outputAt`
 * maps it back onto the timeline, and a marker whose source instant fell
 * outside its clip's current `[in_ms, out_ms)` (a stale marker after a trim)
 * is silently dropped rather than snapping to a position nothing shows. */
export function snapTargets(project: Project | null, playheadMs: number): number[] {
  const targets = new Set<number>([playheadMs]);
  if (project) {
    for (const clip of project.clips) {
      targets.add(clip.start_ms);
      targets.add(clipOutputEnd(clipSpanOf(clip)));
    }
    const clipById = new Map(project.clips.map((c) => [c.id, c] as const));
    for (const marker of project.markers as Marker[]) {
      const clip = clipById.get(marker.clip_id);
      if (!clip) continue;
      const out = outputAt(clipSpanOf(clip), marker.source_ms);
      if (out !== null) targets.add(out);
    }
  }
  return [...targets].sort((a, b) => a - b);
}

/** The nearest of `targets` within `thresholdPx` (converted to ms at
 * `zoom`), or `ms` unchanged when nothing qualifies. `<=` at the threshold
 * boundary (a target exactly `thresholdPx` away still snaps) and strict `<`
 * between candidates, so a tie keeps whichever target the targets array
 * lists first — deterministic, never "whichever iterated last". A
 * non-positive zoom (`pxPerMs` 0) snaps to nothing, the `xToMs` guard: the
 * pixel threshold would otherwise convert to an `Infinity`-ms one and catch
 * every target however far away (Task 20's carried finding, fixed with
 * snap's first consumer, Task 21's drag/trim). */
export function snap(ms: number, targets: readonly number[], thresholdPx: number, zoom: number): number {
  const ppm = pxPerMs(zoom);
  if (ppm <= 0) return ms;
  const thresholdMs = thresholdPx / ppm;
  let best: number | null = null;
  let bestDist = Infinity;
  for (const t of targets) {
    const d = Math.abs(t - ms);
    if (d <= thresholdMs && d < bestDist) {
      bestDist = d;
      best = t;
    }
  }
  return best ?? ms;
}

/**
 * Virtualization: only clips whose OUTPUT span intersects the viewport
 * padded by one full screen on each side (`width` again, not a fraction of
 * it) survive. The padding is load-bearing, not a cosmetic buffer — this
 * task's own mutation check: drop it (test the raw `[scrollLeft, scrollLeft
 * + width]` window alone) and a clip that is only HALF visible at the
 * viewport's edge disappears the instant its midpoint crosses the boundary,
 * which reads as clips popping in/out mid-scroll rather than a clean
 * mount/unmount at a screen's distance away. `clipOutputEnd` (not `out_ms`)
 * is the clip's real end on THIS axis — the axis every position on this
 * timeline is measured in — so a sped-up clip's shorter output span is what
 * gets tested against the viewport, not its longer source range.
 */
export function visibleClips(
  clips: readonly Clip[],
  scrollLeft: number,
  width: number,
  zoom: number,
): Clip[] {
  const ppm = pxPerMs(zoom);
  if (ppm <= 0) return [...clips];
  const pad = Math.max(width, 0);
  const loMs = (scrollLeft - pad) / ppm;
  const hiMs = (scrollLeft + width + pad) / ppm;
  return clips.filter((c) => clipOutputEnd(clipSpanOf(c)) > loMs && c.start_ms < hiMs);
}

// ---- ruler tick spacing -----------------------------------------------------

/** "Nice" tick intervals a ruler can fall back through as zoom shrinks —
 * whole seconds/minutes/quarter-hours a human reads at a glance, never an
 * arbitrary computed number like "847ms". */
const NICE_TICK_INTERVALS_MS: readonly number[] = [
  100, 200, 500, 1000, 2000, 5000, 10_000, 15_000, 30_000, 60_000, 120_000, 300_000, 600_000, 900_000,
  1_800_000, 3_600_000,
];

/** The minimum pixel gap between two adjacent ticks — below this, tick
 * labels overlap and the ruler reads as a smear rather than a scale. */
const MIN_TICK_PX = 60;

/** The smallest "nice" interval whose pixel width at `zoom` still clears
 * `MIN_TICK_PX` — the largest available interval when even the biggest nice
 * step (1 hour) can't reach it at an extremely zoomed-out view. */
export function tickIntervalMs(zoom: number): number {
  const ppm = pxPerMs(zoom);
  if (ppm <= 0) return NICE_TICK_INTERVALS_MS[NICE_TICK_INTERVALS_MS.length - 1];
  for (const interval of NICE_TICK_INTERVALS_MS) {
    if (interval * ppm >= MIN_TICK_PX) return interval;
  }
  return NICE_TICK_INTERVALS_MS[NICE_TICK_INTERVALS_MS.length - 1];
}

/**
 * Task 54: the scroll position that brings output instant `ms` into view,
 * or `null` when it already is. Scroll coordinates include the label
 * column (the lanes scroll it along, see `TimelineView`), so the instant
 * sits at `TRACK_LABEL_WIDTH_PX + msToX(ms)`; out of view, it lands a
 * third of the way in rather than flush against an edge.
 */
export function revealScrollLeft(ms: number, zoom: number, scrollLeft: number, viewportPx: number): number | null {
  const x = TRACK_LABEL_WIDTH_PX + msToX(ms, zoom);
  if (x >= scrollLeft + TRACK_LABEL_WIDTH_PX && x <= scrollLeft + viewportPx) return null;
  return Math.max(0, Math.round(x - TRACK_LABEL_WIDTH_PX - (viewportPx - TRACK_LABEL_WIDTH_PX) / 3));
}
