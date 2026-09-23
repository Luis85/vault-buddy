/**
 * Teaching cues in the editor window (Task 35; F-27–F-33): the preview
 * overlay (`CueOverlay.vue`), its separate handles layer (`CueHandles.vue`),
 * the effect inspector (`EffectSection.vue`), the seven teaching-tool
 * actions (`cueActions.ts`, sent from `PreviewToolbar.vue`) and how
 * `PreviewSurface.vue` layers all of it against Task 31's `LayoutHandles`.
 *
 * Fixtures are asymmetric (the "fixture flaw" rule): a speed-2 clip whose
 * start (1000), in (500) and out are all distinct, so a reading that
 * ignored the speed or the in-point lands somewhere else; a stage whose
 * aspect differs from the canvas, so the letterbox must be undone.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import EffectSection from "../src/components/editor/inspector/EffectSection.vue";
import InspectorPanel from "../src/components/editor/inspector/InspectorPanel.vue";
import CueHandles from "../src/components/editor/preview/CueHandles.vue";
import CueOverlay from "../src/components/editor/preview/CueOverlay.vue";
import PreviewSurface from "../src/components/editor/preview/PreviewSurface.vue";
import PreviewToolbar from "../src/components/editor/shell/PreviewToolbar.vue";
import { baseActionContext } from "../src/editor/actionContext";
import { commandFor, resolveActions } from "../src/editor/actions";
import { arrowPath, IDENTITY_ZOOM } from "../src/editor/cueGeometry";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import type { AudioContextLike } from "../src/editor/previewController";
import { containRect } from "../src/editor/previewGeometry";
import type { Clip, EditorOpenResult, Effect, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

const LANDSCAPE = { width: 1280, height: 720 };

// ---- fixtures ---------------------------------------------------------------

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
/** Output 1000..11000 plays source 500..20500 at 2×. Full frame. */
function clip(overrides: Partial<Clip> = {}): Clip {
  return {
    id: "c1", asset_id: "a1", track_id: "v1", name: "Screen",
    start_ms: 1_000, in_ms: 500, out_ms: 20_500,
    fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear",
    opacity: 1, volume: 1, muted: false, x: 0, y: 0, w: 1, h: 1, speed: 2,
    ...overrides,
  };
}
function effect(overrides: Partial<Effect> = {}): Effect {
  return {
    id: "arr", clip_id: "c1", kind: "arrow", start_ms: 1_500, end_ms: 2_500,
    x: 0.2, y: 0.3, x2: 0.6, y2: 0.6, color: "#ffd279", stroke: 5,
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
    assets: [{ id: "a1", kind: "video", name: "a.mp4", duration_ms: 30_000, width: 1920, height: 1080 }],
    tracks: [track("v1")],
    clips: [clip()],
    effects: [effect()],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

let executed: EditorCommand[];
/** What the fake `execute` answers with — a test sets it to model Rust. */
let reply: (cmd: EditorCommand, p: Project) => Project;

async function openProject(p: Project = project()) {
  executed = [];
  reply = (_cmd, cur) => cur;
  const store = useEditorProjectStore();
  let current = p;
  const snapshot = {
    sessionId: "ses-a", projectId: "project-a", revision: 1, persistedRevision: null, title: "Tutorial",
    durationMs: 11_000, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  };
  const opened: EditorOpenResult = {
    snapshot, project: p, workspace: {}, missing: [], sourceBase: "base", recovered: false,
  };
  let revision = 1;
  store.setPort(
    fakeEditorPort({
      openStaged: () => Promise.resolve(opened),
      execute: (req) => {
        executed.push(req.command);
        current = reply(req.command, current);
        revision += 1;
        return Promise.resolve({ snapshot: { ...snapshot, revision }, project: current });
      },
      mediaUrl: () => Promise.resolve("C:\\x\\a.mp4"),
      getWorkspace: () => Promise.resolve({}),
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged("base");
}

function selectEffect(id = "arr", clipId = "c1") {
  const ws = useEditorWorkspaceStore();
  ws.select([clipId]);
  ws.setSelected({ type: "effect", id });
}

/** 1280x720 in a 1000x500 stage: pillarboxed, scale 500/720. */
const STAGE_RECT = { left: 10, top: 20, width: 1000, height: 500 };
const FRAME = containRect(LANDSCAPE, STAGE_RECT);
const SCALE = 500 / 720;
function clientAt(nx: number, ny: number): { clientX: number; clientY: number } {
  return {
    clientX: STAGE_RECT.left + FRAME.left + nx * LANDSCAPE.width * SCALE,
    clientY: STAGE_RECT.top + FRAME.top + ny * LANDSCAPE.height * SCALE,
  };
}
function stubRect(el: Element) {
  (el as HTMLElement).getBoundingClientRect = () =>
    ({ ...STAGE_RECT, x: STAGE_RECT.left, y: STAGE_RECT.top, right: 1010, bottom: 520, toJSON: () => ({}) }) as DOMRect;
}

beforeEach(() => {
  setActivePinia(createPinia());
});

// ---- CueOverlay ----------------------------------------------------------------

describe("CueOverlay", () => {
  function mountOverlay(p: Project, timeMs: number, draft: Effect | null = null) {
    return mount(CueOverlay, { props: { project: p, timeMs, frame: FRAME, zoom: IDENTITY_ZOOM, draft } });
  }

  // Named test (brief). At output 1700 on the 2× clip (start 1000, in 500):
  // `now` (source 1500..2500 → output 1500..2000) is on; `early` (source
  // 1200..1300 → output 1350..1400) is off — but a reading that ignored the
  // speed (1000 + 700 = 1700) would draw it; `hidden` is on a hidden track.
  it("only cues intersecting the playhead render", () => {
    const p = project({
      tracks: [track("v1"), track("v2", { visible: false })],
      clips: [clip(), clip({ id: "c2", track_id: "v2" })],
      effects: [
        effect({ id: "now", kind: "highlight", w: 0.2, h: 0.1, stroke: 4 }),
        effect({ id: "early", kind: "text", start_ms: 1_200, end_ms: 1_300, text: "Hi", fontSize: 32, w: 0.4, h: 0.15 }),
        effect({ id: "hidden", clip_id: "c2", kind: "mask", w: 0.2, h: 0.2 }),
      ],
    });
    const w = mountOverlay(p, 1_700);
    const drawn = w.findAll('[data-testid^="cue-"][data-kind]').map((g) => g.attributes("data-testid"));
    expect(drawn).toEqual(["cue-now"]);
    expect(mountOverlay(p, 2_000).findAll("[data-kind]")).toHaveLength(0);
  });

  it("draws the arrow as the shared polygons, in canvas px over the letterboxed frame", () => {
    const w = mountOverlay(project(), 1_600);
    const svg = w.get('[data-testid="cue-overlay"]');
    expect(svg.attributes("viewBox")).toBe("0 0 1280 720");
    const style = (svg.element as unknown as HTMLElement).style;
    expect(parseFloat(style.left)).toBeCloseTo(FRAME.left, 6);
    expect(parseFloat(style.width)).toBeCloseTo(FRAME.width, 6);
    const path = arrowPath(0.2, 0.3, 0.6, 0.6, 5, LANDSCAPE)!;
    const points = (pts: [number, number][]) => pts.map(([x, y]) => `${x},${y}`).join(" ");
    expect(w.get('[data-testid="cue-arr"] [data-part="shaft"]').attributes("points")).toBe(points(path.shaft));
    expect(w.get('[data-testid="cue-arr"] [data-part="head"]').attributes("points")).toBe(points(path.head));
    expect(w.get('[data-testid="cue-arr"] [data-part="head"]').attributes("fill")).toBe("#ffd279");
  });

  it("draws every kind: text, highlight, spotlight, step and an opaque mask", () => {
    const p = project({
      effects: [
        effect({ id: "t", kind: "text", text: "Line one\nLine two", fontSize: 40, w: 0.4, h: 0.15, background: true, color: "#ffffff" }),
        effect({ id: "h", kind: "highlight", w: 0.2, h: 0.1, stroke: 6 }),
        effect({ id: "s", kind: "spotlight", w: 0.3, h: 0.25, dim: 0.5 }),
        effect({ id: "n", kind: "step", number: 7, text: "Click Save" }),
        effect({ id: "m", kind: "mask", w: 0.25, h: 0.1, color: "#101010" }),
        effect({ id: "z", kind: "zoom", factor: 2, easing: 0 }),
      ],
    });
    const w = mountOverlay(p, 1_600);
    expect(w.findAll('[data-testid="cue-t"] tspan').map((t) => t.text())).toEqual(["Line one", "Line two"]);
    expect(w.get('[data-testid="cue-t"] text').attributes("font-size")).toBe("40");
    expect(w.find('[data-testid="cue-t"] [data-part="background"]').exists()).toBe(true);
    expect(w.get('[data-testid="cue-h"] rect').attributes("stroke-width")).toBe("6");
    expect(w.findAll('[data-testid="cue-s"] [data-part="dim"]')).toHaveLength(4);
    expect(w.get('[data-testid="cue-s"] [data-part="dim"]').attributes("fill-opacity")).toBe("0.5");
    expect(w.get('[data-testid="cue-n"]').text()).toContain("7");
    expect(w.get('[data-testid="cue-n"]').text()).toContain("Click Save");
    const mask = w.get('[data-testid="cue-m"] rect');
    expect(mask.attributes("fill")).toBe("#101010");
    expect(mask.attributes("width")).toBe(String(0.25 * 1280));
    // A zoom draws nothing into the picture: it IS the stage transform.
    expect(w.find('[data-testid="cue-z"]').exists()).toBe(false);
  });

  it("a drag's transient effect replaces the committed one while it lasts", () => {
    const w = mountOverlay(project(), 1_600, effect({ x2: 0.9, y2: 0.1 }));
    const path = arrowPath(0.2, 0.3, 0.9, 0.1, 5, LANDSCAPE)!;
    expect(w.get('[data-part="head"]').attributes("points")).toContain(`${path.head[0][0]},${path.head[0][1]}`);
  });
});

// ---- CueHandles ------------------------------------------------------------------

describe("CueHandles", () => {
  function mountHandles(timeMs = 1_600) {
    const w = mount(CueHandles, {
      props: { frame: FRAME, canvas: LANDSCAPE, timeMs, zoom: IDENTITY_ZOOM },
      attachTo: document.body,
    });
    stubRect(w.get('[data-testid="cue-handles"]').element);
    return w;
  }

  // Named test (brief). Preview only while dragging (the store is never
  // touched, R14), then exactly ONE updateEffect on release.
  it("endpoint drag sends one updateEffect", async () => {
    await openProject();
    selectEffect();
    const w = mountHandles();
    const end = w.get('[data-testid="cue-handle-end"]');
    await end.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.6, 0.6) });
    await end.trigger("pointermove", { pointerId: 1, ...clientAt(0.65, 0.55) });
    await end.trigger("pointermove", { pointerId: 1, ...clientAt(0.7, 0.5) });
    expect(executed).toEqual([]);
    const previews = w.emitted("preview") as [Effect | null][];
    expect(previews[previews.length - 1][0]).toMatchObject({ id: "arr", x2: 0.7, y2: 0.5 });
    expect(useEditorProjectStore().project!.effects[0].x2).toBe(0.6);

    await end.trigger("pointerup", { pointerId: 1, ...clientAt(0.7, 0.5) });
    await flushPromises();
    expect(executed).toEqual([{ kind: "updateEffect", effectId: "arr", props: { x2: 0.7, y2: 0.5 } }]);
    expect(previews[previews.length - 1][0]).toBeNull();
  });

  it("the tail handle moves only the tail, clamped inside the canvas", async () => {
    await openProject();
    selectEffect();
    const w = mountHandles();
    const start = w.get('[data-testid="cue-handle-start"]');
    await start.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.2, 0.3) });
    await start.trigger("pointermove", { pointerId: 1, ...clientAt(-0.3, 0.35) });
    await start.trigger("pointerup", { pointerId: 1, ...clientAt(-0.3, 0.35) });
    await flushPromises();
    expect(executed).toEqual([{ kind: "updateEffect", effectId: "arr", props: { x: 0, y: 0.35 } }]);
  });

  it("Escape mid-drag sends nothing; a click without movement sends nothing", async () => {
    await openProject();
    selectEffect();
    const w = mountHandles();
    const end = w.get('[data-testid="cue-handle-end"]');
    await end.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.6, 0.6) });
    await end.trigger("pointermove", { pointerId: 1, ...clientAt(0.1, 0.1) });
    await end.trigger("keydown", { key: "Escape" });
    await end.trigger("pointerup", { pointerId: 1, ...clientAt(0.1, 0.1) });
    await end.trigger("pointerdown", { button: 0, pointerId: 2, ...clientAt(0.6, 0.6) });
    await end.trigger("pointerup", { pointerId: 2, ...clientAt(0.6, 0.6) });
    await flushPromises();
    expect(executed).toEqual([]);
  });

  it("clicking an unselected cue selects it (and its clip); a box cue drags and resizes", async () => {
    await openProject(project({ effects: [effect({ id: "hl", kind: "highlight", x: 0.2, y: 0.3, w: 0.2, h: 0.1, stroke: 4 })] }));
    const w = mountHandles();
    expect(w.find('[data-testid="cue-handle-resize"]').exists()).toBe(false);
    await w.get('[data-testid="cue-hit-hl"]').trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.3, 0.35) });
    const ws = useEditorWorkspaceStore();
    expect(ws.selected).toEqual({ type: "effect", id: "hl" });
    expect(ws.selectionClipIds).toEqual(["c1"]);
    await flushPromises();
    const resize = w.get('[data-testid="cue-handle-resize"]');
    await resize.trigger("pointerdown", { button: 0, pointerId: 3, ...clientAt(0.4, 0.4) });
    await resize.trigger("pointermove", { pointerId: 3, ...clientAt(0.5, 0.45) });
    await resize.trigger("pointerup", { pointerId: 3, ...clientAt(0.5, 0.45) });
    const move = w.get('[data-testid="cue-hit-hl"]');
    await move.trigger("pointerdown", { button: 0, pointerId: 4, ...clientAt(0.3, 0.35) });
    await move.trigger("pointermove", { pointerId: 4, ...clientAt(0.25, 0.3) });
    await move.trigger("pointerup", { pointerId: 4, ...clientAt(0.25, 0.3) });
    await flushPromises();
    expect(executed).toEqual([
      { kind: "updateEffect", effectId: "hl", props: { w: 0.3, h: 0.15 } },
      { kind: "updateEffect", effectId: "hl", props: { x: 0.15, y: 0.25 } },
    ]);
  });

  it("a zoom shows its focal marker, which drags the focal point", async () => {
    await openProject(project({ effects: [effect({ id: "z", kind: "zoom", x: 0.5, y: 0.4, factor: 2, easing: 0 })] }));
    selectEffect("z");
    const w = mountHandles();
    const focal = w.get('[data-testid="cue-handle-focal"]');
    await focal.trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.5, 0.4) });
    await focal.trigger("pointermove", { pointerId: 1, ...clientAt(0.6, 0.3) });
    await focal.trigger("pointerup", { pointerId: 1, ...clientAt(0.6, 0.3) });
    await flushPromises();
    expect(executed).toEqual([{ kind: "updateEffect", effectId: "z", props: { x: 0.6, y: 0.3 } }]);
  });

  it("nothing for a cue that is not on screen, and no handles on a locked track", async () => {
    await openProject(project({ tracks: [track("v1", { locked: true })] }));
    selectEffect();
    expect(mountHandles().find('[data-testid="cue-handle-end"]').exists()).toBe(false);
    expect(mountHandles(5_000).find('[data-testid="cue-hit-arr"]').exists()).toBe(false);
  });
});

