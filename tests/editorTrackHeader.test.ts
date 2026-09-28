/**
 * `TrackHeader.vue` (Task 23; F-06; visual-parity Task 17, concept spec
 * §6.4) and `TrackLane.vue`'s row: the sticky label cell, the grid, the
 * empty-lane text, the locked hatch and gating, a hidden video track, the
 * drop highlight, and the header's way into the track menu. The header
 * calls `editorProject.execute` directly (the `ClipItem.vue` precedent), so
 * the command assertions read the `executed` array a fake
 * `EditorPort.execute` records.
 */
import type { VueWrapper } from "@vue/test-utils";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import TrackHeader from "../src/components/editor/timeline/TrackHeader.vue";
import TrackLane from "../src/components/editor/timeline/TrackLane.vue";
import { lockedReason } from "../src/editor/actionMeta";
import { trackRenameRequest } from "../src/editor/revealBus";
import type { Asset, Clip, EditorCommand, EditorOpenResult, EditorSnapshot, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  executed = [];
});

// ---- fixtures ---------------------------------------------------------------

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

function project(): Project {
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
  };
}

function snapshot(): EditorSnapshot {
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
  };
}

let executed: EditorCommand[] = [];

/** Opens a live session so `editorProject.execute` has a session to send
 * against. The fake reply echoes the same project back; every assertion
 * reads `executed`, never a re-rendered projection. */
async function openProject() {
  const store = useEditorProjectStore();
  const p = project();
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
      saveWorkspace: () => Promise.resolve(),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...s, revision: s.revision + 1 }, project: p });
      },
    }),
  );
  await store.openStaged("base");
}

function lastCommand(): EditorCommand {
  const cmd = executed[executed.length - 1];
  if (!cmd) throw new Error("no command was executed");
  return cmd;
}

function header(t: Track, badge = "V1"): VueWrapper {
  return mount(TrackHeader, { props: { track: t, badge } });
}

function part(w: VueWrapper, id: string, name: string) {
  return w.get(`[data-testid="track-header-${id}-${name}"]`);
}

const ACTIVE = ["bg-gold-bg", "text-gold"];

// ---- anatomy (§6.4) ----------------------------------------------------------

describe("TrackHeader — anatomy", () => {
  it("a video header: mono badge, title, eye / M / S / lock at 26px, and no menu button or volume", async () => {
    await openProject();
    const w = header(track("v3", { name: "Webcam · presenter" }), "V3");
    const badge = part(w, "v3", "badge");
    expect(badge.text()).toBe("V3");
    expect(badge.attributes("aria-label")).toBe("Webcam · presenter track properties");
    expect(badge.classes()).toEqual(expect.arrayContaining(["h-7", "w-7", "vb-mono", "text-[10px]", "bg-video-bg", "text-video"]));
    const name = part(w, "v3", "name");
    expect(name.text()).toBe("Webcam · presenter");
    expect(name.attributes("title")).toBe("Webcam · presenter");
    expect(name.classes()).toEqual(expect.arrayContaining(["truncate", "text-[11px]", "font-[550]"]));
    const controls = part(w, "v3", "controls").findAll("button");
    expect(controls.map((b) => b.attributes("data-testid"))).toEqual([
      "track-header-v3-visible",
      "track-header-v3-mute",
      "track-header-v3-solo",
      "track-header-v3-lock",
    ]);
    for (const b of controls) expect(b.classes()).toEqual(expect.arrayContaining(["h-[26px]", "min-w-[26px]"]));
    expect(part(w, "v3", "mute").text()).toBe("M");
    expect(part(w, "v3", "solo").text()).toBe("S");
    expect(w.find('[data-testid="track-header-v3-menu"]').exists()).toBe(false);
    expect(w.find('[data-testid="track-header-v3-volume"]').exists()).toBe(false);
  });

  it("an audio header: its own badge colours and no eye", async () => {
    await openProject();
    const w = header(track("a1", { kind: "audio" }), "A1");
    expect(part(w, "a1", "badge").classes()).toEqual(expect.arrayContaining(["bg-audio-bg", "text-audio"]));
    expect(w.find('[data-testid="track-header-a1-visible"]').exists()).toBe(false);
  });

  it("the badge and the title select the track and show its properties, even on a locked track", async () => {
    await openProject();
    const ws = useEditorWorkspaceStore();
    ws.setViewport(960, 640); // the inspector is a closed drawer here
    ws.select(["c9"]);
    await part(header(track("v2", { locked: true }), "V2"), "v2", "badge").trigger("click");
    expect(ws.selectedTrackId).toBe("v2");
    expect(ws.selectionClipIds).toEqual([]);
    expect(ws.inspectorVisible).toBe(true);

    await part(header(track("v1")), "v1", "name").trigger("click");
    expect(ws.selectedTrackId).toBe("v1");
    expect(executed).toEqual([]);
  });

  it("a 120-character name ellipsizes inside the header", async () => {
    await openProject();
    const long = "N".repeat(120);
    const w = header(track("v1", { name: long }));
    expect(part(w, "v1", "name").attributes("title")).toBe(long);
    expect(part(w, "v1", "name").classes()).toEqual(expect.arrayContaining(["truncate", "max-w-full"]));
    expect(w.get('[data-testid="track-header-v1"]').classes()).toContain("min-w-0");
  });
});

