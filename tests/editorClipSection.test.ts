/**
 * `ClipSection.vue` (Task 21; F-48) — the Inspector's Clip category: name,
 * start, in, out as numeric fields with `useInspectorDraft`, a derived
 * (read-only) duration, and Earlier/Later.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ClipSection from "../src/components/editor/inspector/ClipSection.vue";
import type { EditorPort } from "../src/editor/port";
import { EditorPortError } from "../src/editor/port";
import type { Asset, Clip, EditorOpenResult, EditorSnapshot, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function track(id: string): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1 };
}
function asset(id: string): Asset {
  return { id, kind: "video", name: id, duration_ms: 60_000 };
}
// Asymmetric on purpose: start/in/out are three distinct numbers, so a
// swapped field is caught rather than passing by coincidence.
function clip(overrides: Partial<Clip> = {}): Clip {
  return {
    id: "c1",
    asset_id: "a1",
    track_id: "v1",
    name: "Intro",
    start_ms: 2_000,
    in_ms: 300,
    out_ms: 1_300,
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
    clips: [clip(), clip({ id: "c2", start_ms: 5_000, in_ms: 0, out_ms: 500 })],
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

let executed: unknown[] = [];

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

describe("ClipSection — multi/no selection", () => {
  it("more than one clip shows the honest note, not the fields", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1", "c2"] } });
    await flushPromises();

    expect(w.find('[data-testid="clip-section-multi"]').exists()).toBe(true);
    expect(w.find('[data-testid="clip-section"]').exists()).toBe(false);
  });
});

describe("ClipSection — fields", () => {
  it("renders the committed name/start/in/out and the DERIVED duration", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    expect((w.get('[data-testid="clip-section-name"]').element as HTMLInputElement).value).toBe("Intro");
    expect((w.get('[data-testid="clip-section-start"]').element as HTMLInputElement).value).toBe("2000");
    expect((w.get('[data-testid="clip-section-in"]').element as HTMLInputElement).value).toBe("300");
    expect((w.get('[data-testid="clip-section-out"]').element as HTMLInputElement).value).toBe("1300");
    // duration = out - in = 1000ms = "0:01".
    expect(w.get('[data-testid="clip-section-duration"]').text()).toContain("0:01");
  });

  it("numeric start entry sends trimClip with unchanged in/out", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const start = w.get('[data-testid="clip-section-start"]');
    await start.setValue("2500");
    await start.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([
      { kind: "trimClip", clipId: "c1", startMs: 2_500, inMs: 300, outMs: 1_300 },
    ]);
  });

  it("editing In sends trimClip with the unchanged start/out", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const inField = w.get('[data-testid="clip-section-in"]');
    await inField.setValue("400");
    await inField.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([
      { kind: "trimClip", clipId: "c1", startMs: 2_000, inMs: 400, outMs: 1_300 },
    ]);
  });

  it("renaming sends updateClip", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const name = w.get('[data-testid="clip-section-name"]');
    await name.setValue("Chapter 1");
    await name.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([{ kind: "updateClip", clipId: "c1", name: "Chapter 1" }]);
  });

  it("an out-of-range value stays visible with an inline correction, sending nothing", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const start = w.get('[data-testid="clip-section-start"]');
    await start.setValue("-5");
    await start.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([]);
    expect(w.find('[data-testid="clip-section-start-error"]').exists()).toBe(true);
    expect((start.element as HTMLInputElement).value).toBe("-5");
  });

  // Every EditorCommand time is an integer ms; a fractional entry would be
  // refused by Rust's decoder with a wire error the user cannot act on.
  it("a fractional millisecond entry is refused inline, sending nothing", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const out = w.get('[data-testid="clip-section-out"]');
    await out.setValue("1250.5");
    await out.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([]);
    expect(w.get('[data-testid="clip-section-out-error"]').text()).toContain("whole number");
  });

  // Task 19's carried finding (ClipSection is useInspectorDraft's first
  // consumer): a command landing from ELSEWHERE -- here a timeline nudge on
  // the same clip, acknowledged by Rust with a new projection -- must show
  // in the open section without it being remounted, and the derived
  // duration must follow too.
  it("follows an external edit to the same clip without being remounted", async () => {
    executed = [];
    const store = useEditorProjectStore();
    const p = project();
    const s = snapshot();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve<EditorOpenResult>({
            snapshot: s, project: p, workspace: {}, missing: [], sourceBase: "base", recovered: false,
          }),
        execute: (req) => {
          executed.push(req.command);
          const moved = project({
            clips: [clip({ start_ms: 2_033, out_ms: 2_300 }), clip({ id: "c2", start_ms: 5_000, in_ms: 0, out_ms: 500 })],
          });
          return Promise.resolve({ snapshot: { ...s, revision: 2 }, project: moved });
        },
      }),
    );
    await store.openStaged("base");
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    await store.execute({ kind: "moveClips", clipIds: ["c1"], deltaMs: 33, trackId: null });
    await flushPromises();

    expect((w.get('[data-testid="clip-section-start"]').element as HTMLInputElement).value).toBe("2033");
    expect((w.get('[data-testid="clip-section-out"]').element as HTMLInputElement).value).toBe("2300");
    expect(w.get('[data-testid="clip-section-duration"]').text()).toContain("0:02");
  });

  it("an empty name is refused inline rather than sent (Rust refuses it too)", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const name = w.get('[data-testid="clip-section-name"]');
    await name.setValue("   ");
    await name.trigger("blur");
    await flushPromises();

    expect(executed).toEqual([]);
    expect(w.get('[data-testid="clip-section-name-error"]').text()).toBe("Name must not be empty.");
  });

  // Fix round 1 (review Minor, F14's own case): Out below In + 100 passes the
  // field's own range check but Rust refuses the trimClip. The field must
  // fall back to the committed value, not keep showing the refused one as
  // if the timeline had taken it.
  it("a value Rust refuses (Out below the 100 ms minimum) reverts to the committed value", async () => {
    executed = [];
    const store = useEditorProjectStore();
    const s = snapshot();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve<EditorOpenResult>({
            snapshot: s, project: project(), workspace: {}, missing: [], sourceBase: "base", recovered: false,
          }),
        execute: (req) => {
          executed.push(req.command);
          return Promise.reject(
            new EditorPortError({
              code: "invalidRequest",
              message: "trimClip would produce a 50 ms clip, below the 100 ms minimum",
              retryable: false,
              operationId: "op-1",
            }),
          );
        },
      }),
    );
    await store.openStaged("base");
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const out = w.get('[data-testid="clip-section-out"]');
    await out.setValue("350"); // in is 300
    await out.trigger("keydown", { key: "Enter" });
    await flushPromises();

    expect(executed).toEqual([{ kind: "trimClip", clipId: "c1", startMs: 2_000, inMs: 300, outMs: 350 }]);
    expect(store.lastError?.message).toContain("100 ms minimum");
    expect((out.element as HTMLInputElement).value).toBe("1300");
  });

  it("Escape reverts the draft to the last committed value", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const start = w.get('[data-testid="clip-section-start"]');
    await start.setValue("9999");
    await start.trigger("keydown", { key: "Escape" });

    expect((start.element as HTMLInputElement).value).toBe("2000");
    expect(executed).toEqual([]);
  });
});

describe("ClipSection — Earlier/Later", () => {
  it("reads enabled/reason from the SAME registry as the timeline toolbar", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    // c1 is the only clip on its track before c2 in start order, so
    // "earlier" is disabled (already first) and "later" is enabled.
    expect(w.get('[data-testid="clip-section-earlier"]').attributes("aria-disabled")).toBe("true");
    expect(w.get('[data-testid="clip-section-later"]').attributes("aria-disabled")).toBe("false");
  });

  it("a DISABLED Earlier sends nothing when clicked", async () => {
    executed = [];
    await openProject();
    useEditorWorkspaceStore().select(["c1"]);
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    await w.get('[data-testid="clip-section-earlier"]').trigger("click");
    await flushPromises();

    expect(executed).toEqual([]);
  });

  it("Later sends the SAME reorderClip command the registry builds", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    await w.get('[data-testid="clip-section-later"]').trigger("click");
    await flushPromises();

    expect(executed).toEqual([{ kind: "reorderClip", clipId: "c1", direction: "later" }]);
  });
});
