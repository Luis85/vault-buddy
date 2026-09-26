/**
 * `trackEdits.ts` (visual-parity Task 5): the gaps the
 * lane and track menus close, and the add-a-track path the lane menu, the
 * asset menu and the below-the-last-lane drop share.
 */
import { describe, expect, it, vi } from "vitest";

import type { EditorCommand } from "../src/editor/editorCommandTypes";
import {
  addTrackOfKind,
  addTrackThenInsert,
  closeAllGapsCommands,
  closeGapCommand,
  gapsOnTrack,
} from "../src/editor/trackEdits";
import type { Asset, Clip, Project, Track } from "../src/editorTypes";

function clip(id: string, trackId: string, start: number, length: number, speed = 1): Clip {
  return {
    id,
    asset_id: "a",
    track_id: trackId,
    name: id,
    start_ms: start,
    in_ms: 0,
    out_ms: length * speed,
    speed,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 1,
    volume: 1,
    muted: false,
    x: 0,
    y: 0,
    w: 1,
    h: 1,
  };
}

function track(id: string, kind: Track["kind"]): Track {
  return {
    id,
    kind,
    name: id,
    visible: true,
    locked: false,
    muted: false,
    solo: false,
    volume: 1,
  };
}

function project(clips: Clip[], tracks: Track[] = [track("v1", "video")]): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "p",
    title: "t",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [],
    tracks,
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "v", folder: "", dated: false },
  };
}

describe("gapsOnTrack", () => {
  it("finds the leading gap and the gaps between clips, in time order, on that track only", () => {
    const p = project([clip("b", "v1", 5_000, 1_000), clip("a", "v1", 1_000, 2_000), clip("x", "v2", 3_000, 500)]);
    expect(gapsOnTrack(p, "v1")).toEqual([
      { start: 0, end: 1_000 },
      { start: 3_000, end: 5_000 },
    ]);
  });

  it("measures a clip by its OUTPUT length, so a sped-up clip leaves the gap after it", () => {
    // 4 s of source at 2x plays for 2 s: 0..2000, then a gap to 3000.
    const p = project([clip("fast", "v1", 0, 2_000, 2), clip("next", "v1", 3_000, 1_000)]);
    expect(gapsOnTrack(p, "v1")).toEqual([{ start: 2_000, end: 3_000 }]);
  });

  it("ignores a one-millisecond rounding seam and overlapping clips", () => {
    const p = project([clip("a", "v1", 0, 1_000), clip("b", "v1", 1_001, 1_000), clip("c", "v1", 1_500, 100)]);
    expect(gapsOnTrack(p, "v1")).toEqual([]);
  });
});

describe("closing gaps", () => {
  // Gaps of 1 s, 2 s and 3 s, so their order shows.
  const p = project([clip("a", "v1", 1_000, 2_000), clip("b", "v1", 5_000, 1_000), clip("c", "v1", 9_000, 500)]);

  it("moves every clip after one gap earlier by its length", () => {
    expect(closeGapCommand(p, "v1", { start: 3_000, end: 5_000 })).toEqual({
      kind: "moveClips",
      clipIds: ["b", "c"],
      deltaMs: -2_000,
      trackId: null,
    });
  });

  it("closes the last gap first, so no clip is ever moved onto another", () => {
    const moves = closeAllGapsCommands(p, "v1") as { clipIds: string[]; deltaMs: number }[];
    expect(moves.map((m) => [m.clipIds.join(","), m.deltaMs])).toEqual([
      ["c", -3_000],
      ["b,c", -2_000],
      ["a,b,c", -1_000],
    ]);
  });
});

describe("addTrackOfKind / addTrackThenInsert", () => {
  const asset: Asset = { id: "voice", kind: "audio", name: "Voice", duration_ms: 9_000 };

  it("names the new track after how many of its kind exist, appends it, then inserts onto it", async () => {
    let current = project([], [track("v1", "video"), track("a1", "audio")]);
    const sent: EditorCommand[] = [];
    const execute = vi.fn(async (command: EditorCommand) => {
      sent.push(command);
      if (command.kind === "addTrack") current = { ...current, tracks: [...current.tracks, track("a2", "audio")] };
      return true;
    });
    await addTrackThenInsert(execute, () => current, asset, 1_500);
    expect(sent).toEqual([
      { kind: "addTrack", trackKind: "audio", name: "Audio 2", index: 2 },
      { kind: "insertClip", assetId: "voice", trackId: "a2", startMs: 1_500, inMs: 0, outMs: 9_000 },
    ]);
  });

  it("does not adopt a track some other edit added while this one was refused", async () => {
    let current = project([]);
    const execute = vi.fn(async () => {
      current = { ...current, tracks: [...current.tracks, track("v9", "video")] };
      return false;
    });
    expect(await addTrackOfKind(execute, () => current, "video")).toBeNull();
  });

  it("inserts nothing when the track is refused", async () => {
    const execute = vi.fn(async () => false);
    await addTrackThenInsert(execute, () => project([]), asset, 0);
    expect(execute).toHaveBeenCalledTimes(1);
  });

  it("resolves to the new track, or null when no new one appeared", async () => {
    const p = project([]);
    expect(await addTrackOfKind(async () => true, () => p, "video")).toBeNull();
  });
});