// ---- PreviewSurface layering (Task 31's carried finding) ------------------------------

describe("PreviewSurface layers cues against the layout handles", () => {
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
    useEditorWorkspaceStore().select(["c1"]);
    useEditorWorkspaceStore().setPlayhead(1_600);
    const w = mount(PreviewSurface, { props: { createAudioContext: silentAudio }, attachTo: document.body });
    await flushPromises();
    return w;
  }
  afterEach(() => {
    vi.restoreAllMocks();
  });

  // Regression (Task 31 carry): a FULL-FRAME selected clip's layout box
  // covered the whole canvas and swallowed every pointerdown on the stage,
  // so no cue under it could be picked, and it showed even with the
  // playhead outside that clip.
  it("a full-frame layout box does not swallow a cue, and hides outside its clip", async () => {
    const w = await mountSurface();
    const stage = w.get('[data-testid="preview-stage"]').element;
    const layout = w.get('[data-testid="layout-handles"]').element;
    const cues = w.get('[data-testid="cue-handles"]').element;
    // Picture cues are part of the stage; their handles are a sibling of it,
    // painted AFTER (above) the layout handles.
    expect(stage.contains(w.get('[data-testid="cue-overlay"]').element)).toBe(true);
    expect(stage.contains(cues)).toBe(false);
    expect(layout.compareDocumentPosition(cues) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(cues.classList.contains("pointer-events-none")).toBe(true);
    expect(w.find('[data-testid="layout-box"]').exists()).toBe(true);

    await w.get('[data-testid="cue-hit-arr"]').trigger("pointerdown", { button: 0, pointerId: 1, ...clientAt(0.4, 0.45) });
    await flushPromises();
    expect(useEditorWorkspaceStore().selected).toEqual({ type: "effect", id: "arr" });
    // The cue owns the pointer now: its endpoints are up, the layout box is not.
    expect(w.find('[data-testid="cue-handle-end"]').exists()).toBe(true);
    expect(w.find('[data-testid="layout-box"]').exists()).toBe(false);

    // A click on the bare stage drops the cue selection; the box returns.
    await w.get('[data-testid="preview-stage"]').trigger("pointerdown", { button: 0, ...clientAt(0.9, 0.9) });
    await flushPromises();
    expect(useEditorWorkspaceStore().selected).toBeNull();
    expect(w.find('[data-testid="layout-box"]').exists()).toBe(true);

    // Playhead before the clip's output start (1000): no box to grab.
    useEditorWorkspaceStore().setPlayhead(500);
    await flushPromises();
    expect(w.find('[data-testid="layout-box"]').exists()).toBe(false);
  });

  it("a zoom cue transforms the picture layers, not the stage", async () => {
    const w = await mountSurface(project({ effects: [effect({ id: "z", kind: "zoom", x: 0.5, y: 0.5, factor: 2, easing: 0 })] }));
    const layers = w.get('[data-testid="preview-layers"]').element as HTMLElement;
    expect(layers.style.transform).toContain("scale(2)");
    // Zoomed in, the layers are clipped to the canvas frame (pillarboxed
    // here: 55.6 px bars left and right), never spilling into the bars.
    const clip = (layers.parentElement as HTMLElement).style.clipPath;
    expect(clip).toMatch(/^inset\(0px 55\.5\d+px 0px 55\.5\d+px\)$/);
    expect(w.get('[data-testid="preview-stage"]').attributes("style") ?? "").not.toContain("transform");
    useEditorWorkspaceStore().setPlayhead(3_000);
    await flushPromises();
    expect(layers.style.transform).toContain("scale(1)");
    expect((layers.parentElement as HTMLElement).style.clipPath).toBe("");
  });
});

