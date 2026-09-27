/**
 * The timeline's toolbar and header row (visual-parity Task 16; concept
 * spec §6.2–6.3; design D12, D14; ruling P3), mounted inside the whole
 * `TimelineView` so the ruler, the menus and the store are the real ones.
 *
 * Every enabled control is checked for its effect (a command, a store
 * change, an opened menu); every disabled one for its reason, as its title
 * and as a toast when pressed.
 */
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import { clearClipboardForTest } from "../src/editor/clipboard";
import { pxPerMs } from "../src/editor/timelineLayout";
import type { Clip, EditorOpenResult, EditorSnapshot, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { useNotificationsStore } from "../src/stores/notifications";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

let executed: unknown[] = [];

beforeEach(() => {
  setActivePinia(createPinia());
  // A 1600px window: the full 196px label column (§1.4; happy-dom is 1024 wide).
  useEditorWorkspaceStore().setViewport(1600, 1000);
  clearClipboardForTest();
  executed = [];
});

// ---- fixtures ---------------------------------------------------------------

function track(id: string, overrides: Partial<Track> = {}): Track {
  return { id, kind: "video", name: id, visible: true, locked: false, muted: false, solo: false, volume: 1, ...overrides };
}

function clip(id: string, trackId: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "video-asset",
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

/** Two clips on v1; a chapter on c2, 1 s into its source (c2 starts at
 * 4 s with in 500 ms, so the chapter sits at 4.5 s of output). */
function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "video-asset", kind: "video", name: "video", duration_ms: 60_000 }],
    tracks: [track("v1"), track("a1", { kind: "audio" })],
    clips: [
      clip("c1", "v1", { start_ms: 0, out_ms: 3_000 }),
      clip("c2", "v1", { start_ms: 4_000, in_ms: 500, out_ms: 8_000 }),
    ],
    effects: [],
    markers: [{ id: "m1", clip_id: "c2", source_ms: 1_000, title: "Create a project" }],
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
    durationMs: 11_500,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
    ...overrides,
  };
}

async function mountTimeline(p: Partial<Project> = {}, s: Partial<EditorSnapshot> = {}): Promise<VueWrapper> {
  const store = useEditorProjectStore();
  const proj = project(p);
  const snap = snapshot(s);
  store.setPort(
    fakeEditorPort({
      openStaged: () =>
        Promise.resolve<EditorOpenResult>({ snapshot: snap, project: proj, workspace: {}, missing: [], sourceBase: "base", recovered: false }),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...snap, revision: snap.revision + 1 }, project: proj });
      },
    }),
  );
  await store.openStaged("base");
  const w = mount(TimelineView, { attachTo: document.body });
  await flushPromises();
  return w;
}

const byId = (w: VueWrapper, id: string) => w.get(`[data-testid="${id}"]`);
const toasts = () => useNotificationsStore().items.map((i) => i.message);

// ---- the toolbar -------------------------------------------------------------

