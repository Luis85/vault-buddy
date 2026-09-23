/**
 * `src/editor/fadeCurves.ts` (Task 29; F-17, F-18) and the two paths that
 * send `setFades`: `ClipItem.vue`'s gold-handle drag (`useTimelineDrag.ts`'s
 * `beginFade`/`updateFade`/`endFade`) and `FadesSection.vue`'s numeric
 * fields (`useInspectorDraft`). F-17/F-18's own acceptance line — "Handle
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
import { gainAt } from "../src/editor/fadeCurves";
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

async function mountFadesSection(overrides: Partial<Project> = {}) {
  const executed: unknown[] = [];
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
  it("dragging the fade-in handle by 200ms sends exactly what typing 200 into the Fade in field sends", async () => {
    // Drag path: a pure composable call, no mount (the useTimelineDrag.test.ts
    // precedent) -- clip() here is c1, same shape FadesSection below edits.
    const dragExecute = vi.fn();
    const drag = useTimelineDrag(dragDeps(dragExecute, clip()));
    drag.beginFade("in", 0);
    drag.updateFade(200 * PPM);
    await drag.endFade();
    expect(dragExecute).toHaveBeenCalledWith({ kind: "setFades", clipId: "c1", fadeInMs: 200 });

    // Numeric-entry path: FadesSection.vue's own Fade in field, submitted
    // via Enter -- the useInspectorDraft precedent (editorClipSection.test.ts).
    const { w, executed } = await mountFadesSection();
    const field = w.get('[data-testid="fades-section-fade-in"]');
    await field.setValue("200");
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([dragExecute.mock.calls[0][0]]);
  });

  it("dragging the fade-out handle by 150ms sends exactly what typing 150 into the Fade out field sends", async () => {
    const dragExecute = vi.fn();
    const drag = useTimelineDrag(dragDeps(dragExecute, clip()));
    drag.beginFade("out", 500);
    drag.updateFade(500 - 150 * PPM); // leftward lengthens fade-out
    await drag.endFade();
    expect(dragExecute).toHaveBeenCalledWith({ kind: "setFades", clipId: "c1", fadeOutMs: 150 });

    const { w, executed } = await mountFadesSection();
    const field = w.get('[data-testid="fades-section-fade-out"]');
    await field.setValue("150");
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([dragExecute.mock.calls[0][0]]);
  });
});

describe("FadesSection", () => {
  it("an out-of-range fade stays visible with a correction and sends nothing", async () => {
    const { w, executed } = await mountFadesSection();
    const field = w.get('[data-testid="fades-section-fade-in"]');
    await field.setValue("999"); // half-duration limit is 500ms
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(executed).toEqual([]);
    expect(w.get('[data-testid="fades-section-fade-in-error"]').text()).toContain("between 0 and 500");
    expect((field.element as HTMLInputElement).value).toBe("999");
  });

  it("Escape reverts the draft to the committed value and sends nothing", async () => {
    const { w, executed } = await mountFadesSection({ clips: [clip({ fade_in_ms: 50 })] });
    const field = w.get('[data-testid="fades-section-fade-in"]');
    await field.setValue("400");
    await field.trigger("keydown", { key: "Escape" });
    await flushPromises();
    expect(executed).toEqual([]);
    expect((field.element as HTMLInputElement).value).toBe("50");
  });

  it("changing the curve sends setFades with fadeCurve alone", async () => {
    const { w, executed } = await mountFadesSection();
    const select = w.get('[data-testid="fades-section-curve"]');
    await select.setValue("equal-power");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setFades", clipId: "c1", fadeCurve: "equal-power" }]);
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
