/**
 * `src/editor/actions.ts` (the single action registry) and
 * `src/editor/shortcuts.ts` (the pure keyboard-shortcut map) — Task 17.
 * Pure-function tests only; no component is mounted here (that's
 * `editorContextMenu.test.ts`).
 */
import { describe, expect, it } from "vitest";

import {
  ACTION_IDS,
  type ActionContext,
  commandFor,
  resolveActions,
  SHORTCUT_DISPLAY,
  UNIMPLEMENTED_KINDS,
} from "../src/editor/actions";
import {
  isContextMenuShortcut,
  matchShortcut,
  shortcutKey,
  SHORTCUTS,
  shouldHandle,
} from "../src/editor/shortcuts";
import type { Clip, EditorSnapshot, Project, Track } from "../src/editorTypes";

// ---- fixtures ---------------------------------------------------------------
// Deliberately asymmetric spans/positions (the "fixture flaw" rule): c1 and
// c2 have different durations and neither starts at a round multiple of the
// other, so a swapped index or an off-by-one boundary check cannot pass by
// coincidence.

function track(id: string, overrides: Partial<Track> = {}): Track {
  return {
    id,
    kind: "video",
    name: id,
    visible: true,
    locked: false,
    muted: false,
    solo: false,
    volume: 1,
    ...overrides,
  };
}

function clip(id: string, trackId: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "a1",
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
    assets: [{ id: "a1", kind: "video", name: "a1", duration_ms: 30_000 }],
    tracks: [],
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
    durationMs: 30_000,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
    ...overrides,
  };
}

function ctx(overrides: Partial<ActionContext> = {}): ActionContext {
  return {
    project: null,
    snapshot: null,
    playheadMs: 0,
    selectedClipIds: [],
    pointerTarget: null,
    hasClipboard: false,
    clipboardFragment: null,
    ...overrides,
  };
}

// ---- actions.ts -------------------------------------------------------------

describe("resolveActions / commandFor — split (A14)", () => {
  it("split uses the right-clicked time, not the playhead", () => {
    const proj = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1", { start_ms: 0, in_ms: 0, out_ms: 10_000 })],
    });
    const context = ctx({
      project: proj,
      snapshot: snapshot(),
      playheadMs: 1_000,
      pointerTarget: { kind: "clip", id: "c1", timeMs: 4_000 },
    });

    expect(resolveActions(context).split.enabled).toBe(true);
    expect(commandFor("split", context)).toEqual({ kind: "splitClip", clipId: "c1", atMs: 4_000 });
  });

  it("refuses at a clip boundary (the exact start instant, not just outside the span)", () => {
    const proj = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1", { start_ms: 0, in_ms: 0, out_ms: 10_000 })],
    });
    const atStart = ctx({
      project: proj,
      snapshot: snapshot(),
      pointerTarget: { kind: "clip", id: "c1", timeMs: 0 },
    });
    expect(resolveActions(atStart).split).toMatchObject({
      enabled: false,
      reason: "The playhead is at a clip boundary",
    });
    expect(commandFor("split", atStart)).toBeNull();
  });

  it("falls back to the playhead when there is no pointer target", () => {
    const proj = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1", { start_ms: 0, in_ms: 0, out_ms: 10_000 })],
    });
    const context = ctx({
      project: proj,
      snapshot: snapshot(),
      playheadMs: 2_500,
      selectedClipIds: ["c1"],
    });
    expect(commandFor("split", context)).toEqual({ kind: "splitClip", clipId: "c1", atMs: 2_500 });
  });
});

