/**
 * `TimelineView.vue` and its children (`TrackLane.vue`, `ClipItem.vue`,
 * `TimelineRuler.vue`, `TimelineToolbar.vue`) — Task 20. Mounted as one
 * tree (the `EditorShell.vue` precedent): the interesting behaviors are
 * cross-component (a click on the ruler must not touch selection; a
 * right-click on a clip must open the ONE context menu `TimelineView` owns),
 * so most tests drive the whole `TimelineView`, not its leaves in isolation.
 *
 * The pure geometry/virtualization math has its own suite
 * (`tests/timelineLayout.test.ts`); this file only checks that the
 * components actually USE it correctly, not that the math itself is right.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import TimelineRuler from "../src/components/editor/timeline/TimelineRuler.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import { clearClipboardForTest } from "../src/editor/clipboard";
import { pxPerMs } from "../src/editor/timelineLayout";
import type { Asset, Clip, EditorOpenResult, EditorSnapshot, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  clearClipboardForTest();
});

// ---- fixtures ---------------------------------------------------------------
// Asymmetric on purpose (the "fixture flaw" rule): two tracks with different
// kinds/order, clips of different lengths/positions, so a swapped index or a
// dropped offset cannot pass by coincidence.

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}

function asset(id: string, kind: "video" | "audio"): Asset {
  return { id, kind, name: id, duration_ms: 60_000 };
}

function clip(id: string, trackId: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: trackId === "a1" ? "audio-asset" : "video-asset",
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

function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [asset("video-asset", "video"), asset("audio-asset", "audio")],
    tracks: [track("v2"), track("v1"), track("a1", { kind: "audio" })],
    clips: [
      clip("c1", "v2", { start_ms: 500, in_ms: 0, out_ms: 2_500 }),
      clip("c2", "v1", { start_ms: 0, in_ms: 0, out_ms: 1_000, fade_in_ms: 200, fade_out_ms: 300 }),
      clip("c3", "a1", { start_ms: 4_000, in_ms: 0, out_ms: 1_000 }),
    ],
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

async function openProject(overrides: Partial<Project> = {}, snapshotOverrides: Partial<EditorSnapshot> = {}) {
  const store = useEditorProjectStore();
  const p = project(overrides);
  const s = snapshot(snapshotOverrides);
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
  return { store, project: p, snapshot: s };
}

let executed: unknown[] = [];

/** happy-dom rects are zero-sized; force one so click-position math is
 * exercised deterministically (the `sizeStrip` precedent,
 * `tests/helpers/editorMount.ts`). */
function forceRect(el: Element, rect: Partial<DOMRect>) {
  (el as HTMLElement).getBoundingClientRect = () =>
    ({ left: 0, top: 0, width: 0, height: 0, right: 0, bottom: 0, x: 0, y: 0, ...rect }) as DOMRect;
}

describe("TimelineView — track order", () => {
  it("renders track lanes in project.tracks order (index 0 = top = frontmost video)", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    const lanes = w.findAll('[data-testid^="track-lane-"]:not([data-testid*="-header-"]):not([data-testid*="-body-"])');
    expect(lanes.map((l) => l.attributes("data-testid"))).toEqual([
      "track-lane-v2",
      "track-lane-v1",
      "track-lane-a1",
    ]);
  });
});

describe("TimelineView — ruler", () => {
  it("a ruler click moves the playhead, not the selection", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(TimelineView);
    await flushPromises();

    const ticks = w.get('[data-testid="timeline-ruler-ticks"]');
    forceRect(ticks.element, { left: 0, width: 2000 });

    // 400px at zoom 1 -> 400 / (BASE_PX_PER_MS) ms.
    await ticks.trigger("pointerdown", { clientX: 400, clientY: 0 });

    const expectedMs = 400 / pxPerMs(workspace.timelineZoom);
    expect(workspace.playheadMs).toBeCloseTo(expectedMs, 0);
    expect(workspace.selectionClipIds).toEqual(["c1"]);
  });
});

