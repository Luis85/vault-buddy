/**
 * The context-menu item sets (visual-parity Task 5; concept spec §8, design
 * D13/D14). Each set is a pure function of a `MenuContext`; here that
 * context is a set of spies, so every item's `run` can be pinned to the
 * registry action, the `editor_execute` command or the view change it
 * makes — and no item may do nothing.
 */
import { flushPromises } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";

import type { MenuAction, MenuItem } from "../src/components/editor/menus/menuModel";
import { isSeparator } from "../src/components/editor/menus/menuModel";
import type { MenuContext } from "../src/editor/menuContext";
import { assetMenu, contextMenuFor, cueMenu, laneMenu, trackMenu } from "../src/editor/menuSets";
import { clipMenu, multiClipMenu } from "../src/editor/menuSetsClip";
import type { Clip, EditorSnapshot, Effect, Project, Track } from "../src/editorTypes";

// ---- fixture ------------------------------------------------------------------
// Asymmetric: two video tracks (the lower one locked), one audio track; c2
// sits after a 1 s gap on v1, c3 is an audio clip, e1 is a cue on c2.

function track(id: string, kind: Track["kind"], overrides: Partial<Track> = {}): Track {
  return {
    id,
    kind,
    name: id.toUpperCase(),
    visible: true,
    locked: false,
    muted: false,
    solo: false,
    volume: 1,
    ...overrides,
  };
}

function clip(id: string, trackId: string, assetId: string, start: number, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: assetId,
    track_id: trackId,
    name: `Clip ${id}`,
    start_ms: start,
    in_ms: 0,
    out_ms: 4_000,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 1,
    volume: 1,
    muted: false,
    x: 0.1,
    y: 0.2,
    w: 0.5,
    h: 0.4,
    ...overrides,
  };
}

const CUE: Effect = {
  id: "e1",
  clip_id: "c2",
  kind: "text",
  start_ms: 500,
  end_ms: 1_500,
  x: 0.2,
  y: 0.3,
  w: 0.4,
  h: 0.1,
  color: "#fff",
  text: "Hi",
};

function project(): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "p1",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [
      { id: "vid", kind: "video", name: "Screen.mp4", duration_ms: 20_000 },
      { id: "aud", kind: "audio", name: "Voice.m4a", duration_ms: 20_000 },
      { id: "gone", kind: "video", name: "Missing.mp4", duration_ms: 6_000 },
    ],
    tracks: [
      track("v2", "video"),
      track("v1", "video"),
      track("a1", "audio"),
      track("v0", "video", { locked: true }),
    ],
    clips: [
      clip("c1", "v1", "vid", 1_000),
      clip("c2", "v1", "vid", 6_000),
      clip("c3", "a1", "aud", 0, { muted: true }),
      clip("c4", "v0", "vid", 0),
    ],
    effects: [CUE],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "v", folder: "", dated: false },
  };
}

function snapshot(): EditorSnapshot {
  return {
    sessionId: "s1",
    projectId: "p1",
    revision: 3,
    persistedRevision: 3,
    title: "Tutorial",
    durationMs: 10_000,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
  };
}

function actionCtx(overrides: Partial<MenuContext["action"]> = {}): MenuContext["action"] {
  return {
    project: project(),
    snapshot: snapshot(),
    playheadMs: 7_000,
    selectedClipIds: [],
    pointerTarget: { kind: "clip", id: "c2", timeMs: 8_500 },
    hasClipboard: false,
    clipboardFragment: null,
    ...overrides,
  };
}

/** Every effect a set can cause, as a spy. */
function menuCtx(action: Partial<MenuContext["action"]> = {}, extra: Partial<MenuContext> = {}): MenuContext {
  return {
    action: actionCtx(action),
    snap: true,
    hasSelection: true,
    missingAssetIds: new Set(["gone"]),
    execute: vi.fn(async () => true),
    activate: vi.fn(),
    seek: vi.fn(),
    selectClips: vi.fn(),
    selectEffect: vi.fn(),
    clearSelection: vi.fn(),
    openProperty: vi.fn(),
    focusClipName: vi.fn(),
    openLibrary: vi.fn(),
    fitRange: vi.fn(),
    fitTimeline: vi.fn(),
    toggleSnap: vi.fn(),
    addTrack: vi.fn(),
    addAssetOnFreeTrack: vi.fn(),
    addAssetOnNewTrack: vi.fn(),
    renameTrack: vi.fn(),
    removeTrack: vi.fn(),
    reconnect: vi.fn(),
    ...extra,
  };
}