describe("the timeline toolbar (§6.2)", () => {
  it("lays out the concept's controls, left to right", async () => {
    const w = await mountTimeline();
    const bar = byId(w, "timeline-toolbar");
    expect(bar.classes()).toContain("h-11"); // 44px
    expect(bar.element.firstElementChild?.textContent).toBe("Timeline");
    const order = bar.findAll("[data-testid^='timeline-toolbar-']").map((el) => el.attributes("data-testid"));
    expect(order).toEqual([
      "timeline-toolbar-undo", "timeline-toolbar-redo", "timeline-toolbar-split", "timeline-toolbar-delete",
      "timeline-toolbar-delete-mode", "timeline-toolbar-marker", "timeline-toolbar-more", "timeline-toolbar-snap",
      "timeline-toolbar-zoom-out", "timeline-toolbar-zoom-range", "timeline-toolbar-zoom-in", "timeline-toolbar-fit",
    ]);
    // Two dividers: after Redo and after Edit actions.
    const kids = Array.from(bar.element.children);
    const dividers = kids.filter((el) => el.classList.contains("w-px"));
    expect(dividers.map((d) => kids.indexOf(d))).toEqual([3, 9]);
    expect(byId(w, "timeline-toolbar-split").text()).toBe("Split");
    expect(byId(w, "timeline-toolbar-more").text()).toBe("Edit actions");
    expect(byId(w, "timeline-toolbar-snap").text()).toBe("Snap");
    expect(byId(w, "timeline-toolbar-fit").text()).toBe("Fit");
  });

  it("Undo and Redo are icon buttons named by their registry labels, with the shortcut in the tooltip", async () => {
    const w = await mountTimeline({}, { canUndo: true, undoLabel: "Move clip" });
    const undo = byId(w, "timeline-toolbar-undo");
    expect(undo.attributes("aria-label")).toBe("Undo Move clip");
    expect(undo.attributes("title")).toBe("Undo Move clip (Ctrl+Z)");
    expect(undo.find("svg").exists()).toBe(true);
    await undo.trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "undo" }]);

    const redo = byId(w, "timeline-toolbar-redo");
    expect(redo.attributes("aria-disabled")).toBe("true");
    expect(redo.attributes("title")).toBe("Nothing to redo");
  });

  it("a disabled control says its reason when pressed, and sends nothing", async () => {
    const w = await mountTimeline(); // nothing selected
    const split = byId(w, "timeline-toolbar-split");
    expect(split.attributes("aria-disabled")).toBe("true");
    expect(split.attributes("title")).toBe("Select a clip first");
    await split.trigger("click");
    await byId(w, "timeline-toolbar-delete").trigger("click");
    expect(executed).toEqual([]);
    expect(toasts()).toContain("Select a clip first");
  });

  it("Split and the trash send the registry's commands; the Delete-mode select picks the trash's shape", async () => {
    const w = await mountTimeline();
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    workspace.setPlayhead(1_000);
    await flushPromises();

    expect(byId(w, "timeline-toolbar-split").attributes("title")).toBe("Split selected clip at playhead (S)");
    await byId(w, "timeline-toolbar-split").trigger("click");

    const trash = byId(w, "timeline-toolbar-delete");
    expect(trash.attributes("aria-label")).toBe("Delete selection");
    expect(trash.attributes("title")).toBe("Delete selection, leaving a gap (Delete)");
    await trash.trigger("click");

    const mode = byId(w, "timeline-toolbar-delete-mode");
    expect(mode.findAll("option").map((o) => o.text())).toEqual(["Delete: leave gap", "Delete: close gap"]);
    expect((mode.element as HTMLSelectElement).value).toBe("gap");
    await mode.setValue("close");
    expect(workspace.deleteMode).toBe("close");
    expect(trash.attributes("title")).toBe("Delete selection and close the gap on its track (Shift+Delete)");
    await trash.trigger("click");
    await flushPromises();

    expect(executed).toEqual([
      { kind: "splitClip", clipId: "c1", atMs: 1_000 },
      { kind: "deleteClips", clipIds: ["c1"], closeGap: false },
      { kind: "deleteClips", clipIds: ["c1"], closeGap: true },
    ]);
  });

  it("the bookmark adds a chapter at the playhead (M)", async () => {
    const w = await mountTimeline();
    useEditorWorkspaceStore().setPlayhead(1_500);
    await flushPromises();
    const marker = byId(w, "timeline-toolbar-marker");
    expect(marker.attributes("aria-label")).toBe("Add chapter marker");
    expect(marker.attributes("title")).toBe("Add chapter marker at the playhead (M)");
    await marker.trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "addMarker", clipId: "c1", sourceMs: 1_500, title: "Chapter 2" }]);
  });

  it("Snap toggles, and reads active in the accent colours", async () => {
    const w = await mountTimeline();
    const workspace = useEditorWorkspaceStore();
    const snap = byId(w, "timeline-toolbar-snap");
    const before = workspace.snap;
    expect(snap.attributes("aria-pressed")).toBe(String(before));
    await snap.trigger("click");
    expect(workspace.snap).toBe(!before);
    expect(snap.attributes("aria-pressed")).toBe(String(!before));
    expect(snap.classes().includes("bg-accent-bg")).toBe(!before);
  });

  it("zoom out, the range and zoom in change the zoom; the end of the range disables its button with a reason", async () => {
    const w = await mountTimeline();
    const workspace = useEditorWorkspaceStore();
    await byId(w, "timeline-toolbar-zoom-in").trigger("click");
    expect(workspace.timelineZoom).toBeCloseTo(1.25, 5);
    await byId(w, "timeline-toolbar-zoom-out").trigger("click");
    await byId(w, "timeline-toolbar-zoom-out").trigger("click");
    expect(workspace.timelineZoom).toBeCloseTo(0.8, 5);

    const range = byId(w, "timeline-toolbar-zoom-range");
    expect(range.attributes("aria-label")).toBe("Timeline zoom");
    await range.setValue("2"); // log2: 4×
    expect(workspace.timelineZoom).toBeCloseTo(4, 5);
    expect(range.attributes("aria-valuetext")).toBe("4.00×");

    workspace.setZoom(20);
    await flushPromises();
    const zoomIn = byId(w, "timeline-toolbar-zoom-in");
    expect(zoomIn.attributes("aria-disabled")).toBe("true");
    expect(zoomIn.attributes("title")).toBe("Zoomed all the way in");
    await zoomIn.trigger("click");
    expect(workspace.timelineZoom).toBe(20);
    expect(toasts()).toContain("Zoomed all the way in");

    workspace.setZoom(0.1);
    await flushPromises();
    expect(byId(w, "timeline-toolbar-zoom-out").attributes("title")).toBe("Zoomed all the way out");
    expect(byId(w, "timeline-toolbar-zoom-in").attributes("aria-disabled")).toBe("false");
  });

  it("Fit computes a real zoom from the measured viewport width", async () => {
    await mountTimeline({}, { durationMs: 60_000 });
    const workspace = useEditorWorkspaceStore();
    workspace.setZoom(5);
    const w = mount(TimelineView, { props: { viewportWidth: 1200 } });
    await flushPromises();
    await byId(w, "timeline-toolbar-fit").trigger("click");
    // fitZoom(60_000, 1200) = 1200 / (BASE_PX_PER_MS * 60_000) = 0.4.
    expect(workspace.timelineZoom).toBeCloseTo(0.4, 5);
    expect(workspace.timelineScrollLeft).toBe(0);
  });
});