describe("TimelineView — clip selection", () => {
  it("clicking a clip selects only that clip", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    const w = mount(TimelineView);
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("click");

    expect(workspace.selectionClipIds).toEqual(["c2"]);
  });

  // Brief: "Click selects (Ctrl/Shift extends)".
  it("Ctrl+click toggles a clip in and out of the selection", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(TimelineView);
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("click", { ctrlKey: true });
    expect(workspace.selectionClipIds).toEqual(["c1", "c2"]);

    await w.get('[data-testid="clip-c1"]').trigger("click", { ctrlKey: true });
    expect(workspace.selectionClipIds).toEqual(["c2"]);
  });

  it("Shift+click adds a clip to the selection without dropping the rest", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(TimelineView);
    await flushPromises();

    await w.get('[data-testid="clip-c3"]').trigger("click", { shiftKey: true });
    await w.get('[data-testid="clip-c3"]').trigger("click", { shiftKey: true }); // idempotent

    expect(workspace.selectionClipIds).toEqual(["c1", "c3"]);
  });

  it("applies aria-selected only to the selected clip", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(TimelineView);
    await flushPromises();

    expect(w.get('[data-testid="clip-c1"]').attributes("aria-selected")).toBe("true");
    expect(w.get('[data-testid="clip-c2"]').attributes("aria-selected")).toBe("false");
  });
});

describe("ClipItem — accessible label and media-kind styling", () => {
  it("aria-label reads 'Clip <name>, <start>-<end>'", async () => {
    executed = [];
    // c2: start_ms 0, out_ms 1000, in_ms 0, speed default 1 -> ends at 1000ms.
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    expect(w.get('[data-testid="clip-c2"]').attributes("aria-label")).toBe("Clip c2, 0:00–0:01");
  });

  it("is role=option, not role=button -- aria-selected is invalid on button (fix round 1, finding 3)", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    expect(w.get('[data-testid="clip-c2"]').attributes("role")).toBe("option");
  });

  it("uses the video vs audio colour token by the clip's own asset kind", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    expect(w.get('[data-testid="clip-c2"]').classes()).toContain("bg-video-bg");
    expect(w.get('[data-testid="clip-c3"]').classes()).toContain("bg-audio-bg");
  });

  it("reserves a waveform lane slot for audio clips only (F-26)", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    expect(w.find('[data-testid="clip-c3-waveform-slot"]').exists()).toBe(true);
    expect(w.find('[data-testid="clip-c2-waveform-slot"]').exists()).toBe(false);
  });
});

describe("ClipItem — fade wedges (gold)", () => {
  it("renders a fade wedge only for a nonzero fade_in_ms/fade_out_ms", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    // c2 has fade_in_ms 200 and fade_out_ms 300.
    expect(w.find('[data-testid="clip-c2-fade-in"]').exists()).toBe(true);
    expect(w.find('[data-testid="clip-c2-fade-out"]').exists()).toBe(true);
    // c1 has neither.
    expect(w.find('[data-testid="clip-c1-fade-in"]').exists()).toBe(false);
    expect(w.find('[data-testid="clip-c1-fade-out"]').exists()).toBe(false);
  });
});

describe("TimelineView — context menu (right-click and Shift+F10)", () => {
  it("right-click on a clip opens the menu targeted at THAT clip without touching selection", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]); // a DIFFERENT clip is already selected
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("contextmenu");

    expect(w.find('[data-testid="editor-context-menu-root"]').exists()).toBe(true);
    // A14: right-click acts on ITS OWN target, not stale selection -- c2's
    // Split/Delete/etc. must be enabled even though c2 is not selected.
    expect(w.get('[data-testid="editor-context-menu-item-delete"]').attributes("aria-disabled")).toBe("false");
    expect(workspace.selectionClipIds).toEqual(["c1"]);
  });

  it("Shift+F10 on a focused clip opens the context menu", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c1"]').trigger("keydown", { key: "F10", shiftKey: true });

    expect(w.find('[data-testid="editor-context-menu-root"]').exists()).toBe(true);
  });

  it("Escape closes the menu and returns focus without executing anything", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c1"]').trigger("contextmenu");
    expect(w.find('[data-testid="editor-context-menu-root"]').exists()).toBe(true);

    await w.get('[data-testid="editor-context-menu"]').trigger("keydown", { key: "Escape" });
    await flushPromises();

    expect(w.find('[data-testid="editor-context-menu-root"]').exists()).toBe(false);
    expect(executed).toEqual([]);
  });
});