// ---- helpers --------------------------------------------------------------------

const labels = (items: MenuItem[]) => items.map((i) => (isSeparator(i) ? "—" : i.label));

function find(items: MenuItem[], label: string): MenuAction {
  const hit = items.find((i): i is MenuAction => !isSeparator(i) && i.label === label);
  if (!hit) throw new Error(`no item "${label}" in ${labels(items).join(" | ")}`);
  return hit;
}

function sub(items: MenuItem[], parent: string, label: string): MenuAction {
  return find(find(items, parent).submenu ?? [], label);
}

function flatten(items: MenuItem[]): MenuAction[] {
  return items.flatMap((i) => (isSeparator(i) ? [] : [i, ...flatten(i.submenu ?? [])]));
}

function run(item: MenuAction): void {
  expect(item.disabledReason ?? null).toBeNull();
  item.run?.();
}

const executed = (ctx: MenuContext) => vi.mocked(ctx.execute).mock.calls.map(([c]) => c);

// ---- the clip menu --------------------------------------------------------------

describe("clipMenu — the §8 single-clip items that have a native backend", () => {
  it("lists exactly the concept's labels and order for a video clip", () => {
    expect(labels(clipMenu(menuCtx())).join(" | ")).toBe(
      [
        "Go to this clip", "Fit this clip", "—", "Copy clip", "Cut clip", "Duplicate", "Rename…", "—",
        "Split at 00:08.5", "Trim to pointer", "Speed", "Transform", "Color treatment",
        "Cover private information", "Fades", "Audio", "Add caption here…", "—", "Clear selection",
        "Delete · leave gap", "Delete · ripple this track",
      ].join(" | "),
    );
  });

  it("drops the picture-only items for an audio clip", () => {
    const items = clipMenu(menuCtx({ pointerTarget: { kind: "clip", id: "c3", timeMs: 1_000 } }));
    expect(labels(items)).not.toContain("Transform");
    expect(labels(items)).not.toContain("Color treatment");
    expect(labels(items)).not.toContain("Cover private information");
    expect(labels(find(items, "Audio").submenu ?? [])).toEqual(["Unmute clip", "Audio properties…"]);
  });

  it("wires navigation, the clipboard and delete to the registry and the view", () => {
    const ctx = menuCtx();
    const items = clipMenu(ctx);
    run(find(items, "Go to this clip"));
    expect(ctx.selectClips).toHaveBeenCalledWith(["c2"]);
    expect(ctx.seek).toHaveBeenCalledWith(6_400);
    run(find(items, "Fit this clip"));
    expect(ctx.fitRange).toHaveBeenCalledWith(6_000, 10_000);
    for (const [label, id] of [
      ["Copy clip", "copy"], ["Cut clip", "cut"], ["Duplicate", "duplicate"], ["Split at 00:08.5", "split"],
      ["Delete · leave gap", "delete"], ["Delete · ripple this track", "deleteClose"],
      ["Cover private information", "addMask"],
    ] as const) {
      run(find(items, label));
      expect(vi.mocked(ctx.activate).mock.lastCall?.[0]).toBe(id);
    }
    expect(find(items, "Delete · leave gap").danger).toBe(true);
    expect(find(items, "Delete · ripple this track").danger).toBe(true);
    expect(find(items, "Copy clip").kbd).toBe("Ctrl+C");
    run(find(items, "Clear selection"));
    expect(ctx.clearSelection).toHaveBeenCalled();
  });

  it("covers private information on THIS clip, at a time inside it", () => {
    const ctx = menuCtx();
    run(find(clipMenu(ctx), "Cover private information"));
    const [, target] = vi.mocked(ctx.activate).mock.calls[0];
    expect(target.selectedClipIds).toEqual(["c2"]);
    expect(target.playheadMs).toBe(8_500);
    expect(ctx.seek).toHaveBeenCalledWith(8_500);
  });

  it("Rename… opens the inspector's Clip tab with the name field focused", () => {
    const ctx = menuCtx();
    run(find(clipMenu(ctx), "Rename…"));
    expect(ctx.selectClips).toHaveBeenCalledWith(["c2"]);
    expect(ctx.openProperty).toHaveBeenCalledWith("clip");
    expect(ctx.focusClipName).toHaveBeenCalledWith("c2");
  });

  it("trims to the pointer with trimClip", () => {
    const ctx = menuCtx();
    const items = clipMenu(ctx);
    run(sub(items, "Trim to pointer", "Trim start to here"));
    run(sub(items, "Trim to pointer", "Trim end to here"));
    expect(executed(ctx)).toEqual([
      { kind: "trimClip", clipId: "c2", startMs: 8_500, inMs: 2_500, outMs: 4_000 },
      { kind: "trimClip", clipId: "c2", startMs: 6_000, inMs: 0, outMs: 2_500 },
    ]);
  });

  it("refuses to split or trim at a clip edge, with the reason", () => {
    const items = clipMenu(menuCtx({ pointerTarget: { kind: "clip", id: "c2", timeMs: 6_000 } }));
    expect(find(items, "Split at 00:06.0").disabledReason).toBe("The playhead is at a clip boundary");
    expect(find(items, "Trim to pointer").disabledReason).toBe("Choose a point inside the clip.");
  });

  it("sets speed, checks the current one, and opens the Speed tab", () => {
    const ctx = menuCtx();
    const items = clipMenu(ctx);
    expect(labels(find(items, "Speed").submenu ?? [])).toEqual([
      "0.25×", "0.5×", "1× · normal", "1.5×", "2×", "4×", "—", "Speed & timing options…",
    ]);
    expect(sub(items, "Speed", "1× · normal").checked).toBe(true);
    expect(sub(items, "Speed", "2×").checked).toBe(false);
    run(sub(items, "Speed", "0.5×"));
    expect(executed(ctx)).toEqual([{ kind: "setSpeed", clipId: "c2", speed: 0.5, preservePitch: true }]);
    run(sub(items, "Speed", "Speed & timing options…"));
    expect(ctx.openProperty).toHaveBeenCalledWith("speed");
  });

  it("transforms the picture with setLayout", () => {
    const ctx = menuCtx();
    const items = clipMenu(ctx);
    expect(labels(find(items, "Transform").submenu ?? [])).toEqual([
      "Fit entire source", "Fill frame · crop", "Rotate 90° clockwise", "Flip horizontally", "Flip vertically",
      "Center in canvas", "Full frame", "Reset crop & orientation",
    ]);
    expect(sub(items, "Transform", "Fit entire source").checked).toBe(true);
    for (const label of labels(find(items, "Transform").submenu ?? [])) run(sub(items, "Transform", label));
    expect(executed(ctx)).toEqual([
      { kind: "setLayout", clipIds: ["c2"], fit: "contain" },
      { kind: "setLayout", clipIds: ["c2"], fit: "cover" },
      { kind: "setLayout", clipIds: ["c2"], rotation: 90 },
      { kind: "setLayout", clipIds: ["c2"], mirror: true },
      { kind: "setLayout", clipIds: ["c2"], flipY: true },
      { kind: "setLayout", clipIds: ["c2"], x: 0.25, y: 0.3 },
      { kind: "setLayout", clipIds: ["c2"], x: 0, y: 0, w: 1, h: 1 },
      { kind: "setLayout", clipIds: ["c2"], rotation: 0, mirror: false, flipY: false, cropZoom: 1, cropX: 0.5, cropY: 0.5 },
    ]);
  });

  it("applies the concept's colour treatments with setAdjustments and opens the Color tab", () => {
    const ctx = menuCtx();
    const items = clipMenu(ctx);
    expect(labels(find(items, "Color treatment").submenu ?? [])).toEqual([
      "Original", "Clear", "Warm", "Soft", "Mono", "Adjust color…",
    ]);
    expect(sub(items, "Color treatment", "Original").checked).toBe(true);
    run(sub(items, "Color treatment", "Mono"));
    run(sub(items, "Color treatment", "Original"));
    expect(executed(ctx)).toEqual([
      { kind: "setAdjustments", clipIds: ["c2"], adjustments: { brightness: 1, contrast: 1.12, saturation: 1, sepia: 0, grayscale: 1 } },
      { kind: "setAdjustments", clipIds: ["c2"], adjustments: null },
    ]);
    run(sub(items, "Color treatment", "Adjust color…"));
    expect(ctx.openProperty).toHaveBeenCalledWith("color");
  });

  it("checks no treatment when the clip's colour is its own mix", () => {
    const own = { brightness: 1, contrast: 1, saturation: 1, sepia: 0, grayscale: 1 };
    const p = project();
    p.clips = p.clips.map((c) => (c.id === "c2" ? { ...c, adjustments: own } : c));
    const items = clipMenu(menuCtx({ project: p }));
    const checked = (find(items, "Color treatment").submenu ?? []).filter((i) => !isSeparator(i) && i.checked);
    expect(checked).toEqual([]);
  });

  it("clamps a both-edges fade to half of a short clip", () => {
    const p = project();
    p.clips = p.clips.map((c) => (c.id === "c2" ? { ...c, out_ms: 1_500 } : c));
    const ctx = menuCtx({ project: p, pointerTarget: { kind: "clip", id: "c2", timeMs: 6_500 } });
    run(sub(clipMenu(ctx), "Fades", "1s · both edges"));
    expect(executed(ctx)).toEqual([{ kind: "setFades", clipId: "c2", fadeInMs: 750, fadeOutMs: 750 }]);
  });

  it("sets both edge fades, never past half the clip, and opens the Fades tab", () => {
    const ctx = menuCtx();
    const items = clipMenu(ctx);
    expect(labels(find(items, "Fades").submenu ?? [])).toEqual([
      "Remove edge fades", "0.25s · both edges", "0.5s · both edges", "1s · both edges", "Fades & transitions…",
    ]);
    run(sub(items, "Fades", "1s · both edges"));
    run(sub(items, "Fades", "Remove edge fades"));
    expect(executed(ctx)).toEqual([
      { kind: "setFades", clipId: "c2", fadeInMs: 1_000, fadeOutMs: 1_000 },
      { kind: "setFades", clipId: "c2", fadeInMs: 0, fadeOutMs: 0 },
    ]);
    run(sub(items, "Fades", "Fades & transitions…"));
    expect(ctx.openProperty).toHaveBeenCalledWith("fades");
  });

  it("mutes with setClipMix, detaches through the registry and opens the Audio tab", () => {
    const ctx = menuCtx();
    const items = clipMenu(ctx);
    expect(labels(find(items, "Audio").submenu ?? [])).toEqual([
      "Mute clip", "Detach audio · keep aligned", "Audio properties…",
    ]);
    run(sub(items, "Audio", "Mute clip"));
    expect(executed(ctx)).toEqual([{ kind: "setClipMix", clipIds: ["c2"], muted: true }]);
    run(sub(items, "Audio", "Detach audio · keep aligned"));
    expect(vi.mocked(ctx.activate).mock.calls[0][0]).toBe("detachAudio");
    run(sub(items, "Audio", "Audio properties…"));
    expect(ctx.openProperty).toHaveBeenCalledWith("audio");
  });

  it("adds a caption on this clip at the pointer and opens the Captions tab", () => {
    const ctx = menuCtx();
    run(find(clipMenu(ctx), "Add caption here…"));
    expect(ctx.seek).toHaveBeenCalledWith(8_500);
    expect(executed(ctx)).toEqual([{ kind: "addCaption", clipId: "c2", startMs: 2_500, endMs: 4_000, text: "New caption" }]);
    expect(ctx.openLibrary).toHaveBeenCalledWith("captions");
  });

  it("disables every edit on a locked track with the track's reason, but still offers copy", () => {
    const items = clipMenu(menuCtx({ pointerTarget: { kind: "clip", id: "c4", timeMs: 1_000 } }));
    for (const label of ["Rename…", "Trim to pointer", "Speed", "Transform", "Color treatment", "Fades", "Audio", "Cut clip"]) {
      expect(find(items, label).disabledReason).toBe("Track V0 is locked");
    }
    expect(find(items, "Copy clip").disabledReason).toBeNull();
  });

  it("offers Clear selection only when something is selected", () => {
    const items = clipMenu(menuCtx({}, { hasSelection: false }));
    expect(find(items, "Clear selection").disabledReason).toBe("Nothing is selected.");
  });
});

