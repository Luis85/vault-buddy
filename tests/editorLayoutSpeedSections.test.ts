/**
 * The Inspector's Layout and Speed categories (Task 31; F-16, F-21, F-23):
 * `LayoutSection.vue` and `SpeedSection.vue`. Every control sends one
 * command through the store — `setLayout` over the whole selection,
 * `setSpeed` for one clip — and an out-of-range entry stays visible with
 * its correction and sends nothing.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import LayoutSection from "../src/components/editor/inspector/LayoutSection.vue";
import SpeedSection from "../src/components/editor/inspector/SpeedSection.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import type { Clip, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
function clip(id: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "cam",
    track_id: "v1",
    name: id,
    start_ms: 700,
    in_ms: 1_000,
    out_ms: 5_000,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 0.8,
    volume: 1,
    muted: false,
    x: 0.61,
    y: 0.23,
    w: 0.27,
    h: 0.19,
    ...overrides,
  };
}
function project(
  clips: Clip[],
  tracks: Track[] = [track("v1"), track("a1", { kind: "audio" })],
  canvas = { width: 720, height: 1280, fps: 30 },
): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas,
    master_gain: 1,
    assets: [{ id: "cam", kind: "video", name: "cam.mp4", duration_ms: 60_000, width: 1280, height: 720 }],
    tracks,
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}

let executed: EditorCommand[];

async function open(p: Project): Promise<void> {
  executed = [];
  const store = useEditorProjectStore();
  const snapshot = {
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
  store.setPort(
    fakeEditorPort({
      openStaged: () =>
        Promise.resolve({ snapshot, project: p, workspace: {}, missing: [], sourceBase: "base", recovered: false }),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...snapshot, revision: 2 }, project: p });
      },
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged("base");
}

async function type(w: ReturnType<typeof mount>, testid: string, value: string): Promise<void> {
  const input = w.get(`[data-testid="${testid}"]`);
  await input.setValue(value);
  await input.trigger("keydown", { key: "Enter" });
  await flushPromises();
}

beforeEach(() => {
  setActivePinia(createPinia());
});

describe("LayoutSection", () => {
  it("shows the box in percent and sends a typed X as one setLayout", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect((w.get('[data-testid="layout-section-w"]').element as HTMLInputElement).value).toBe("27");
    await type(w, "layout-section-x", "50");
    expect(executed).toEqual([{ kind: "setLayout", clipIds: ["c1"], x: 0.5 }]);
  });

  it("refuses an X past the room the width leaves, inline and unsent", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await type(w, "layout-section-x", "80");
    expect(w.get('[data-testid="layout-section-x-error"]').text()).toContain("73%");
    expect(executed).toEqual([]);
  });

  it("a multi-selection gets the same values in one command", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 6_000 })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1", "c2"] } });
    await w.get('[data-testid="layout-section-rotation"]').setValue("270");
    await w.get('[data-testid="layout-section-mirror"]').setValue(true);
    expect(executed).toEqual([
      { kind: "setLayout", clipIds: ["c1", "c2"], rotation: 270 },
      { kind: "setLayout", clipIds: ["c1", "c2"], mirror: true },
    ]);
  });

  it("a corner preset keeps the PiP's width and is aspect-correct on a portrait canvas", async () => {
    await open(project([clip("c1", { frame_shape: "circle" })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-corner-br"]').trigger("click");
    const [sent] = executed as Extract<EditorCommand, { kind: "setLayout" }>[];
    expect(sent.w).toBe(0.27);
    // A circle: equal pixel sides on 720x1280.
    expect(sent.w! * 720).toBeCloseTo(sent.h! * 1280, 1);
    expect(sent.x).toBeCloseTo(1 - 0.27 - 0.025, 4);
  });

  it("choosing a circle makes the box square in pixels and fills it", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-section-shape"]').setValue("circle");
    const [sent] = executed as Extract<EditorCommand, { kind: "setLayout" }>[];
    expect(sent).toMatchObject({ frameShape: "circle", fit: "cover", w: 0.27 });
    expect(sent.h).toBeCloseTo(0.1519, 4);
  });

  it("a full-frame clip turned into a circle narrows so it stays round on a landscape canvas", async () => {
    await open(project([clip("c1", { x: 0, y: 0, w: 1, h: 1 })], undefined, { width: 1280, height: 720, fps: 30 }));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-section-shape"]').setValue("circle");
    const [sent] = executed as Extract<EditorCommand, { kind: "setLayout" }>[];
    expect(sent).toMatchObject({ frameShape: "circle", fit: "cover", h: 1 });
    expect(sent.w! * 1280).toBeCloseTo(sent.h! * 720, 0);
  });

  it("a full-frame clip in a corner becomes the default picture-in-picture size", async () => {
    await open(project([clip("c1", { x: 0, y: 0, w: 1, h: 1 })], undefined, { width: 1280, height: 720, fps: 30 }));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-corner-tl"]').trigger("click");
    const [sent] = executed as Extract<EditorCommand, { kind: "setLayout" }>[];
    // A 16:9 source on a 16:9 canvas: as tall (in fractions) as it is wide.
    expect(sent).toMatchObject({ x: 0.025, y: 0.025, w: 0.19, h: 0.19 });
  });

  it("shape, fit and flip send just their own field", async () => {
    await open(project([clip("c1", { frame_shape: "circle" })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-section-shape"]').setValue("rectangle");
    await w.get('[data-testid="layout-section-fit"]').setValue("cover");
    await w.get('[data-testid="layout-section-flip"]').setValue(true);
    expect(executed).toEqual([
      { kind: "setLayout", clipIds: ["c1"], frameShape: "rectangle" },
      { kind: "setLayout", clipIds: ["c1"], fit: "cover" },
      { kind: "setLayout", clipIds: ["c1"], flipY: true },
    ]);
  });

  it("crop controls appear only when the picture fills its frame", async () => {
    await open(project([clip("c1", { fit: "cover", crop_zoom: 1.7 })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await type(w, "layout-section-crop-zoom", "3.5");
    expect(w.get('[data-testid="layout-section-crop-zoom-error"]').text()).toContain("3×");
    await type(w, "layout-section-crop-y", "80");
    expect(executed).toEqual([{ kind: "setLayout", clipIds: ["c1"], cropY: 0.8 }]);
  });

  it("a contained picture has no crop controls", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect(w.find('[data-testid="layout-section-crop-zoom"]').exists()).toBe(false);
  });

  it("an audio clip in the selection gets a note instead of controls", async () => {
    await open(project([clip("c1"), clip("s1", { track_id: "a1" })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1", "s1"] } });
    expect(w.find('[data-testid="layout-section"]').exists()).toBe(false);
    expect(w.get('[data-testid="layout-section-audio"]').text()).toContain("video");
  });

  it("a locked track disables the controls and says why", async () => {
    await open(project([clip("c1")], [track("v1", { locked: true, name: "Webcam" })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="layout-section-locked"]').text()).toContain("Track Webcam is locked");
    expect(w.get("fieldset").attributes("disabled")).toBeDefined();
    await w.get('[data-testid="layout-corner-tr"]').trigger("click");
    expect(executed).toEqual([]);
  });
});

describe("SpeedSection", () => {
  it("a preset sends setSpeed with the clip's pitch setting", async () => {
    await open(project([clip("c1", { preserve_pitch: false })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="speed-preset-1"]').attributes("aria-pressed")).toBe("true");
    await w.get('[data-testid="speed-preset-2"]').trigger("click");
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 2, preservePitch: false }]);
    // The current speed's own preset sends nothing.
    await w.get('[data-testid="speed-preset-1"]').trigger("click");
    expect(executed).toHaveLength(1);
  });

  it("numeric speed is bounded to 0.25x..4x and says how long the clip plays", async () => {
    await open(project([clip("c1", { speed: 1.5 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    // 4000 ms of source at 1.5x.
    expect(w.get('[data-testid="speed-section-duration"]').text()).toContain("2.67 s");
    await type(w, "speed-section-speed", "5");
    expect(w.get('[data-testid="speed-section-speed-error"]').text()).toContain("0.25× and 4×");
    expect(executed).toEqual([]);
    await type(w, "speed-section-speed", "0.75");
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 0.75, preservePitch: true }]);
  });

  it("Escape puts a typed speed back and sends nothing; leaving the field sends it", async () => {
    await open(project([clip("c1", { speed: 1.5 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    const input = w.get('[data-testid="speed-section-speed"]');
    await input.setValue("3");
    await input.trigger("keydown", { key: "Escape" });
    expect((input.element as HTMLInputElement).value).toBe("1.5");
    await input.setValue("1.25");
    await input.trigger("blur");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 1.25, preservePitch: true }]);
  });

  it("a locked track disables speed and says why", async () => {
    await open(project([clip("c1")], [track("v1", { locked: true, name: "Screen" })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="speed-section-locked"]').text()).toContain("Track Screen is locked");
    expect(w.get('[data-testid="speed-preset-2"]').attributes("disabled")).toBeDefined();
    await w.get('[data-testid="speed-section-pitch"]').setValue(false);
    expect(executed).toEqual([]);
  });

  it("preserve pitch toggles with the current speed", async () => {
    await open(project([clip("c1", { speed: 1.5 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="speed-section-pitch"]').setValue(false);
    expect(executed).toEqual([{ kind: "setSpeed", clipId: "c1", speed: 1.5, preservePitch: false }]);
  });

  it("a multi-selection gets a note, not a silent edit of the first clip", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 6_000 })]));
    const w = mount(SpeedSection, { props: { clipIds: ["c1", "c2"] } });
    expect(w.find('[data-testid="speed-section"]').exists()).toBe(false);
    expect(w.get('[data-testid="speed-section-multi"]').text()).toContain("single clip");
  });
});
