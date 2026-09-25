/**
 * Multi-track placement and still images (Task 26; F-04, F-10). Dropping a
 * `LibraryAssetCard` onto a compatible `TrackLane` inserts a clip at the
 * snapped drop time; a drop onto the strip below the last lane mints a
 * track of the dropped asset's own kind first, then inserts onto it (two
 * separate, labelled `editorProject.execute` calls); a cross-lane move of
 * an EXISTING clip (Task 21) keeps its cue links intact because `moveClips`
 * never touches the clip's own id.
 *
 * `assetPayload` builds a fake `DataTransfer` via `trackCompat.
 * setAssetDragData` itself — the SAME function `LibraryAssetCard.vue`'s
 * `dragstart` calls — so these tests exercise the real wire shape rather
 * than a hand-rolled MIME string that could silently drift from it.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import LibraryAssetCard from "../src/components/editor/library/LibraryAssetCard.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import TrackLane from "../src/components/editor/timeline/TrackLane.vue";
import { lockedReason } from "../src/editor/actionMeta";
import { draggedAssetId, draggedAssetKind, setAssetDragData } from "../src/editor/trackCompat";
import type {
  Asset,
  Clip,
  EditorCommand,
  EditorOpenResult,
  EditorSnapshot,
  Marker,
  Project,
  Track,
} from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

// ---- fixtures ---------------------------------------------------------------
// Asymmetric on purpose (the "fixture flaw" rule): two video tracks in a
// non-alphabetical order plus an audio track, clips of different lengths,
// so a swapped track/index or a dropped offset cannot pass by coincidence.

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}

function asset(id: string, kind: "video" | "audio", durationMs: number, overrides: Partial<Asset> = {}): Asset {
  return { id, kind, name: id, duration_ms: durationMs, ...overrides };
}

function clip(id: string, trackId: string, assetId: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: assetId,
    track_id: trackId,
    name: id,
    start_ms: 0,
    in_ms: 0,
    out_ms: 1_000,
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
    ...overrides,
  };
}

function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [asset("vid-1", "video", 12_000), asset("aud-1", "audio", 7_500)],
    tracks: [track("v2"), track("v1")],
    clips: [],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

function snapshot(overrides: Partial<EditorSnapshot> = {}): EditorSnapshot {
  return {
    sessionId: "ses-a",
    projectId: "project-a",
    revision: 1,
    persistedRevision: null,
    title: "Tutorial",
    durationMs: 10_000,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
    ...overrides,
  };
}

/** A fake `DataTransfer`, built by `setAssetDragData` itself — the SAME
 * function `LibraryAssetCard.vue`'s own `dragstart` calls — so a lane's
 * drop handling is exercised against the real wire shape rather than a
 * hand-rolled type string. `types` is a live getter (real `DataTransfer`
 * behaviour): a `dragover` handler that read a snapshot taken before
 * `setData` ran would see an empty list. */
function fakeDataTransfer(): DataTransfer {
  const store = new Map<string, string>();
  return {
    dropEffect: "none",
    effectAllowed: "uninitialized",
    get types() {
      return [...store.keys()];
    },
    setData: (type: string, value: string) => {
      store.set(type, value);
    },
    getData: (type: string) => store.get(type) ?? "",
  } as unknown as DataTransfer;
}

function assetPayload(assetId: string, kind: "video" | "audio"): DataTransfer {
  const dt = fakeDataTransfer();
  setAssetDragData(dt, assetId, kind);
  return dt;
}

let executed: EditorCommand[] = [];

/** Just enough of Rust's own `addTrack`/`insertClip`/`moveClips` semantics
 * to drive these tests end to end: mint an id, push/patch the entity, and
 * (critically for the cross-lane-move test) touch NOTHING else -- markers/
 * effects are never read or rewritten by any of the three, so a real
 * `moveClips` reply proves cues survive a cross-lane move by construction,
 * not by this fixture's own goodwill. */
