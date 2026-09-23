/**
 * Picture-in-picture layout (Task 31; F-21, F-23, F-38): the corner
 * presets (`layoutGeometry.cornerPreset`), the preview's drag handles
 * (`LayoutHandles.vue`) and how `PreviewSurface.vue` hosts them. The rest of
 * the pure box and placement arithmetic is `editorLayoutGeometry.test.ts`. Named `editorPipLayout`, not
 * `editorLayout` (F29): `tests/editorLayout.test.ts` is the phase-4 layout
 * contract and already exists.
 *
 * Fixtures are asymmetric on purpose (the "fixture flaw" rule): a portrait
 * canvas for the circle, a stage whose aspect differs from the canvas (so
 * the letterbox has to be undone), a box with four different numbers.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import LayoutHandles from "../src/components/editor/preview/LayoutHandles.vue";
import PreviewSurface from "../src/components/editor/preview/PreviewSurface.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { CORNER_MARGIN, cornerPreset } from "../src/editor/layoutGeometry";
import type { AudioContextLike } from "../src/editor/previewController";
import { containRect } from "../src/editor/previewGeometry";
import type { Clip, EditorOpenResult, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

const PORTRAIT = { width: 720, height: 1280 };
const LANDSCAPE = { width: 1280, height: 720 };

// ---- layoutGeometry --------------------------------------------------------

describe("cornerPreset", () => {
  // Named test (brief). The mutation it pins: dropping the canvas aspect
  // from the height makes h == w, a tall ellipse on a portrait canvas.
  it("corner preset keeps a circle circular on a portrait canvas", () => {
    for (const corner of ["tl", "tr", "bl", "br"] as const) {
      const box = cornerPreset(corner, PORTRAIT, 0.19, { circle: true, width: 1920, height: 1080 });
      expect(box.w).toBeCloseTo(0.19, 9);
      // Equal PIXEL sides: 0.19 * 720 == h * 1280.
      expect(box.w * PORTRAIT.width).toBeCloseTo(box.h * PORTRAIT.height, 6);
      expect(box.h).toBeCloseTo(0.106875, 9);
    }
    const br = cornerPreset("br", PORTRAIT, 0.19, { circle: true });
    expect(br.x).toBeCloseTo(1 - 0.19 - CORNER_MARGIN, 9);
    expect(br.y).toBeCloseTo(1 - 0.106875 - CORNER_MARGIN, 9);
    const tl = cornerPreset("tl", PORTRAIT, 0.19, { circle: true });
    expect([tl.x, tl.y]).toEqual([CORNER_MARGIN, CORNER_MARGIN]);
  });

  it("a rectangle keeps the SOURCE's aspect, and an unknown source the canvas's", () => {
    const box = cornerPreset("tr", LANDSCAPE, 0.19, { width: 640, height: 480 });
    expect((box.w * LANDSCAPE.width) / (box.h * LANDSCAPE.height)).toBeCloseTo(640 / 480, 6);
    expect(box.x).toBeCloseTo(1 - 0.19 - CORNER_MARGIN, 9);
    expect(box.y).toBeCloseTo(CORNER_MARGIN, 9);
    const unknown = cornerPreset("bl", LANDSCAPE);
    expect(unknown.h).toBeCloseTo(unknown.w, 9);
  });

  it("a box too small for the schema grows as a whole, keeping its aspect", () => {
    const box = cornerPreset("tl", PORTRAIT, 0.12, { circle: true });
    expect(box.h).toBeCloseTo(0.1, 9);
    expect(box.w * PORTRAIT.width).toBeCloseTo(box.h * PORTRAIT.height, 6);
  });

  it("a box too tall for the canvas shrinks as a whole, keeping its aspect", () => {
    // A 9:16 source at 0.6 of a landscape canvas would be 1.9 tall.
    const box = cornerPreset("tl", LANDSCAPE, 0.6, { width: 1080, height: 1920 });
    expect(box.h).toBeLessThanOrEqual(1);
    expect(box.y + box.h).toBeLessThanOrEqual(1 + 1e-9);
    expect((box.w * LANDSCAPE.width) / (box.h * LANDSCAPE.height)).toBeCloseTo(1080 / 1920, 6);
  });
});

// ---- LayoutHandles / PreviewSurface -----------------------------------------

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
function clip(overrides: Partial<Clip> = {}): Clip {
  return {
    id: "pip",
    asset_id: "cam",
    track_id: "v1",
    name: "Presenter",
    start_ms: 0,
    in_ms: 0,
    out_ms: 6_000,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 1,
    volume: 1,
    muted: false,
    x: 0.5,
    y: 0.2,
    w: 0.3,
    h: 0.4,
    ...overrides,
  };
}
function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { ...LANDSCAPE, fps: 30 },
    master_gain: 1,
    assets: [{ id: "cam", kind: "video", name: "cam.mp4", duration_ms: 6_000, width: 1920, height: 1080 }],
    tracks: [track("v1")],
    clips: [clip()],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

let executed: EditorCommand[];

async function openProject(p: Project = project()) {
  executed = [];
  const store = useEditorProjectStore();
  const opened: EditorOpenResult = {
    snapshot: {
      sessionId: "ses-a",
      projectId: "project-a",
      revision: 1,
      persistedRevision: null,
      title: "Tutorial",
      durationMs: 6_000,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    },
    project: p,
    workspace: {},
    missing: [],
    sourceBase: "base",
    recovered: false,
  };
  store.setPort(
    fakeEditorPort({
      openStaged: () => Promise.resolve(opened),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...opened.snapshot, revision: 2 }, project: p });
      },
      mediaUrl: () => Promise.resolve("C:\\x\\cam.mp4"),
      getWorkspace: () => Promise.resolve({}),
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged("base");
  useEditorWorkspaceStore().select(["pip"]);
}

/** 1280x720 in a 1000x500 stage: scale 500/720, the canvas box 888.9 wide
 * at left 55.6 (pillarboxed) — so a pointer that ignored the letterbox
 * would land somewhere else. */
