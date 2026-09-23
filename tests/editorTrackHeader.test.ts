/**
 * `TrackHeader.vue` (Task 23; F-06) and `TrackLane.vue`'s locked-track
 * clip-body gating. `TrackHeader` calls `editorProject.execute` directly
 * (the `ClipItem.vue`/`TimelineToolbar.vue` precedent), so every assertion
 * here reads the `executed` array a fake `EditorPort.execute` records —
 * the `editorTimelineView.test.ts` fixture/mount shape, trimmed to what
 * this file needs.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import TrackHeader from "../src/components/editor/timeline/TrackHeader.vue";
import TrackLane from "../src/components/editor/timeline/TrackLane.vue";
import { lockedReason } from "../src/editor/actionMeta";
import type { EditorPort } from "../src/editor/port";
import type { Asset, Clip, EditorCommand, EditorOpenResult, EditorSnapshot, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

// ---- fixtures ---------------------------------------------------------------
// Asymmetric on purpose (the "fixture flaw" rule): distinct kinds/names/
// indices/volumes so a swapped field cannot pass by coincidence.

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}

function asset(id: string, kind: "video" | "audio"): Asset {
  return { id, kind, name: id, duration_ms: 60_000 };
}

function clip(id: string, trackId: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "video-asset",
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
    assets: [asset("video-asset", "video")],
    tracks: [track("v1"), track("v2")],
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

function fakePort(overrides: Partial<EditorPort> = {}): EditorPort {
  const unimplemented = (name: string) => (): never => {
    throw new Error(`fakePort.${name} not stubbed for this test`);
  };
  return {
    openStaged: unimplemented("openStaged"),
    openProject: unimplemented("openProject"),
    listProjects: unimplemented("listProjects"),
    getSnapshot: unimplemented("getSnapshot"),
    execute: unimplemented("execute"),
    save: unimplemented("save"),
    closeSession: unimplemented("closeSession"),
    hideWindow: unimplemented("hideWindow"),
    getWorkspace: unimplemented("getWorkspace"),
    saveWorkspace: unimplemented("saveWorkspace"),
    mediaUrl: unimplemented("mediaUrl"),
    ...overrides,
  };
}

let executed: EditorCommand[] = [];

/** Opens a live session so `editorProject.execute` has a `sessionId`/
 * `snapshot` to send against — `editorTimelineView.test.ts`'s own
 * `openProject` helper. The fake reply always echoes the SAME `project`
 * object back (it never actually applies the command), which is fine: every
 * test here asserts against `executed`, never against a re-rendered store
 * projection. */
async function openProject(overrides: Partial<Project> = {}) {
  const store = useEditorProjectStore();
  const p = project(overrides);
  const s = snapshot();
  store.setPort(
    fakePort({
      openStaged: () =>
        Promise.resolve<EditorOpenResult>({
          snapshot: s,
          project: p,
          workspace: {},
          missing: [],
          sourceBase: "base",
          recovered: false,
        }),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...s, revision: s.revision + 1 }, project: p });
      },
    }),
  );
  await store.openStaged("base");
  return { store, project: p };
}

function lastCommand(): EditorCommand {
  const cmd = executed[executed.length - 1];
  if (!cmd) throw new Error("no command was executed");
  return cmd;
}

// ---- TrackHeader: lock ------------------------------------------------------