describe("resolveActions — disabled actions carry a reason", () => {
  it("every disabled action in an empty context has a non-empty reason", () => {
    const context = ctx({ project: project(), snapshot: snapshot() });
    const resolved = resolveActions(context);
    // Split into two non-conditional assertions (vitest/no-conditional-expect)
    // rather than branching inside the loop: every disabled action's reason
    // is a non-empty string, and every enabled action's reason is null.
    const disabledReasons = ACTION_IDS.filter((id) => !resolved[id].enabled).map((id) => resolved[id].reason);
    const enabledReasons = ACTION_IDS.filter((id) => resolved[id].enabled).map((id) => resolved[id].reason);
    expect(disabledReasons.length).toBeGreaterThan(0);
    expect(disabledReasons.every((reason) => typeof reason === "string" && reason.length > 0)).toBe(true);
    expect(enabledReasons.every((reason) => reason === null)).toBe(true);
  });

  it("resolves every one of the 38 declared action ids exactly once", () => {
    expect(ACTION_IDS).toHaveLength(38);
    expect(new Set(ACTION_IDS).size).toBe(38);
    expect(Object.keys(resolveActions(ctx())).length).toBe(38);
  });

  it("still-unimplemented wire kinds are gated regardless of selection", () => {
    const proj = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1")],
    });
    const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1"] });
    const resolved = resolveActions(context);
    for (const id of ["addText", "addCaption", "addMarker", "addTrackVideo", "ratio"] as const) {
      expect(resolved[id].enabled).toBe(false);
      expect(commandFor(id, context)).toBeNull();
    }
  });

  // Task 27: `detachAudio` left UNIMPLEMENTED_KINDS with a real resolver and
  // builder. The detached clip lands on the first FREE unlocked audio track
  // (a busy or locked one is skipped), or asks Rust for a new one.
  it("detachAudio targets the first free unlocked audio track, else asks for a new one", () => {
    const tracks = [
      track("v1"),
      track("busy", { kind: "audio" }),
      track("locked", { kind: "audio", locked: true }),
      track("free", { kind: "audio" }),
    ];
    // c1 plays 2000..3300 (in 400, out 1700); the music on `busy` at
    // 3200..4200 overlaps its last 100 ms, so `busy` is not free.
    const c1 = clip("c1", "v1", { start_ms: 2_000, in_ms: 400, out_ms: 1_700 });
    const music = clip("m", "busy", { asset_id: "m1", start_ms: 3_200 });
    const assets = [
      { id: "a1", kind: "video" as const, name: "a1", duration_ms: 30_000 },
      { id: "m1", kind: "audio" as const, name: "m1", duration_ms: 30_000 },
    ];
    const context = ctx({ project: project({ tracks, clips: [c1, music], assets }), snapshot: snapshot(), selectedClipIds: ["c1"] });
    expect(resolveActions(context).detachAudio).toMatchObject({ enabled: true, reason: null });
    expect(commandFor("detachAudio", context)).toEqual({ kind: "detachAudio", clipId: "c1", audioTrackId: "free" });

    const noFree = ctx({
      project: project({ tracks: tracks.slice(0, 3), clips: [c1, music], assets }),
      snapshot: snapshot(),
      selectedClipIds: ["c1"],
    });
    expect(commandFor("detachAudio", noFree)).toEqual({ kind: "detachAudio", clipId: "c1", audioTrackId: null });
  });

  it("detachAudio is refused, with a reason, for a still, an audio clip or an already-detached clip", () => {
    const still = project({
      assets: [{ id: "a1", kind: "video", name: "a1", duration_ms: 5_000, media_type: "image" }],
      tracks: [track("v1")],
      clips: [clip("c1", "v1")],
    });
    const detached = project({
      assets: [
        { id: "a1", kind: "video", name: "a1", duration_ms: 30_000 },
        { id: "a1-audio", kind: "audio", name: "a1 · audio", duration_ms: 30_000, linked_asset: "a1" },
      ],
      tracks: [track("v1"), track("au", { kind: "audio" })],
      clips: [clip("c1", "v1", { muted: true }), clip("d", "au", { asset_id: "a1-audio" })],
    });
    for (const [proj, reason] of [
      [still, "Only a video clip's audio can be detached"],
      [detached, "This clip's audio is already detached"],
    ] as const) {
      const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1"] });
      expect(resolveActions(context).detachAudio).toMatchObject({ enabled: false, reason });
      expect(commandFor("detachAudio", context)).toBeNull();
    }
  });

  // Task 29: fadeIn/fadeOut's own quick-toggle command builder, independent
  // of FadesSection's numeric drafts and ClipItem's handle drag (neither
  // reads this registry at all).
  it("fadeIn/fadeOut toggle a default fade on and off, clamped to half the clip's duration", () => {
    const proj = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1", { out_ms: 1_000 })], // 1000ms duration, half 500ms
    });
    const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1"] });
    expect(resolveActions(context).fadeIn).toMatchObject({ enabled: true, reason: null });
    expect(commandFor("fadeIn", context)).toEqual({ kind: "setFades", clipId: "c1", fadeInMs: 500 });
    expect(commandFor("fadeOut", context)).toEqual({ kind: "setFades", clipId: "c1", fadeOutMs: 500 });

    const alreadyFaded = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1", { out_ms: 1_000, fade_in_ms: 200, fade_out_ms: 200 })],
    });
    const faded = ctx({ project: alreadyFaded, snapshot: snapshot(), selectedClipIds: ["c1"] });
    expect(commandFor("fadeIn", faded)).toEqual({ kind: "setFades", clipId: "c1", fadeInMs: 0 });
    expect(commandFor("fadeOut", faded)).toEqual({ kind: "setFades", clipId: "c1", fadeOutMs: 0 });
  });

  it("fadeIn's default clamps below 500ms on a short clip, and a locked track refuses both", () => {
    const short = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1", { out_ms: 300 })], // 300ms duration, half 150ms < the 500ms default
    });
    const context = ctx({ project: short, snapshot: snapshot(), selectedClipIds: ["c1"] });
    expect(commandFor("fadeIn", context)).toEqual({ kind: "setFades", clipId: "c1", fadeInMs: 150 });

    const locked = project({ tracks: [track("v1", { locked: true })], clips: [clip("c1", "v1")] });
    const lockedCtx = ctx({ project: locked, snapshot: snapshot(), selectedClipIds: ["c1"] });
    expect(resolveActions(lockedCtx).fadeIn).toMatchObject({ enabled: false, reason: "Track v1 is locked" });
    expect(commandFor("fadeIn", lockedCtx)).toBeNull();
    expect(commandFor("fadeOut", lockedCtx)).toBeNull();
  });

  it("fix round 1, finding 1: a gated action's reason is human copy, never the raw Rust wire kind", () => {
    // A button labelled "Text" must never show the literal string
    // "addEffect" -- that is Rust's own error vocabulary
    // (core::editor::commands::mod.rs), not a word the person reading a
    // teaching-tool button typed or clicked. The expected sentence is
    // built from each action's OWN label, in the same register as
    // EditorHeader's "Rendering a video arrives in a later update."
    const context = ctx({ project: project(), snapshot: snapshot() });
    const resolved = resolveActions(context);
    const expectedReasons: Partial<Record<string, string>> = {
      addText: "Text arrives in a later update.",
      addArrow: "Arrow arrives in a later update.",
      addCaption: "Add caption arrives in a later update.",
      addMarker: "Add marker arrives in a later update.",
      addTrackVideo: "Add video track arrives in a later update.",
      ratio: "Aspect ratio arrives in a later update.",
    };
    for (const [id, expected] of Object.entries(expectedReasons)) {
      expect(resolved[id as keyof typeof resolved].reason).toBe(expected);
      // The raw wire kind never leaks into the sentence at all.
      expect(resolved[id as keyof typeof resolved].reason).not.toMatch(
        /addEffect|addCaption|addMarker|addTrack\b|setFades|addTransition|is not available yet/,
      );
    }
  });

  // Task 26: `addTrack` is no longer in `UNIMPLEMENTED_KINDS` (dropping a
  // media asset below the timeline's last lane sends it directly through
  // `editorProject.execute`, the `TrackHeader.vue` direct-call precedent) --
  // but `addTrackVideo`/`addTrackAudio`, the only `ActionId`s that map to
  // it, still have no keyboard/menu/toolbar surface consuming them, so
  // `actions.ts` now gives them their OWN `RESOLVERS` entry reproducing the
  // exact same disabled-with-reason text `UNIMPLEMENTED_KINDS` used to
  // supply, rather than silently reporting `enabled:false, reason:null`
  // (which the "every disabled action carries a reason" invariant below
  // would catch).
  it("addTrackVideo/addTrackAudio stay disabled with the same reason even though addTrack itself is ungated", () => {
    const context = ctx({ project: project(), snapshot: snapshot() });
    const resolved = resolveActions(context);
    expect(resolved.addTrackVideo.enabled).toBe(false);
    expect(resolved.addTrackVideo.reason).toBe("Add video track arrives in a later update.");
    expect(resolved.addTrackAudio.enabled).toBe(false);
    expect(resolved.addTrackAudio.reason).toBe("Add audio track arrives in a later update.");
    expect(commandFor("addTrackVideo", context)).toBeNull();
    expect(commandFor("addTrackAudio", context)).toBeNull();
  });
});

