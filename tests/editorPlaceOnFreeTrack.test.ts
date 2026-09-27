/**
 * `placeOnFreeTrack` (visual-parity Task 7; no-op audit finding 1a): the ONE
 * rule every "insert at the playhead" surface — the media library's "+",
 * the Titles cards, the asset menu's "Add at playhead" — uses to pick a
 * track. Before it, each took the FIRST track of the right kind, and Rust
 * refused the overlap: in the parity sample, an Intro card at 0 ms aimed at
 * v3 (the presenter overlay covers it) and nothing happened at all.
 *
 * The library tests below run against the parity sample itself
 * (`tests/e2e/fixtures/parityProject.ts`), the project the audit measured.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import MediaLibrary from "../src/components/editor/library/MediaLibrary.vue";
import TitlesLibrary from "../src/components/editor/library/TitlesLibrary.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { placeOnFreeTrack } from "../src/editor/placeOnFreeTrack";
import type { Clip, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { PARITY_OPEN_RESULT } from "./e2e/fixtures/parityProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

const parity: Project = PARITY_OPEN_RESULT.project;

function track(id: string, kind: "video" | "audio", locked = false): Track {
  return { id, kind, name: id, visible: true, locked, muted: false, solo: false, volume: 1 };
}

function clip(id: string, trackId: string, startMs: number, lengthMs: number, speed?: number): Clip {
  return {
    id,
    asset_id: "a",
    track_id: trackId,
    name: id,
    start_ms: startMs,
    in_ms: 0,
    out_ms: lengthMs,
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
    ...(speed ? { speed } : {}),
  };
}

function project(tracks: Track[], clips: Clip[]): Project {
  return { ...parity, tracks, clips, effects: [], markers: [], transitions: [], captions: null };
}

describe("placeOnFreeTrack", () => {
  it("skips a video track whose clip overlaps the span and picks the next free one", () => {
    // Parity: c5 covers v3 from 1.5 s, c4 sits on v2 from 11 s.
    expect(placeOnFreeTrack(parity, "video", 0, 3_000)).toEqual({ trackId: "v2" });
  });

  it("takes the topmost video track when it is free", () => {
    expect(placeOnFreeTrack(parity, "video", 0, 1_000)).toEqual({ trackId: "v3" });
  });

  it("asks for a new track above the topmost video track when every video track is busy", () => {
    // 12 s: c5 (v3), c4 (v2) and c2 (v1) all run through it.
    expect(placeOnFreeTrack(parity, "video", 12_000, 1_000)).toEqual({ newTrackIndex: 0 });
    const audioFirst = project([track("a1", "audio"), track("v1", "video")], [clip("x", "v1", 0, 5_000)]);
    expect(placeOnFreeTrack(audioFirst, "video", 0, 1_000)).toEqual({ newTrackIndex: 1 });
  });

  it("an end touching the next clip's start is not an overlap", () => {
    const p = project([track("v1", "video")], [clip("x", "v1", 2_000, 1_000)]);
    expect(placeOnFreeTrack(p, "video", 0, 2_000)).toEqual({ trackId: "v1" });
    expect(placeOnFreeTrack(p, "video", 3_000, 1_000)).toEqual({ trackId: "v1" });
    expect(placeOnFreeTrack(p, "video", 2_999, 1_000)).toEqual({ newTrackIndex: 0 });
  });

  it("measures a clip by its OUTPUT length (speed-aware)", () => {
    // 4 s of source at 2x occupies 2 s of output.
    const p = project([track("v1", "video")], [clip("x", "v1", 0, 4_000, 2)]);
    expect(placeOnFreeTrack(p, "video", 2_000, 1_000)).toEqual({ trackId: "v1" });
  });

  it("never picks a locked track, or a track of the other kind", () => {
    const p = project([track("v1", "video", true), track("a1", "audio")], []);
    expect(placeOnFreeTrack(p, "video", 0, 1_000)).toEqual({ newTrackIndex: 0 });
  });

  it("audio: the first free audio track, or a new one after the last audio track", () => {
    const tracks = [track("v1", "video"), track("a1", "audio"), track("a2", "audio"), track("v9", "video")];
    const busyA1 = project(tracks, [clip("m", "a1", 0, 10_000)]);
    expect(placeOnFreeTrack(busyA1, "audio", 1_000, 1_000)).toEqual({ trackId: "a2" });
    const busyBoth = project(tracks, [clip("m", "a1", 0, 10_000), clip("n", "a2", 500, 10_000)]);
    expect(placeOnFreeTrack(busyBoth, "audio", 1_000, 1_000)).toEqual({ newTrackIndex: 3 });
    expect(placeOnFreeTrack(parity, "audio", 1_000, 1_000)).toEqual({ newTrackIndex: 4 });
    expect(placeOnFreeTrack(project([track("v1", "video")], []), "audio", 0, 1_000)).toEqual({ newTrackIndex: 1 });
  });
});

// ---- the library inserts, against the parity sample -------------------------------

/** A fake Rust that installs `addTrack` so the insert that follows can find
 * the new track, and accepts everything else unchanged. */