describe("multiClipMenu", () => {
  const multi = () => menuCtx({ selectedClipIds: ["c1", "c2"], pointerTarget: { kind: "clip", id: "c2", timeMs: 7_000 } });

  it("lists the concept's multi-clip items", () => {
    expect(labels(multiClipMenu(multi()))).toEqual([
      "Copy selection", "Cut selection", "Duplicate selection", "—", "Group selection", "Ungroup", "Fit selection",
      "Fades", "Color treatment", "Mute selection", "Unmute selection", "—", "Clear selection",
      "Delete selection · leave gaps",
    ]);
  });

  it("wires each item", async () => {
    const ctx = multi();
    const items = multiClipMenu(ctx);
    for (const [label, id] of [
      ["Copy selection", "copy"], ["Cut selection", "cut"], ["Duplicate selection", "duplicate"],
      ["Group selection", "group"], ["Delete selection · leave gaps", "delete"],
    ] as const) {
      run(find(items, label));
      expect(vi.mocked(ctx.activate).mock.lastCall?.[0]).toBe(id);
    }
    expect(find(items, "Ungroup").disabledReason).toBe("Select a clip in a group");
    run(find(items, "Fit selection"));
    expect(ctx.fitRange).toHaveBeenCalledWith(1_000, 10_000);
    run(find(items, "Mute selection"));
    run(sub(items, "Color treatment", "Warm"));
    run(sub(items, "Fades", "0.5s · fade both edges"));
    await flushPromises();
    expect(executed(ctx)).toEqual([
      { kind: "setClipMix", clipIds: ["c1", "c2"], muted: true },
      { kind: "setAdjustments", clipIds: ["c1", "c2"], adjustments: { brightness: 1.02, contrast: 1.04, saturation: 1.08, sepia: 0.18, grayscale: 0 } },
      { kind: "setFades", clipId: "c1", fadeInMs: 500, fadeOutMs: 500 },
      { kind: "setFades", clipId: "c2", fadeInMs: 500, fadeOutMs: 500 },
    ]);
    expect(find(items, "Unmute selection").disabledReason).toBe("No selected clip is muted.");
  });
});

