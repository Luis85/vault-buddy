/**
 * `placeTake.placePresenterTake` (Task 50; F-21): three steps, each stopped
 * by a refusal of the one before — a refused `addTrack` inserts nothing, a
 * refused `insertClip` lays nothing out — and a presenter track name that is
 * already taken gets the next number.
 */
import { describe, expect, it } from "vitest";

import { type PlacementTarget, placePresenterTake } from "../src/editor/placeTake";
import type { EditorCommand, Project, TakeDto } from "../src/editorTypes";

const TAKE: TakeDto = { takeId: "take-3", assetId: "take-3", durationMs: 2_500, width: 640, height: 480, hasAudio: false };

function track(id: string, name: string) {
  return { id, kind: "video" as const, name, visible: true, locked: false, muted: false, solo: false, volume: 1 };
}

function project(): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "p",
    title: "T",
    // Portrait: a circle 0.19 wide is 0.1069 tall here.
    canvas: { width: 720, height: 1280, fps: 30 },
    master_gain: 1,
    assets: [],
    tracks: [track("v1", "Presenter")],
    clips: [],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "", folder: "", dated: false },
  };
}

type Target = PlacementTarget & { project: Project | null; sent: EditorCommand[] };

/** A minimal Rust that refuses `refuseAt` (and everything without a project). */
function target(refuseAt: EditorCommand["kind"] | null, start: Project | null = project()): Target {
  const t: Target = {
    project: start,
    sent: [],
    execute(command: EditorCommand): Promise<boolean> {
      t.sent.push(command);
      const p = t.project;
      if (command.kind === refuseAt || !p) return Promise.resolve(false);
      if (command.kind === "addTrack") p.tracks.splice(command.index, 0, track("trk-2", command.name));
      if (command.kind === "insertClip") {
        p.clips.push({
          id: "clip-2", asset_id: command.assetId, track_id: command.trackId, name: "Take",
          start_ms: command.startMs, in_ms: command.inMs, out_ms: command.outMs, fade_in_ms: 0, fade_out_ms: 0,
          fade_curve: "linear", opacity: 1, volume: 1, muted: false, x: 0, y: 0, w: 1, h: 1,
        });
      }
      return Promise.resolve(true);
    },
  };
  return t;
}

describe("placePresenterTake", () => {
  it("names a second presenter track 'Presenter 2' and sizes the circle for the canvas", async () => {
    const t = target(null);
    expect(await placePresenterTake(t, TAKE, 900)).toBe(true);
    expect(t.sent[0]).toEqual({ kind: "addTrack", trackKind: "video", name: "Presenter 2", index: 0 });
    expect(t.sent[1]).toEqual({ kind: "insertClip", assetId: "take-3", trackId: "trk-2", startMs: 900, inMs: 0, outMs: 2_500 });
    expect(t.sent[2]).toMatchObject({ clipIds: ["clip-2"], x: 0.775, y: 0.06, w: 0.19, h: 0.1069 });
  });

  it("a refused addTrack inserts nothing", async () => {
    const t = target("addTrack");
    expect(await placePresenterTake(t, TAKE, 0)).toBe(false);
    expect(t.sent.map((c) => c.kind)).toEqual(["addTrack"]);
  });

  it("a refused insertClip lays nothing out", async () => {
    const t = target("insertClip");
    expect(await placePresenterTake(t, TAKE, 0)).toBe(false);
    expect(t.sent.map((c) => c.kind)).toEqual(["addTrack", "insertClip"]);
  });

  it("without a project nothing is sent", async () => {
    const t = target(null, null);
    expect(await placePresenterTake(t, TAKE, 0)).toBe(false);
    expect(t.sent).toEqual([]);
  });
});
