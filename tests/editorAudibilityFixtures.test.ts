import { describe, expect, it } from "vitest";

import { isTrackAudible } from "../src/editor/mixRules";
import type { Track } from "../src/editorTypes";
import rawTable from "./fixtures/editor-audibility-cases.json";

/**
 * The TypeScript half of the SHARED audibility table (final whole-branch
 * review I4). `core::editor::render_plan_audio::track_is_audible` decides
 * what the RENDER mixes; `src/editor/mixRules.ts`' `isTrackAudible` decides
 * what the PREVIEW plays and what the mixer labels "silenced by solo". Both
 * read `tests/fixtures/editor-audibility-cases.json`, so a rule changed on
 * one side only reddens a suite instead of making the preview play what
 * the rendered video drops.
 */

interface Case {
  name: string;
  tracks: { muted: boolean; solo: boolean }[];
  audible: boolean[];
}

const table = rawTable as unknown as { version: number; cases: Case[] };

function track(index: number, flags: { muted: boolean; solo: boolean }): Track {
  return {
    id: `a${index}`,
    kind: "audio",
    name: `Audio ${index}`,
    visible: true,
    locked: false,
    muted: flags.muted,
    solo: flags.solo,
    volume: 1,
  };
}

describe("the shared audibility table", () => {
  // The Rust half asserts the same count against the same file.
  it("has 8 cases", () => {
    expect(table.cases).toHaveLength(8);
  });

  it.each(table.cases.map((c) => [c.name, c] as const))("%s matches the render", (_name, c) => {
    const tracks = c.tracks.map((flags, i) => track(i, flags));
    expect(tracks.map((t) => isTrackAudible(t, tracks))).toEqual(c.audible);
  });
});