describe("TimelineToolbar", () => {
  it("split/delete/undo/redo read enabled/reason from the SAME action registry as PreviewToolbar", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    // Nothing selected -> split/delete read the shared NO_CLIP reason.
    expect(w.get('[data-testid="timeline-toolbar-split"]').attributes("aria-disabled")).toBe("true");
    expect(w.get('[data-testid="timeline-toolbar-delete"]').attributes("aria-disabled")).toBe("true");
    expect(w.get('[data-testid="timeline-toolbar-undo"]').attributes("aria-disabled")).toBe("true");
  });

  it("delete sends the SAME deleteClips command the action registry builds", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(TimelineView);
    await flushPromises();

    await w.get('[data-testid="timeline-toolbar-delete"]').trigger("click");
    await flushPromises();

    expect(executed).toEqual([{ kind: "deleteClips", clipIds: ["c1"], closeGap: false }]);
  });

  it("clicking a DISABLED action does nothing", async () => {
    executed = [];
    await openProject(); // nothing selected -> split/delete/undo/redo all disabled
    const w = mount(TimelineView);
    await flushPromises();

    await w.get('[data-testid="timeline-toolbar-split"]').trigger("click");

    expect(executed).toEqual([]);
  });

  it("the delete-mode toggle picks which deleteClips shape the Delete button sends", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(TimelineView);
    await flushPromises();

    expect(workspace.deleteMode).toBe("gap"); // the store's own default
    await w.get('[data-testid="timeline-toolbar-delete"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "deleteClips", clipIds: ["c1"], closeGap: false }]);

    executed = [];
    await w.get('[data-testid="timeline-toolbar-delete-mode-close"]').trigger("click");
    expect(workspace.deleteMode).toBe("close");
    await w.get('[data-testid="timeline-toolbar-delete"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "deleteClips", clipIds: ["c1"], closeGap: true }]);
  });

  // Brief: 'the close-gap button says "on this track"' -- Rust's closeGap
  // ripples only the deleted clip's own track, and the visible label (not
  // just a hover title) must not read as closing every track's gap.
  it("the close-gap mode button says 'on this track' in its visible label", async () => {
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    const close = w.get('[data-testid="timeline-toolbar-delete-mode-close"]');
    expect(close.text()).toContain("on this track");
    expect(close.attributes("aria-pressed")).toBe("false");
    await close.trigger("click");
    expect(close.attributes("aria-pressed")).toBe("true");
    expect(w.get('[data-testid="timeline-toolbar-delete-mode-gap"]').attributes("aria-pressed")).toBe("false");
  });

  it("snap toggles editorWorkspace.snap", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    const before = workspace.snap;
    const w = mount(TimelineView);
    await flushPromises();

    await w.get('[data-testid="timeline-toolbar-snap"]').trigger("click");

    expect(workspace.snap).toBe(!before);
  });

  it("zoom in/out change editorWorkspace.timelineZoom", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    const before = workspace.timelineZoom;
    const w = mount(TimelineView);
    await flushPromises();

    await w.get('[data-testid="timeline-toolbar-zoom-in"]').trigger("click");
    expect(workspace.timelineZoom).toBeGreaterThan(before);

    await w.get('[data-testid="timeline-toolbar-zoom-out"]').trigger("click");
    await w.get('[data-testid="timeline-toolbar-zoom-out"]').trigger("click");
    expect(workspace.timelineZoom).toBeLessThan(before);
  });

  it("fit computes a REAL zoom from the measured viewport width, not the store's honest placeholder", async () => {
    executed = [];
    await openProject({}, { durationMs: 60_000 });
    const workspace = useEditorWorkspaceStore();
    workspace.setZoom(5); // move it away from the default so "fit" is observable
    const w = mount(TimelineView, { props: { viewportWidth: 1200 } });
    await flushPromises();

    await w.get('[data-testid="timeline-toolbar-fit"]').trigger("click");

    // fitZoom(60_000, 1200) = 1200 / (BASE_PX_PER_MS * 60_000) = 0.4.
    expect(workspace.timelineZoom).toBeCloseTo(0.4, 5);
    expect(workspace.timelineScrollLeft).toBe(0);
  });
});

describe("TimelineView — virtualization", () => {
  it("with a small forced viewport, a clip far outside it is not mounted", async () => {
    executed = [];
    const manyClips: Clip[] = [];
    for (let i = 0; i < 50; i += 1) {
      manyClips.push(clip(`m${i}`, "v1", { start_ms: i * 3_000, in_ms: 0, out_ms: 500 }));
    }
    await openProject({ clips: manyClips }, { durationMs: 150_000 });
    const w = mount(TimelineView, { props: { viewportWidth: 500 } });
    await flushPromises();

    const rendered = w.findAll('[data-testid^="clip-m"]');
    expect(rendered.length).toBeLessThan(manyClips.length);
    // The very first clip (near scrollLeft 0) must still be there.
    expect(w.find('[data-testid="clip-m0"]').exists()).toBe(true);
    // A clip far past even the one-screen padding must not be mounted.
    expect(w.find('[data-testid="clip-m49"]').exists()).toBe(false);
  });
});