// ---- teaching-tool actions ---------------------------------------------------------

describe("teaching-tool actions", () => {
  // Named test (brief). The playhead is OUTPUT time; a cue's times are
  // SOURCE time. On the 2× clip (start 1000, in 500) output 2000 plays
  // source 500 + 1000·2 = 2500, and the default 3000 ms of output is 6000
  // ms of source.
  it("adding a cue converts the playhead to source time", async () => {
    await openProject(project({ effects: [] }));
    reply = (cmd, cur) =>
      cmd.kind === "addEffect" ? { ...cur, effects: [...cur.effects, effect({ id: "fx-new", kind: "text" })] } : cur;
    const ws = useEditorWorkspaceStore();
    ws.select(["c1"]);
    ws.setPlayhead(2_000);
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    await w.get('[data-testid="preview-toolbar-addText"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "addEffect", clipId: "c1", effectKind: "text", startMs: 2_500, endMs: 8_500, props: {} },
    ]);
    // The new cue is selected, ready for the inspector.
    expect(ws.selected).toEqual({ type: "effect", id: "fx-new" });
  });

  it("the default span stops at the clip's source end when that is sooner", async () => {
    // Output end 1000 + (9500-500)/2 = 5500; output 4000 plays source 6500.
    const p = project({ clips: [clip({ out_ms: 9_500 })], effects: [] });
    const ctx = baseActionContext(p, snapshotOf(), 4_000, ["c1"]);
    expect(commandFor("addArrow", ctx)).toEqual({
      kind: "addEffect", clipId: "c1", effectKind: "arrow", startMs: 6_500, endMs: 9_500, props: {},
    });
  });

  it("with nothing selected, the topmost visible video clip under the stage centre is the target", () => {
    // tracks[0] paints on top: `pip` (not over the centre) is skipped for
    // `full` below it; the hidden and audio tracks never count.
    const p = project({
      tracks: [track("top"), track("hid", { visible: false }), track("mid"), track("aud", { kind: "audio" })],
      clips: [
        clip({ id: "pip", track_id: "top", x: 0.7, y: 0.7, w: 0.25, h: 0.25 }),
        clip({ id: "ghost", track_id: "hid" }),
        clip({ id: "full", track_id: "mid", speed: 1 }),
        clip({ id: "music", track_id: "aud" }),
      ],
      effects: [effect({ id: "s1", kind: "step", number: 1, text: "a" }), effect({ id: "s2", kind: "step", number: 2, text: "b" })],
    });
    const ctx = baseActionContext(p, snapshotOf(), 1_200, []);
    expect(commandFor("addStep", ctx)).toEqual({
      kind: "addEffect", clipId: "full", effectKind: "step", startMs: 700, endMs: 3_700, props: { number: 3 },
    });
    // Over the centre, the upper clip wins.
    const covered = { ...p, clips: p.clips.map((c) => (c.id === "pip" ? { ...c, x: 0.3, y: 0.3 } : c)) };
    expect(commandFor("addMask", baseActionContext(covered, snapshotOf(), 1_200, []))).toMatchObject({ clipId: "pip" });
  });

  it("disabled, with a reason, when there is nothing to annotate or the clip's track is locked", () => {
    const p = project({ effects: [] });
    const outside = resolveActions(baseActionContext(p, snapshotOf(), 200, []));
    expect(outside.addZoom).toMatchObject({ enabled: false, reason: "No visible clip at the playhead to add a cue to" });
    const locked = project({ tracks: [track("v1", { locked: true, name: "Screen" })], effects: [] });
    expect(resolveActions(baseActionContext(locked, snapshotOf(), 2_000, ["c1"])).addHighlight).toMatchObject({
      enabled: false,
      reason: "Track Screen is locked",
    });
    expect(resolveActions(baseActionContext(null, null, 0, [])).addText.enabled).toBe(false);
    // The selected clip is used only while the playhead is on it.
    const before = resolveActions(baseActionContext(p, snapshotOf(), 200, ["c1"]));
    expect(before.addSpotlight.enabled).toBe(false);
  });

  it("a full project refuses another cue", () => {
    const many = Array.from({ length: 1_200 }, (_, i) => effect({ id: `e${i}` }));
    const p = project({ effects: many });
    expect(resolveActions(baseActionContext(p, snapshotOf(), 2_000, ["c1"])).addText).toMatchObject({
      enabled: false,
      reason: "This project already has the maximum of 1200 teaching cues",
    });
  });
});

