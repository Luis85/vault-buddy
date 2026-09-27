/**
 * `src/editor/fadeCurves.ts` (Task 29; F-17, F-18) and the two paths that
 * send `setFades`: `ClipItem.vue`'s gold-handle drag (`useTimelineDrag.ts`'s
 * `beginFade`/`updateFade`/`endFade`) and `FadesSection.vue`'s numeric
 * fields (`useInspectorDraft`, in seconds since visual-parity Task 15, which
 * also added the fade graph, the presets and Preview entrance). F-17/F-18's own acceptance line — "Handle
 * and numeric edits produce equivalent opacity envelopes" — is exactly why
 * both paths must send the IDENTICAL command for equivalent input; this
 * suite checks that end to end rather than trusting each path's own
 * (already-covered elsewhere) unit tests to agree by construction.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import FadesSection from "../src/components/editor/inspector/FadesSection.vue";
import { useTimelineDrag } from "../src/composables/useTimelineDrag";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { gainAt } from "../src/editor/fadeCurves";
import { fadeGraphPaths } from "../src/editor/fadeGraph";
import { requestedPlaybackMs, revealSerial } from "../src/editor/revealBus";
import { BASE_PX_PER_MS } from "../src/editor/timelineLayout";
import type { Asset, Clip, EditorOpenResult, EditorSnapshot, FadeCurve, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import rawTable from "./fixtures/editor-fade-cases.json";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

const PPM = BASE_PX_PER_MS; // zoom 1

// ---- fixtures (asymmetric: start/in/out are all distinct, the "fixture
// flaw" rule) ----------------------------------------------------------------

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
function asset(id: string, overrides: Partial<Asset> = {}): Asset {
  return { id, kind: "video", name: id, duration_ms: 60_000, ...overrides };
}
function clip(overrides: Partial<Clip> = {}): Clip {
  return {
    id: "c1",
    asset_id: "a1",
    track_id: "v1",
    name: "Intro",
    start_ms: 2_000,
    in_ms: 300,
    out_ms: 1_300, // 1000ms output duration, unit speed -- half-duration limit 500ms
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
    assets: [asset("a1")],
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

// ---- the shared fade-curve fixture table (the `editorTimeFixtures.test.ts`
// precedent): Rust's `fades.rs` reads the SAME file by `include_str!`, so a
// disagreement between the two languages has to redden one of the two
// suites rather than staying invisible with every test in the repo green. --

interface FadeCase {
  name: string;
  curve: FadeCurve;
  u: number;
  gain: number;
}
const table = rawTable as unknown as { version: number; cases: FadeCase[] };

describe("the shared fade-curve fixture table", () => {
  it("the fixture table has 6 cases", () => {
    expect(table.cases).toHaveLength(6);
  });

  it.each(table.cases.map((c) => [c.name, c] as const))("gainAt(%s) agrees with Rust's gain_at", (_name, c) => {
    expect(gainAt(c.curve, c.u)).toBeCloseTo(c.gain, 9);
  });
});

describe("gainAt", () => {
  it("linear is the identity", () => {
    expect(gainAt("linear", 0)).toBe(0);
    expect(gainAt("linear", 0.3)).toBeCloseTo(0.3, 9);
    expect(gainAt("linear", 1)).toBe(1);
  });

  it("smooth eases in and out (not the identity) but still shares linear's endpoints and midpoint", () => {
    expect(gainAt("smooth", 0)).toBe(0);
    expect(gainAt("smooth", 1)).toBe(1);
    expect(gainAt("smooth", 0.5)).toBeCloseTo(0.5, 9);
    expect(gainAt("smooth", 0.25)).toBeCloseTo(0.15625, 9);
  });

  // The named case (brief): a quarter sine's midpoint is 1/sqrt(2), never
  // 0.5 -- the exact mutation ("use linear for equal-power") this pins.
  it("equal-power midpoint is 0.7071, not 0.5", () => {
    const mid = gainAt("equal-power", 0.5);
    expect(mid).toBeCloseTo(0.70710678, 6);
    expect(mid).not.toBeCloseTo(0.5, 3);
  });

  it("clamps u outside [0,1] rather than extrapolating", () => {
    expect(gainAt("linear", -1)).toBe(0);
    expect(gainAt("linear", 2)).toBe(1);
    expect(gainAt("equal-power", -1)).toBe(0);
  });
});

// ---- handle drag vs numeric entry: the same command for the same input ----

function dragDeps(execute: (c: unknown) => unknown, theClip: Clip) {
  return {
    clip: () => theClip,
    zoom: () => 1,
    snapEnabled: () => false,
    snapTargets: () => [] as number[],
    moveTargetClipIds: () => [theClip.id],
    trackOrder: () => ["v1"] as const,
    trackIndex: () => 0,
    trackAccepts: () => true,
    execute,
  };
}

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function withFades(p: Project, command: EditorCommand): Project {
  if (command.kind !== "setFades") return p;
  const patch = (c: Clip): Clip => ({
    ...c,
    fade_in_ms: command.fadeInMs ?? c.fade_in_ms,
    fade_out_ms: command.fadeOutMs ?? c.fade_out_ms,
    fade_curve: command.fadeCurve ?? c.fade_curve,
  });
  return { ...p, clips: p.clips.map((c) => (c.id === command.clipId ? patch(c) : c)) };
}

async function mountFadesSection(overrides: Partial<Project> = {}) {
  const executed: EditorCommand[] = [];
  const store = useEditorProjectStore();
  let current = project(overrides);
  const s = snapshot();
  store.setPort(
    fakePort({
      openStaged: () =>
        Promise.resolve<EditorOpenResult>({
          snapshot: s,
          project: current,
          workspace: {},
          missing: [],
          sourceBase: "base",
          recovered: false,
        }),
      // Applies a setFades, so what the section draws can follow it.
      execute: (req) => {
        executed.push(req.command);
        current = withFades(current, req.command);
        return Promise.resolve({ snapshot: { ...s, revision: s.revision + executed.length }, project: current });
      },
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged("base");
  useEditorWorkspaceStore().select(["c1"]);
  const w = mount(FadesSection, { props: { clipIds: ["c1"] } });
  await flushPromises();
  return { w, executed };
}

describe("handle drag and numeric entry send the same command", () => {
  it("dragging the fade-in handle by 200ms sends exactly what typing 0.2 s into the Fade in field sends", async () => {
    // Drag path: a pure composable call, no mount (the useTimelineDrag.test.ts
    // precedent) -- clip() here is c1, same shape FadesSection below edits.
    const dragExecute = vi.fn();
    const drag = useTimelineDrag(dragDeps(dragExecute, clip()));
    drag.beginFade("in", 0);
    drag.updateFade(200 * PPM);
    await drag.endFade();
    expect(dragExecute).toHaveBeenCalledWith({ kind: "setFades", clipId: "c1", fadeInMs: 200 });

    // Numeric-entry path: FadesSection.vue's own Fade in field, in seconds
    // (visual-parity Task 15), submitted via Enter.
    const { w, executed } = await mountFadesSection();
    const field = w.get('[data-testid="fades-section-fade-in"]');
    await field.setValue("0.2");
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([dragExecute.mock.calls[0][0]]);
  });

  it("dragging the fade-out handle by 150ms sends exactly what typing 0.15 s into the Fade out field sends", async () => {
    const dragExecute = vi.fn();
    const drag = useTimelineDrag(dragDeps(dragExecute, clip()));
    drag.beginFade("out", 500);
    drag.updateFade(500 - 150 * PPM); // leftward lengthens fade-out
    await drag.endFade();
    expect(dragExecute).toHaveBeenCalledWith({ kind: "setFades", clipId: "c1", fadeOutMs: 150 });

    const { w, executed } = await mountFadesSection();
    const field = w.get('[data-testid="fades-section-fade-out"]');
    await field.setValue("0.15");
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([dragExecute.mock.calls[0][0]]);
  });
});

describe("fadeGraphPaths", () => {
  // The concept's `fadeGraph` (editor.js): a 240x70 box, the envelope
  // between x 10 and 230, knees at the fade lengths' share of the clip.
  it("puts the knees at each fade's share of the clip, and draws a straight edge for no fade", () => {
    const g = fadeGraphPaths(250, 0, 1_000);
    expect(g.knees).toEqual([65, 230]);
    expect(g.envelope).toBe("M10 55 Q 23.75 15 65 15 L230 15 L230 15");
    expect(g.fill).toBe("M10 55 L65 15 H230 L230 55Z");
    expect(fadeGraphPaths(0, 500, 1_000).envelope).toBe("M10 55 L10 15 L120 15 Q 202.5 15 230 55");
  });
});

describe("FadesSection", () => {
  // Visual-parity Task 15 (concept spec §5 "Fades", screen 04).
  it("a picture's fades: the heading, the graph, the help and the fields in seconds", async () => {
    const { w } = await mountFadesSection({ clips: [clip({ fade_in_ms: 250 })] });
    expect(w.findAll("h3").map((h) => h.text())).toEqual(["A softer entrance. A cleaner exit.", "Between two clips"]);
    expect(w.get('[data-testid="fades-section-help"]').text()).toBe(
      "Fade to reveal the layer underneath. On the bottom track, fade to black.",
    );
    expect(w.get('[data-testid="fades-section-graph"]').attributes("aria-label")).toBe("Fade envelope");
    expect(w.findAll('[data-testid="fades-section-graph"] circle').map((c) => c.attributes("cx"))).toEqual(["65", "230"]);
    expect((w.get('[data-testid="fades-section-fade-in"]').element as HTMLInputElement).value).toBe("0.25");
    expect(w.get('[data-testid="fades-section-curve"]').findAll("option").map((o) => o.text())).toEqual([
      "Linear", "Smooth", "Equal power (audio)",
    ]);
  });

  it("a sound's fades have their own heading and help", async () => {
    const { w } = await mountFadesSection({
      assets: [asset("a1", { kind: "audio" })],
      tracks: [track("v1", { kind: "audio" })],
    });
    expect(w.findAll("h3")[0].text()).toBe("Let the sound arrive naturally.");
    expect(w.get('[data-testid="fades-section-help"]').text()).toBe("Fade volume up from silence, then down again.");
  });

  it("the graph follows the committed values", async () => {
    const { w } = await mountFadesSection();
    const field = w.get('[data-testid="fades-section-fade-out"]');
    await field.setValue("0.5");
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(w.findAll('[data-testid="fades-section-graph"] circle').map((c) => c.attributes("cx"))).toEqual(["10", "120"]);
  });

  it("None / Quick · 0.5s / Gentle · 1s set both edges, the menu's own rule, and light the current one", async () => {
    // 5000ms of output: half of it allows either preset in full.
    const { w, executed } = await mountFadesSection({ clips: [clip({ out_ms: 5_300, fade_in_ms: 500, fade_out_ms: 500 })] });
    const labels = ["fades-section-preset-0", "fades-section-preset-500", "fades-section-preset-1000"].map((id) =>
      w.get(`[data-testid="${id}"]`).text(),
    );
    expect(labels).toEqual(["None", "Quick · 0.5s", "Gentle · 1s"]);
    expect(w.get('[data-testid="fades-section-preset-500"]').attributes("aria-pressed")).toBe("true");
    expect(w.get('[data-testid="fades-section-preset-0"]').attributes("aria-pressed")).toBe("false");
    await w.get('[data-testid="fades-section-preset-1000"]').trigger("click");
    await w.get('[data-testid="fades-section-preset-0"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "setFades", clipId: "c1", fadeInMs: 1_000, fadeOutMs: 1_000 },
      { kind: "setFades", clipId: "c1", fadeInMs: 0, fadeOutMs: 0 },
    ]);
  });

  it("Preview entrance plays from the clip's start", async () => {
    const { w, executed } = await mountFadesSection();
    const before = revealSerial("playback");
    await w.get('[data-testid="fades-section-preview"]').trigger("click");
    expect(revealSerial("playback")).toBe(before + 1);
    expect(requestedPlaybackMs()).toBe(2_000);
    expect(useEditorWorkspaceStore().playheadMs).toBe(2_000);
    expect(executed).toEqual([]);
  });

  it("an out-of-range fade stays visible with a correction and sends nothing", async () => {
    const { w, executed } = await mountFadesSection();
    const field = w.get('[data-testid="fades-section-fade-in"]');
    await field.setValue("0.9"); // half-duration limit is 500ms
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(executed).toEqual([]);
    expect(w.get('[data-testid="fades-section-fade-in-error"]').text()).toContain("between 0 and 0.5 s");
    expect((field.element as HTMLInputElement).value).toBe("0.9");
  });

  it("Escape reverts the draft to the committed value and sends nothing", async () => {
    const { w, executed } = await mountFadesSection({ clips: [clip({ fade_in_ms: 50 })] });
    const field = w.get('[data-testid="fades-section-fade-in"]');
    await field.setValue("0.4");
    await field.trigger("keydown", { key: "Escape" });
    await flushPromises();
    expect(executed).toEqual([]);
    expect((field.element as HTMLInputElement).value).toBe("0.05");
  });

  it("changing the curve sends setFades with fadeCurve alone", async () => {
    const { w, executed } = await mountFadesSection();
    const select = w.get('[data-testid="fades-section-curve"]');
    await select.setValue("equal-power");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setFades", clipId: "c1", fadeCurve: "equal-power" }]);
  });

  it("a locked track disables the fades and every preset says why; Preview entrance still plays", async () => {
    const { w, executed } = await mountFadesSection({ tracks: [track("v1", { locked: true, name: "Screen" })] });
    expect(w.get('[data-testid="fades-section-fade-in"]').attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="fades-section-curve"]').attributes("title")).toBe("Track Screen is locked");
    expect(w.get('[data-testid="fades-section-preview"]').attributes("aria-disabled")).toBeUndefined();
    const quick = w.get('[data-testid="fades-section-preset-500"]');
    expect(quick.attributes("title")).toBe("Track Screen is locked");
    await quick.trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
  });

  it("a multi-selection shows a note instead of acting on the first clip", async () => {
    const { w } = await mountFadesSection({ clips: [clip(), clip({ id: "c2", start_ms: 4_000 })] });
    useEditorWorkspaceStore().select(["c1", "c2"]);
    const w2 = mount(FadesSection, { props: { clipIds: ["c1", "c2"] } });
    await flushPromises();
    expect(w2.find('[data-testid="fades-section-multi"]').exists()).toBe(true);
    expect(w2.find('[data-testid="fades-section-fade-in"]').exists()).toBe(false);
    w.unmount();
    w2.unmount();
  });
});