describe("TimelineView — resize handle (fix round 1, finding 4)", () => {
  it("sits at the timeline's TOP edge, above the toolbar", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView);
    await flushPromises();

    // The timeline panel is the last (bottom) row of the whole editor shell,
    // so a block element's height grows DOWNWARD from a fixed top -- the
    // handle has to sit at that TOP edge for its own position to move WITH
    // an upward drag. A handle below the scroll area would move AWAY from
    // the pointer on every drag (the defect this fix round named).
    const root = w.get('[data-testid="timeline-view"]').element;
    const handleIndex = Array.from(root.children).findIndex(
      (el) => el.getAttribute("data-testid") === "timeline-resize-handle",
    );
    const toolbarIndex = Array.from(root.children).findIndex(
      (el) => el.getAttribute("data-testid") === "timeline-toolbar",
    );
    expect(handleIndex).toBeGreaterThanOrEqual(0);
    expect(handleIndex).toBeLessThan(toolbarIndex);
  });

  it("dragging UP grows the timeline -- consistent with the handle sitting at the top edge", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    const before = workspace.timelineHeight;
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const handle = w.get('[data-testid="timeline-resize-handle"]');
    await handle.trigger("pointerdown", { clientY: 500, pointerId: 1 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientY: 440 }));
    window.dispatchEvent(new PointerEvent("pointerup"));
    await flushPromises();

    // Dragging UP (a smaller clientY) grows the timeline -- and because the
    // handle sits at the top edge (the test above), growth moves that same
    // top edge further up, in the same direction the pointer moved.
    expect(workspace.timelineHeight).toBeGreaterThan(before);
  });
});

describe("TimelineView — context menu copy/cut (Task 21)", () => {
  it("Copy from the context menu writes the clipboard and sends no command", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("contextmenu");
    await w.get('[data-testid="editor-context-menu-item-copy"]').trigger("click");
    await flushPromises();

    expect(executed).toEqual([]);
  });

  it("Cut from the context menu copies AND sends exactly one cutClips", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("contextmenu");
    await w.get('[data-testid="editor-context-menu-item-cut"]').trigger("click");
    await flushPromises();

    expect(executed).toEqual([{ kind: "cutClips", clipIds: ["c2"], closeGap: true }]);
  });
});

describe("TimelineView — context menu activation", () => {
  it("activating an enabled item sends the SAME command the registry builds", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    // Right-click c2 directly -- A14: it becomes the target with no prior
    // selection needed.
    await w.get('[data-testid="clip-c2"]').trigger("contextmenu");
    await w.get('[data-testid="editor-context-menu-item-duplicate"]').trigger("click");
    await flushPromises();

    expect(executed).toEqual([{ kind: "duplicateClips", clipIds: ["c2"], offsetMs: 1_000 }]);
  });
});

describe("ClipItem — keyboard gate", () => {
  it("a key other than Shift+F10/Menu does nothing", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c1"]').trigger("keydown", { key: "a" });

    expect(w.find('[data-testid="editor-context-menu-root"]').exists()).toBe(false);
  });
});

describe("ClipItem — keyboard activation (fix round 1, finding 3)", () => {
  // role=option/tabindex=0 is not a native <button>, so Enter/Space must be
  // wired by hand (the TranscriptionSummary.vue precedent,
  // @keydown.enter/@keydown.space.prevent) or a keyboard user who can TAB to
  // a clip and open its context menu (Shift+F10) still has no way to select
  // it without a mouse.
  it("Enter selects the focused clip", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("keydown", { key: "Enter" });

    expect(workspace.selectionClipIds).toEqual(["c2"]);
  });

  it("Space selects the focused clip", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c1"]').trigger("keydown", { key: " " });

    expect(workspace.selectionClipIds).toEqual(["c1"]);
  });
});