function snapshotOf() {
  return {
    sessionId: "ses-a", projectId: "project-a", revision: 1, persistedRevision: null, title: "Tutorial",
    durationMs: 11_000, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  };
}

// ---- EffectSection + InspectorPanel ----------------------------------------------------

const MASK_WARNING = "Covers pixels only while visible. It does not track motion and the original recording is unchanged.";

describe("EffectSection", () => {
  function mountSection(id: string) {
    return mount(EffectSection, { props: { effectId: id } });
  }
  async function type(w: ReturnType<typeof mountSection>, testid: string, value: string) {
    const input = w.get(`[data-testid="${testid}"]`);
    await input.setValue(value);
    await input.trigger("keydown", { key: "Enter" });
    await flushPromises();
  }

  // Named test (brief): the privacy cover's limitation is permanent copy,
  // not a dismissible hint — F-33 "Warn about motion/layers and uncensored
  // originals".
  it("mask inspector always shows the limitation warning", async () => {
    await openProject(project({ effects: [effect({ id: "m", kind: "mask", w: 0.25, h: 0.1 })] }));
    const w = mountSection("m");
    expect(w.get('[data-testid="effect-mask-warning"]').text()).toBe(MASK_WARNING);
    await type(w, "effect-field-w", "30");
    expect(executed).toEqual([{ kind: "updateEffect", effectId: "m", props: { w: 0.3 } }]);
    expect(w.get('[data-testid="effect-mask-warning"]').text()).toBe(MASK_WARNING);
    // No close control exists for it.
    expect(w.find('[data-testid="effect-mask-warning"] button').exists()).toBe(false);
    // Only the mask carries it.
    expect(mountSection("m").find('[data-testid="effect-mask-warning"]').exists()).toBe(true);
  });

  it("no warning for any other kind", async () => {
    await openProject();
    expect(mountSection("arr").find('[data-testid="effect-mask-warning"]').exists()).toBe(false);
  });

  it("shows start/end in OUTPUT time and commits SOURCE time", async () => {
    await openProject();
    const w = mountSection("arr");
    // Source 1500..2500 on the 2× clip = output 1500..2000.
    expect((w.get('[data-testid="effect-field-start"]').element as HTMLInputElement).value).toBe("1500");
    expect((w.get('[data-testid="effect-field-end"]').element as HTMLInputElement).value).toBe("2000");
    await type(w, "effect-field-start", "1600");
    await type(w, "effect-field-end", "2100");
    expect(executed).toEqual([
      { kind: "updateEffect", effectId: "arr", startMs: 1_700 },
      { kind: "updateEffect", effectId: "arr", endMs: 2_700 },
    ]);
    await type(w, "effect-field-start", "900");
    expect(w.get('[data-testid="effect-field-start-error"]').text()).toBe("Start must be between 1000 and 11000 ms");
  });

  it("edits every prop of its kind: arrow endpoints, stroke and colour", async () => {
    await openProject();
    const w = mountSection("arr");
    expect(w.findAll("[data-testid^=effect-field-]").map((f) => f.attributes("data-testid"))).toEqual([
      "effect-field-start", "effect-field-end", "effect-field-x", "effect-field-y",
      "effect-field-x2", "effect-field-y2", "effect-field-stroke", "effect-field-color",
    ]);
    await type(w, "effect-field-x2", "75");
    await type(w, "effect-field-stroke", "9");
    await type(w, "effect-field-stroke", "30");
    expect(w.get('[data-testid="effect-field-stroke-error"]').text()).toBe("Stroke must be between 1 and 20");
    const color = w.get('[data-testid="effect-field-color"]');
    // `setValue` on a colour input fires its `change` itself.
    await color.setValue("#00ff88");
    await flushPromises();
    // Re-picking the colour the cue already has sends nothing.
    await color.setValue("#ffd279");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "updateEffect", effectId: "arr", props: { x2: 0.75 } },
      { kind: "updateEffect", effectId: "arr", props: { stroke: 9 } },
      { kind: "updateEffect", effectId: "arr", props: { color: "#00ff88" } },
    ]);
  });

  it("text: copy, size, background; step: number; spotlight: dim; zoom: factor and easing", async () => {
    await openProject(
      project({
        effects: [
          effect({ id: "t", kind: "text", text: "Hi", fontSize: 32, w: 0.4, h: 0.15, background: false }),
          effect({ id: "n", kind: "step", number: 1, text: "Go" }),
          effect({ id: "s", kind: "spotlight", w: 0.3, h: 0.3, dim: 0.65 }),
          effect({ id: "z", kind: "zoom", factor: 2, easing: 0 }),
        ],
      }),
    );
    const t = mountSection("t");
    await type(t, "effect-field-text", "Open settings");
    await type(t, "effect-field-fontSize", "48");
    await t.get('[data-testid="effect-field-background"]').setValue(true);
    await flushPromises();
    const n = mountSection("n");
    await type(n, "effect-field-number", "4");
    const s = mountSection("s");
    expect(s.find('[data-testid="effect-field-color"]').exists()).toBe(false);
    await type(s, "effect-field-dim", "40");
    const z = mountSection("z");
    await type(z, "effect-field-factor", "2.5");
    await type(z, "effect-field-easing", "600");
    expect(executed).toEqual([
      { kind: "updateEffect", effectId: "t", props: { text: "Open settings" } },
      { kind: "updateEffect", effectId: "t", props: { fontSize: 48 } },
      { kind: "updateEffect", effectId: "t", props: { background: true } },
      { kind: "updateEffect", effectId: "n", props: { number: 4 } },
      { kind: "updateEffect", effectId: "s", props: { dim: 0.4 } },
      { kind: "updateEffect", effectId: "z", props: { factor: 2.5 } },
      { kind: "updateEffect", effectId: "z", props: { easing: 600 } },
    ]);
  });

  it("Remove cue sends removeEffect and drops the cue selection", async () => {
    await openProject();
    selectEffect();
    const w = mountSection("arr");
    await w.get('[data-testid="effect-remove"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "removeEffect", effectId: "arr" }]);
    expect(useEditorWorkspaceStore().selected).toBeNull();
  });
});