// ---- the other sets --------------------------------------------------------------

describe("trackMenu", () => {
  it("lists the track header items and wires flags, moves and removal", async () => {
    const ctx = menuCtx();
    const items = trackMenu(ctx, "v1");
    expect(labels(items)).toEqual([
      "Select clips on this track", "Rename track…", "—", "Visible video", "Mute audio", "Solo audio", "Lock track", "—",
      "Move track up", "Move track down", "Close gaps on this track", "Clear selection", "Remove track…",
    ]);
    expect(find(items, "Visible video").checked).toBe(true);
    expect(find(items, "Lock track").checked).toBe(false);
    run(find(items, "Select clips on this track"));
    expect(ctx.selectClips).toHaveBeenCalledWith(["c1", "c2"]);
    run(find(items, "Rename track…"));
    expect(ctx.renameTrack).toHaveBeenCalledWith("v1");
    run(find(items, "Visible video"));
    run(find(items, "Solo audio"));
    run(find(items, "Move track up"));
    // The later gap closes first, so no clip ever moves onto another.
    run(find(items, "Close gaps on this track"));
    await flushPromises();
    expect(executed(ctx)).toEqual([
      { kind: "setTrackFlags", trackId: "v1", visible: false },
      { kind: "setTrackFlags", trackId: "v1", solo: true },
      { kind: "moveTrack", trackId: "v1", toIndex: 0 },
      { kind: "moveClips", clipIds: ["c2"], deltaMs: -1_000, trackId: null },
      { kind: "moveClips", clipIds: ["c1", "c2"], deltaMs: -1_000, trackId: null },
    ]);
    run(find(items, "Remove track…"));
    expect(ctx.removeTrack).toHaveBeenCalledWith("v1");
    expect(find(items, "Remove track…").danger).toBe(true);
  });

  it("says why a track cannot move further (audit finding 6)", () => {
    expect(find(trackMenu(menuCtx(), "v2"), "Move track up").disabledReason).toBe("Already the top track");
    expect(find(trackMenu(menuCtx(), "v0"), "Move track down").disabledReason).toBe("Track V0 is locked");
    expect(labels(trackMenu(menuCtx(), "a1"))).not.toContain("Visible video");
  });

  // Fix round 1: core refuses every flag change on a locked track except
  // unlocking it.
  it("on a locked track every flag but Lock track says why, and Lock track still unlocks", () => {
    const ctx = menuCtx();
    const items = trackMenu(ctx, "v0");
    for (const label of ["Visible video", "Mute audio", "Solo audio"]) {
      expect(find(items, label).disabledReason).toBe("Track V0 is locked");
    }
    const lock = find(items, "Lock track");
    expect(lock.checked).toBe(true);
    run(lock);
    expect(executed(ctx)).toEqual([{ kind: "setTrackFlags", trackId: "v0", locked: false }]);
  });

  it("an audio track at the bottom cannot move down", () => {
    const ctx = menuCtx({ project: { ...project(), tracks: project().tracks.slice(0, 3) } });
    expect(find(trackMenu(ctx, "a1"), "Move track down").disabledReason).toBe("Already the bottom track");
    expect(find(trackMenu(ctx, "a1"), "Close gaps on this track").disabledReason).toBe("This track has no gaps.");
  });
});