describe("TrackHeader — lock", () => {
  it("lock toggles through setTrackFlags", async () => {
    executed = [];
    await openProject();
    const unlocked = track("v1", { locked: false });
    const w = mount(TrackHeader, { props: { track: unlocked, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    await w.get('[data-testid="track-header-v1-lock"]').trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", locked: true });

    const locked = track("v1", { locked: true });
    const w2 = mount(TrackHeader, { props: { track: locked, trackIndex: 0, trackCount: 2 } });
    await flushPromises();
    await w2.get('[data-testid="track-header-v1-lock"]').trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", locked: false });
  });

  it("aria-pressed on the lock button reflects the current locked state", async () => {
    executed = [];
    await openProject();
    const locked = track("v1", { locked: true });
    const w = mount(TrackHeader, { props: { track: locked, trackIndex: 0, trackCount: 2 } });
    expect(w.get('[data-testid="track-header-v1-lock"]').attributes("aria-pressed")).toBe("true");
  });

  it("every OTHER control is refused (no command sent) while the track is locked, with the shared registry reason", async () => {
    executed = [];
    await openProject();
    const locked = track("a1", { kind: "audio", locked: true, name: "Ambient" });
    const w = mount(TrackHeader, { props: { track: locked, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    await w.get('[data-testid="track-header-a1-mute"]').trigger("click");
    await w.get('[data-testid="track-header-a1-solo"]').trigger("click");
    expect(executed).toEqual([]);

    const reason = lockedReason("Ambient");
    expect(w.get('[data-testid="track-header-a1-mute"]').attributes("title")).toBe(reason);
    expect(w.get('[data-testid="track-header-a1-solo"]').attributes("title")).toBe(reason);
    expect(w.get('[data-testid="track-header-a1-mute"]').attributes("aria-disabled")).toBe("true");

    // Rename is refused too: clicking the name never opens the edit input.
    await w.get('[data-testid="track-header-a1-name"]').trigger("click");
    expect(w.find('[data-testid="track-header-a1-name-input"]').exists()).toBe(false);

    // The track menu's Move up/Move down/Delete are all refused.
    await w.get('[data-testid="track-header-a1-menu"]').trigger("click");
    await w.get('[data-testid="track-header-a1-move-down"]').trigger("click");
    await w.get('[data-testid="track-header-a1-menu"]').trigger("click");
    await w.get('[data-testid="track-header-a1-delete"]').trigger("click");
    expect(executed).toEqual([]);
  });
});

// ---- TrackHeader: visibility / mute / solo / volume -------------------------

describe("TrackHeader — eye / mute / solo / volume", () => {
  it("the eye toggles visible and renders only for a video track", async () => {
    executed = [];
    await openProject();
    const video = track("v1", { kind: "video", visible: true });
    const w = mount(TrackHeader, { props: { track: video, trackIndex: 0, trackCount: 2 } });
    await flushPromises();
    await w.get('[data-testid="track-header-v1-visible"]').trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", visible: false });

    const audio = track("a1", { kind: "audio" });
    const w2 = mount(TrackHeader, { props: { track: audio, trackIndex: 0, trackCount: 2 } });
    expect(w2.find('[data-testid="track-header-a1-visible"]').exists()).toBe(false);
  });

  it("mute and solo toggle independently", async () => {
    executed = [];
    await openProject();
    const t = track("v1", { muted: false, solo: true });
    const w = mount(TrackHeader, { props: { track: t, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    await w.get('[data-testid="track-header-v1-mute"]').trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", muted: true });

    await w.get('[data-testid="track-header-v1-solo"]').trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", solo: false });
  });

  it("the volume slider renders only for an audio track and sends its new value on change", async () => {
    executed = [];
    await openProject();
    const audio = track("a1", { kind: "audio", volume: 0.6 });
    const w = mount(TrackHeader, { props: { track: audio, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    const slider = w.get<HTMLInputElement>('[data-testid="track-header-a1-volume"]');
    expect(slider.element.value).toBe("0.6");
    slider.element.value = "1.5";
    await slider.trigger("change");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "a1", volume: 1.5 });

    const video = track("v1", { kind: "video" });
    const w2 = mount(TrackHeader, { props: { track: video, trackIndex: 0, trackCount: 2 } });
    expect(w2.find('[data-testid="track-header-v1-volume"]').exists()).toBe(false);
  });
});

// ---- TrackHeader: rename ------------------------------------------------------

describe("TrackHeader — rename", () => {
  it("commits a trimmed, changed name via renameTrack on Enter", async () => {
    executed = [];
    await openProject();
    const t = track("v1", { name: "Webcam" });
    const w = mount(TrackHeader, { props: { track: t, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    await w.get('[data-testid="track-header-v1-name"]').trigger("click");
    const input = w.get<HTMLInputElement>('[data-testid="track-header-v1-name-input"]');
    await input.setValue("  Screen  ");
    await input.trigger("keydown.enter");
    expect(lastCommand()).toEqual({ kind: "renameTrack", trackId: "v1", name: "Screen" });
  });

  it("sends nothing when the trimmed name is unchanged or empty", async () => {
    executed = [];
    await openProject();
    const t = track("v1", { name: "Webcam" });
    const w = mount(TrackHeader, { props: { track: t, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    await w.get('[data-testid="track-header-v1-name"]').trigger("click");
    await w.get<HTMLInputElement>('[data-testid="track-header-v1-name-input"]').setValue("  Webcam  ");
    await w.get('[data-testid="track-header-v1-name-input"]').trigger("keydown.enter");
    expect(executed).toEqual([]);

    await w.get('[data-testid="track-header-v1-name"]').trigger("click");
    await w.get<HTMLInputElement>('[data-testid="track-header-v1-name-input"]').setValue("   ");
    await w.get('[data-testid="track-header-v1-name-input"]').trigger("keydown.enter");
    expect(executed).toEqual([]);
  });

  it("Escape cancels without sending anything", async () => {
    executed = [];
    await openProject();
    const t = track("v1", { name: "Webcam" });
    const w = mount(TrackHeader, { props: { track: t, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    await w.get('[data-testid="track-header-v1-name"]').trigger("click");
    await w.get<HTMLInputElement>('[data-testid="track-header-v1-name-input"]').setValue("Discarded");
    await w.get('[data-testid="track-header-v1-name-input"]').trigger("keydown.escape");
    expect(executed).toEqual([]);
    expect(w.find('[data-testid="track-header-v1-name-input"]').exists()).toBe(false);
  });

  it("an external rename (an undo, a landing edit from elsewhere) updates the displayed name when NOT mid-edit", async () => {
    executed = [];
    await openProject();
    const t = track("v1", { name: "Webcam" });
    const w = mount(TrackHeader, { props: { track: t, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    await w.setProps({ track: track("v1", { name: "Screen" }) });
    expect(w.get('[data-testid="track-header-v1-name"]').text()).toBe("Screen");
  });

  it("an external rename does NOT clobber a live in-progress edit", async () => {
    executed = [];
    await openProject();
    const t = track("v1", { name: "Webcam" });
    const w = mount(TrackHeader, { props: { track: t, trackIndex: 0, trackCount: 2 } });
    await flushPromises();

    await w.get('[data-testid="track-header-v1-name"]').trigger("click");
    await w.get<HTMLInputElement>('[data-testid="track-header-v1-name-input"]').setValue("Mid-edit draft");
    // The committed name changes from OUTSIDE while the user is still typing.
    await w.setProps({ track: track("v1", { name: "Screen" }) });
    expect(w.get<HTMLInputElement>('[data-testid="track-header-v1-name-input"]').element.value).toBe(
      "Mid-edit draft",
    );
  });
});

// ---- TrackHeader: track menu (move / delete) ---------------------------------

describe("TrackHeader — track menu", () => {
  it("Move up/Move down send moveTrack with the adjacent index, disabled at either boundary", async () => {
    executed = [];
    await openProject();
    // Middle track: both directions enabled.
    const mid = track("v2");
    const w = mount(TrackHeader, { props: { track: mid, trackIndex: 1, trackCount: 3 } });
    await flushPromises();
    await w.get('[data-testid="track-header-v2-menu"]').trigger("click");
    await w.get('[data-testid="track-header-v2-move-up"]').trigger("click");
    expect(lastCommand()).toEqual({ kind: "moveTrack", trackId: "v2", toIndex: 0 });

    await w.get('[data-testid="track-header-v2-menu"]').trigger("click");
    await w.get('[data-testid="track-header-v2-move-down"]').trigger("click");
    expect(lastCommand()).toEqual({ kind: "moveTrack", trackId: "v2", toIndex: 2 });

    // Frontmost track: Move up is disabled and sends nothing.
    executed = [];
    const front = track("v1");
    const w2 = mount(TrackHeader, { props: { track: front, trackIndex: 0, trackCount: 3 } });
    await w2.get('[data-testid="track-header-v1-menu"]').trigger("click");
    expect(w2.get('[data-testid="track-header-v1-move-up"]').attributes("aria-disabled")).toBe("true");
    await w2.get('[data-testid="track-header-v1-move-up"]').trigger("click");
    expect(executed).toEqual([]);

    // Last track: Move down is disabled and sends nothing.
    const back = track("v3");
    const w3 = mount(TrackHeader, { props: { track: back, trackIndex: 2, trackCount: 3 } });
    await w3.get('[data-testid="track-header-v3-menu"]').trigger("click");
    expect(w3.get('[data-testid="track-header-v3-move-down"]').attributes("aria-disabled")).toBe("true");
    await w3.get('[data-testid="track-header-v3-move-down"]').trigger("click");
    expect(executed).toEqual([]);
  });

  it("Delete track sends deleteTrack", async () => {
    executed = [];
    await openProject();
    const t = track("v1");
    const w = mount(TrackHeader, { props: { track: t, trackIndex: 0, trackCount: 2 } });
    await flushPromises();
    await w.get('[data-testid="track-header-v1-menu"]').trigger("click");
    await w.get('[data-testid="track-header-v1-delete"]').trigger("click");
    expect(lastCommand()).toEqual({ kind: "deleteTrack", trackId: "v1" });
  });

  it("a pointerdown outside the open menu closes it", async () => {
    executed = [];
    await openProject();
    const t = track("v1");
    const w = mount(TrackHeader, {
      attachTo: document.body,
      props: { track: t, trackIndex: 0, trackCount: 2 },
    });
    await flushPromises();
    await w.get('[data-testid="track-header-v1-menu"]').trigger("click");
    expect(w.find('[data-testid="track-header-v1-menu-list"]').exists()).toBe(true);

    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await flushPromises();
    expect(w.find('[data-testid="track-header-v1-menu-list"]').exists()).toBe(false);
    w.unmount();
  });

  it("a pointerdown while the menu is already closed is a no-op", async () => {
    executed = [];
    await openProject();
    const t = track("v1");
    const w = mount(TrackHeader, {
      attachTo: document.body,
      props: { track: t, trackIndex: 0, trackCount: 2 },
    });
    await flushPromises();
    expect(w.find('[data-testid="track-header-v1-menu-list"]').exists()).toBe(false);

    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await flushPromises();
    expect(w.find('[data-testid="track-header-v1-menu-list"]').exists()).toBe(false);
    w.unmount();
  });
});

// ---- TrackLane: a locked track's clips are not draggable ---------------------

describe("TrackLane — locked track clip body", () => {
  it("a locked track's clip body is pointer-events-none, carrying the shared registry reason as its title", async () => {
    executed = [];
    await openProject();
    const locked = track("v1", { locked: true, name: "Screen recording" });
    const c = clip("c1", "v1");
    const w = mount(TrackLane, {
      props: {
        track: locked,
        clips: [c],
        assets: [asset("video-asset", "video")],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v1"],
      },
    });
    await flushPromises();

    const body = w.get('[data-testid="track-lane-body-v1"]');
    expect(body.classes()).toContain("pointer-events-none");
    expect(body.attributes("title")).toBe(lockedReason("Screen recording"));
  });

  it("an UNLOCKED track's clip body carries neither the class nor the title", async () => {
    executed = [];
    await openProject();
    const unlocked = track("v2", { locked: false });
    const w = mount(TrackLane, {
      props: {
        track: unlocked,
        clips: [],
        assets: [],
        selectedClipIds: [],
        zoom: 1,
        widthPx: 2000,
        trackIndex: 0,
        trackOrder: ["v2"],
      },
    });
    await flushPromises();

    const body = w.get('[data-testid="track-lane-body-v2"]');
    expect(body.classes()).not.toContain("pointer-events-none");
    expect(body.attributes("title")).toBeUndefined();
  });
});