// ---- the header row -----------------------------------------------------------

describe("the timeline header row (§6.3)", () => {
  it("is 32px and sticky at the top; its label cell is sticky at the left (D12) with Add track and Layers ↓", async () => {
    const w = await mountTimeline();
    const row = byId(w, "timeline-ruler");
    expect(row.classes()).toEqual(expect.arrayContaining(["sticky", "top-0", "h-8", "w-max", "min-w-full"]));
    const label = byId(w, "timeline-ruler-label");
    expect(label.classes()).toEqual(expect.arrayContaining(["sticky", "left-0", "bg-panel"]));
    expect(label.attributes("style")).toContain("width: 196px");
    expect(byId(w, "timeline-add-track").text()).toBe("Add track");
    expect(label.text()).toContain("Layers ↓");
  });

  it("ticks every 2 s at the default zoom, every 5 s zoomed out to 20 px/s", async () => {
    const w = await mountTimeline();
    const labels = () => w.findAll('[data-testid="timeline-ruler-tick"]').map((t) => t.text());
    expect(labels().slice(0, 4)).toEqual(["00:00", "00:02", "00:04", "00:06"]);
    const ticks = w.findAll('[data-testid="timeline-ruler-tick"]');
    expect(ticks[1].attributes("style")).toContain("left: 100px");
    expect(ticks[1].classes()).toEqual(expect.arrayContaining(["font-mono", "text-[9px]", "after:h-[7px]"]));

    useEditorWorkspaceStore().setZoom(0.4);
    await flushPromises();
    expect(labels().slice(0, 3)).toEqual(["00:00", "00:05", "00:10"]);
  });

  it("the ruler is the playhead's slider: arrows step a frame, Page Up/Down a second", async () => {
    const w = await mountTimeline();
    const workspace = useEditorWorkspaceStore();
    const ruler = byId(w, "timeline-ruler-ticks");
    expect(ruler.attributes("role")).toBe("slider");
    expect(ruler.attributes("aria-valuemax")).toBe("11.5");
    await ruler.trigger("keydown", { key: "ArrowRight" });
    expect(workspace.playheadMs).toBe(33);
    await ruler.trigger("keydown", { key: "PageUp" });
    expect(workspace.playheadMs).toBe(1_033);
    await ruler.trigger("keydown", { key: "ArrowDown" });
    expect(workspace.playheadMs).toBe(1_000);
    expect(ruler.attributes("aria-valuetext")).toBe("00:01.0");
    await ruler.trigger("keydown", { key: "PageDown" });
    await ruler.trigger("keydown", { key: "ArrowLeft" });
    expect(workspace.playheadMs).toBe(0);
  });

  // Fix round 1, finding 4: one frame is the project's own, not a fixed 33 ms.
  it("an arrow steps one frame of the project's own frame rate (whole ms)", async () => {
    const w = await mountTimeline({ canvas: { width: 1280, height: 720, fps: 60 } });
    const workspace = useEditorWorkspaceStore();
    const ruler = byId(w, "timeline-ruler-ticks");
    await ruler.trigger("keydown", { key: "ArrowRight" });
    expect(workspace.playheadMs).toBe(17);
    await ruler.trigger("keydown", { key: "ArrowUp" });
    expect(workspace.playheadMs).toBe(34);
  });

  it("a chapter is a gold ◆ at its OUTPUT time; clicking it seeks there, not to the press", async () => {
    const w = await mountTimeline();
    const workspace = useEditorWorkspaceStore();
    const marker = byId(w, "timeline-marker-m1");
    expect(marker.text()).toBe("◆");
    expect(marker.attributes("aria-label")).toBe("Go to Create a project");
    expect(marker.attributes("title")).toBe("Create a project");
    expect(marker.classes()).toEqual(expect.arrayContaining(["text-gold", "h-6", "w-6", "-translate-x-1/2"]));
    expect(marker.attributes("style")).toContain(`left: ${4_500 * pxPerMs(1)}px`);
    // Fix round 1, finding 1: a slider's children are presentational, so a
    // marker nested in it would lose its role and name. It sits beside it.
    expect(marker.element.closest('[role="slider"]')).toBeNull();
    expect(marker.element.closest('[data-testid="timeline-ruler"]')).not.toBeNull();
    expect(byId(w, "timeline-ruler-ticks").findAll("button")).toHaveLength(0);

    // The press does not reach the ruler (which would seek to the pointer).
    await marker.trigger("pointerdown", { clientX: 10 });
    expect(workspace.playheadMs).toBe(0);
    await marker.trigger("click");
    expect(workspace.playheadMs).toBe(4_500);
    // Its keys are its own: an arrow on a focused marker does not step the playhead.
    await marker.trigger("keydown", { key: "ArrowRight" });
    expect(workspace.playheadMs).toBe(4_500);
  });

  it("Add track opens the registry's two actions; choosing one adds that track (ruling P3)", async () => {
    const w = await mountTimeline();
    const trigger = byId(w, "timeline-add-track");
    expect(trigger.attributes("aria-expanded")).toBe("false");
    await trigger.trigger("click");
    expect(trigger.attributes("aria-expanded")).toBe("true");
    // The row lifts above the playhead while its menu is open.
    expect(byId(w, "timeline-ruler").classes()).toContain("z-40");
    const video = byId(w, "timeline-add-track-panel-item-addTrackVideo");
    expect(video.text()).toContain("Add video track");
    expect(byId(w, "timeline-add-track-panel-item-addTrackAudio").text()).toContain("Add audio track");
    await video.trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "addTrack", trackKind: "video", name: "Video 2", index: 0 }]);
    expect(w.find('[data-testid="timeline-add-track-panel-root"]').exists()).toBe(false);

    await trigger.trigger("click");
    await byId(w, "timeline-add-track-panel-item-addTrackAudio").trigger("click");
    await flushPromises();
    expect(executed[1]).toEqual({ kind: "addTrack", trackKind: "audio", name: "Audio 2", index: 2 });
  });

  it("at Rust's 32-track limit both items are disabled and say why", async () => {
    const w = await mountTimeline({ tracks: Array.from({ length: 32 }, (_, i) => track(`t${i}`)), clips: [], markers: [] });
    await byId(w, "timeline-add-track").trigger("click");
    const video = byId(w, "timeline-add-track-panel-item-addTrackVideo");
    expect(video.attributes("aria-disabled")).toBe("true");
    await video.trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
    (video.element as HTMLElement).focus();
    await flushPromises();
    expect(byId(w, "timeline-add-track-panel-hint").text()).toBe("This project already has the maximum of 32 tracks");
  });
});