let trackSeq = 0;
function applyForTest(p: Project, cmd: EditorCommand): Project {
  if (cmd.kind === "addTrack") {
    trackSeq += 1;
    const newTrack: Track = {
      id: `trk-new-${trackSeq}`,
      kind: cmd.trackKind,
      name: cmd.name,
      visible: true,
      locked: false,
      muted: false,
      solo: false,
      volume: 1,
    };
    const tracks = [...p.tracks];
    tracks.splice(Math.min(cmd.index, tracks.length), 0, newTrack);
    return { ...p, tracks };
  }
  if (cmd.kind === "insertClip") {
    const newClip = clip(`clip-new-${trackSeq}`, cmd.trackId, cmd.assetId, {
      start_ms: cmd.startMs,
      in_ms: cmd.inMs,
      out_ms: cmd.outMs,
    });
    return { ...p, clips: [...p.clips, newClip] };
  }
  if (cmd.kind === "moveClips") {
    const ids = new Set(cmd.clipIds);
    const clips = p.clips.map((c) =>
      ids.has(c.id) ? { ...c, start_ms: c.start_ms + cmd.deltaMs, track_id: cmd.trackId ?? c.track_id } : c,
    );
    return { ...p, clips };
  }
  return p;
}

async function openProject(overrides: Partial<Project> = {}) {
  executed = [];
  trackSeq = 0;
  let state = project(overrides);
  const s = snapshot();
  const store = useEditorProjectStore();
  store.setPort(
    fakePort({
      openStaged: () =>
        Promise.resolve<EditorOpenResult>({
          snapshot: s,
          project: state,
          workspace: {},
          missing: [],
          sourceBase: "base",
          recovered: false,
        }),
      execute: (req) => {
        executed.push(req.command);
        state = applyForTest(state, req.command);
        return Promise.resolve({ snapshot: { ...s, revision: s.revision + executed.length }, project: state });
      },
    }),
  );
  await store.openStaged("base");
  return { store };
}

// ---- TrackLane: dropping onto an existing lane -----------------------------