describe("InspectorPanel with a selected cue", () => {
  it("shows the effect slot instead of the clip categories, with a way back", async () => {
    await openProject();
    selectEffect();
    const w = mount(InspectorPanel, {
      slots: { effect: '<template #effect="{ effectId }"><p data-testid="slot">{{ effectId }}</p></template>' },
    });
    expect(w.get('[data-testid="slot"]').text()).toBe("arr");
    expect(w.find('[data-testid="inspector-tablist"]').exists()).toBe(false);
    await w.get('[data-testid="inspector-effect-back"]').trigger("click");
    expect(w.find('[data-testid="inspector-tablist"]').exists()).toBe(true);
    // A cue whose clip is no longer the selection is not "selected".
    selectEffect("arr", "other");
    await flushPromises();
    expect(w.find('[data-testid="slot"]').exists()).toBe(false);
  });
});

// ---- edges: locked tracks, stray input, refusals ---------------------------------------

describe("teaching cues — edges", () => {
  it("EffectSection on a locked track: every control disabled, nothing sent", async () => {
    await openProject(project({ tracks: [track("v1", { locked: true, name: "Screen" })] }));
    const w = mount(EffectSection, { props: { effectId: "arr" } });
    expect(w.get('[data-testid="effect-section-locked"]').text()).toContain("Track Screen is locked");
    expect(w.get('[data-testid="effect-field-x2"]').attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="effect-remove"]').attributes("disabled")).toBeDefined();
    // Even a submit is refused before it reaches Rust.
    await w.get('[data-testid="effect-field-x2"]').setValue("80");
    await w.get('[data-testid="effect-field-x2"]').trigger("keydown", { key: "Enter" });
    await w.get('[data-testid="effect-field-start"]').setValue("1600");
    await w.get('[data-testid="effect-field-start"]').trigger("keydown", { key: "Enter" });
    await w.get('[data-testid="effect-remove"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
  });

  it("EffectSection renders nothing for a cue that no longer exists", async () => {
    await openProject();
    const w = mount(EffectSection, { props: { effectId: "gone" } });
    expect(w.find('[data-testid="effect-section"]').exists()).toBe(false);
  });

  it("CueHandles ignores other buttons, stray moves and releases, and a locked cue's drag", async () => {
    await openProject(project({ tracks: [track("v1", { locked: true })] }));
    const w = mount(CueHandles, {
      props: { frame: { left: 0, top: 0, width: 0, height: 0 }, canvas: LANDSCAPE, timeMs: 1_600, zoom: IDENTITY_ZOOM },
      attachTo: document.body,
    });
    const hit = w.get('[data-testid="cue-hit-arr"]');
    await hit.trigger("pointerdown", { button: 2, pointerId: 1 });
    expect(useEditorWorkspaceStore().selected).toBeNull();
    await hit.trigger("pointermove", { pointerId: 1 });
    await hit.trigger("pointerup", { pointerId: 1 });
    await hit.trigger("keydown", { key: "Escape" });
    // Selecting a locked cue works; dragging it does not.
    await hit.trigger("pointerdown", { button: 0, pointerId: 1 });
    expect(useEditorWorkspaceStore().selected).toEqual({ type: "effect", id: "arr" });
    await hit.trigger("pointerdown", { button: 0, pointerId: 2, clientX: 10, clientY: 10 });
    await hit.trigger("pointermove", { pointerId: 2, clientX: 90, clientY: 90 });
    await hit.trigger("pointerup", { pointerId: 2, clientX: 90, clientY: 90 });
    await flushPromises();
    expect(executed).toEqual([]);
    expect(w.emitted("preview")).toBeUndefined();
  });

  it("a toolbar add that Rust refuses selects nothing", async () => {
    await openProject(project({ effects: [] }));
    const store = useEditorProjectStore();
    const port = store.port;
    store.setPort({ ...port, execute: () => Promise.reject(new Error("refused")) });
    const ws = useEditorWorkspaceStore();
    ws.select(["c1"]);
    ws.setPlayhead(2_000);
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    await w.get('[data-testid="preview-toolbar-addMask"]').trigger("click");
    await flushPromises();
    expect(ws.selected).toBeNull();
  });

  it("an add whose reply carries no new cue selects nothing", async () => {
    await openProject(project({ effects: [] }));
    const ws = useEditorWorkspaceStore();
    ws.select(["c1"]);
    ws.setPlayhead(2_000);
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    await w.get('[data-testid="preview-toolbar-addHighlight"]').trigger("click");
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect(ws.selected).toBeNull();
  });
});
