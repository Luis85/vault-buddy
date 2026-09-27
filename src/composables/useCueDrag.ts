/**
 * Moving and trimming a teaching cue on the timeline (visual-parity Task
 * 19; concept spec §6.4, design D11): the clip drag's discipline
 * (`useTimelineDrag`) applied to a cue. `CueChip.vue` owns the pointer and
 * the focus; this owns the numbers — a preview while the pointer moves, ONE
 * `updateEffect` when it is released, nothing at all after Escape or a drag
 * that ends where it began.
 *
 * **A cue lives in its clip's SOURCE time** (`cues.rs`), so an OUTPUT delta
 * is turned into a source delta at the clip's speed, and the result is kept
 * inside the clip's own source range — `check_cue_range`, which refuses
 * anything else — so the preview never shows a place Rust would refuse. A
 * move keeps the cue's length and stops at either end of the clip; an edge
 * grip moves one end and leaves at least 100 ms of output (the clip's own
 * `MIN_CLIP_MS`, reused as a cue's floor so a trim cannot make a cue too
 * short to grab again). The dragged edge snaps to the targets a clip drag
 * snaps to, and names the one it caught for the dashed guide.
 */
import type { Ref } from "vue";
import { ref } from "vue";

import type { EditorCommand } from "../editor/editorCommandTypes";
import { pxPerMs } from "../editor/timelineLayout";
import { cueOutputSpan, roundHalfAway } from "../editor/timeMap";
import type { Clip, Effect } from "../editorTypes";
import type { SnapOptions } from "./useTimelineDrag";
import { MIN_CLIP_MS, SNAP_THRESHOLD_PX, snapGuideFor, snappedMs } from "./useTimelineDrag";

/** A cue's SOURCE range, what `updateEffect` carries. */
export interface CueRange {
  startMs: number;
  endMs: number;
}

/** A drag step: the range to preview and the snap target it sits on. */
export interface CueStep {
  range: CueRange;
  guide: number | null;
}

export type CueGrip = "move" | "start" | "end";

function speedOf(clip: Clip): number {
  return clip.speed ?? 1;
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(Math.max(v, lo), hi);
}

/** The source instant output `t` plays on `clip` (unclamped). */
function sourceOf(clip: Clip, t: number): number {
  return clip.in_ms + roundHalfAway((t - clip.start_ms) * speedOf(clip));
}

/** The output instant source `s` plays at on `clip` (unclamped). */
function outputOf(clip: Clip, s: number): number {
  return clip.start_ms + roundHalfAway((s - clip.in_ms) / speedOf(clip));
}

function spanOf(clip: Clip): { start_ms: number; in_ms: number; out_ms: number; speed: number } {
  return { start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed: speedOf(clip) };
}

/** Where the cue paints now — its clip's start when it plays nowhere. */
function outputSpan(clip: Clip, effect: Effect): [number, number] {
  return cueOutputSpan(spanOf(clip), effect.start_ms, effect.end_ms) ?? [clip.start_ms, clip.start_ms];
}

/** `effect` shifted by `ds` source ms, kept inside the clip whole. */
function shiftWithin(clip: Clip, effect: Effect, ds: number): CueRange {
  const lo = clip.in_ms - effect.start_ms;
  const hi = clip.out_ms - effect.end_ms;
  if (lo > hi) return { startMs: Math.max(effect.start_ms, clip.in_ms), endMs: Math.min(effect.end_ms, clip.out_ms) };
  const d = clamp(ds, lo, hi);
  return { startMs: effect.start_ms + d, endMs: effect.end_ms + d };
}

/** A body drag by `rawOutputDeltaMs`: the cue's start snaps, its length stays. */
export function cueMoveRange(clip: Clip, effect: Effect, rawOutputDeltaMs: number, opts: SnapOptions): CueStep {
  const start = outputSpan(clip, effect)[0];
  const rawStart = start + rawOutputDeltaMs;
  const ds = roundHalfAway((snappedMs(rawStart, opts) - start) * speedOf(clip));
  const range = shiftWithin(clip, effect, ds);
  return { range, guide: snapGuideFor(rawStart, outputOf(clip, range.startMs), opts) };
}

/** The source length a cue keeps at the least. */
function minSourceMs(clip: Clip): number {
  return Math.max(1, roundHalfAway(MIN_CLIP_MS * speedOf(clip)));
}

