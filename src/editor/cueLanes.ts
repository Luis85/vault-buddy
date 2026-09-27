/**
 * The timeline's Teaching layers row, its pure half (visual-parity Task 19;
 * concept spec §6.4, design D11). Every project effect that still plays
 * inside its clip is a cue on the OUTPUT timeline — `timeMap.cueOutputSpan`
 * at the clip's speed, the preview's and the render's own mapping — packed
 * into rows 22 px apart the way the concept packs them (`editor.js`: the
 * first row whose last cue has ended), so overlapping cues stack and the
 * row grows to hold them. A cue whose source range no longer overlaps its
 * clip (the clip was trimmed past it) plays nowhere, so it has no place on
 * the timeline either — the preview draws it nowhere too.
 */
import type { Clip, Effect, Project } from "../editorTypes";
import { clipSpanOf } from "./actionTargets";
import { EFFECT_NAMES } from "./effectFields";
import { cueOutputSpan } from "./timeMap";

/** One cue's OUTPUT span, the only thing packing reads. */
export interface CueSpan {
  id: string;
  startMs: number;
  endMs: number;
}

/** Rows are this far apart; a cue (19 px tall, `CueChip`) sits 3 px down. */
const CUE_LANE_PX = 22;
const CUE_TOP_PX = 3;
/** The row is never shorter than this, cues or not. */
const ROW_MIN_PX = 44;
/** What the row adds under its last lane. */
const ROW_TAIL_PX = 5;

function byStartThenId(a: CueSpan, b: CueSpan): number {
  if (a.startMs !== b.startMs) return a.startMs - b.startMs;
  return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
}

/** Greedy by start, then id: each cue takes the first lane whose last end
 * is at or before its start, else a new lane below. */
export function packCues(cues: readonly CueSpan[]): { id: string; lane: number }[] {
  const ends: number[] = [];
  return [...cues].sort(byStartThenId).map((cue) => {
    let lane = ends.findIndex((end) => end <= cue.startMs);
    if (lane < 0) lane = ends.length;
    ends[lane] = cue.endMs;
    return { id: cue.id, lane };
  });
}

export function laneCount(packed: readonly { lane: number }[]): number {
  return packed.reduce((n, p) => Math.max(n, p.lane + 1), 0);
}

/** `max(44, lanes × 22 + 5)` (§6.4; 49 for the concept's two rows). */
export function teachingRowHeight(lanes: number): number {
  return Math.max(ROW_MIN_PX, lanes * CUE_LANE_PX + ROW_TAIL_PX);
}

export function cueTop(lane: number): number {
  return lane * CUE_LANE_PX + CUE_TOP_PX;
}

/** What a cue reads: a zoom its magnification ("1.65× Focus"), anything
 * else its text, or its kind when it has none. */
export function cueLabel(effect: Effect): string {
  if (effect.kind === "zoom" && effect.factor !== undefined) return `${effect.factor}× Focus`;
  return effect.text || EFFECT_NAMES[effect.kind];
}

/** A cue as the row draws it. */
export interface TeachingCue {
  effect: Effect;
  clip: Clip;
  startMs: number;
  endMs: number;
  lane: number;
  label: string;
}

/** Every cue that plays somewhere, packed, in lane order (start, then id). */
export function teachingCues(project: Project | null): TeachingCue[] {
  if (!project) return [];
  const clips = new Map(project.clips.map((c) => [c.id, c] as const));
  const placed = new Map<string, Omit<TeachingCue, "lane">>();
  for (const effect of project.effects) {
    const clip = clips.get(effect.clip_id);
    const span = clip ? cueOutputSpan(clipSpanOf(clip), effect.start_ms, effect.end_ms) : null;
    if (!clip || !span) continue;
    placed.set(effect.id, { effect, clip, startMs: span[0], endMs: span[1], label: cueLabel(effect) });
  }
  const spans = [...placed.values()].map((c) => ({ id: c.effect.id, startMs: c.startMs, endMs: c.endMs }));
  return packCues(spans).map(({ id, lane }) => ({ ...(placed.get(id) as Omit<TeachingCue, "lane">), lane }));
}