describe("TrackLane — native asset drop (Task 26)", () => {
  it("audio asset cannot be dropped on a video lane (reason shown)", async () => {
    const w = mount(TrackLane, {
      props: {
        track: track("v1", { kind: "video" }),
        clips: [],
        assets: [],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v1"],
      },
    });
    await flushPromises();

    const dt = assetPayload("aud-1", "audio");
    const root = w.get('[data-testid="track-lane-v1"]');
    await root.trigger("dragover", { dataTransfer: dt });

    // Refused: the browser never gets `preventDefault()`, so `dropEffect`
    // stays whatever the handler explicitly set it to for a refusal.
    expect(dt.dropEffect).toBe("none");
    const body = w.get('[data-testid="track-lane-body-v1"]');
    expect(body.attributes("title")).toBe("Track v1 only accepts video media");
    expect(body.classes()).toContain("cursor-not-allowed");

    await root.trigger("drop", { dataTransfer: dt });
    await flushPromises();
    expect(w.emitted("asset-drop")).toBeUndefined();
  });

  it("a locked lane refuses ANY drop with the same lockedReason text a clip drag already shows", async () => {
    const w = mount(TrackLane, {
      props: {
        track: track("v1", { kind: "video", locked: true, name: "Screen recording" }),
        clips: [],
        assets: [],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v1"],
      },
    });
    await flushPromises();

    // A VIDEO asset -- the kind this track would otherwise accept -- so the
    // refusal can only be the LOCK, not a kind mismatch.
    const dt = assetPayload("vid-1", "video");
    const root = w.get('[data-testid="track-lane-v1"]');
    await root.trigger("dragover", { dataTransfer: dt });

    expect(dt.dropEffect).toBe("none");
    expect(w.get('[data-testid="track-lane-body-v1"]').attributes("title")).toBe(lockedReason("Screen recording"));

    await root.trigger("drop", { dataTransfer: dt });
    expect(w.emitted("asset-drop")).toBeUndefined();
  });

  it("a compatible, unlocked drop is accepted and emits asset-drop with the asset id, track id and clientX", async () => {
    const w = mount(TrackLane, {
      props: {
        track: track("v1", { kind: "video" }),
        clips: [],
        assets: [],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v1"],
      },
    });
    await flushPromises();

    const dt = assetPayload("vid-1", "video");
    const root = w.get('[data-testid="track-lane-v1"]');
    await root.trigger("dragover", { dataTransfer: dt });
    expect(dt.dropEffect).toBe("copy");
    expect(w.get('[data-testid="track-lane-body-v1"]').attributes("title")).toBeUndefined();

    await root.trigger("drop", { dataTransfer: dt, clientX: 321 });

    expect(w.emitted("asset-drop")).toEqual([[{ assetId: "vid-1", trackId: "v1", clientX: 321 }]]);
  });

  it("dragleave clears the refusal state", async () => {
    const w = mount(TrackLane, {
      props: {
        track: track("v1", { kind: "video" }),
        clips: [],
        assets: [],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v1"],
      },
    });
    await flushPromises();

    const dt = assetPayload("aud-1", "audio");
    const root = w.get('[data-testid="track-lane-v1"]');
    await root.trigger("dragover", { dataTransfer: dt });
    expect(w.get('[data-testid="track-lane-body-v1"]').attributes("title")).toBeTruthy();

    await root.trigger("dragleave");
    expect(w.get('[data-testid="track-lane-body-v1"]').attributes("title")).toBeUndefined();
  });

  it("ignores a drag that carries no library-asset MIME type at all", async () => {
    const w = mount(TrackLane, {
      props: {
        track: track("v1", { kind: "video" }),
        clips: [],
        assets: [],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v1"],
      },
    });
    await flushPromises();

    const dt = fakeDataTransfer(); // no setAssetDragData call at all
    const root = w.get('[data-testid="track-lane-v1"]');
    await root.trigger("dragover", { dataTransfer: dt });
    expect(dt.dropEffect).toBe("none"); // untouched -- the handler returned before setting anything
    expect(w.get('[data-testid="track-lane-body-v1"]').attributes("title")).toBeUndefined();

    await root.trigger("drop", { dataTransfer: dt });
    expect(w.emitted("asset-drop")).toBeUndefined();
  });

  // Defensive branch: `event.dataTransfer` can in principle be absent (a
  // non-drag caller synthesizing a bare "dragover" event) -- covered here
  // rather than left as dead-looking code the coverage floor would flag.
  it("a dragover with no dataTransfer at all is a no-op", async () => {
    const w = mount(TrackLane, {
      props: {
        track: track("v1", { kind: "video" }),
        clips: [],
        assets: [],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v1"],
      },
    });
    await flushPromises();

    await expect(w.get('[data-testid="track-lane-v1"]').trigger("dragover")).resolves.not.toThrow();
    expect(w.get('[data-testid="track-lane-body-v1"]').attributes("title")).toBeUndefined();
  });

  // Defensive branch: `dragover` already saw the video MIME type in
  // `types` (so `dragKind` was set and the drop was going to be accepted),
  // but `getData` returns "" for it at drop time -- a payload shape a
  // GENUINE `LibraryAssetCard` drag can never produce (`setAssetDragData`
  // always pairs the type with a value), but a caller must not crash on
  // one. Hand-built rather than through `assetPayload`/`setAssetDragData`
  // precisely because it simulates the one shape those helpers cannot
  // produce.
  it("a drop whose declared type carries no value emits nothing", async () => {
    const w = mount(TrackLane, {
      props: {
        track: track("v1", { kind: "video" }),
        clips: [],
        assets: [],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v1"],
      },
    });
    await flushPromises();

    const dt = {
      dropEffect: "none",
      effectAllowed: "uninitialized",
      types: ["application/x-vault-buddy-asset-video"],
      getData: () => "",
      setData: () => {},
    } as unknown as DataTransfer;
    const root = w.get('[data-testid="track-lane-v1"]');
    await root.trigger("dragover", { dataTransfer: dt });
    expect(dt.dropEffect).toBe("copy"); // accepted at dragover time -- the kind matches

    await root.trigger("drop", { dataTransfer: dt });
    expect(w.emitted("asset-drop")).toBeUndefined();
  });
});