// ---- the flag controls ---------------------------------------------------------

describe("TrackHeader — eye / M / S / lock", () => {
  it("the eye hides a visible track and reads active (gold) while the track is hidden", async () => {
    await openProject();
    const shown = header(track("v1", { visible: true }));
    const eye = part(shown, "v1", "visible");
    expect(eye.attributes("aria-pressed")).toBe("false");
    expect(eye.attributes("title")).toBe("Hide video track");
    expect(eye.classes()).not.toContain("bg-gold-bg");
    await eye.trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", visible: false });

    const hidden = part(header(track("v1", { visible: false })), "v1", "visible");
    expect(hidden.attributes("aria-pressed")).toBe("true");
    expect(hidden.attributes("title")).toBe("Show video track");
    expect(hidden.classes()).toEqual(expect.arrayContaining(ACTIVE));
    await hidden.trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", visible: true });
  });

  it("M and S toggle independently and read gold while on", async () => {
    await openProject();
    const w = header(track("v1", { name: "Screen", muted: false, solo: true }));
    expect(part(w, "v1", "mute").classes()).not.toContain("bg-gold-bg");
    expect(part(w, "v1", "solo").classes()).toEqual(expect.arrayContaining(ACTIVE));
    expect(part(w, "v1", "mute").attributes("title")).toBe("Mute Screen");
    expect(part(w, "v1", "solo").attributes("title")).toBe("Solo audio on Screen");
    await part(w, "v1", "mute").trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", muted: true });
    await part(w, "v1", "solo").trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", solo: false });
  });

  it("lock toggles, reads gold while locked, and is never disabled", async () => {
    await openProject();
    const open = part(header(track("v1", { name: "Screen" })), "v1", "lock");
    expect(open.attributes("title")).toBe("Lock Screen");
    expect(open.attributes("aria-pressed")).toBe("false");
    await open.trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", locked: true });

    const locked = part(header(track("v1", { name: "Screen", locked: true })), "v1", "lock");
    expect(locked.attributes("title")).toBe("Unlock Screen");
    expect(locked.attributes("aria-pressed")).toBe("true");
    expect(locked.attributes("aria-disabled")).toBeUndefined();
    expect(locked.classes()).toEqual(expect.arrayContaining(ACTIVE));
    await locked.trigger("click");
    expect(lastCommand()).toEqual({ kind: "setTrackFlags", trackId: "v1", locked: false });
  });

  it("every other control is refused while the track is locked, with the shared registry reason", async () => {
    await openProject();
    const w = header(track("v1", { locked: true, name: "Ambient" }));
    const reason = lockedReason("Ambient");
    for (const name of ["visible", "mute", "solo"]) {
      expect(part(w, "v1", name).attributes("aria-disabled")).toBe("true");
      expect(part(w, "v1", name).attributes("title")).toBe(reason);
      await part(w, "v1", name).trigger("click");
    }
    expect(executed).toEqual([]);
  });
});