describe("laneMenu", () => {
  it("lists the lane items for a track and wires each one", async () => {
    const ctx = menuCtx({ pointerTarget: { kind: "gap", id: "v1", timeMs: 5_500 } });
    const items = laneMenu(ctx, "v1", 5_500);
    expect(labels(items)).toEqual([
      "Move playhead to 00:05.5", "Paste clips here", "—", "Close this gap · this track", "Close all gaps · this track",
      "Add title here", "Insert intro · shift all tracks", "Add video track", "Add audio track", "Fit timeline",
      "Snapping",
    ]);
    run(find(items, "Move playhead to 00:05.5"));
    expect(ctx.seek).toHaveBeenCalledWith(5_500);
    expect(find(items, "Paste clips here").disabledReason).toBe("Clipboard is empty");
    run(find(items, "Close this gap · this track"));
    run(find(items, "Close all gaps · this track"));
    await flushPromises();
    run(find(items, "Add title here"));
    run(find(items, "Insert intro · shift all tracks"));
    expect(executed(ctx)).toEqual([
      { kind: "moveClips", clipIds: ["c2"], deltaMs: -1_000, trackId: null },
      { kind: "moveClips", clipIds: ["c2"], deltaMs: -1_000, trackId: null },
      { kind: "moveClips", clipIds: ["c1", "c2"], deltaMs: -1_000, trackId: null },
      { kind: "addCard", preset: "chapter", trackId: "v1", startMs: 5_500, durationMs: 3_000, title: "Chapter", subtitle: "" },
      { kind: "insertIntro", durationMs: 3_000, title: "Intro", subtitle: "" },
    ]);
    run(find(items, "Add video track"));
    run(find(items, "Add audio track"));
    expect(vi.mocked(ctx.addTrack).mock.calls).toEqual([["video"], ["audio"]]);
    run(find(items, "Fit timeline"));
    expect(ctx.fitTimeline).toHaveBeenCalled();
    const snap = find(items, "Snapping");
    expect(snap.checked).toBe(true);
    run(snap);
    expect(ctx.toggleSnap).toHaveBeenCalled();
  });

  it("without a track it has no gap items and adds a title on a new track", () => {
    const ctx = menuCtx({ pointerTarget: null });
    const items = laneMenu(ctx, null, 7_000);
    expect(labels(items)).not.toContain("Close this gap · this track");
    run(find(items, "Add title here"));
    expect(executed(ctx)[0]).toMatchObject({ kind: "addCard", trackId: null, startMs: 7_000 });
  });

  it("refuses to close a gap where there is none", () => {
    const items = laneMenu(menuCtx({ pointerTarget: { kind: "gap", id: "v1", timeMs: 3_000 } }), "v1", 3_000);
    expect(find(items, "Close this gap · this track").disabledReason).toBe("There is no gap here.");
  });
});

