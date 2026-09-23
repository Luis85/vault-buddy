/**
 * Transitions on the frontend (Task 30; F-19): the `transition` action
 * (`actions.ts` over `transitionRules.ts` — the clip context menu's and the
 * Fades inspector's "Add transition") and the Fades inspector's transition
 * rows (`TransitionRow.vue`), which send `setTransitionDuration`/
 * `removeTransition` directly. Rust (`core::editor::commands::transitions`)
 * is the authority for every rule; these tests pin that the frontend
 * offers only what Rust would accept for the reasons the graph alone can
 * decide, and says why when it does not.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import FadesSection from "../src/components/editor/inspector/FadesSection.vue";
import { type ActionContext, commandFor, resolveActions } from "../src/editor/actions";
import { NOT_ADJACENT } from "../src/editor/transitionRules";
import type { Asset, Clip, EditorOpenResult, EditorSnapshot, Project, Track, Transition } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

// ---- fixtures (asymmetric: every clip a different length, c2 at 1.5x) -----

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}
function asset(id: string, overrides: Partial<Asset> = {}): Asset {
  return { id, kind: "video", name: id, duration_ms: 60_000, ...overrides };
}
function clip(id: string, start: number, inMs: number, outMs: number, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "av",
    track_id: "v1",
    name: `Clip ${id}`,
    start_ms: start,
    in_ms: inMs,
    out_ms: outMs,
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

/** v1: c1 [400,1700) (1300ms), c2 [1700,3700) (1.5x: 3000ms of source is
 * 2000ms of output), then a GAP, c3 [4200,5100). a1: y1 [0,1100) and
 * y2 [1100,2000). */
function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [asset("av"), asset("aa", { kind: "audio" })],
    tracks: [track("v1"), track("a1", { kind: "audio" })],
    clips: [
      clip("c1", 400, 100, 1_400),
      clip("c2", 1_700, 2_000, 5_000, { speed: 1.5 }),
      clip("c3", 4_200, 300, 1_200),
      clip("y1", 0, 0, 1_100, { asset_id: "aa", track_id: "a1" }),
      clip("y2", 1_100, 200, 1_100, { asset_id: "aa", track_id: "a1" }),
    ],
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
function transition(overrides: Partial<Transition> = {}): Transition {
  return { id: "tr1", from: "c1", to: "c2", duration_ms: 400, kind: "dissolve", ...overrides };
}

/** A right-click on `clipId` (the context menu's own target shape). */
function ctx(p: Project, clipId: string): ActionContext {
  return {
    project: p,
    snapshot: snapshot(),
    playheadMs: 0,
    selectedClipIds: [],
    pointerTarget: { kind: "clip", id: clipId, timeMs: null },
    hasClipboard: false,
    clipboardFragment: null,
  };
}

// ---- the action ---------------------------------------------------------------

