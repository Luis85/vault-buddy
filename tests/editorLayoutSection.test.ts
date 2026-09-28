/**
 * The Inspector's Layout category (Task 31; F-21, F-23, F-38; visual-parity
 * Task 14, concept spec §5 "Layout", screens 02/03): `LayoutSection.vue`.
 * "Webcam overlay" (or "Video layout"): Full frame / Picture-in-picture,
 * Horizontal / Vertical (%), a Size range, the 2×2 corner presets, then the
 * "Frame & crop" and "Transform source" disclosures, whose open state is
 * remembered per selected clip as view state. Every control is one
 * `setLayout` over the whole selection.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import LayoutSection from "../src/components/editor/inspector/LayoutSection.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { EditorPortError } from "../src/editor/port";
import type { Asset, Clip, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

type SetLayout = Extract<EditorCommand, { kind: "setLayout" }>;

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
const CAM: Asset = { id: "cam", kind: "video", name: "cam.mp4", duration_ms: 60_000, width: 1280, height: 720 };

function project(
  clips: Clip[],
  opts: { tracks?: Track[]; canvas?: Project["canvas"]; assets?: Asset[] } = {},
): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: opts.canvas ?? { width: 720, height: 1280, fps: 30 },
    master_gain: 1,
    assets: opts.assets ?? [CAM],
    tracks: opts.tracks ?? [track("v1"), track("a1", { kind: "audio" })],
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}
const LANDSCAPE = { width: 1280, height: 720, fps: 30 };

let executed: EditorCommand[];
let workspaceSaves: number;
/** When set, Rust refuses every command (the store records the error). */
let refuse = false;

async function open(p: Project): Promise<void> {
  executed = [];
  workspaceSaves = 0;
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
  const port = fakeEditorPort({
    openStaged: () =>
      Promise.resolve({ snapshot, project: p, workspace: {}, missing: [], sourceBase: "base", recovered: false }),
    execute: (req) => {
      executed.push(req.command);
      if (refuse) {
        return Promise.reject(
          new EditorPortError({ code: "invalidRequest", message: "refused", retryable: false, operationId: "op-1" }),
        );
      }
      return Promise.resolve({ snapshot: { ...snapshot, revision: 2 }, project: p });
    },
    saveWorkspace: () => {
      workspaceSaves += 1;
      return Promise.resolve();
    },
  });
  store.setPort(port);
  useEditorWorkspaceStore().setPort(port);
  await store.openStaged("base");
}

function sent(): SetLayout {
  expect(executed).toHaveLength(1);
  return executed[0] as SetLayout;
}

async function type(w: ReturnType<typeof mount>, testid: string, value: string): Promise<void> {
  const input = w.get(`[data-testid="${testid}"]`);
  await input.setValue(value);
  await input.trigger("keydown", { key: "Enter" });
  await flushPromises();
}

/** Moves a range's thumb (an `input`), then, unless `release` is false,
 * lets go of it (a `change`). */
async function slide(w: ReturnType<typeof mount>, testid: string, value: string, release = true): Promise<void> {
  const range = w.get(`[data-testid="${testid}"]`);
  (range.element as HTMLInputElement).value = value;
  await range.trigger("input");
  if (release) await range.trigger("change");
  await flushPromises();
}

async function openDisclosure(w: ReturnType<typeof mount>, testid: string): Promise<void> {
  const details = w.get(`[data-testid="${testid}"]`);
  (details.element as HTMLDetailsElement).open = true;
  await details.trigger("toggle");
}

beforeEach(() => {
  setActivePinia(createPinia());
  refuse = false;
});