function parityPort(sent: EditorCommand[]) {
  let current: Project = parity;
  let revision = PARITY_OPEN_RESULT.snapshot.revision;
  return fakeEditorPort({
    openStaged: () => Promise.resolve(PARITY_OPEN_RESULT),
    execute: (req) => {
      sent.push(req.command);
      if (req.command.kind === "addTrack") {
        const tracks = [...current.tracks];
        tracks.splice(req.command.index, 0, track("new", req.command.trackKind));
        current = { ...current, tracks };
      }
      revision += 1;
      return Promise.resolve({ snapshot: { ...PARITY_OPEN_RESULT.snapshot, revision }, project: current });
    },
  });
}

async function openParity(playheadMs: number, sent: EditorCommand[]): Promise<void> {
  const store = useEditorProjectStore();
  store.setPort(parityPort(sent));
  await store.openStaged("parity");
  useEditorWorkspaceStore().setPlayhead(playheadMs);
}

describe("Titles cards land on a free track", () => {
  it("the Intro card at 0 ms goes on v2 (v3 is covered by the presenter overlay)", async () => {
    const sent: EditorCommand[] = [];
    await openParity(0, sent);
    const w = mount(TitlesLibrary);
    await w.get('[data-testid="titles-add-intro"]').trigger("click");
    await flushPromises();
    expect(sent).toEqual([
      { kind: "addCard", preset: "intro", trackId: "v2", startMs: 0, durationMs: 3_000, title: "Intro", subtitle: "" },
    ]);
  });

  it("with every video track busy, adds a video track on top and puts the card on it", async () => {
    const sent: EditorCommand[] = [];
    await openParity(12_000, sent);
    const w = mount(TitlesLibrary);
    await w.get('[data-testid="titles-add-intro"]').trigger("click");
    await flushPromises();
    expect(sent).toEqual([
      { kind: "addTrack", trackKind: "video", name: "Video 4", index: 0 },
      { kind: "addCard", preset: "intro", trackId: "new", startMs: 12_000, durationMs: 3_000, title: "Intro", subtitle: "" },
    ]);
  });
});

describe("Media + lands on a free track", () => {
  it("inserts on the free track, or adds one first", async () => {
    const sent: EditorCommand[] = [];
    await openParity(0, sent);
    const w = mount(MediaLibrary);
    // The detail asset is 18 s: v3 (from 1.5 s), v2 (from 11 s) and v1
    // (from 0) are all busy, so a new track goes on top.
    await w.get('[data-testid="library-asset-detail-insert"]').trigger("click");
    await flushPromises();
    expect(sent).toEqual([
      { kind: "addTrack", trackKind: "video", name: "Video 4", index: 0 },
      { kind: "insertClip", assetId: "detail", trackId: "new", startMs: 0, inMs: 0, outMs: 18_000 },
    ]);
  });
});