describe("the transition action", () => {
  it("transition action is disabled with a reason when clips are not adjacent", () => {
    // c2 ends at 3700 and c3 starts at 4200: nothing starts where c2 ends.
    const context = ctx(project(), "c2");
    const resolved = resolveActions(context).transition;
    expect(resolved.enabled).toBe(false);
    expect(resolved.reason).toBe(NOT_ADJACENT);
    expect(commandFor("transition", context)).toBeNull();
  });

  it("joins a clip to the next one, clamped to half the shorter clip, as a dissolve on video", () => {
    const context = ctx(project(), "c1");
    expect(resolveActions(context).transition).toMatchObject({ enabled: true, reason: null, label: "Add transition" });
    // c1 is 1300ms (bound 650), c2 2000ms of OUTPUT (bound 1000; its 3000ms
    // source span would say 1500): the default 1000ms clamps to 650.
    expect(commandFor("transition", context)).toEqual({
      kind: "addTransition",
      fromClipId: "c1",
      toClipId: "c2",
      durationMs: 650,
      transitionKind: "dissolve",
    });
  });

  it("an audio pair crossfades at equal power", () => {
    // y1 1100ms (bound 550), y2 900ms (bound 450).
    expect(commandFor("transition", ctx(project(), "y1"))).toEqual({
      kind: "addTransition",
      fromClipId: "y1",
      toClipId: "y2",
      durationMs: 450,
      transitionKind: "equal-power",
    });
  });

  it("explains a taken side and a locked track instead of offering the action", () => {
    const withOut = project({ transitions: [transition()] });
    expect(resolveActions(ctx(withOut, "c1")).transition.reason).toBe(
      "This clip already has a transition into the next clip",
    );
    // c2 already has a transition IN (tr9, from another clip), so c1 --
    // which ends exactly where c2 starts -- cannot join it.
    const intoTaken = project({
      clips: [clip("c1", 400, 100, 1_400), clip("c2", 1_700, 2_000, 5_000, { speed: 1.5 }), clip("c0", 3_700, 0, 700)],
      transitions: [transition({ from: "c0", to: "c2", id: "tr9" })],
    });
    expect(resolveActions(ctx(intoTaken, "c1")).transition.reason).toBe("The next clip already has a transition in");

    const locked = project({ tracks: [track("v1", { locked: true }), track("a1", { kind: "audio" })] });
    const verdict = resolveActions(ctx(locked, "c1")).transition;
    expect(verdict.enabled).toBe(false);
    expect(verdict.reason).toBe("Track v1 is locked");
  });
});

// ---- the Fades inspector ------------------------------------------------------

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

async function mountFades(clipId: string, overrides: Partial<Project> = {}) {
  const executed: unknown[] = [];
  const store = useEditorProjectStore();
  const p = project(overrides);
  const s = snapshot();
  store.setPort(
    fakeEditorPort({
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
  useEditorWorkspaceStore().select([clipId]);
  const w = mount(FadesSection, { props: { clipIds: [clipId] } });
  await flushPromises();
  return { w, executed };
}

describe("FadesSection transitions", () => {
  it("Add transition sends the action's own command", async () => {
    const { w, executed } = await mountFades("c1");
    await w.get('[data-testid="fades-section-add-transition"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "addTransition", fromClipId: "c1", toClipId: "c2", durationMs: 650, transitionKind: "dissolve" },
    ]);
  });

  it("a disabled Add transition names its reason and sends nothing", async () => {
    const { w, executed } = await mountFades("c2");
    const button = w.get('[data-testid="fades-section-add-transition"]');
    expect(button.attributes("aria-disabled")).toBe("true");
    expect(button.attributes("title")).toBe(NOT_ADJACENT);
    await button.trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
  });

  it("a transitioned clip shows its row; the duration and Remove send their own commands", async () => {
    const { w, executed } = await mountFades("c2", { transitions: [transition()] });
    const rows = w.findAll('[data-testid="transition-row"]');
    expect(rows).toHaveLength(1);
    expect(rows[0].text()).toContain("Cross dissolve · From Clip c1");

    const field = w.get('[data-testid="transition-row-duration"]');
    expect((field.element as HTMLInputElement).value).toBe("400");
    await field.setValue("520");
    await field.trigger("keydown", { key: "Enter" });
    await w.get('[data-testid="transition-row-remove"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "setTransitionDuration", transitionId: "tr1", durationMs: 520 },
      { kind: "removeTransition", transitionId: "tr1" },
    ]);
  });

  it("a duration past half the shorter clip stays visible with a correction and sends nothing", async () => {
    const { w, executed } = await mountFades("c1", { transitions: [transition()] });
    const field = w.get('[data-testid="transition-row-duration"]');
    await field.setValue("651"); // c1 is 1300ms: the bound is 650
    await field.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(executed).toEqual([]);
    expect(w.get('[data-testid="transition-row-duration-error"]').text()).toContain("between 1 and 650");
    expect(w.get('[data-testid="transition-row"]').text()).toContain("Into Clip c2");
  });
});