const STAGE_RECT = { left: 10, top: 20, width: 1000, height: 500 };
const FRAME = containRect(LANDSCAPE, STAGE_RECT);
const SCALE = 500 / 720;

function clientAt(nx: number, ny: number): { clientX: number; clientY: number } {
  return {
    clientX: STAGE_RECT.left + FRAME.left + nx * LANDSCAPE.width * SCALE,
    clientY: STAGE_RECT.top + FRAME.top + ny * LANDSCAPE.height * SCALE,
  };
}

function mountHandles() {
  const w = mount(LayoutHandles, {
    props: { frame: FRAME, canvas: LANDSCAPE },
    attachTo: document.body,
  });
  const root = w.get('[data-testid="layout-handles"]').element as HTMLElement;
  root.getBoundingClientRect = () =>
    ({ ...STAGE_RECT, x: STAGE_RECT.left, y: STAGE_RECT.top, right: 1010, bottom: 520, toJSON: () => ({}) }) as DOMRect;
  return w;
}

beforeEach(() => {
  setActivePinia(createPinia());
});

describe("LayoutHandles", () => {
  it("draws the selected clip's box inside the letterboxed canvas, with eight handles", async () => {
    await openProject();
    const w = mountHandles();
    const box = w.get('[data-testid="layout-box"]').element as HTMLElement;
    expect(parseFloat(box.style.left)).toBeCloseTo(FRAME.left + 0.5 * FRAME.width, 6);
    expect(parseFloat(box.style.top)).toBeCloseTo(FRAME.top + 0.2 * FRAME.height, 6);
    expect(parseFloat(box.style.width)).toBeCloseTo(0.3 * FRAME.width, 6);
    expect(parseFloat(box.style.height)).toBeCloseTo(0.4 * FRAME.height, 6);
    expect(w.findAll('[data-testid^="layout-handle-"]')).toHaveLength(8);
  });

  // Named test (brief).
  it("resize handle drag sends one setLayout", async () => {
    await openProject();
    const w = mountHandles();
    const se = w.get('[data-testid="layout-handle-se"]');
    await se.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.8, 0.6) });
    await se.trigger("pointermove", { pointerId: 1, shiftKey: true, ...clientAt(0.85, 0.65) });
    await se.trigger("pointermove", { pointerId: 1, shiftKey: true, ...clientAt(0.9, 0.7) });
    // Preview only while dragging: the box moved, nothing was sent.
    expect(executed).toEqual([]);
    const previews = w.emitted("preview") as [Project | null][];
    const last = previews[previews.length - 1][0]!;
    expect(last.clips[0].w).toBeCloseTo(0.4, 9);
    expect(useEditorProjectStore().project!.clips[0].w).toBe(0.3);

    await se.trigger("pointerup", { pointerId: 1, ...clientAt(0.9, 0.7) });
    await flushPromises();
    expect(executed).toEqual([{ kind: "setLayout", clipIds: ["pip"], x: 0.5, y: 0.2, w: 0.4, h: 0.5 }]);
    expect(previews[previews.length - 1][0]).toBeNull();
  });

  it("a corner drag keeps the picture's proportions unless Shift is held", async () => {
    await openProject();
    const w = mountHandles();
    const se = w.get('[data-testid="layout-handle-se"]');
    await se.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.8, 0.6) });
    await se.trigger("pointermove", { pointerId: 1, ...clientAt(0.95, 0.62) });
    await se.trigger("pointerup", { pointerId: 1, ...clientAt(0.95, 0.62) });
    await flushPromises();
    const [sent] = executed as Extract<EditorCommand, { kind: "setLayout" }>[];
    expect(sent.w! / sent.h!).toBeCloseTo(0.3 / 0.4, 3);
  });

  it("Escape mid-drag sends nothing and drops the preview", async () => {
    await openProject();
    const w = mountHandles();
    const body = w.get('[data-testid="layout-box"]');
    await body.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.6, 0.3) });
    await body.trigger("pointermove", { pointerId: 1, ...clientAt(0.4, 0.1) });
    await body.trigger("keydown", { key: "Escape" });
    await body.trigger("pointerup", { pointerId: 1, ...clientAt(0.4, 0.1) });
    await flushPromises();
    expect(executed).toEqual([]);
    const previews = w.emitted("preview") as [Project | null][];
    expect(previews[previews.length - 1][0]).toBeNull();
  });

  it("dragging the body moves the box, kept inside the frame", async () => {
    await openProject();
    const w = mountHandles();
    const body = w.get('[data-testid="layout-box"]');
    await body.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.6, 0.3) });
    await body.trigger("pointermove", { pointerId: 1, ...clientAt(0.95, 0.25) });
    await body.trigger("pointerup", { pointerId: 1, ...clientAt(0.95, 0.25) });
    await flushPromises();
    expect(executed).toEqual([{ kind: "setLayout", clipIds: ["pip"], x: 0.7, y: 0.15, w: 0.3, h: 0.4 }]);
  });

  it("a click without movement sends nothing", async () => {
    await openProject();
    const w = mountHandles();
    const body = w.get('[data-testid="layout-box"]');
    await body.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.6, 0.3) });
    await body.trigger("pointerup", { pointerId: 1, ...clientAt(0.6, 0.3) });
    await flushPromises();
    expect(executed).toEqual([]);
  });

  it("a circle keeps its proportions even from an edge handle, centred on the other axis", async () => {
    const h0 = 0.3 * (1280 / 720);
    await openProject(project({ clips: [clip({ frame_shape: "circle", h: h0 })] }));
    const w = mountHandles();
    const e = w.get('[data-testid="layout-handle-e"]');
    await e.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.8, 0.2 + h0 / 2) });
    await e.trigger("pointermove", { pointerId: 1, ...clientAt(0.75, 0.2 + h0 / 2) });
    await e.trigger("pointerup", { pointerId: 1, ...clientAt(0.75, 0.2 + h0 / 2) });
    await flushPromises();
    const [sent] = executed as Extract<EditorCommand, { kind: "setLayout" }>[];
    expect(sent.w).toBeCloseTo(0.25, 4);
    expect((sent.w! * 1280) / (sent.h! * 720)).toBeCloseTo(1, 2);
    // The left edge stays; the vertical centre stays.
    expect(sent.x).toBe(0.5);
    expect(sent.y! + sent.h! / 2).toBeCloseTo(0.2 + h0 / 2, 3);
  });

  it("only the primary button starts a drag; stray moves, releases and Escapes do nothing", async () => {
    await openProject();
    const w = mountHandles();
    const body = w.get('[data-testid="layout-box"]');
    await body.trigger("pointermove", { pointerId: 1, ...clientAt(0.1, 0.1) });
    await body.trigger("pointerdown", { button: 2, pointerId: 1, ...clientAt(0.6, 0.3) });
    await body.trigger("pointermove", { pointerId: 1, ...clientAt(0.2, 0.1) });
    await body.trigger("pointerup", { pointerId: 1, ...clientAt(0.2, 0.1) });
    const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    body.element.dispatchEvent(escape);
    await flushPromises();
    expect(executed).toEqual([]);
    expect(w.emitted("preview")).toBeUndefined();
    expect(escape.defaultPrevented).toBe(false);
  });

  it("shows nothing for an audio clip, a multi-selection or a locked track", async () => {
    await openProject(project({ tracks: [track("v1", { locked: true })] }));
    expect(mountHandles().find('[data-testid="layout-box"]').exists()).toBe(false);
    useEditorWorkspaceStore().select([]);
    expect(mountHandles().find('[data-testid="layout-box"]').exists()).toBe(false);
  });
});