describe("ClipItem — keyboard nudge (Task 21)", () => {
  it("ArrowRight sends one moveClips of +33ms (one frame)", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("keydown", { key: "ArrowRight" });
    await flushPromises();

    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c2"], deltaMs: 33, trackId: null }]);
  });

  it("ArrowLeft sends a NEGATIVE delta", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("keydown", { key: "ArrowLeft" });
    await flushPromises();

    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c2"], deltaMs: -33, trackId: null }]);
  });

  it("Shift+ArrowRight nudges by a full second", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    await w.get('[data-testid="clip-c2"]').trigger("keydown", { key: "ArrowRight", shiftKey: true });
    await flushPromises();

    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c2"], deltaMs: 1_000, trackId: null }]);
  });

  // The fake returns a GENUINELY different projection (c2 moved by the
  // nudge, a fresh project object -- `editorProject` replaces it wholesale
  // on every acknowledged command), so the clip really re-renders at its
  // new position; asserting focus against a fake that echoed the old
  // projection back would pass without any re-render happening at all.
  it("keyboard nudge keeps focus on the clip after re-render", async () => {
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
            clips: p.clips.map((c) => (c.id === "c2" ? { ...c, start_ms: c.start_ms + 33 } : { ...c })),
          });
          return Promise.resolve({ snapshot: { ...s, revision: s.revision + 1 }, project: moved });
        },
      }),
    );
    await store.openStaged("base");
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const c2 = w.get('[data-testid="clip-c2"]').element as HTMLElement;
    const before = c2.getAttribute("style");
    c2.focus();
    expect(document.activeElement).toBe(c2);

    await w.get('[data-testid="clip-c2"]').trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    await flushPromises(); // one more tick for the component's own nextTick

    const after = w.get('[data-testid="clip-c2"]').element as HTMLElement;
    expect(after.getAttribute("style")).not.toBe(before); // it really re-rendered
    expect(document.activeElement).toBe(after);
  });
});