describe("cueMenu", () => {
  it("edits, duplicates 500 ms later, selects the clip and deletes", () => {
    const ctx = menuCtx();
    const items = cueMenu(ctx, "e1");
    expect(labels(items)).toEqual([
      "Edit annotation", "Duplicate annotation", "Select attached clip", "Clear selection", "Delete annotation",
    ]);
    run(find(items, "Edit annotation"));
    expect(vi.mocked(ctx.selectEffect).mock.calls[0][0].id).toBe("e1");
    run(find(items, "Select attached clip"));
    expect(ctx.selectClips).toHaveBeenCalledWith(["c2"]);
    run(find(items, "Duplicate annotation"));
    run(find(items, "Delete annotation"));
    expect(executed(ctx)).toEqual([
      { kind: "addEffect", clipId: "c2", effectKind: "text", startMs: 1_000, endMs: 2_000, props: { x: 0.2, y: 0.3, w: 0.4, h: 0.1, color: "#fff", text: "Hi" } },
      { kind: "removeEffect", effectId: "e1" },
    ]);
    expect(find(items, "Delete annotation").danger).toBe(true);
  });
});

describe("assetMenu", () => {
  it("adds at the playhead or on a new track", () => {
    const ctx = menuCtx();
    const items = assetMenu(ctx, "vid");
    expect(labels(items)).toEqual(["Add at playhead", "Add on a new track"]);
    run(find(items, "Add at playhead"));
    expect(vi.mocked(ctx.addAssetOnFreeTrack).mock.calls[0]).toEqual([expect.objectContaining({ id: "vid" }), 7_000]);
    run(find(items, "Add on a new track"));
    expect(vi.mocked(ctx.addAssetOnNewTrack).mock.calls[0][0].id).toBe("vid");
  });

  it("offers Reconnect original… for a missing source and refuses to place it", () => {
    const ctx = menuCtx();
    const items = assetMenu(ctx, "gone");
    expect(labels(items)).toEqual(["Add at playhead", "Add on a new track", "Reconnect original…"]);
    expect(find(items, "Add at playhead").disabledReason).toBe("Reconnect the source first.");
    run(find(items, "Reconnect original…"));
    expect(ctx.reconnect).toHaveBeenCalledWith("gone");
  });
});