describe("PreviewSurface hosts the handles", () => {
  function silentAudio(): AudioContextLike {
    return {
      destination: {},
      createGain: () => ({ gain: { value: 1 }, connect: () => undefined }),
      createMediaElementSource: () => ({ connect: () => undefined }),
    };
  }

  async function mountSurface(p: Project = project()) {
    mockConvertFileSrc("windows");
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(STAGE_RECT.width);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(STAGE_RECT.height);
    await openProject(p);
    const w = mount(PreviewSurface, { props: { createAudioContext: silentAudio }, attachTo: document.body });
    await flushPromises();
    return w;
  }

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // Named test (brief): the handles are a SIBLING overlay, never part of
  // the stage the layers are composed on.
  it("handles are not inside the stage element", async () => {
    const w = await mountSurface();
    const stage = w.get('[data-testid="preview-stage"]').element;
    const handles = w.get('[data-testid="layout-handles"]').element;
    expect(w.find('[data-testid="layout-box"]').exists()).toBe(true);
    expect(stage.contains(handles)).toBe(false);
    expect(handles.contains(stage)).toBe(false);
    expect(w.get('[data-testid="preview-layers"]').element.contains(handles)).toBe(false);
  });

  it("the handles' box and the preview's picture frame agree", async () => {
    const w = await mountSurface();
    const box = w.get('[data-testid="layout-box"]').element as HTMLElement;
    const frame = w.get('[data-testid="preview-layers"] [data-preview-frame]').element as HTMLElement;
    for (const side of ["left", "top", "width", "height"] as const) {
      expect(parseFloat(frame.style[side])).toBeCloseTo(parseFloat(box.style[side]), 6);
    }
    // The picture inside the frame follows the clip's look (a rounded PiP).
    expect(frame.style.borderRadius).not.toBe("0");
  });

  it("the preview plays a sped-up clip at its speed, with its pitch setting", async () => {
    const w = await mountSurface(project({ clips: [clip({ speed: 2, preserve_pitch: false })] }));
    const video = w.get('[data-testid="preview-layers"] video').element as HTMLVideoElement;
    expect(video.playbackRate).toBe(2);
    expect(video.preservesPitch).toBe(false);
  });

  it("a drag previews in the picture itself, and Escape puts it back", async () => {
    const w = await mountSurface();
    const handles = w.get('[data-testid="layout-handles"]').element as HTMLElement;
    handles.getBoundingClientRect = () =>
      ({ ...STAGE_RECT, x: STAGE_RECT.left, y: STAGE_RECT.top, right: 1010, bottom: 520, toJSON: () => ({}) }) as DOMRect;
    const frame = () => w.get('[data-testid="preview-layers"] [data-preview-frame]').element as HTMLElement;
    const before = frame().style.left;
    const body = w.get('[data-testid="layout-box"]');
    await body.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.6, 0.3) });
    await body.trigger("pointermove", { pointerId: 1, ...clientAt(0.4, 0.3) });
    expect(frame().style.left).not.toBe(before);
    await body.trigger("keydown", { key: "Escape" });
    expect(frame().style.left).toBe(before);
    expect(executed).toEqual([]);
  });
});