describe("resolveActions — locked track disables every clip mutation", () => {
  const LOCK_REASON = "Track Screen is locked";

  function lockedProject(): Project {
    return project({
      tracks: [track("v1", { name: "Screen", locked: true })],
      clips: [
        clip("c1", "v1", { start_ms: 0, in_ms: 0, out_ms: 1_000, group_id: "g1" }),
        clip("c2", "v1", { start_ms: 1_500, in_ms: 0, out_ms: 500, group_id: "g1" }),
      ],
    });
  }

  it("disables split, delete, deleteClose, cut, duplicate, earlier, later with the lock reason", () => {
    const proj = lockedProject();
    const pointerAt = (id: string, timeMs: number | null = null): ActionContext =>
      ctx({ project: proj, snapshot: snapshot(), pointerTarget: { kind: "clip", id, timeMs } });

    expect(resolveActions(pointerAt("c1", 500)).split).toMatchObject({ enabled: false, reason: LOCK_REASON });
    expect(resolveActions(pointerAt("c1")).delete).toMatchObject({ enabled: false, reason: LOCK_REASON });
    expect(resolveActions(pointerAt("c1")).deleteClose).toMatchObject({ enabled: false, reason: LOCK_REASON });
    expect(resolveActions(pointerAt("c1")).cut).toMatchObject({ enabled: false, reason: LOCK_REASON });
    expect(resolveActions(pointerAt("c1")).duplicate).toMatchObject({ enabled: false, reason: LOCK_REASON });
    expect(resolveActions(pointerAt("c1")).earlier).toMatchObject({ enabled: false, reason: LOCK_REASON });
    expect(resolveActions(pointerAt("c2")).later).toMatchObject({ enabled: false, reason: LOCK_REASON });

    for (const id of ["split", "delete", "deleteClose", "cut", "duplicate", "earlier"] as const) {
      expect(commandFor(id, pointerAt("c1", 500))).toBeNull();
    }
  });

  it("disables group and ungroup with the lock reason too", () => {
    const proj = lockedProject();
    const selected = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1", "c2"] });
    expect(resolveActions(selected).group).toMatchObject({ enabled: false, reason: LOCK_REASON });
    expect(commandFor("group", selected)).toBeNull();

    const pointerOnGroupMember = ctx({
      project: proj,
      snapshot: snapshot(),
      pointerTarget: { kind: "clip", id: "c1", timeMs: null },
    });
    expect(resolveActions(pointerOnGroupMember).ungroup).toMatchObject({ enabled: false, reason: LOCK_REASON });
    expect(commandFor("ungroup", pointerOnGroupMember)).toBeNull();
  });

  it("copy is read-only and stays enabled on a locked track", () => {
    const proj = lockedProject();
    const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1"] });
    expect(resolveActions(context).copy).toMatchObject({ enabled: true, reason: null });
  });
});