// ---- the whole-menu rules ----------------------------------------------------------

describe("every menu", () => {
  const OMITTED = [
    "New project…", "Restore sample project…", "Download annotated frame…", "Rename library label…",
    "Remove from library", "Copy / paste look", "Paste look & fades",
  ];

  function allMenus(): MenuItem[][] {
    const ctx = menuCtx();
    return [
      clipMenu(ctx), clipMenu(menuCtx({ pointerTarget: { kind: "clip", id: "c3", timeMs: 500 } })),
      multiClipMenu(menuCtx({ selectedClipIds: ["c1", "c2"] })), trackMenu(ctx, "v1"), trackMenu(ctx, "a1"),
      laneMenu(ctx, "v1", 5_500), laneMenu(ctx, null, 0), cueMenu(ctx, "e1"), assetMenu(ctx, "vid"),
      assetMenu(ctx, "gone"),
    ];
  }

  it("has no item that does nothing: each runs, opens a submenu, or says why not", () => {
    const idle = allMenus()
      .flatMap(flatten)
      .filter((i) => i.run === undefined && i.submenu === undefined && !i.disabledReason);
    expect(idle.map((i) => i.label)).toEqual([]);
  });

  it("never offers an item with no native backend", () => {
    const all = allMenus().flatMap(flatten).map((i) => i.label);
    for (const label of OMITTED) expect(all).not.toContain(label);
  });

  it("gives every item an icon", () => {
    const bare = allMenus()
      .flatMap((m) => m.filter((i): i is MenuAction => !isSeparator(i)))
      .filter((i) => !i.icon);
    expect(bare.map((i) => i.label)).toEqual([]);
  });
});

describe("contextMenuFor — which set a target opens", () => {
  it("a clip opens its own menu headed by its name; a multi-selection says how many", () => {
    expect(contextMenuFor(menuCtx()).heading).toBe("Clip c2");
    const multi = contextMenuFor(menuCtx({ selectedClipIds: ["c1", "c2"] }));
    expect(multi.heading).toBe("2 selected clips");
    expect(labels(multi.items)[0]).toBe("Copy selection");
  });

  it("a lane opens the gap menu, and no target the editor actions", () => {
    expect(contextMenuFor(menuCtx({ pointerTarget: { kind: "gap", id: "v1", timeMs: 5_500 } })).heading).toBe("Timeline gap");
    const none = contextMenuFor(menuCtx({ pointerTarget: null }));
    expect(none.heading).toBe("Editor actions");
    expect(labels(none.items)[0]).toBe("Move playhead to 00:07.0");
  });
});
