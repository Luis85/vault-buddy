/**
 * "When duration changes" for the Speed tab (visual-parity Task 15; concept
 * spec §5 "Speed"). Rust's `setSpeed` never moves another clip: a slowdown
 * that would run into the next clip is refused ("nothing is moved
 * automatically"). So "Move following clips on this track" is the Speed tab
 * asking for that room itself — a `moveClips` of every later clip on the
 * clip's own track by the change in length — ordered so neither step
 * overlaps anything: a slowdown makes room first, a speed-up closes the gap
 * after. "Keep following clips in place" is `setSpeed` alone.
 *
 * **Groups** (fix round 1, ruling T15-2). Rust's `move_clips` moves every
 * clip grouped with a moved one, on any track, and pulls a move that would
 * pass the timeline's start back to it (`commands/clips.rs`). So a ripple is
 * sent only when the client can see neither would bite (`rippleProblem`, a
 * mirror of that expansion and clamp, plus the overlaps `move_clips`
 * refuses): the sped clip is not itself among the moved, no moved clip
 * would pass the start, and no moved clip would land on a clip that stays.
 * Otherwise the choice falls back to keeping the following clips in place,
 * and the Speed tab says why — never a sequence Rust refuses for a reason
 * the client could see.
 *
 * The choice is a window-local preference, never project state, like the
 * concept's own `ui.speedBehavior`.
 */
import { ref } from "vue";

import type { Clip, Project } from "../editorTypes";
import type { EditorCommand } from "./editorCommandTypes";
import { clipOutputEnd } from "./timeMap";

export type SpeedRipple = "ripple" | "leave";

/** The Speed tab's "When duration changes" choice; moving is the default. */
export const speedRipple = ref<SpeedRipple>("ripple");

export const GROUPED_WITH_CLIP =
  "This clip is grouped with a clip after it. Ungroup them to move following clips.";
export const REACHES_START =
  "A clip grouped with a following clip would move past the start. Ungroup it to move following clips.";
export const COLLIDES = "Grouped clips on other tracks would collide. Ungroup them to move following clips.";

function endAt(clip: Clip, speed: number): number {
  return clipOutputEnd({ start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed });
}

/** The clips after `clip` on its track, and how far a speed moves them. */
function followersOf(project: Project, clip: Clip, speed: number): { ids: string[]; deltaMs: number } {
  const ids = project.clips.filter((c) => c.track_id === clip.track_id && c.start_ms > clip.start_ms).map((c) => c.id);
  return { ids, deltaMs: endAt(clip, speed) - endAt(clip, clip.speed ?? 1) };
}

/** `ids` grown to whole groups, as Rust's `move_clips` grows them. */
export function withGroups(project: Project, ids: string[]): Set<string> {
  const groups = new Set(project.clips.filter((c) => ids.includes(c.id) && c.group_id).map((c) => c.group_id));
  return new Set(project.clips.filter((c) => ids.includes(c.id) || groups.has(c.group_id)).map((c) => c.id));
}

function joined(project: Project, a: string, b: string): boolean {
  return project.transitions.some((t) => (t.from === a && t.to === b) || (t.from === b && t.to === a));
}

/** Whether a moved clip would land on a clip that stays (the sped clip at
 * its new length). Clips joined by a transition are Rust's to judge. */
function collides(project: Project, sped: Clip, speed: number, moved: Set<string>, deltaMs: number): boolean {
  const span = (c: Clip): [number, number] =>
    c.id === sped.id ? [c.start_ms, endAt(c, speed)] : [c.start_ms, endAt(c, c.speed ?? 1)];
  const stays = project.clips.filter((c) => !moved.has(c.id));
  return project.clips
    .filter((m) => moved.has(m.id))
    .some((m) => {
      const [s, e] = span(m).map((t) => t + deltaMs);
      return stays.some((o) => {
        if (o.track_id !== m.track_id || joined(project, o.id, m.id)) return false;
        const [os, oe] = span(o);
        return s < oe && os < e;
      });
    });
}

/** Why moving the following clips cannot go with `clip` at `speed`, or
 * `null` when it can (or there is nothing to move). */
export function rippleProblem(project: Project, clip: Clip, speed: number): string | null {
  const { ids, deltaMs } = followersOf(project, clip, speed);
  if (deltaMs === 0 || ids.length === 0) return null;
  const moved = withGroups(project, ids);
  if (moved.has(clip.id)) return GROUPED_WITH_CLIP;
  const earliest = Math.min(...project.clips.filter((c) => moved.has(c.id)).map((c) => c.start_ms));
  if (earliest + deltaMs < 0) return REACHES_START;
  return collides(project, clip, speed, moved, deltaMs) ? COLLIDES : null;
}

/** The first reason any of `speeds` could not move the following clips —
 * why the Speed tab offers only keeping them in place for this clip. */
export function rippleRefusal(project: Project, clip: Clip, speeds: readonly number[]): string | null {
  for (const speed of speeds) {
    const problem = rippleProblem(project, clip, speed);
    if (problem) return problem;
  }
  return null;
}

/** The commands that set `clip`'s speed, in the order they must land. */
export function speedCommands(
  project: Project,
  clip: Clip,
  speed: number,
  preservePitch: boolean,
  ripple: SpeedRipple,
): EditorCommand[] {
  const set: EditorCommand = { kind: "setSpeed", clipId: clip.id, speed, preservePitch };
  const { ids: clipIds, deltaMs } = followersOf(project, clip, speed);
  if (ripple === "leave" || deltaMs === 0 || clipIds.length === 0) return [set];
  if (rippleProblem(project, clip, speed)) return [set];
  const move: EditorCommand = { kind: "moveClips", clipIds, deltaMs, trackId: null };
  return deltaMs > 0 ? [move, set] : [set, move];
}

/** Where a refusal lives, so it can outlast the undo that rolls it back. */
export interface HeldRefusal<E> {
  read: () => E | null;
  keep: (error: E) => void;
}

/** Sends `commands` in order. When Rust refuses one after others landed,
 * those are undone, so a speed change is never left half applied — and the
 * refusal (`hold`), which the undo's own reply clears, is put back. */
export async function runInOrderOrUndo<E>(
  execute: (command: EditorCommand) => Promise<boolean>,
  commands: EditorCommand[],
  hold?: HeldRefusal<E>,
): Promise<boolean> {
  for (const [landed, command] of commands.entries()) {
    if (await execute(command)) continue;
    const refusal = hold?.read() ?? null;
    for (let i = 0; i < landed; i += 1) await execute({ kind: "undo" });
    if (refusal !== null && landed > 0) hold?.keep(refusal);
    return false;
  }
  return true;
}