describe("LayoutSection — the overlay section", () => {
  it("is titled Webcam overlay for a webcam asset and Video layout otherwise", async () => {
    await open(project([clip("c1"), clip("c2", { asset_id: "webcam", start_ms: 6_000 })], {
      assets: [CAM, { ...CAM, id: "webcam", name: "webcam.mp4" }],
    }));
    const plain = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect(plain.get("h3").text()).toBe("Video layout");
    const cam = mount(LayoutSection, { props: { clipIds: ["c2"] } });
    expect(cam.get("h3").text()).toBe("Webcam overlay");
  });

  it("Full frame fills the canvas as a plain contained rectangle", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-preset-full"]').trigger("click");
    expect(sent()).toEqual({
      kind: "setLayout",
      clipIds: ["c1"],
      x: 0,
      y: 0,
      w: 1,
      h: 1,
      frameShape: "rectangle",
      fit: "contain",
    });
  });

  it("Picture-in-picture is a rounded, filled box in the bottom-right corner", async () => {
    await open(project([clip("c1", { x: 0, y: 0, w: 1, h: 1 })], { canvas: LANDSCAPE }));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-preset-pip"]').trigger("click");
    expect(sent()).toMatchObject({ frameShape: "rounded", fit: "cover", x: 0.785, y: 0.785, w: 0.19, h: 0.19 });
  });

  it("shows Horizontal / Vertical in percent and sends a typed value as one setLayout", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect(w.text()).toContain("Horizontal (%)");
    expect(w.text()).toContain("Vertical (%)");
    expect((w.get('[data-testid="layout-section-x"]').element as HTMLInputElement).value).toBe("61");
    await type(w, "layout-section-x", "50");
    expect(executed).toEqual([{ kind: "setLayout", clipIds: ["c1"], x: 0.5 }]);
  });

  it("refuses a Horizontal past the room the width leaves, inline and unsent", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await type(w, "layout-section-x", "80");
    expect(w.get('[data-testid="layout-section-x-error"]').text()).toContain("73%");
    expect(executed).toEqual([]);
  });

  // Fix round 1: the range offers exactly the widths Rust accepts — w and
  // h both at least 0.1 (validate::check_clip), the proportions kept.
  it("Size's minimum is the narrowest width whose height Rust still accepts", async () => {
    await open(project([clip("c1"), clip("c2", { w: 0.4, h: 0.8, start_ms: 6_000 })]));
    const wide = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    // 0.27 x 0.19: h reaches 0.1 at w = 0.1421, so 14.3 % is the first safe step.
    expect((wide.get('[data-testid="layout-section-size"]').element as HTMLInputElement).min).toBe("14.3");
    const tall = mount(LayoutSection, { props: { clipIds: ["c2"] } });
    expect((tall.get('[data-testid="layout-section-size"]').element as HTMLInputElement).min).toBe("10");
  });

  it("a multi-selection's Size sends only the size, never the first clip's place", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 6_000, x: 0.1, y: 0.1 })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1", "c2"] } });
    await slide(w, "layout-section-size", "50");
    const sent0 = sent();
    expect(Object.keys(sent0).sort()).toEqual(["clipIds", "h", "kind", "w"]);
    expect(sent0.w).toBe(0.5);
  });

  it("Size is a 10–100 range with its mono value, bounded by the frame's proportions", async () => {
    await open(project([clip("c1", { w: 0.4, h: 0.8 })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    const size = w.get('[data-testid="layout-section-size"]').element as HTMLInputElement;
    expect(size.type).toBe("range");
    expect(size.min).toBe("10");
    expect(size.max).toBe("50");
    expect(size.value).toBe("40");
    const value = w.get('[data-testid="layout-section-size-value"]');
    expect(value.text()).toBe("40%");
    expect(value.classes()).toContain("vb-mono");
  });

  it("changing Size keeps the proportions and the box inside the frame", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await slide(w, "layout-section-size", "50", false);
    expect(w.get('[data-testid="layout-section-size-value"]').text()).toBe("50%");
    expect(executed).toEqual([]);
    await w.get('[data-testid="layout-section-size"]').trigger("change");
    const box = sent();
    expect(box.w).toBe(0.5);
    expect(box.h).toBeCloseTo(0.19 * (0.5 / 0.27), 3);
    expect(box.x).toBe(0.5);
    expect(box.y).toBe(0.23);
  });

  it("a Size Rust refuses puts the thumb and its value back", async () => {
    await open(project([clip("c1")]));
    refuse = true;
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await slide(w, "layout-section-size", "50");
    expect(executed).toHaveLength(1);
    expect(w.get('[data-testid="layout-section-size-value"]').text()).toBe("27%");
  });

  it("offers the four corners with a corner glyph each", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    const corners = w.findAll('[data-testid^="layout-corner-"]');
    expect(corners.map((b) => b.text())).toEqual(["Top left", "Top right", "Bottom left", "Bottom right"]);
    expect(corners.map((b) => b.attributes("aria-label"))).toEqual([
      "Place top left",
      "Place top right",
      "Place bottom left",
      "Place bottom right",
    ]);
    for (const b of corners) expect(b.find('[data-testid="corner-icon"]').exists()).toBe(true);
  });

  it("a corner preset keeps the PiP's width and is aspect-correct on a portrait canvas", async () => {
    await open(project([clip("c1", { frame_shape: "circle" })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-corner-br"]').trigger("click");
    const box = sent();
    expect(box.w).toBe(0.27);
    expect(box.w! * 720).toBeCloseTo(box.h! * 1280, 1);
    expect(box.x).toBeCloseTo(1 - 0.27 - 0.025, 4);
  });

  it("a full-frame clip in a corner becomes the default picture-in-picture size", async () => {
    await open(project([clip("c1", { x: 0, y: 0, w: 1, h: 1 })], { canvas: LANDSCAPE }));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-corner-tl"]').trigger("click");
    expect(sent()).toMatchObject({ x: 0.025, y: 0.025, w: 0.19, h: 0.19 });
  });

  it("shows the concept's help line", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="layout-section-help"]').text()).toBe(
      "Drag to move. Drag any corner to resize. Size keeps the frame’s proportions; these controls work without dragging.",
    );
  });

  it("a multi-selection gets the same values in one command", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 6_000 })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1", "c2"] } });
    await w.get('[data-testid="layout-corner-tr"]').trigger("click");
    expect(sent()).toMatchObject({ kind: "setLayout", clipIds: ["c1", "c2"] });
  });

  it("an audio clip in the selection gets a note instead of controls", async () => {
    await open(project([clip("c1"), clip("s1", { track_id: "a1" })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1", "s1"] } });
    expect(w.find('[data-testid="layout-section"]').exists()).toBe(false);
    expect(w.get('[data-testid="layout-section-audio"]').text()).toContain("video");
  });

  it("a locked track disables the controls and says why", async () => {
    await open(project([clip("c1")], { tracks: [track("v1", { locked: true, name: "Webcam" })] }));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    // The inspector's frame says it once (visual-parity Task 13); the
    // fields carry the reason as their tooltip.
    expect(w.find('[data-testid="layout-section-locked"]').exists()).toBe(false);
    const fieldset = w.get('[data-testid="layout-section"]');
    expect(fieldset.element.tagName).toBe("FIELDSET");
    expect(fieldset.attributes("title")).toContain("Track Webcam is locked");
    expect(fieldset.attributes("disabled")).toBeDefined();
    await w.get('[data-testid="layout-corner-tr"]').trigger("click");
    await w.get('[data-testid="layout-preset-full"]').trigger("click");
    expect(executed).toEqual([]);
  });
});

describe("LayoutSection — Frame & crop", () => {
  it("is a collapsed disclosure until opened", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    const details = w.get('[data-testid="layout-frame-crop"]');
    expect(details.element.tagName).toBe("DETAILS");
    expect((details.element as HTMLDetailsElement).open).toBe(false);
    expect(details.get("summary").text()).toContain("Frame & crop");
  });

  it("offers the three shapes, the current one pressed", async () => {
    await open(project([clip("c1", { frame_shape: "circle" })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    const shapes = w.findAll('[data-testid^="layout-shape-"]');
    expect(shapes.map((b) => b.text())).toEqual(["Rounded", "Circle", "Rectangle"]);
    expect(shapes.map((b) => b.attributes("aria-pressed"))).toEqual(["false", "true", "false"]);
    await w.get('[data-testid="layout-shape-rectangle"]').trigger("click");
    expect(executed).toEqual([{ kind: "setLayout", clipIds: ["c1"], frameShape: "rectangle" }]);
  });

  it("choosing a circle makes the box square in pixels and fills it", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-shape-circle"]').trigger("click");
    expect(sent()).toMatchObject({ frameShape: "circle", fit: "cover", w: 0.27 });
    expect(sent().h).toBeCloseTo(0.1519, 4);
  });

  it("a full-frame clip turned into a circle narrows so it stays round on a landscape canvas", async () => {
    await open(project([clip("c1", { x: 0, y: 0, w: 1, h: 1 })], { canvas: LANDSCAPE }));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-shape-circle"]').trigger("click");
    const box = sent();
    expect(box).toMatchObject({ frameShape: "circle", fit: "cover", h: 1 });
    expect(box.w! * 1280).toBeCloseTo(box.h! * 720, 0);
  });

  it("Image fitting is a select with the concept's two wordings", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    const options = w.findAll('[data-testid="layout-section-fit"] option').map((o) => o.text());
    expect(options).toEqual(["Fill · crop to frame", "Fit · show entire image"]);
    await w.get('[data-testid="layout-section-fit"]').setValue("cover");
    expect(executed).toEqual([{ kind: "setLayout", clipIds: ["c1"], fit: "cover" }]);
  });

  it("crop zoom and focus appear only when the picture fills its frame", async () => {
    await open(project([clip("c1")]));
    const contained = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect(contained.find('[data-testid="layout-section-crop-zoom"]').exists()).toBe(false);
    expect(contained.find('[data-testid="layout-section-crop-x"]').exists()).toBe(false);
  });

  it("crop zoom is a 1–3× range; focus is typed in percent", async () => {
    await open(project([clip("c1", { fit: "cover", crop_zoom: 1.7 })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    const zoom = w.get('[data-testid="layout-section-crop-zoom"]');
    expect((zoom.element as HTMLInputElement).min).toBe("1");
    expect((zoom.element as HTMLInputElement).max).toBe("3");
    expect(w.get('[data-testid="layout-section-crop-zoom-value"]').text()).toBe("1.7×");
    await slide(w, "layout-section-crop-zoom", "2.5");
    await type(w, "layout-section-crop-y", "80");
    expect(executed).toEqual([
      { kind: "setLayout", clipIds: ["c1"], cropZoom: 2.5 },
      { kind: "setLayout", clipIds: ["c1"], cropY: 0.8 },
    ]);
  });

  // Fix round 1: a refused edit must not leave the control showing the
  // refused value.
  it("an Image fitting Rust refuses puts the select back", async () => {
    await open(project([clip("c1")]));
    refuse = true;
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-section-fit"]').setValue("cover");
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect((w.get('[data-testid="layout-section-fit"]').element as HTMLSelectElement).value).toBe("contain");
  });

  it("a Mirror Rust refuses unticks the box again", async () => {
    await open(project([clip("c1")]));
    refuse = true;
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await w.get('[data-testid="layout-section-mirror"]').setValue(true);
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect((w.get('[data-testid="layout-section-mirror"]').element as HTMLInputElement).checked).toBe(false);
  });

  // Ruling T14-1: keyboard users keep a non-proportional size.
  it("Width and Height live in Frame & crop and change only their own side", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    const frame = w.get('[data-testid="layout-frame-crop"]');
    expect(frame.text()).toContain("Width (%)");
    expect(frame.text()).toContain("Height (%)");
    expect((w.get('[data-testid="layout-section-w"]').element as HTMLInputElement).value).toBe("27");
    await type(w, "layout-section-w", "30");
    await type(w, "layout-section-h", "25");
    expect(executed).toEqual([
      { kind: "setLayout", clipIds: ["c1"], w: 0.3 },
      { kind: "setLayout", clipIds: ["c1"], h: 0.25 },
    ]);
  });

  it("a Width past the room the position leaves is refused inline", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await type(w, "layout-section-w", "50");
    expect(w.get('[data-testid="layout-section-w-error"]').text()).toContain("39%");
    expect(executed).toEqual([]);
  });

  it("Mirror and Opacity send their own field", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect(w.get('[data-testid="layout-section-opacity-value"]').text()).toBe("80%");
    await w.get('[data-testid="layout-section-mirror"]').setValue(true);
    await slide(w, "layout-section-opacity", "45");
    expect(executed).toEqual([
      { kind: "setLayout", clipIds: ["c1"], mirror: true },
      { kind: "setLayout", clipIds: ["c1"], opacity: 0.45 },
    ]);
  });

  it("its help mentions the webcam's own track only for a webcam", async () => {
    await open(project([clip("c1"), clip("c2", { asset_id: "take-1", start_ms: 6_000 })], {
      assets: [CAM, { ...CAM, id: "take-1", name: "Webcam take 1" }],
    }));
    const plain = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    expect(plain.get('[data-testid="layout-frame-help"]').text()).toBe(
      "Mirroring and crop affect preview and renders, never the original.",
    );
    const cam = mount(LayoutSection, { props: { clipIds: ["c2"] } });
    expect(cam.get('[data-testid="layout-frame-help"]').text()).toContain(
      "The webcam stays on its own editable video track.",
    );
  });
});

describe("LayoutSection — Transform source", () => {
  it("is a collapsed disclosure with the concept's six buttons", async () => {
    await open(project([clip("c1")]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    const details = w.get('[data-testid="layout-transform"]');
    expect((details.element as HTMLDetailsElement).open).toBe(false);
    expect(details.get("summary").text()).toContain("Transform source");
    expect(details.findAll("button").map((b) => b.text())).toEqual([
      "Rotate 90°",
      "Flip horizontal",
      "Flip vertical",
      "Center",
      "Fit source",
      "Fill frame",
    ]);
  });

  it("each button sends one setLayout computed from the clip", async () => {
    await open(project([clip("c1", { rotation: 270, mirror: true, flip_y: false })]));
    const w = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    for (const key of ["rotate", "flip", "flip-y", "center", "fit", "fill"]) {
      await w.get(`[data-testid="layout-transform-${key}"]`).trigger("click");
    }
    const patches = (executed as SetLayout[]).map(({ kind: _kind, clipIds: _ids, ...patch }) => patch);
    expect(patches).toEqual([
      { rotation: 0 },
      { mirror: false },
      { flipY: true },
      { x: 0.365, y: 0.405 },
      { fit: "contain" },
      { fit: "cover" },
    ]);
  });
});

// Concept `workspace-ui.js`: a precision disclosure's open state is kept per
// selection, never saved with the project.
describe("LayoutSection — disclosures remember their state per clip", () => {
  it("an opened disclosure stays open for its clip only, and is never an edit or a saved preference", async () => {
    await open(project([clip("c1"), clip("c2", { start_ms: 6_000 })]));
    const first = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await openDisclosure(first, "layout-frame-crop");
    first.unmount();

    const other = mount(LayoutSection, { props: { clipIds: ["c2"] } });
    expect((other.get('[data-testid="layout-frame-crop"]').element as HTMLDetailsElement).open).toBe(false);
    other.unmount();

    const again = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    expect((again.get('[data-testid="layout-frame-crop"]').element as HTMLDetailsElement).open).toBe(true);
    expect((again.get('[data-testid="layout-transform"]').element as HTMLDetailsElement).open).toBe(false);
    expect(again.get('[data-testid="layout-frame-crop"] summary').text()).toContain("−");
    expect(executed).toEqual([]);
    expect(workspaceSaves).toBe(0);
  });

  it("is forgotten with the session that had it", async () => {
    await open(project([clip("c1")]));
    const workspace = useEditorWorkspaceStore();
    workspace.sessionId = "ses-a";
    const first = mount(LayoutSection, { props: { clipIds: ["c1"] } });
    await openDisclosure(first, "layout-transform");
    expect(workspace.disclosureOpen("c1:transform")).toBe(true);
    workspace.sessionId = "ses-b";
    await flushPromises();
    expect(workspace.disclosureOpen("c1:transform")).toBe(false);
  });
});