describe("resolveActions/commandFor — the rest of the implemented commands", () => {
  it("undo/redo reflect the snapshot flags and carry the snapshot's own label", () => {
    const enabled = ctx({ snapshot: snapshot({ canUndo: true, canRedo: true, undoLabel: "Split", redoLabel: "Delete" }) });
    expect(resolveActions(enabled).undo).toMatchObject({ enabled: true, reason: null, label: "Undo Split" });
    expect(resolveActions(enabled).redo).toMatchObject({ enabled: true, reason: null, label: "Redo Delete" });
    expect(commandFor("undo", enabled)).toEqual({ kind: "undo" });
    expect(commandFor("redo", enabled)).toEqual({ kind: "redo" });

    const disabled = ctx({ snapshot: snapshot({ canUndo: false, canRedo: false }) });
    expect(resolveActions(disabled).undo).toMatchObject({ enabled: false, reason: "Nothing to undo", label: "Undo" });
    expect(resolveActions(disabled).redo).toMatchObject({ enabled: false, reason: "Nothing to redo", label: "Redo" });
  });

  it("group requires at least two clips", () => {
    const proj = project({ tracks: [track("v1")], clips: [clip("c1", "v1")] });
    const one = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1"] });
    expect(resolveActions(one).group).toMatchObject({ enabled: false, reason: "Select at least two clips" });

    const proj2 = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1"), clip("c2", "v1", { start_ms: 1_000 })],
    });
    const two = ctx({ project: proj2, snapshot: snapshot(), selectedClipIds: ["c1", "c2"] });
    expect(resolveActions(two).group.enabled).toBe(true);
    expect(commandFor("group", two)).toEqual({ kind: "groupClips", clipIds: ["c1", "c2"] });
  });

  it("ungroup resolves the group id from whichever target clip carries one", () => {
    const proj = project({
      tracks: [track("v1")],
      clips: [
        clip("c1", "v1", { group_id: "g1" }),
        clip("c2", "v1", { start_ms: 1_000, group_id: "g1" }),
      ],
    });
    const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c2"] });
    expect(commandFor("ungroup", context)).toEqual({ kind: "ungroupClips", groupId: "g1" });

    const noGroup = ctx({ project: proj, snapshot: snapshot() });
    expect(resolveActions(noGroup).ungroup).toMatchObject({ enabled: false, reason: "Select a clip in a group" });

    // Multiple clips selected (no single-clip pointer target) exercises the
    // targetClipIds() fallback inside targetGroupId, not primaryTargetClip's
    // single-selection branch.
    const multiSelected = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1", "c2"] });
    expect(commandFor("ungroup", multiSelected)).toEqual({ kind: "ungroupClips", groupId: "g1" });
  });

  it("duplicate offsets by the SELECTION's own span, not any one clip's duration (fix round 1, finding 2)", () => {
    // c1 ends at 2000, c2 spans 3000..3500 -- the OLD (buggy) offset was
    // the longest target clip's own duration (2000), which would shift
    // c1's duplicate to 2000..4000 and overlap c2's UNTOUCHED original at
    // 3000..3500. Rust's `duplicateClips` adds this ONE offsetMs to every
    // selected clip's start_ms uniformly and then refuses the whole
    // command on any resulting overlap (`check_no_overlap`) -- so that
    // offset would have been refused. The correct offset is the
    // selection's own span: max(every target's output end) - min(every
    // target's start) = 3500 - 0 = 3500, which clears every original in
    // the selection regardless of the gap between them.
    const proj = project({
      tracks: [track("v1")],
      clips: [
        clip("c1", "v1", { start_ms: 0, in_ms: 0, out_ms: 2_000 }),
        clip("c2", "v1", { start_ms: 3_000, in_ms: 0, out_ms: 500 }),
      ],
    });
    const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1", "c2"] });
    expect(commandFor("duplicate", context)).toEqual({
      kind: "duplicateClips",
      clipIds: ["c1", "c2"],
      offsetMs: 3_500,
    });
  });

  it("duplicate's span offset still holds with a wide gap between the selected clips", () => {
    // A clip's own output end is start_ms + (out_ms - in_ms), NOT out_ms
    // alone -- this fixture deliberately gives c1 a nonzero start_ms so a
    // formula that forgot to add it back in (or that used out_ms as if it
    // were already an absolute end) would fail here even though the
    // task's OTHER duplicate test (start_ms: 0) could not catch it.
    const proj = project({
      tracks: [track("v1")],
      clips: [
        clip("c1", "v1", { start_ms: 100, in_ms: 0, out_ms: 200 }), // ends 100+200=300
        clip("c2", "v1", { start_ms: 900, in_ms: 0, out_ms: 500 }), // ends 900+500=1400, gap 300..900
      ],
    });
    const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1", "c2"] });
    // minStart(100) .. maxEnd(1400) => span 1300, regardless of the 600ms
    // gap sitting inside the selection.
    expect(commandFor("duplicate", context)).toEqual({
      kind: "duplicateClips",
      clipIds: ["c1", "c2"],
      offsetMs: 1_300,
    });
  });

  it("duplicate falls back to a 0 offset rather than -Infinity when no target id resolves (Task 20 carried finding)", () => {
    // resolveClipMutation only checks targetClipIds(ctx).length > 0 -- it
    // never confirms those ids still resolve against ctx.project.clips. A
    // stale id (nothing named "ghost" exists in this project) makes
    // buildDuplicate's own targetClips filter come back empty while `ids`
    // itself is non-empty, so `Math.min(...[])`/`Math.max(...[])` would be
    // Infinity/-Infinity without the guard -- offsetMs must be the
    // documented 0 fallback, never a non-finite number sent over IPC.
    const proj = project({ tracks: [track("v1")], clips: [] });
    const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["ghost"] });

    expect(resolveActions(context).duplicate.enabled).toBe(true);
    expect(commandFor("duplicate", context)).toEqual({
      kind: "duplicateClips",
      clipIds: ["ghost"],
      offsetMs: 0,
    });
  });

  it("earlier/later refuse at the ends of the track's own clip order", () => {
    const proj = project({
      tracks: [track("v1")],
      clips: [
        clip("c1", "v1", { start_ms: 0, out_ms: 500 }),
        clip("c2", "v1", { start_ms: 700, out_ms: 200 }),
      ],
    });
    const first = ctx({ project: proj, snapshot: snapshot(), pointerTarget: { kind: "clip", id: "c1", timeMs: null } });
    expect(resolveActions(first).earlier).toMatchObject({
      enabled: false,
      reason: "Already the first clip on this track",
    });
    expect(resolveActions(first).later.enabled).toBe(true);
    expect(commandFor("later", first)).toEqual({ kind: "reorderClip", clipId: "c1", direction: "later" });

    const last = ctx({ project: proj, snapshot: snapshot(), pointerTarget: { kind: "clip", id: "c2", timeMs: null } });
    expect(resolveActions(last).later).toMatchObject({
      enabled: false,
      reason: "Already the last clip on this track",
    });
  });

  it("cut and delete both build deleteClips-shaped commands, cut always closing the gap", () => {
    const proj = project({ tracks: [track("v1")], clips: [clip("c1", "v1")] });
    const context = ctx({ project: proj, snapshot: snapshot(), selectedClipIds: ["c1"] });
    expect(commandFor("delete", context)).toEqual({ kind: "deleteClips", clipIds: ["c1"], closeGap: false });
    expect(commandFor("deleteClose", context)).toEqual({ kind: "deleteClips", clipIds: ["c1"], closeGap: true });
    expect(commandFor("cut", context)).toEqual({ kind: "cutClips", clipIds: ["c1"], closeGap: true });
  });

  it("paste needs clipboard content AND a target track, and prefers the pointer's own time", () => {
    const proj = project({ tracks: [track("v1")] });
    const fragment = { clips: [], effects: [], captions: [], markers: [], originMs: 0 };

    const noClipboard = ctx({ project: proj, snapshot: snapshot(), pointerTarget: { kind: "track", id: "v1", timeMs: 100 } });
    expect(resolveActions(noClipboard).paste).toMatchObject({ enabled: false, reason: "Clipboard is empty" });

    const noTarget = ctx({ project: proj, snapshot: snapshot(), hasClipboard: true, clipboardFragment: fragment });
    expect(resolveActions(noTarget).paste).toMatchObject({ enabled: false, reason: "Select a track first" });

    const ready = ctx({
      project: proj,
      snapshot: snapshot(),
      playheadMs: 999,
      hasClipboard: true,
      clipboardFragment: fragment,
      pointerTarget: { kind: "track", id: "v1", timeMs: 250 },
    });
    expect(resolveActions(ready).paste).toMatchObject({ enabled: true, reason: null });
    expect(commandFor("paste", ready)).toEqual({ kind: "pasteFragment", fragment, trackId: "v1", atMs: 250 });

    // A pointer target on a CLIP (not a track/gap) resolves its own track --
    // the second way `targetTrackId` can name a track.
    const projWithClip = project({
      tracks: [track("v1")],
      clips: [clip("c1", "v1")],
    });
    const pointerOnClip = ctx({
      project: projWithClip,
      snapshot: snapshot(),
      hasClipboard: true,
      clipboardFragment: fragment,
      pointerTarget: { kind: "clip", id: "c1", timeMs: 50 },
    });
    expect(commandFor("paste", pointerOnClip)).toEqual({ kind: "pasteFragment", fragment, trackId: "v1", atMs: 50 });

    // A pointer target of a kind that names no track at all (e.g. an
    // effect) falls through to "no target".
    const pointerOnEffect = ctx({
      project: proj,
      snapshot: snapshot(),
      hasClipboard: true,
      clipboardFragment: fragment,
      pointerTarget: { kind: "effect", id: "e1", timeMs: 50 },
    });
    expect(resolveActions(pointerOnEffect).paste).toMatchObject({ enabled: false, reason: "Select a track first" });
  });

  it("actions with no wire command always build a null command", () => {
    const context = ctx({ project: project(), snapshot: snapshot() });
    for (const id of ["copy", "save", "render", "checks", "help", "importMedia", "webcam", "toggleLibrary", "toggleInspector", "focusPreview", "ratio"] as const) {
      expect(commandFor(id, context)).toBeNull();
    }
  });
});

