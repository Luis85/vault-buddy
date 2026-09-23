/**
 * Direct manipulation of a teaching cue in the preview (Task 35; F-28
 * "Endpoints update predictably"; SCREENS-AND-INTERACTIONS.md: "Arrows
 * expose endpoints; … highlights and spotlights expose bounds; zoom exposes
 * focal point"). PURE: `CueHandles.vue` owns the pointer lifecycle and
 * calls these for the numbers.
 *
 * A drag is a delta in canvas FRACTIONS applied to the effect as it was at
 * pointerdown (never accumulated move by move, so a long drag cannot
 * drift), clamped to the bounds `validate.rs` enforces for an effect —
 * positions `[0, 1]`, a box's `w`/`h` `[0.01, 1]` — and additionally so a
 * moved box stays on the canvas and a moved arrow keeps both ends on it.
 * Values are rounded to 1/10000 of the canvas before they are sent (the
 * `layoutGeometry.roundBox` precision), so an undo label or a saved project
 * never carries `0.7000000000000001` and a click that did not move sends
 * nothing at all.
 */
import type { Effect } from "../editorTypes";
import type { Size } from "./previewGeometry";

export type CueHandle = "start" | "end" | "move" | "resize" | "focal";

/** The geometric fields a drag may change. */
export type CuePatch = Partial<Pick<Effect, "x" | "y" | "x2" | "y2" | "w" | "h">>;

/** `validate.rs`'s floor for a cue box's `w`/`h`. */
const MIN_SIZE = 0.01;

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

function round4(v: number): number {
  return Math.round(v * 10_000) / 10_000;
}

function hasBox(e: Effect): boolean {
  return e.w !== undefined && e.h !== undefined;
}

/** How far a group of fractions can shift along one axis and stay in `[0, 1]`. */
function shiftWithin(d: number, lo: number, hi: number): number {
  return clamp(d, -lo, Math.max(0, 1 - hi));
}

function movePatch(e: Effect, dx: number, dy: number): CuePatch {
  if (e.kind === "arrow") {
    const [x2, y2] = [e.x2 ?? e.x, e.y2 ?? e.y];
    const sx = shiftWithin(dx, Math.min(e.x, x2), Math.max(e.x, x2));
    const sy = shiftWithin(dy, Math.min(e.y, y2), Math.max(e.y, y2));
    return { x: e.x + sx, y: e.y + sy, x2: x2 + sx, y2: y2 + sy };
  }
  if (hasBox(e)) {
    return { x: e.x + shiftWithin(dx, e.x, e.x + (e.w ?? 0)), y: e.y + shiftWithin(dy, e.y, e.y + (e.h ?? 0)) };
  }
  return { x: clamp(e.x + dx, 0, 1), y: clamp(e.y + dy, 0, 1) };
}

/** The unrounded patch a drag of `handle` by `(dx, dy)` asks for. */
function rawPatch(e: Effect, handle: CueHandle, dx: number, dy: number): CuePatch {
  switch (handle) {
    case "end":
      return { x2: clamp((e.x2 ?? e.x) + dx, 0, 1), y2: clamp((e.y2 ?? e.y) + dy, 0, 1) };
    case "resize":
      return {
        w: clamp((e.w ?? 0) + dx, MIN_SIZE, Math.max(MIN_SIZE, 1 - e.x)),
        h: clamp((e.h ?? 0) + dy, MIN_SIZE, Math.max(MIN_SIZE, 1 - e.y)),
      };
    case "move":
      return movePatch(e, dx, dy);
    default: // "start" and "focal" both move the effect's own point
      return { x: clamp(e.x + dx, 0, 1), y: clamp(e.y + dy, 0, 1) };
  }
}

/** The rounded patch for a drag of `handle` by `(dx, dy)` canvas fractions. */
export function dragPatch(effect: Effect, handle: CueHandle, dx: number, dy: number): CuePatch {
  const raw = rawPatch(effect, handle, dx, dy);
  const out: CuePatch = {};
  for (const [key, value] of Object.entries(raw) as [keyof CuePatch, number][]) out[key] = round4(value);
  return out;
}

/** `patch` if it changes anything about `effect` at the sent precision,
 * else `null` — a click without movement sends nothing. */
export function changedPatch(effect: Effect, patch: CuePatch): CuePatch | null {
  const changed = (Object.keys(patch) as (keyof CuePatch)[]).some(
    (key) => patch[key] !== round4(effect[key] ?? Number.NaN),
  );
  return changed ? patch : null;
}

export interface HandleSpot {
  handle: CueHandle;
  /** Canvas px. */
  cx: number;
  cy: number;
}

/** Where a SELECTED cue's grab handles sit (canvas px). The body of a box,
 * step or arrow is grabbed through its hit shape instead (`cueShapes`). */
export function handleSpots(effect: Effect, canvas: Size): HandleSpot[] {
  const at = (x: number, y: number) => ({ cx: x * canvas.width, cy: y * canvas.height });
  if (effect.kind === "arrow") {
    return [
      { handle: "start", ...at(effect.x, effect.y) },
      { handle: "end", ...at(effect.x2 ?? effect.x, effect.y2 ?? effect.y) },
    ];
  }
  if (effect.kind === "zoom") return [{ handle: "focal", ...at(effect.x, effect.y) }];
  if (hasBox(effect)) return [{ handle: "resize", ...at(effect.x + (effect.w ?? 0), effect.y + (effect.h ?? 0)) }];
  return [];
}