// ---- keyboard (fix round 1, findings 2 and 3) ---------------------------------

describe("the timeline toolbar from the keyboard", () => {
  const ROVING = ["undo", "redo", "split", "delete", "marker", "more", "snap", "zoom-out", "zoom-in", "fit"];
  const tabindexes = (w: VueWrapper) => ROVING.map((id) => byId(w, `timeline-toolbar-${id}`).attributes("tabindex"));
  const focused = () => (document.activeElement as HTMLElement | null)?.dataset.testid;
  function press(w: VueWrapper, id: string, key: string, init: KeyboardEventInit = {}): KeyboardEvent {
    const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
    byId(w, id).element.dispatchEvent(event);
    return event;
  }

  it("its buttons are one Tab stop with roving arrows (D16); the select and the range stay their own", async () => {
    const w = await mountTimeline();
    expect(tabindexes(w)).toEqual(["0", "-1", "-1", "-1", "-1", "-1", "-1", "-1", "-1", "-1"]);
    expect(byId(w, "timeline-toolbar-delete-mode").attributes("tabindex")).toBeUndefined();
    expect(byId(w, "timeline-toolbar-zoom-range").attributes("tabindex")).toBeUndefined();

    (byId(w, "timeline-toolbar-undo").element as HTMLElement).focus();
    press(w, "timeline-toolbar-undo", "ArrowRight");
    await flushPromises();
    expect(focused()).toBe("timeline-toolbar-redo");
    expect(tabindexes(w).indexOf("0")).toBe(1);

    // End jumps to Fit, across the select and the range; → wraps to Undo.
    const end = press(w, "timeline-toolbar-redo", "End");
    await flushPromises();
    expect(end.defaultPrevented).toBe(true); // so the editor's own End (seek) stays out
    expect(focused()).toBe("timeline-toolbar-fit");
    press(w, "timeline-toolbar-fit", "ArrowRight");
    await flushPromises();
    expect(focused()).toBe("timeline-toolbar-undo");
    press(w, "timeline-toolbar-undo", "ArrowLeft");
    await flushPromises();
    expect(focused()).toBe("timeline-toolbar-fit");
    press(w, "timeline-toolbar-fit", "Home");
    await flushPromises();
    expect(focused()).toBe("timeline-toolbar-undo");

    // A click (or Tab) onto a button makes it the toolbar's stop.
    await byId(w, "timeline-toolbar-snap").trigger("focus");
    expect(tabindexes(w).indexOf("0")).toBe(ROVING.indexOf("snap"));

    // The select's and the range's arrows are their own.
    for (const id of ["timeline-toolbar-delete-mode", "timeline-toolbar-zoom-range"]) {
      (byId(w, id).element as HTMLElement).focus();
      const arrow = press(w, id, "ArrowRight");
      await flushPromises();
      expect(arrow.defaultPrevented).toBe(false);
      expect(focused()).toBe(id);
    }
  });

  it("Shift+F10 or the Menu key on the toolbar opens Edit actions, as its tooltip says", async () => {
    const w = await mountTimeline();
    const more = byId(w, "timeline-toolbar-more");
    expect(more.attributes("title")).toContain("(Shift+F10)");
    const event = press(w, "timeline-toolbar-undo", "F10", { shiftKey: true });
    await flushPromises();
    expect(event.defaultPrevented).toBe(true);
    expect(more.attributes("aria-expanded")).toBe("true");
    expect(w.find('[data-testid="editor-context-menu-root"]').exists()).toBe(true);

    // Escape closes it and hands focus back to the toolbar.
    (document.activeElement as HTMLElement | null)?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await flushPromises();
    expect(more.attributes("aria-expanded")).toBe("false");
    press(w, "timeline-toolbar-more", "ContextMenu");
    await flushPromises();
    expect(more.attributes("aria-expanded")).toBe("true");
  });
});