describe("LibraryAssetCard — native drag source (Task 26)", () => {
  it("dragstart on a non-missing card sets the asset drag payload", async () => {
    const w = mount(LibraryAssetCard, {
      props: {
        id: "vid-1",
        name: "Take 1",
        kind: "video",
        kindLabel: "Video",
        duration: "0:12",
        missing: false,
        refusal: null,
        targetName: "V1",
      },
    });

    const dt = fakeDataTransfer();
    await w.get('[data-testid="library-asset-vid-1"]').trigger("dragstart", { dataTransfer: dt });

    expect(draggedAssetId(dt, "video")).toBe("vid-1");
  });

  it("a missing asset is not draggable, and sets nothing even if dragstart somehow still fires", async () => {
    const w = mount(LibraryAssetCard, {
      props: {
        id: "gone",
        name: "Old clip",
        kind: "video",
        kindLabel: "Video",
        duration: "0:03",
        missing: true,
        refusal: "This media's file is missing.",
        targetName: "",
      },
    });

    const card = w.get('[data-testid="library-asset-gone"]');
    expect(card.attributes("draggable")).toBe("false");

    const dt = fakeDataTransfer();
    await card.trigger("dragstart", { dataTransfer: dt });
    expect(draggedAssetKind(dt)).toBeNull();
  });

  // Defensive branch: a "dragstart" with no real DataTransfer at all.
  it("dragstart with no dataTransfer at all is a no-op", async () => {
    const w = mount(LibraryAssetCard, {
      props: {
        id: "vid-1",
        name: "Take 1",
        kind: "video",
        kindLabel: "Video",
        duration: "0:12",
        missing: false,
        refusal: null,
        targetName: "V1",
      },
    });

    await expect(w.get('[data-testid="library-asset-vid-1"]').trigger("dragstart")).resolves.not.toThrow();
  });
});

// ---- TimelineView: drop-to-insert and below-the-last-lane track creation --

describe("TimelineView — asset drop onto an existing lane (Task 26)", () => {
  it("dropping a compatible asset on a lane inserts a clip at the snapped drop time", async () => {
    await openProject({ tracks: [track("v1", { kind: "video" })] });
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const dt = assetPayload("vid-1", "video");
    const root = w.get('[data-testid="track-lane-v1"]');
    await root.trigger("dragover", { dataTransfer: dt });
    // 400px at zoom 1 (BASE_PX_PER_MS 0.05) -> 8_000ms; snap is on by
    // default but there is no target anywhere near it, so it lands as-is.
    await root.trigger("drop", { dataTransfer: dt, clientX: 400 + 196 }); // + the label column width
    await flushPromises();

    expect(executed).toEqual([{ kind: "insertClip", assetId: "vid-1", trackId: "v1", startMs: 8_000, inMs: 0, outMs: 12_000 }]);
  });

  it("an image asset defaults outMs to its own (5000ms) duration_ms, same as the library '+' button", async () => {
    await openProject({
      tracks: [track("v1", { kind: "video" })],
      assets: [asset("pic-1", "video", 5_000, { media_type: "image" })],
    });
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const dt = assetPayload("pic-1", "video"); // an image asset is `kind: "video"`
    const root = w.get('[data-testid="track-lane-v1"]');
    await root.trigger("dragover", { dataTransfer: dt });
    await root.trigger("drop", { dataTransfer: dt, clientX: 196 });
    await flushPromises();

    expect(executed).toEqual([{ kind: "insertClip", assetId: "pic-1", trackId: "v1", startMs: 0, inMs: 0, outMs: 5_000 }]);
  });
});

