/**
 * `src/editor/speedRipple.ts` (visual-parity Task 15, concept spec §5
 * "Speed": "When duration changes — Move following clips on this track /
 * Keep following clips in place"). Rust's `setSpeed` never moves another
 * clip, so "Move following clips" is a `moveClips` of the clips after this
 * one on its track, ordered so neither step overlaps them, and undone
 * together when Rust refuses the second.
 */
import { describe, expect, it, vi } from "vitest";

import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { runInOrderOrUndo, speedCommands } from "../src/editor/speedRipple";
import type { Clip, Project } from "../src/editorTypes";

function clip(id: string, start: number, overrides: Partial<Clip> = {}): Clip {
  return {
    id, asset_id: "cam", track_id: "v1", name: id, start_ms: start, in_ms: 1_000, out_ms: 5_000,
    fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false,
    x: 0, y: 0, w: 1, h: 1, ...overrides,
  };
}
function project(clips: Clip[]): Project {
  return {
    schema: "vault-buddy-video-project/3", id: "p", title: "T", canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1, assets: [], tracks: [], clips, effects: [], markers: [], transitions: [], captions: null,
    destination: { vault: "v", folder: "", dated: false },
  };
}

// c1 plays 700..4700; c2 starts exactly at its end, c3 later, c0 before it,
// and o1 sits on another track.
const PROJECT = project([
  clip("c0", 0, { out_ms: 1_500 }),
  clip("c1", 700),
  clip("c2", 4_700),
  clip("c3", 9_000),
  clip("o1", 5_000, { track_id: "v2" }),
]);
const C1 = PROJECT.clips[1];

describe("speedCommands", () => {
  it("keeping following clips in place is setSpeed alone", () => {
    expect(speedCommands(PROJECT, C1, 0.5, true, "leave")).toEqual([
      { kind: "setSpeed", clipId: "c1", speed: 0.5, preservePitch: true },
    ]);
  });

  it("a slowdown moves the clips after it on its own track first, by the added length", () => {
    expect(speedCommands(PROJECT, C1, 0.5, false, "ripple")).toEqual([
      { kind: "moveClips", clipIds: ["c2", "c3"], deltaMs: 4_000, trackId: null },
      { kind: "setSpeed", clipId: "c1", speed: 0.5, preservePitch: false },
    ]);
  });

  it("a speed-up changes the speed first, then pulls the following clips in", () => {
    expect(speedCommands(PROJECT, C1, 2, true, "ripple")).toEqual([
      { kind: "setSpeed", clipId: "c1", speed: 2, preservePitch: true },
      { kind: "moveClips", clipIds: ["c2", "c3"], deltaMs: -2_000, trackId: null },
    ]);
  });

  it("no following clip, or no change of length, moves nothing", () => {
    expect(speedCommands(PROJECT, PROJECT.clips[3], 0.5, true, "ripple")).toHaveLength(1);
    expect(speedCommands(PROJECT, C1, 1, false, "ripple")).toEqual([
      { kind: "setSpeed", clipId: "c1", speed: 1, preservePitch: false },
    ]);
  });
});

describe("runInOrderOrUndo", () => {
  const first: EditorCommand = { kind: "removeEffect", effectId: "a" };
  const second: EditorCommand = { kind: "removeEffect", effectId: "b" };

  it("sends every command while Rust accepts them", async () => {
    const execute = vi.fn((_command: EditorCommand) => Promise.resolve(true));
    expect(await runInOrderOrUndo(execute, [first, second])).toBe(true);
    expect(execute.mock.calls.map((c) => c[0])).toEqual([first, second]);
  });

  it("a refused later step undoes the steps that landed, so nothing is half done", async () => {
    const execute = vi.fn((c: EditorCommand) => Promise.resolve(c !== second));
    expect(await runInOrderOrUndo(execute, [first, second])).toBe(false);
    expect(execute.mock.calls.map((c) => c[0])).toEqual([first, second, { kind: "undo" }]);
  });

  it("a refused first step sends nothing more", async () => {
    const execute = vi.fn((_command: EditorCommand) => Promise.resolve(false));
    expect(await runInOrderOrUndo(execute, [first, second])).toBe(false);
    expect(execute).toHaveBeenCalledTimes(1);
  });
});