// ---- rename (asked for by the track menu's "Rename track…") --------------------

async function renaming(t: Track): Promise<VueWrapper> {
  const w = header(t);
  trackRenameRequest.value = t.id;
  await flushPromises();
  return w;
}

describe("TrackHeader — rename", () => {
  it("the track menu's request starts the inline rename on that header only", async () => {
    await openProject();
    const a = header(track("v1"));
    const b = header(track("v2"));
    trackRenameRequest.value = "v2";
    await flushPromises();
    expect(b.find('[data-testid="track-header-v2-name-input"]').exists()).toBe(true);
    expect(a.find('[data-testid="track-header-v1-name-input"]').exists()).toBe(false);
    expect(trackRenameRequest.value).toBeNull();
  });

  it("commits a trimmed, changed name via renameTrack on Enter", async () => {
    await openProject();
    const w = await renaming(track("v1", { name: "Webcam" }));
    await part(w, "v1", "name-input").setValue("  Screen  ");
    await part(w, "v1", "name-input").trigger("keydown.enter");
    expect(lastCommand()).toEqual({ kind: "renameTrack", trackId: "v1", name: "Screen" });
  });

  it("sends nothing when the trimmed name is unchanged or empty, and Escape cancels", async () => {
    await openProject();
    for (const value of ["  Webcam  ", "   "]) {
      const w = await renaming(track("v1", { name: "Webcam" }));
      await part(w, "v1", "name-input").setValue(value);
      await part(w, "v1", "name-input").trigger("keydown.enter");
      w.unmount(); // the next request is for the next header
    }
    const w = await renaming(track("v1", { name: "Webcam" }));
    await part(w, "v1", "name-input").setValue("Discarded");
    await part(w, "v1", "name-input").trigger("keydown.escape");
    expect(executed).toEqual([]);
    expect(w.find('[data-testid="track-header-v1-name-input"]').exists()).toBe(false);
  });

  it("a locked track never opens the rename", async () => {
    await openProject();
    const w = await renaming(track("v1", { locked: true }));
    expect(w.find('[data-testid="track-header-v1-name-input"]').exists()).toBe(false);
  });

  it("an external rename updates the name when not mid-edit, and never clobbers a live edit", async () => {
    await openProject();
    const idle = header(track("v1", { name: "Webcam" }));
    await idle.setProps({ track: track("v1", { name: "Screen" }) });
    expect(part(idle, "v1", "name").text()).toBe("Screen");
    idle.unmount();

    const w = await renaming(track("v1", { name: "Webcam" }));
    await part(w, "v1", "name-input").setValue("Mid-edit draft");
    await w.setProps({ track: track("v1", { name: "Screen" }) });
    expect((part(w, "v1", "name-input").element as HTMLInputElement).value).toBe("Mid-edit draft");
  });
});

// ---- TrackLane: the row ----------------------------------------------------------

function lane(t: Track, clips: Clip[] = [], extra: Record<string, unknown> = {}): VueWrapper {
  return mount(TrackLane, {
    props: {
      track: t,
      badge: "V1",
      clips,
      assets: [asset("video-asset", "video"), asset("audio-asset", "audio")],
      selectedClipIds: [],
      zoom: 1,
      widthPx: 2000,
      labelWidth: 196,
      trackIndex: 0,
      trackOrder: [t.id],
      ...extra,
    },
  });
}

const laneBody = (w: VueWrapper, id: string) => w.get(`[data-testid="track-lane-body-${id}"]`);

function dragOf(kind: "video" | "audio"): { dataTransfer: DataTransfer } {
  const dataTransfer = {
    dropEffect: "none",
    types: [`application/x-vault-buddy-asset-${kind}`],
    getData: () => "",
  } as unknown as DataTransfer;
  return { dataTransfer };
}