describe("TimelineView — drop below the last lane (Task 26)", () => {
  it("drop below the last lane creates a track then inserts", async () => {
    await openProject({ tracks: [track("v1", { kind: "video" })], assets: [asset("aud-1", "audio", 7_500)] });
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const strip = w.get('[data-testid="timeline-below-lanes"]');
    const dt = assetPayload("aud-1", "audio");
    await strip.trigger("dragover", { dataTransfer: dt });
    expect(dt.dropEffect).toBe("copy");
    await strip.trigger("drop", { dataTransfer: dt, clientX: 196 });
    await flushPromises();

    // Two separate commands -- addTrack THEN insertClip -- never one
    // command Rust has no shape for; that is what gives Undo two labelled
    // steps instead of one merged edit.
    expect(executed).toHaveLength(2);
    expect(executed[0]).toMatchObject({ kind: "addTrack", trackKind: "audio" });
    expect(executed[1]).toMatchObject({ kind: "insertClip", assetId: "aud-1", inMs: 0, outMs: 7_500 });

    // The inserted clip lands on the JUST-CREATED track, never a
    // pre-existing one -- there was no audio track before this drop.
    const insertCmd = executed[1] as { trackId: string };
    expect(insertCmd.trackId).not.toBe("v1");

    const store = useEditorProjectStore();
    expect(store.project?.tracks).toHaveLength(2);
    const newTrack = store.project?.tracks.find((t) => t.id === insertCmd.trackId);
    expect(newTrack?.kind).toBe("audio");
    expect(store.project?.clips).toHaveLength(1);
    expect(store.project?.clips[0].track_id).toBe(insertCmd.trackId);
  });

  it("a drag with no library-asset payload is ignored below the last lane too", async () => {
    await openProject({ tracks: [track("v1", { kind: "video" })] });
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const strip = w.get('[data-testid="timeline-below-lanes"]');
    const dt = fakeDataTransfer();
    await strip.trigger("dragover", { dataTransfer: dt });
    expect(dt.dropEffect).toBe("none");
    await strip.trigger("drop", { dataTransfer: dt });
    await flushPromises();

    expect(executed).toEqual([]);
  });
});

// ---- cross-lane move of an EXISTING clip keeps its cue links --------------

describe("TimelineView — cross-lane move keeps cue ids (Task 21's moveClips, exercised here)", () => {
  it("cross-lane move keeps cue ids", async () => {
    const marker: Marker = { id: "m1", clip_id: "c2", source_ms: 100, title: "Marker" };
    await openProject({
      tracks: [track("v2", { kind: "video" }), track("v1", { kind: "video" })],
      clips: [clip("c2", "v1", "vid-1", { start_ms: 0, in_ms: 0, out_ms: 1_000 })],
      markers: [marker],
    });
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const c2 = w.get('[data-testid="clip-c2"]');
    // c2 is on v1 (lane index 1 of 2); v2 sits directly above it. +10px at
    // zoom 1 is +200ms; -56px (one LANE_HEIGHT_PX) is one lane up.
    await c2.trigger("pointerdown", { clientX: 20, clientY: 100, pointerId: 1 });
    await c2.trigger("pointermove", { clientX: 30, clientY: 44, pointerId: 1 });
    await c2.trigger("pointerup", { clientX: 30, clientY: 44, pointerId: 1 });
    await flushPromises();

    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c2"], deltaMs: 200, trackId: "v2" }]);

    const store = useEditorProjectStore();
    // The SAME clip id, now on the new track -- moveClips never mints a new
    // clip id, so nothing needed to "follow" it.
    expect(store.project?.clips.find((c) => c.id === "c2")?.track_id).toBe("v2");
    // The marker's own `clip_id` still names "c2" -- untouched, because
    // Rust's moveClips reads/writes no cue collection at all.
    expect(store.project?.markers).toEqual([marker]);
  });
});