/** An edge grip dragged by `rawOutputDeltaMs`: that end only. */
export function cueTrimRange(
  clip: Clip,
  effect: Effect,
  edge: "start" | "end",
  rawOutputDeltaMs: number,
  opts: SnapOptions,
): CueStep {
  const [os, oe] = outputSpan(clip, effect);
  const rawEdge = (edge === "start" ? os : oe) + rawOutputDeltaMs;
  const src = sourceOf(clip, snappedMs(rawEdge, opts));
  const min = minSourceMs(clip);
  const range =
    edge === "start"
      ? { startMs: clamp(src, clip.in_ms, Math.max(clip.in_ms, effect.end_ms - min, Math.min(effect.start_ms, effect.end_ms - 1))), endMs: effect.end_ms }
      : { startMs: effect.start_ms, endMs: clamp(src, Math.min(clip.out_ms, effect.start_ms + min, Math.max(effect.end_ms, effect.start_ms + 1)), clip.out_ms) };
  const moved = edge === "start" ? range.startMs : range.endMs;
  return { range, guide: snapGuideFor(rawEdge, outputOf(clip, moved), opts) };
}

/** An arrow key's step of `outputDeltaMs`, kept inside the clip. */
export function cueNudgeRange(clip: Clip, effect: Effect, outputDeltaMs: number): CueRange {
  return shiftWithin(clip, effect, roundHalfAway(outputDeltaMs * speedOf(clip)));
}

// ---- the composable -----------------------------------------------------------

export interface CueDragDeps {
  /** The cue and its clip now (every commit replaces the project). */
  effect: () => Effect;
  clip: () => Clip;
  zoom: () => number;
  snapEnabled: () => boolean;
  snapTargets: () => readonly number[];
  execute: (command: EditorCommand) => Promise<boolean>;
}

export interface UseCueDrag {
  /** The source range to draw while a drag is live. */
  preview: Ref<CueRange | null>;
  snapGuideMs: Ref<number | null>;
  begin: (grip: CueGrip, clientX: number) => void;
  update: (clientX: number) => void;
  end: () => Promise<void>;
  /** Drops a live preview; whether there was one. */
  cancel: () => boolean;
  /** One `updateEffect` for an arrow key; whether the cue could move. */
  nudge: (outputDeltaMs: number) => Promise<boolean>;
}

function sameRange(effect: Effect, range: CueRange): boolean {
  return range.startMs === effect.start_ms && range.endMs === effect.end_ms;
}

function updateCommand(effect: Effect, range: CueRange): EditorCommand {
  return { kind: "updateEffect", effectId: effect.id, startMs: range.startMs, endMs: range.endMs };
}

export function useCueDrag(deps: CueDragDeps): UseCueDrag {
  const preview = ref<CueRange | null>(null);
  const snapGuideMs = ref<number | null>(null);
  let grip: CueGrip | null = null;
  let anchorX = 0;

  function snapOpts(): SnapOptions {
    return { snapEnabled: deps.snapEnabled(), targets: deps.snapTargets(), thresholdPx: SNAP_THRESHOLD_PX, zoom: deps.zoom() };
  }

  function begin(next: CueGrip, clientX: number): void {
    grip = next;
    anchorX = clientX;
    const e = deps.effect();
    preview.value = { startMs: e.start_ms, endMs: e.end_ms };
  }

  function update(clientX: number): void {
    if (!grip) return;
    const ppm = pxPerMs(deps.zoom());
    const raw = ppm > 0 ? (clientX - anchorX) / ppm : 0;
    const step =
      grip === "move"
        ? cueMoveRange(deps.clip(), deps.effect(), raw, snapOpts())
        : cueTrimRange(deps.clip(), deps.effect(), grip, raw, snapOpts());
    preview.value = step.range;
    snapGuideMs.value = step.guide;
  }

  function cancel(): boolean {
    const active = grip !== null;
    grip = null;
    preview.value = null;
    snapGuideMs.value = null;
    return active;
  }

  async function end(): Promise<void> {
    const range = preview.value;
    cancel();
    const effect = deps.effect();
    if (range && !sameRange(effect, range)) await deps.execute(updateCommand(effect, range));
  }

  async function nudge(outputDeltaMs: number): Promise<boolean> {
    const effect = deps.effect();
    const range = cueNudgeRange(deps.clip(), effect, outputDeltaMs);
    if (sameRange(effect, range)) return false;
    await deps.execute(updateCommand(effect, range));
    return true;
  }

  return { preview, snapGuideMs, begin, update, end, cancel, nudge };
}