describe("TrackLane — the row (§6.4)", () => {
  it("a 68px row whose label cell is sticky at the left, above the lanes, at the label width", async () => {
    await openProject();
    const w = lane(track("v1"), [], { labelWidth: 174 });
    const row = w.get('[data-testid="track-lane-v1"]');
    expect(row.attributes("style")).toContain("height: 68px");
    expect(row.classes()).toEqual(expect.arrayContaining(["min-h-[58px]", "border-b", "border-line"]));
    const cell = w.get('[data-testid="track-lane-header-v1"]');
    expect(cell.classes()).toEqual(expect.arrayContaining(["sticky", "left-0", "z-[15]", "bg-panel", "border-r"]));
    expect(cell.attributes("style")).toContain("width: 174px");
  });

  it("the lane draws a grid line at every ruler tick", async () => {
    await openProject();
    // 50 px/s: 2 s ticks, 100px; 200 px/s: 0.5 s, 100px; 150 px/s: 0.5 s, 75px.
    expect(laneBody(lane(track("v1")), "v1").attributes("style")).toContain("background-size: 100px 100%");
    expect(laneBody(lane(track("v1"), [], { zoom: 4 }), "v1").attributes("style")).toContain("background-size: 100px 100%");
    expect(laneBody(lane(track("v1"), [], { zoom: 3 }), "v1").attributes("style")).toContain("background-size: 75px 100%");
  });

  it("an empty lane says what it takes; a lane with clips does not", async () => {
    await openProject();
    expect(lane(track("v1")).get('[data-testid="track-lane-empty-v1"]').text()).toBe("Drop video here · or add from Media");
    expect(lane(track("a1", { kind: "audio" })).get('[data-testid="track-lane-empty-a1"]').text()).toBe(
      "Drop audio here · or add from Media",
    );
    expect(lane(track("v1"), [clip("c1", "v1")]).find('[data-testid="track-lane-empty-v1"]').exists()).toBe(false);
  });

  it("a locked lane is hatched and gated; an unlocked one is neither", async () => {
    await openProject();
    const body = laneBody(lane(track("v1", { locked: true, name: "Screen recording" }), [clip("c1", "v1")]), "v1");
    expect(body.classes()).toEqual(expect.arrayContaining(["pointer-events-none", "vb-lane-locked"]));
    expect(body.attributes("title")).toBe(lockedReason("Screen recording"));

    const open = laneBody(lane(track("v2")), "v2");
    expect(open.classes()).not.toContain("pointer-events-none");
    expect(open.classes()).not.toContain("vb-lane-locked");
    expect(open.attributes("title")).toBeUndefined();
  });

  it("a hidden video track dims its clips", async () => {
    await openProject();
    expect(laneBody(lane(track("v1", { visible: false })), "v1").classes()).toContain("*:opacity-40");
    expect(laneBody(lane(track("v1")), "v1").classes()).not.toContain("*:opacity-40");
  });

  it("an accepted drag highlights the lane until it leaves; a refused one does not", async () => {
    await openProject();
    const w = lane(track("v1"));
    const row = w.get('[data-testid="track-lane-v1"]');
    await row.trigger("dragover", dragOf("video"));
    expect(laneBody(w, "v1").classes()).toContain("bg-accent-bg");
    await row.trigger("dragleave");
    expect(laneBody(w, "v1").classes()).not.toContain("bg-accent-bg");
    await row.trigger("dragover", dragOf("audio"));
    expect(laneBody(w, "v1").classes()).not.toContain("bg-accent-bg");
  });

  it("a right-click, Shift+F10 or the Menu key on the header asks for the track menu", async () => {
    await openProject();
    const w = lane(track("v1"));
    const cell = w.get('[data-testid="track-lane-header-v1"]');
    await cell.trigger("contextmenu", { clientX: 40, clientY: 90 });
    await part(w, "v1", "badge").trigger("keydown", { key: "F10", shiftKey: true });
    await cell.trigger("keydown", { key: "ContextMenu" });
    await cell.trigger("keydown", { key: "F10" });
    const asked = w.emitted("track-context-menu") ?? [];
    expect(asked).toHaveLength(3);
    expect(asked[0]).toEqual([{ trackId: "v1", clientX: 40, clientY: 90 }]);
    expect(asked[1]).toEqual([expect.objectContaining({ trackId: "v1" })]);
  });
});
