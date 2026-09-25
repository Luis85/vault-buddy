import { describe, expect, it } from "vitest";

import { buildFragment } from "../src/editor/fragment";
import type { Project } from "../src/editorTypes";

/** Three clips, deliberately asymmetric starts (500, 1_200, 50 — not a
 * neat sequence) so a min-vs-selection mixup or a filter that reads the
 * wrong field cannot pass by coincidence. c3 is never copied in either
 * test below, and carries its own effect/marker/caption + the EARLIEST
 * start in the whole project — both are the traps: an unfiltered "every
 * cue" copy or an "earliest in the project" originMs would each leak c3's
 * data into the fragment. */
function project(): Project {
  return {
    // Task 13 widened `Project` to the full interchange graph; these five
    // fields are structurally required but never read by `buildFragment`
    // itself, so they are filled with the smallest valid values.
    schema: "vault-buddy-video-project/3",
    id: "project",
    title: "Project",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [],
    tracks: [],
    transitions: [],
    destination: { vault: "", folder: "", dated: false },
    clips: [
      {
        id: "c1",
        asset_id: "a1",
        track_id: "v1",
        name: "c1",
        start_ms: 500,
        in_ms: 0,
        out_ms: 300,
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
      },
      {
        id: "c2",
        asset_id: "a1",
        track_id: "v1",
        name: "c2",
        start_ms: 1_200,
        in_ms: 0,
        out_ms: 200,
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
      },
      {
        id: "c3",
        asset_id: "a1",
        track_id: "v1",
        name: "c3",
        start_ms: 50,
        in_ms: 0,
        out_ms: 100,
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
      },
    ],
    effects: [
      { id: "e1", clip_id: "c1", kind: "highlight", start_ms: 0, end_ms: 100, x: 0, y: 0, color: "#fff" },
      { id: "e2", clip_id: "c3", kind: "highlight", start_ms: 0, end_ms: 50, x: 0, y: 0, color: "#fff" },
    ],
    markers: [
      { id: "m1", clip_id: "c2", source_ms: 10, title: "M1" },
      { id: "m2", clip_id: "c3", source_ms: 10, title: "M2" },
    ],
    captions: {
      enabled: true,
      burn_in: false,
      font_size: 16,
      position: "bottom",
      background: false,
      cues: [
        { id: "cap1", clip_id: "c1", start_ms: 0, end_ms: 100, text: "hi" },
        { id: "cap2", clip_id: "c3", start_ms: 0, end_ms: 50, text: "bye" },
      ],
    },
  };
}

describe("buildFragment", () => {
  it("copies attached cues and markers only", () => {
    const fragment = buildFragment(project(), ["c1", "c2"]);

    expect(fragment.clips.map((c) => c.id).sort()).toEqual(["c1", "c2"]);
    // e2/m2/cap2 all belong to c3, which was never copied.
    expect(fragment.effects.map((e) => e.id)).toEqual(["e1"]);
    expect(fragment.markers.map((m) => m.id)).toEqual(["m1"]);
    expect(fragment.captions.map((c) => c.id)).toEqual(["cap1"]);
  });

  it("copies nothing when the project has no captions container at all", () => {
    const bare: Project = { ...project(), captions: null };
    const fragment = buildFragment(bare, ["c1", "c2"]);
    expect(fragment.captions).toEqual([]);
  });

  it("originMs is the earliest start", () => {
    // c3's start_ms=50 is the earliest in the WHOLE project but c3 is not
    // in the selection -- originMs must be min(500, 1200) = 500, not 50.
    const fragment = buildFragment(project(), ["c1", "c2"]);
    expect(fragment.originMs).toBe(500);
  });

  it("originMs is 0 for an empty selection", () => {
    const fragment = buildFragment(project(), []);
    expect(fragment.clips).toEqual([]);
    expect(fragment.originMs).toBe(0);
  });

  // The envelope-vs-entity spelling split (global-constraints: "Every new
  // DTO gets ... a decoder test in TS"): `originMs` is the ENVELOPE's own
  // camelCase field, but every entity inside it stays document spelling
  // (`start_ms`, `clip_id`, ...) -- the real port/decoders land in Task
  // 13, so a JSON-shape assertion on the plain serialized object is
  // enough to pin the split now.
  it("serializes with originMs (camelCase envelope) and document-spelled entity keys", () => {
    const fragment = buildFragment(project(), ["c1", "c2"]);
    const json = JSON.parse(JSON.stringify(fragment)) as Record<string, unknown>;

    expect(json).toHaveProperty("originMs");
    expect(json).not.toHaveProperty("origin_ms");

    const clips = json.clips as Record<string, unknown>[];
    expect(clips[0]).toHaveProperty("start_ms");
    expect(clips[0]).not.toHaveProperty("startMs");
    expect(clips[0]).toHaveProperty("asset_id");

    const effects = json.effects as Record<string, unknown>[];
    expect(effects[0]).toHaveProperty("clip_id");
    expect(effects[0]).not.toHaveProperty("clipId");
  });
});
