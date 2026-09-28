/**
 * `ClipSection.vue` (Task 21; F-48; visual-parity Task 14, concept spec §5
 * "Clip") — the Inspector's Clip category: **Placement** (Clip name, the
 * same-kind Track select, Timeline start in seconds, Earlier / Later /
 * Duplicate) and **Source range** (In / Out in seconds, the original
 * length). Every time is typed in SECONDS and sent as whole milliseconds.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ClipSection from "../src/components/editor/inspector/ClipSection.vue";
import { EditorPortError } from "../src/editor/port";
import { clipNameFocus } from "../src/editor/revealBus";
import type { Asset, Clip, EditorOpenResult, EditorSnapshot, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
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

// Visual-parity Task 5: the clip menu's "Rename…" focuses the name field
// of the section showing THAT clip; another clip's section ignores it.
describe("ClipSection — a focus request from the clip menu", () => {
  it("focuses and selects the name field when the request names this clip, and clears it", async () => {
    await openProject();
    const w = mount(ClipSection, { attachTo: document.body, props: { clipIds: ["c1"] } });
    clipNameFocus.value = "other";
    await flushPromises();
    expect(document.activeElement?.getAttribute("data-testid")).not.toBe("clip-section-name");
    expect(clipNameFocus.value).toBe("other");

    clipNameFocus.value = "c1";
    await flushPromises();
    expect(document.activeElement).toBe(w.get('[data-testid="clip-section-name"]').element);
    expect(clipNameFocus.value).toBeNull();
  });
});


function input(w: ReturnType<typeof mount>, testid: string): HTMLInputElement {
  return w.get(`[data-testid="${testid}"]`).element as HTMLInputElement;
}

async function type(w: ReturnType<typeof mount>, testid: string, value: string, key = "Enter"): Promise<void> {
  const field = w.get(`[data-testid="${testid}"]`);
  await field.setValue(value);
  await field.trigger("keydown", { key });
  await flushPromises();
}

describe("ClipSection — Placement and Source range (concept §5 Clip)", () => {
  it("names the two sections and every field in seconds, never milliseconds", async () => {
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();

    const headings = w.findAll("h3").map((h) => h.text());
    expect(headings).toEqual(["Placement", "Source range"]);
    const text = w.text();
    for (const label of ["Clip name", "Track", "Timeline start (s)", "In (s)", "Out (s)"]) {
      expect(text).toContain(label);
    }
    expect(text).not.toMatch(/\bms\b/);
    expect(input(w, "clip-section-name").value).toBe("Intro");
    expect(input(w, "clip-section-start").value).toBe("2");
    expect(input(w, "clip-section-in").value).toBe("0.3");
    expect(input(w, "clip-section-out").value).toBe("1.3");
  });

  it("time fields are number inputs with a 0.1 s step", async () => {
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    for (const id of ["clip-section-start", "clip-section-in", "clip-section-out"]) {
      expect(input(w, id).type).toBe("number");
      expect(input(w, id).step).toBe("0.1");
    }
  });

  it("shows the concept's help lines, with the original length as MM:SS.d", async () => {
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    expect(w.get('[data-testid="clip-section-placement-help"]').text()).toBe(
      "Earlier / Later swaps adjacent clips. Other tracks are not moved.",
    );
    expect(w.get('[data-testid="clip-section-source-help"]').text()).toBe(
      "Original length 01:00.0. Numeric source trims keep this clip’s timeline start fixed.",
    );
  });
});

describe("ClipSection — Placement", () => {
  it("renaming sends updateClip", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await type(w, "clip-section-name", "Chapter 1");
    expect(executed).toEqual([{ kind: "updateClip", clipId: "c1", name: "Chapter 1" }]);
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

  it("Timeline start in seconds moves the clip by the difference, in whole ms", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await type(w, "clip-section-start", "2.5");
    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c1"], deltaMs: 500, trackId: null }]);
  });

  it("a finer-than-ms start is rounded to whole milliseconds before it is sent", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await type(w, "clip-section-start", "1.23456");
    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c1"], deltaMs: -765, trackId: null }]);
  });

  it("an out-of-range start stays visible with a correction in seconds, sending nothing", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await type(w, "clip-section-start", "-5");
    expect(executed).toEqual([]);
    expect(w.get('[data-testid="clip-section-start-error"]').text()).toContain("between 0 and 7200 s");
    expect(input(w, "clip-section-start").value).toBe("-5");
  });

  it("Escape reverts the start to the committed value", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await type(w, "clip-section-start", "99", "Escape");
    expect(input(w, "clip-section-start").value).toBe("2");
    expect(executed).toEqual([]);
  });

  it("the Track select lists the same-kind tracks, a locked one suffixed and unavailable", async () => {
    await openProject({
      tracks: [
        track("v1", { name: "Screen" }),
        track("v2", { name: "Webcam" }),
        track("v3", { name: "Detail", locked: true }),
        track("a1", { name: "Music", kind: "audio" }),
      ],
    });
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    const options = w.findAll('[data-testid="clip-section-track"] option');
    expect(options.map((o) => [o.attributes("value"), o.text(), o.attributes("disabled") !== undefined])).toEqual([
      ["v1", "Screen", false],
      ["v2", "Webcam", false],
      ["v3", "Detail · locked", true],
    ]);
    expect((w.get('[data-testid="clip-section-track"]').element as HTMLSelectElement).value).toBe("v1");
  });

  it("choosing another track sends moveClips to it, in place", async () => {
    executed = [];
    await openProject({ tracks: [track("v1"), track("v2")] });
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await w.get('[data-testid="clip-section-track"]').setValue("v2");
    await flushPromises();
    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c1"], deltaMs: 0, trackId: "v2" }]);
  });

  // Fix round 1: a move Rust refuses must not leave the select naming the
  // track the clip never reached.
  it("a track move Rust refuses puts the select back on the clip's own track", async () => {
    executed = [];
    const store = useEditorProjectStore();
    const s = snapshot();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve<EditorOpenResult>({
            snapshot: s, project: project({ tracks: [track("v1"), track("v2")] }), workspace: {}, missing: [], sourceBase: "base", recovered: false,
          }),
        execute: (req) => {
          executed.push(req.command);
          return Promise.reject(
            new EditorPortError({ code: "invalidRequest", message: "overlap", retryable: false, operationId: "op-1" }),
          );
        },
      }),
    );
    await store.openStaged("base");
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await w.get('[data-testid="clip-section-track"]').setValue("v2");
    await flushPromises();
    expect(executed).toHaveLength(1);
    expect((w.get('[data-testid="clip-section-track"]').element as HTMLSelectElement).value).toBe("v1");
  });

  it("a grouped clip's Track select is unavailable and says why (Rust moves one clip only)", async () => {
    executed = [];
    await openProject({
      tracks: [track("v1"), track("v2")],
      clips: [clip({ group_id: "g1" }), clip({ id: "c2", start_ms: 5_000, in_ms: 0, out_ms: 500, group_id: "g1" })],
    });
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    const select = w.get('[data-testid="clip-section-track"]');
    expect(select.attributes("disabled")).toBeDefined();
    expect(select.attributes("title")).toBe("Ungroup this clip to move it to another track.");
    await select.setValue("v2");
    expect(executed).toEqual([]);
  });

  it("a clip on a locked track disables every field and carries the reason", async () => {
    executed = [];
    await openProject({ tracks: [track("v1", { name: "Screen", locked: true }), track("v2")] });
    useEditorWorkspaceStore().select(["c1"]);
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    const fieldset = w.get('[data-testid="clip-section"]');
    expect(fieldset.element.tagName).toBe("FIELDSET");
    expect(fieldset.attributes("disabled")).toBeDefined();
    expect(fieldset.attributes("title")).toContain("Track Screen is locked");
    expect(w.get('[data-testid="clip-section-later"]').attributes("title")).toContain("Track Screen is locked");
    await w.get('[data-testid="clip-section-later"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
  });
});

// Visual-parity Task 14: the preset row is the registry's own actions, the
// same rules (and disabled reasons) the context menu and toolbar use.
describe("ClipSection — Earlier / Later / Duplicate", () => {
  it("reads enabled/reason from the SAME registry as the timeline toolbar", async () => {
    await openProject();
    useEditorWorkspaceStore().select(["c1"]);
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    const earlier = w.get('[data-testid="clip-section-earlier"]');
    expect(earlier.text()).toBe("Earlier");
    expect(earlier.attributes("aria-disabled")).toBe("true");
    expect(earlier.attributes("title")).toBeTruthy();
    expect(w.get('[data-testid="clip-section-later"]').attributes("aria-disabled")).toBeUndefined();
    expect(w.get('[data-testid="clip-section-duplicate"]').text()).toBe("Duplicate");
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
    useEditorWorkspaceStore().select(["c1"]);
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await w.get('[data-testid="clip-section-later"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "reorderClip", clipId: "c1", direction: "later" }]);
  });

  it("Duplicate sends the registry's duplicateClips, placed right after the clip", async () => {
    executed = [];
    await openProject();
    useEditorWorkspaceStore().select(["c1"]);
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await w.get('[data-testid="clip-section-duplicate"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "duplicateClips", clipIds: ["c1"], offsetMs: 1_000 }]);
  });
});

describe("ClipSection — Source range", () => {
  it("editing In (s) sends trimClip in whole ms with the unchanged start/out", async () => {
    executed = [];
    await openProject();
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await type(w, "clip-section-in", "0.4");
    expect(executed).toEqual([{ kind: "trimClip", clipId: "c1", startMs: 2_000, inMs: 400, outMs: 1_300 }]);
  });

  it("follows an external edit to the same clip without being remounted", async () => {
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

    expect(input(w, "clip-section-start").value).toBe("2.033");
    expect(input(w, "clip-section-out").value).toBe("2.3");
  });

  // Fix round 1 (review Minor, F14's own case): Out below In + 100 passes the
  // field's own range check but Rust refuses the trimClip. The field must
  // fall back to the committed value, not keep showing the refused one.
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

    await type(w, "clip-section-out", "0.35"); // in is 0.3
    expect(executed).toEqual([{ kind: "trimClip", clipId: "c1", startMs: 2_000, inMs: 300, outMs: 350 }]);
    expect(store.lastError?.message).toContain("100 ms minimum");
    expect(input(w, "clip-section-out").value).toBe("1.3");
  });

  // Task 26 (F-10): an image clip may extend up to the project maximum,
  // not its import-time default length.
  it("an image clip's Out may extend past the asset's own duration_ms, sending trimClip", async () => {
    executed = [];
    await openProject({
      assets: [{ id: "a1", kind: "video", name: "a1", duration_ms: 5_000, media_type: "image" }],
      clips: [clip({ start_ms: 0, in_ms: 0, out_ms: 5_000 })],
    });
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await type(w, "clip-section-out", "6");
    expect(w.find('[data-testid="clip-section-out-error"]').exists()).toBe(false);
    expect(executed).toEqual([{ kind: "trimClip", clipId: "c1", startMs: 0, inMs: 0, outMs: 6_000 }]);
  });

  it("a NON-image clip's Out is still bounded by the asset's own length", async () => {
    executed = [];
    await openProject({ clips: [clip({ start_ms: 0, in_ms: 0, out_ms: 5_000 })] });
    const w = mount(ClipSection, { props: { clipIds: ["c1"] } });
    await flushPromises();
    await type(w, "clip-section-out", "70");
    expect(w.get('[data-testid="clip-section-out-error"]').text()).toContain("between 0 and 60 s");
    expect(executed).toEqual([]);
  });
});