describe("UNIMPLEMENTED_KINDS", () => {
  it("carries exactly the 18 kinds this registry still gates", () => {
    expect(UNIMPLEMENTED_KINDS.size).toBe(18);
    // The ten kinds an ActionId in this registry maps to that ARE
    // implemented must be absent, or every action built on them would be
    // wrongly gated -- plus the four track kinds Task 23 implemented that
    // map to NO ActionId (renameTrack/moveTrack/setTrackFlags/deleteTrack:
    // TrackHeader.vue calls editorProject.execute directly, never through
    // this registry).
    for (const implemented of [
      "undo", "redo", "splitClip", "deleteClips", "cutClips", "pasteFragment",
      "duplicateClips", "groupClips", "ungroupClips", "reorderClip",
      "renameTrack", "moveTrack", "setTrackFlags", "deleteTrack",
      // Task 27: the three mix kinds, each with a consumer (AudioSection/
      // MixerPopover, and the detachAudio action).
      "setClipMix", "setMasterGain", "detachAudio",
      // Task 29: fadeIn/fadeOut's own quick-toggle resolver/builder.
      "setFades",
      // Task 30: the transition action, and TransitionRow's two direct calls.
      "addTransition", "setTransitionDuration", "removeTransition",
    ]) {
      expect(UNIMPLEMENTED_KINDS.has(implemented)).toBe(false);
    }
    expect(UNIMPLEMENTED_KINDS.has("addEffect")).toBe(true);
    // Task 26: `addTrack` is no longer gated -- dropping a media asset
    // below the timeline's last lane sends it directly
    // (`TimelineView.vue`'s `editorProject.execute`, the `TrackHeader.vue`
    // direct-call precedent), which is a real UI consumer even though
    // `addTrackVideo`/`addTrackAudio` (the only ActionIds that map to it)
    // still have no RESOLVERS/BUILDERS entry of their own -- see
    // actionMeta.ts's own doc on this constant.
    expect(UNIMPLEMENTED_KINDS.has("addTrack")).toBe(false);
  });
});