describe("ClipItem — drag (Task 21)", () => {
  it("a body drag submits exactly one moveClips on pointer-up", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const c2 = w.get('[data-testid="clip-c2"]');
    await c2.trigger("pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
    await c2.trigger("pointermove", { clientX: 200, clientY: 0, pointerId: 1 });
    await c2.trigger("pointerup", { clientX: 200, clientY: 0, pointerId: 1 });
    await flushPromises();

    expect(executed).toHaveLength(1);
    const cmd = executed[0] as { kind: string; clipIds: string[]; deltaMs: number };
    expect(cmd.kind).toBe("moveClips");
    expect(cmd.clipIds).toEqual(["c2"]);
    expect(cmd.deltaMs).toBeGreaterThan(0);
  });

  it("a leftward body drag sends a NEGATIVE delta", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    // c1 starts at 500ms: 10px left at zoom 1 is 200ms, so it lands at 300.
    const c1 = w.get('[data-testid="clip-c1"]');
    await c1.trigger("pointerdown", { clientX: 400, clientY: 0, pointerId: 1 });
    await c1.trigger("pointermove", { clientX: 390, clientY: 0, pointerId: 1 });
    await c1.trigger("pointerup", { clientX: 390, clientY: 0, pointerId: 1 });
    await flushPromises();

    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c1"], deltaMs: -200, trackId: null }]);
  });

  // Snap is ON by default. The dragged clip's OWN edges are not targets: a
  // small drag (within the 8px threshold, 160ms at zoom 1) would otherwise
  // snap straight back onto the clip's original start and send nothing --
  // every nudge-sized drag silently undone by the magnet.
  it("a small drag with snap on is not pulled back onto the clip's own start", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    expect(workspace.snap).toBe(true);
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const c3 = w.get('[data-testid="clip-c3"]'); // 4000..5000, alone on a1
    await c3.trigger("pointerdown", { clientX: 900, clientY: 0, pointerId: 1 });
    await c3.trigger("pointermove", { clientX: 905, clientY: 0, pointerId: 1 }); // +100ms
    await c3.trigger("pointerup", { clientX: 905, clientY: 0, pointerId: 1 });
    await flushPromises();

    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c3"], deltaMs: 100, trackId: null }]);
  });

  // Fix round 1 (review Important 1), through the REAL predicate ClipItem
  // passes: the lane below c2 (v1) is a1, an AUDIO track, and c2 is a video
  // clip. Rust would refuse moveClips{trackId:"a1"} outright and drop the
  // horizontal move with it; the drop stays on v1 and keeps the delta.
  it("a vertical drop onto an incompatible (audio) lane keeps the clip's track and its move", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const c2 = w.get('[data-testid="clip-c2"]'); // 0..1000 on v1 (lane 2 of 3)
    await c2.trigger("pointerdown", { clientX: 20, clientY: 100, pointerId: 1 });
    await c2.trigger("pointermove", { clientX: 30, clientY: 156, pointerId: 1 }); // +200ms, one lane down
    await c2.trigger("pointerup", { clientX: 30, clientY: 156, pointerId: 1 });
    await flushPromises();

    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c2"], deltaMs: 200, trackId: null }]);
  });

  it("a vertical drop onto a LOCKED lane keeps the clip's track; an unlocked one is targeted", async () => {
    for (const locked of [true, false]) {
      executed = [];
      setActivePinia(createPinia());
      await openProject({ tracks: [track("v2", { locked }), track("v1"), track("a1", { kind: "audio" })] });
      const w = mount(TimelineView, { attachTo: document.body });
      await flushPromises();

      const c2 = w.get('[data-testid="clip-c2"]'); // on v1; v2 is the lane ABOVE
      await c2.trigger("pointerdown", { clientX: 20, clientY: 100, pointerId: 1 });
      await c2.trigger("pointermove", { clientX: 30, clientY: 44, pointerId: 1 }); // +200ms, one lane up
      await c2.trigger("pointerup", { clientX: 30, clientY: 44, pointerId: 1 });
      await flushPromises();

      expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c2"], deltaMs: 200, trackId: locked ? null : "v2" }]);
      w.unmount();
    }
  });

  it("a secondary-button press never starts a drag", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const c1 = w.get('[data-testid="clip-c1"]');
    const before = c1.attributes("style");
    await c1.trigger("pointerdown", { clientX: 400, clientY: 0, pointerId: 1, button: 2 });
    await c1.trigger("pointermove", { clientX: 300, clientY: 0, pointerId: 1, button: 2 });
    expect(c1.attributes("style")).toBe(before);
    await c1.trigger("pointerup", { clientX: 300, clientY: 0, pointerId: 1, button: 2 });
    await flushPromises();

    expect(executed).toEqual([]);
  });

  // A drag ends in a click on the same element. Letting that click run the
  // ordinary "click selects only this clip" rule collapsed a multi-clip
  // selection the user had just dragged as a group down to one clip.
  it("the click that ends a group drag keeps the whole selection", async () => {
    executed = [];
    await openProject();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1", "c3"]);
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const c1 = w.get('[data-testid="clip-c1"]');
    await c1.trigger("pointerdown", { clientX: 400, clientY: 0, pointerId: 1 });
    // 60px = 1200ms: c1 lands at 1700, clear of every snap target.
    await c1.trigger("pointermove", { clientX: 460, clientY: 0, pointerId: 1 });
    await c1.trigger("pointerup", { clientX: 460, clientY: 0, pointerId: 1 });
    await c1.trigger("click", { clientX: 460, clientY: 0 });
    await flushPromises();

    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c1", "c3"], deltaMs: 1_200, trackId: null }]);
    expect(workspace.selectionClipIds).toEqual(["c1", "c3"]);
  });

  // The handles through the real component: a trim previews locally while
  // the pointer moves (the clip's rendered width changes, nothing is sent)
  // and commits exactly ONE trimClip on release.
  it("dragging the end handle previews, then submits exactly one trimClip", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const handle = w.get('[data-testid="clip-c1-trim-end"]');
    const before = w.get('[data-testid="clip-c1"]').attributes("style");
    // c1 is 500..3000; 20px left at zoom 1 is 400ms off its end.
    await handle.trigger("pointerdown", { clientX: 150, clientY: 0, pointerId: 1 });
    await handle.trigger("pointermove", { clientX: 130, clientY: 0, pointerId: 1 });
    expect(w.get('[data-testid="clip-c1"]').attributes("style")).not.toBe(before);
    expect(executed).toEqual([]);
    await handle.trigger("pointerup", { clientX: 130, clientY: 0, pointerId: 1 });
    await flushPromises();

    expect(executed).toEqual([{ kind: "trimClip", clipId: "c1", startMs: 500, inMs: 0, outMs: 2_100 }]);
  });

  it("dragging the start handle moves the start and keeps the end", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const handle = w.get('[data-testid="clip-c1-trim-start"]');
    await handle.trigger("pointerdown", { clientX: 25, clientY: 0, pointerId: 1 });
    await handle.trigger("pointermove", { clientX: 40, clientY: 0, pointerId: 1 }); // +300ms
    await handle.trigger("pointerup", { clientX: 40, clientY: 0, pointerId: 1 });
    await flushPromises();

    expect(executed).toEqual([{ kind: "trimClip", clipId: "c1", startMs: 800, inMs: 300, outMs: 2_500 }]);
  });

  it("Escape during a drag sends nothing and restores the clip's position", async () => {
    executed = [];
    await openProject();
    const w = mount(TimelineView, { attachTo: document.body });
    await flushPromises();

    const c2 = w.get('[data-testid="clip-c2"]');
    const before = c2.attributes("style");
    await c2.trigger("pointerdown", { clientX: 0, clientY: 0, pointerId: 1 });
    await c2.trigger("pointermove", { clientX: 200, clientY: 0, pointerId: 1 });
    expect(c2.attributes("style")).not.toBe(before);

    await c2.trigger("keydown", { key: "Escape" });
    await flushPromises();

    expect(w.get('[data-testid="clip-c2"]').attributes("style")).toBe(before);
    expect(executed).toEqual([]);
  });
});

