/**
 * "When duration changes" for the Speed tab (visual-parity Task 15; concept
 * spec §5 "Speed"). Rust's `setSpeed` never moves another clip: a slowdown
 * that would run into the next clip is refused ("nothing is moved
 * automatically"). So "Move following clips on this track" is the Speed tab
 * asking for that room itself — a `moveClips` of every later clip on the
 * clip's own track by the change in length (Rust moves a grouped clip's
 * partners with it) — ordered so neither step overlaps anything: a slowdown
 * makes room first, a speed-up closes the gap after. "Keep following clips
 * in place" is `setSpeed` alone.
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

function endAt(clip: Clip, speed: number): number {
  return clipOutputEnd({ start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed });
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
  const deltaMs = endAt(clip, speed) - endAt(clip, clip.speed ?? 1);
  const clipIds = project.clips
    .filter((c) => c.track_id === clip.track_id && c.start_ms > clip.start_ms)
    .map((c) => c.id);
  if (ripple === "leave" || deltaMs === 0 || clipIds.length === 0) return [set];
  const move: EditorCommand = { kind: "moveClips", clipIds, deltaMs, trackId: null };
  return deltaMs > 0 ? [move, set] : [set, move];
}

/** Sends `commands` in order. When Rust refuses one after others landed,
 * those are undone, so a speed change is never left half applied. */
export async function runInOrderOrUndo(
  execute: (command: EditorCommand) => Promise<boolean>,
  commands: EditorCommand[],
): Promise<boolean> {
  for (const [landed, command] of commands.entries()) {
    if (await execute(command)) continue;
    for (let i = 0; i < landed; i += 1) await execute({ kind: "undo" });
    return false;
  }
  return true;
}