// ---- shortcuts.ts -------------------------------------------------------------

function dispatchAndCapture(target: HTMLElement, event: KeyboardEvent, opts?: { menuOwnsKeys?: boolean }): boolean {
  let captured: boolean | null = null;
  target.addEventListener("keydown", (e) => (captured = shouldHandle(e as KeyboardEvent, opts)), { once: true });
  target.dispatchEvent(event);
  return captured as unknown as boolean;
}

describe("shouldHandle — shortcuts are ignored inside text inputs", () => {
  it("returns false for an <input>, a <textarea> and a contenteditable region", () => {
    const input = document.createElement("input");
    document.body.appendChild(input);
    expect(dispatchAndCapture(input, new KeyboardEvent("keydown", { key: "s", bubbles: true }))).toBe(false);
    input.remove();

    const textarea = document.createElement("textarea");
    document.body.appendChild(textarea);
    expect(dispatchAndCapture(textarea, new KeyboardEvent("keydown", { key: "s", bubbles: true }))).toBe(false);
    textarea.remove();

    const editable = document.createElement("div");
    Object.defineProperty(editable, "isContentEditable", { value: true });
    document.body.appendChild(editable);
    expect(dispatchAndCapture(editable, new KeyboardEvent("keydown", { key: "s", bubbles: true }))).toBe(false);
    editable.remove();
  });

  it("returns true for a plain element, and false whenever a menu already owns the keys", () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    expect(dispatchAndCapture(div, new KeyboardEvent("keydown", { key: "s", bubbles: true }))).toBe(true);
    expect(
      dispatchAndCapture(div, new KeyboardEvent("keydown", { key: "s", bubbles: true }), { menuOwnsKeys: true }),
    ).toBe(false);
    div.remove();
  });

  it("treats a null target as handleable (a synthetic/global event)", () => {
    const event = new KeyboardEvent("keydown", { key: "s" });
    expect(shouldHandle(event)).toBe(true);
  });
});