describe("TimelineRuler — degenerate zoom", () => {
  it("a non-positive zoom seeks to 0 rather than dividing by zero", async () => {
    const workspace = useEditorWorkspaceStore();
    const w = mount(TimelineRuler, { props: { zoom: 0, widthPx: 500 } });

    const ticks = w.get('[data-testid="timeline-ruler-ticks"]');
    (ticks.element as HTMLElement).getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 500, height: 24, right: 500, bottom: 24, x: 0, y: 0 }) as DOMRect;

    await ticks.trigger("pointerdown", { clientX: 300 });

    expect(workspace.playheadMs).toBe(0);
  });
});

describe("TimelineRuler — drag (fix round 1, finding 2)", () => {
  it("a drag moves the playhead repeatedly and never touches the selection", async () => {
    executed = [];
    // A large durationMs -- workspace.setPlayhead clamps to
    // [0, editorProject.durationMs], and a standalone TimelineRuler mount
    // opens no project at all (durationMs 0) unless one is opened first.
    await openProject({}, { durationMs: 1_000_000 });
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    const w = mount(TimelineRuler, { props: { zoom: 1, widthPx: 2000 } });

    const ticks = w.get('[data-testid="timeline-ruler-ticks"]');
    (ticks.element as HTMLElement).getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 2000, height: 24, right: 2000, bottom: 24, x: 0, y: 0 }) as DOMRect;

    await ticks.trigger("pointerdown", { clientX: 400, pointerId: 1 });
    const afterDown = workspace.playheadMs;
    await ticks.trigger("pointermove", { clientX: 800, pointerId: 1 });
    const afterFirstMove = workspace.playheadMs;
    await ticks.trigger("pointermove", { clientX: 1200, pointerId: 1 });
    const afterSecondMove = workspace.playheadMs;
    await ticks.trigger("pointerup", { pointerId: 1 });

    // Each move genuinely moved the playhead further -- a drag, not a
    // single click plus inert hovering.
    expect(afterFirstMove).toBeGreaterThan(afterDown);
    expect(afterSecondMove).toBeGreaterThan(afterFirstMove);
    // Never the selection, at any point in the drag.
    expect(workspace.selectionClipIds).toEqual(["c1"]);

    // A move AFTER pointerup must not still be dragging.
    const stoppedAt = workspace.playheadMs;
    await ticks.trigger("pointermove", { clientX: 1900, pointerId: 1 });
    expect(workspace.playheadMs).toBe(stoppedAt);
  });

  it("a plain hover (no press) never moves the playhead", async () => {
    executed = [];
    // A real, nonzero range -- otherwise "never moves" would hold trivially
    // (workspace.setPlayhead clamps into [0, durationMs]) regardless of
    // whether the hover gate works at all.
    await openProject({}, { durationMs: 1_000_000 });
    const workspace = useEditorWorkspaceStore();
    workspace.setPlayhead(0);
    const w = mount(TimelineRuler, { props: { zoom: 1, widthPx: 2000 } });
    const ticks = w.get('[data-testid="timeline-ruler-ticks"]');
    (ticks.element as HTMLElement).getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 2000, height: 24, right: 2000, bottom: 24, x: 0, y: 0 }) as DOMRect;

    await ticks.trigger("pointermove", { clientX: 900 });

    expect(workspace.playheadMs).toBe(0);
  });
});