describe("matchShortcut / shortcutKey", () => {
  it("matches every documented binding to its action id", () => {
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "s" }))).toBe("split");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "Delete" }))).toBe("delete");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "Backspace" }))).toBe("delete");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "Delete", shiftKey: true }))).toBe("deleteClose");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "z", ctrlKey: true }))).toBe("undo");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, shiftKey: true }))).toBe("redo");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "y", ctrlKey: true }))).toBe("redo");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "c", ctrlKey: true }))).toBe("copy");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "x", ctrlKey: true }))).toBe("cut");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "v", ctrlKey: true }))).toBe("paste");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "d", ctrlKey: true }))).toBe("duplicate");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "g", ctrlKey: true }))).toBe("group");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "g", ctrlKey: true, shiftKey: true }))).toBe("ungroup");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "s", ctrlKey: true }))).toBe("save");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "e", ctrlKey: true }))).toBe("render");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "F1" }))).toBe("help");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "?" }))).toBe("help");
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "F6" }))).toBe("focusPreview");
  });

  it("does not double-apply shift for an already-shifted punctuation character", () => {
    // Real browsers report shiftKey: true alongside key: "?" -- the shifted
    // character is already folded into `key`, so the combo must stay "?",
    // never "shift+?".
    expect(shortcutKey(new KeyboardEvent("keydown", { key: "?", shiftKey: true }))).toBe("?");
  });

  it("returns null for an unbound combo", () => {
    expect(matchShortcut(new KeyboardEvent("keydown", { key: "q" }))).toBeNull();
  });

  it("SHORTCUTS and SHORTCUT_DISPLAY only ever name a real ActionId", () => {
    for (const actionId of SHORTCUTS.values()) {
      expect(ACTION_IDS).toContain(actionId);
    }
    for (const actionId of Object.keys(SHORTCUT_DISPLAY)) {
      expect(ACTION_IDS).toContain(actionId);
    }
  });
});

describe("isContextMenuShortcut", () => {
  it("matches Shift+F10 and the Menu key, and nothing else", () => {
    expect(isContextMenuShortcut(new KeyboardEvent("keydown", { key: "F10", shiftKey: true }))).toBe(true);
    expect(isContextMenuShortcut(new KeyboardEvent("keydown", { key: "ContextMenu" }))).toBe(true);
    expect(isContextMenuShortcut(new KeyboardEvent("keydown", { key: "F10" }))).toBe(false);
    expect(isContextMenuShortcut(new KeyboardEvent("keydown", { key: "s" }))).toBe(false);
  });
});
