/**
 * The timeline footer (visual-parity Task 20; concept spec §7, design D10,
 * D14, D16): the live edit hint on the left, "Audio mixer" with the
 * audio-track count on the right. Mounted as the whole `TimelineView`
 * where the hint is concerned — the drag lives in a `ClipItem` or a
 * `CueChip`, the footer in the view, and the hint has to cross from one to
 * the other. The 27 px height is measured in real Chromium
 * (`tests/e2e/editorParity.spec.ts`).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TransportBar from "../src/components/editor/preview/TransportBar.vue";
import TimelineFooter from "../src/components/editor/timeline/TimelineFooter.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import { clipDragHint } from "../src/editor/dragHint";
import { requestReveal } from "../src/editor/revealBus";
import type { Effect, Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { clip, openInspectorProject, project, track } from "./helpers/inspectorProject";

enableAutoUnmount(afterEach);

const DEFAULT_HINT = "Callouts follow their clip.";

function fx(id: string, clipId: string, start: number, end: number): Effect {
  return { id, clip_id: clipId, kind: "arrow", start_ms: start, end_ms: end, x: 0.5, y: 0.5, color: "#fff" };
}

/** Two audio tracks (the concept's pill reads "2"); c1 and c2 grouped on
 * v1; an arrow cue on c1. Zoom 1 is 50 px a second, 20 ms a pixel. */
function sample(overrides: Partial<Project> = {}): Project {
  return project({
    tracks: [track("v3", "Presenter"), track("v1", "Screen recording"), track("a1", "Voice", "audio"), track("a2", "Music", "audio")],
    clips: [
      clip("c5", "presenter", "v3", 1_500, 20_000, "Presenter"),
      clip("c1", "capture", "v1", 0, 9_500, "Open", { group_id: "g1" }),
      clip("c2", "capture", "v1", 9_500, 10_000, "Create", { group_id: "g1" }),
      clip("c3", "capture", "v1", 20_000, 5_000, "Loose"),
      clip("c6", "music", "a1", 300, 20_000, "Bed"),
    ],
    effects: [fx("fx1", "c1", 500, 6_500)],
    ...overrides,
  });
}

beforeEach(() => {
  setActivePinia(createPinia());
});

async function timeline(p: Project = sample()) {
  await openInspectorProject(p);
  const workspace = useEditorWorkspaceStore();
  workspace.setViewport(1600, 1000);
  workspace.select([]);
  workspace.setSelected(null);
  if (workspace.snap) workspace.toggleSnap();
  const w = mount(TimelineView, { attachTo: document.body, props: { viewportWidth: 1_000 } });
  await flushPromises();
  return w;
}

const hint = (w: Awaited<ReturnType<typeof timeline>>) => w.get('[data-testid="timeline-footer-hint"]').text();

describe("the timeline footer (§7)", () => {
  it("sits under the lanes: the link icon and the default hint left, Audio mixer right", async () => {
    const w = await timeline();
    const footer = w.get('[data-testid="timeline-footer"]');
    expect(w.get('[data-testid="timeline-scroll"]').element.nextElementSibling).toBe(footer.element);
    expect(footer.classes()).toEqual(expect.arrayContaining(["h-[27px]", "border-t", "border-line", "text-[10px]", "text-fg-muted"]));
    expect(w.get('[data-testid="timeline-footer-hint-icon"]').element.tagName.toLowerCase()).toBe("svg");
    expect(hint(w)).toBe(DEFAULT_HINT);
    const mixer = w.get('[data-testid="mixer-toggle"]');
    expect(footer.element.contains(mixer.element)).toBe(true);
    expect(mixer.text()).toContain("Audio mixer");
  });

  it("the pill counts the live project's audio tracks", async () => {
    const w = await timeline();
    expect(w.get('[data-testid="mixer-toggle-count"]').text()).toBe("2");
    expect(w.get('[data-testid="mixer-toggle"]').attributes("aria-label")).toBe("Audio mixer, 2 audio tracks");
    const store = useEditorProjectStore();
    store.project = { ...store.project!, tracks: store.project!.tracks.filter((t) => t.id !== "a2") };
    await flushPromises();
    expect(w.get('[data-testid="mixer-toggle-count"]').text()).toBe("1");
    expect(w.get('[data-testid="mixer-toggle"]').attributes("aria-label")).toBe("Audio mixer, 1 audio track");
  });

  it("is the guide's audio target: the concept's data-action=mixer", async () => {
    const w = await timeline();
    expect(w.findAll('[data-action="mixer"]')).toHaveLength(1);
    expect(w.get('[data-testid="mixer-toggle"]').attributes("data-action")).toBe("mixer");
  });

  it("opens the mixer above itself, fixed so the timeline's clip cannot cut it off", async () => {
    const w = await timeline();
    const toggle = w.get('[data-testid="mixer-toggle"]');
    expect(toggle.attributes("aria-expanded")).toBe("false");
    await toggle.trigger("click");
    const popover = w.get('[data-testid="mixer-popover"]');
    expect(toggle.attributes("aria-expanded")).toBe("true");
    expect(popover.classes()).toContain("fixed");
    expect(popover.attributes("style")).toMatch(/bottom: \d+px/);
  });

  // Fix round 1: the panel's height is held to the room above the button
  // in the WINDOW's terms (`100vh`), and it is placed again on every window
  // resize while open — a height frozen at open strands its first rows above
  // the top once the window shrinks.
  it("holds its height to the window and follows a resize while open", async () => {
    const w = await timeline();
    const toggle = w.get('[data-testid="mixer-toggle"]');
    const rect = (top: number) => ({ top, bottom: top + 25, left: 1_400, right: 1_587, width: 187, height: 25, x: 1_400, y: top, toJSON: () => ({}) });
    const box = vi.spyOn(toggle.element, "getBoundingClientRect").mockReturnValue(rect(948) as DOMRect);
    vi.spyOn(window, "innerHeight", "get").mockReturnValue(1_000);
    await toggle.trigger("click");
    const style = () => w.get('[data-testid="mixer-popover"]').attributes("style") ?? "";
    expect(style()).toContain("bottom: 56px");
    expect(style()).toContain("max-height: calc(100vh - 64px)");

    box.mockReturnValue(rect(500) as DOMRect);
    vi.spyOn(window, "innerHeight", "get").mockReturnValue(640);
    window.dispatchEvent(new Event("resize"));
    await flushPromises();
    expect(style()).toContain("bottom: 144px");
    expect(style()).toContain("max-height: calc(100vh - 152px)");
    vi.restoreAllMocks();
  });

  it("stops listening for resizes once it closes", async () => {
    const w = await timeline();
    const added = vi.spyOn(window, "addEventListener");
    const removed = vi.spyOn(window, "removeEventListener");
    const toggle = w.get('[data-testid="mixer-toggle"]');
    await toggle.trigger("click");
    const listener = added.mock.calls.find(([type]) => type === "resize")?.[1];
    expect(listener).toBeTypeOf("function");
    await toggle.trigger("click");
    expect(removed).toHaveBeenCalledWith("resize", listener);
    vi.restoreAllMocks();
  });

  it("stops listening for resizes when it unmounts open", async () => {
    const w = await timeline();
    const added = vi.spyOn(window, "addEventListener");
    const removed = vi.spyOn(window, "removeEventListener");
    await w.get('[data-testid="mixer-toggle"]').trigger("click");
    const listener = added.mock.calls.find(([type]) => type === "resize")?.[1];
    w.unmount();
    expect(removed).toHaveBeenCalledWith("resize", listener);
    vi.restoreAllMocks();
  });

  it("Escape closes the mixer and gives focus back to the footer's button (D16)", async () => {
    const w = await timeline();
    const toggle = w.get('[data-testid="mixer-toggle"]');
    (toggle.element as HTMLElement).focus();
    await toggle.trigger("click");
    w.get<HTMLInputElement>('[data-testid="mixer-monitor-mute"]').element.focus();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    await flushPromises();
    expect(w.find('[data-testid="mixer-popover"]').exists()).toBe(false);
    expect(document.activeElement).toBe(toggle.element);
  });

  it("a before-you-share finding's Open the mixer opens it here", async () => {
    const w = await timeline();
    requestReveal("mixer");
    await flushPromises();
    expect(w.find('[data-testid="mixer-popover"]').exists()).toBe(true);
  });
});

describe("the edit hint follows the live drag", () => {
  it("one clip dragged: where it starts now, and back to the default on release", async () => {
    const w = await timeline();
    const c3 = w.get('[data-testid="clip-c3"]');
    await c3.trigger("pointerdown", { button: 0, clientX: 500, clientY: 0, pointerId: 1 });
    await c3.trigger("pointermove", { clientX: 550, clientY: 0, pointerId: 1 });
    expect(hint(w)).toBe("Starts at 00:21.0 · release to place");
    await c3.trigger("pointerup", { clientX: 550, clientY: 0, pointerId: 1 });
    await flushPromises();
    expect(hint(w)).toBe(DEFAULT_HINT);
  });

  it("a selection counts what really moves: a grouped partner joins it", async () => {
    const w = await timeline();
    useEditorWorkspaceStore().select(["c1", "c5"]);
    await flushPromises();
    const c1 = w.get('[data-testid="clip-c1"]');
    await c1.trigger("pointerdown", { button: 0, clientX: 100, clientY: 0, pointerId: 1 });
    await c1.trigger("pointermove", { clientX: 150, clientY: 0, pointerId: 1 });
    // c1 and c5 selected, c2 grouped with c1: moveClips moves three.
    expect(hint(w)).toBe("Moving 3 clips together · tracks stay fixed · Esc cancels");
  });

  it("a lone clip in a group moves its group too", async () => {
    const w = await timeline();
    const c2 = w.get('[data-testid="clip-c2"]');
    await c2.trigger("pointerdown", { button: 0, clientX: 600, clientY: 0, pointerId: 1 });
    await c2.trigger("pointermove", { clientX: 620, clientY: 0, pointerId: 1 });
    expect(hint(w)).toBe("Moving 2 clips together · tracks stay fixed · Esc cancels");
  });

  // Final review, Important 4: a lone clip of a group, dropped on another
  // lane, sent `trackId` (one id named) while Rust's group expansion made
  // it a multi-clip move — refused whole, horizontal delta included — as
  // the hint said "tracks stay fixed". Now the release agrees with the
  // hint: the time change lands, the track does not change.
  it("a grouped clip dropped on another lane keeps its track and its time change, as the hint says", async () => {
    const executed = await openInspectorProject(sample());
    const workspace = useEditorWorkspaceStore();
    workspace.setViewport(1600, 1000);
    workspace.select([]);
    if (workspace.snap) workspace.toggleSnap();
    const w = mount(TimelineView, { attachTo: document.body, props: { viewportWidth: 1_000 } });
    await flushPromises();
    const c2 = w.get('[data-testid="clip-c2"]');
    // c2 sits on v1; one lane up (68 px) is v3, a video lane that would
    // accept a lone clip.
    await c2.trigger("pointerdown", { button: 0, clientX: 600, clientY: 100, pointerId: 1 });
    await c2.trigger("pointermove", { clientX: 620, clientY: 32, pointerId: 1 });
    expect(hint(w)).toBe("Moving 2 clips together · tracks stay fixed · Esc cancels");
    // No lane preview: the dragged clip is still drawn in its own lane.
    expect(w.get('[data-testid="track-lane-v1"]').find('[data-testid="clip-c2"]').exists()).toBe(true);
    await c2.trigger("pointerup", { clientX: 620, clientY: 32, pointerId: 1 });
    await flushPromises();
    expect(executed).toEqual([{ kind: "moveClips", clipIds: ["c2"], deltaMs: 400, trackId: null }]);
  });

  it("Escape cancels the drag and the hint with it", async () => {
    const w = await timeline();
    const c3 = w.get('[data-testid="clip-c3"]');
    await c3.trigger("pointerdown", { button: 0, clientX: 500, clientY: 0, pointerId: 1 });
    await c3.trigger("pointermove", { clientX: 550, clientY: 0, pointerId: 1 });
    await c3.trigger("keydown", { key: "Escape" });
    expect(hint(w)).toBe(DEFAULT_HINT);
  });

  it("a trim says how it ends", async () => {
    const w = await timeline();
    const grip = w.get('[data-testid="clip-c3-trim-end"]');
    await grip.trigger("pointerdown", { button: 0, clientX: 1_250, clientY: 0, pointerId: 1 });
    await grip.trigger("pointermove", { clientX: 1_200, clientY: 0, pointerId: 1 });
    expect(hint(w)).toBe("Release to apply · Escape to cancel");
  });

  it("a fade handle shows the fade it sets", async () => {
    const w = await timeline();
    const handle = w.get('[data-testid="clip-c3-fade-in-handle"]');
    await handle.trigger("pointerdown", { button: 0, clientX: 1_000, clientY: 0, pointerId: 1 });
    await handle.trigger("pointermove", { clientX: 1_025, clientY: 0, pointerId: 1 });
    expect(hint(w)).toBe("Fade in: 0.50s");
  });

  it("a cue dragged says it stays on its clip; a cue trimmed says it resizes", async () => {
    const w = await timeline();
    const chip = w.get('[data-testid="timeline-cue-fx1"]');
    await chip.trigger("pointerdown", { button: 0, clientX: 100, pointerId: 1 });
    await chip.trigger("pointermove", { clientX: 150, pointerId: 1 });
    expect(hint(w)).toBe("Moving arrow · stays on its clip · Esc cancels");
    await chip.trigger("keydown", { key: "Escape" });
    expect(hint(w)).toBe(DEFAULT_HINT);

    const grip = w.get('[data-testid="timeline-cue-fx1-trim-end"]');
    await grip.trigger("pointerdown", { button: 0, clientX: 320, pointerId: 1 });
    await grip.trigger("pointermove", { clientX: 370, pointerId: 1 });
    expect(hint(w)).toBe("Resizing arrow · stays on its clip · Esc cancels");
  });
});

// Fix round 1: every clip on the timeline keeps a hint computed, so an
// idle one must not read the project or the selection at all — otherwise
// each selection change re-runs every clip's hint for nothing.
describe("clipDragHint", () => {
  it("reads neither the project nor the selection while nothing is dragged", () => {
    const project = vi.fn(() => sample());
    const moveIds = vi.fn(() => ["c1"]);
    const c = clip("c1", "capture", "v1", 0, 1_000, "One");
    expect(clipDragHint({ project, clip: c, moveIds, move: null, trim: null, fade: null })).toBeNull();
    expect(project).not.toHaveBeenCalled();
    expect(moveIds).not.toHaveBeenCalled();
    expect(clipDragHint({ project, clip: c, moveIds, move: { deltaMs: 0 }, trim: null, fade: null })).toBe(
      "Moving 2 clips together · tracks stay fixed · Esc cancels",
    );
  });
});

describe("the mixer's one home", () => {
  it("the transport row no longer carries a mixer trigger", () => {
    const w = mount(TransportBar, { props: { playing: false, currentMs: 0, durationMs: 0, canvas: null } });
    expect(w.find('[data-testid="mixer-toggle"]').exists()).toBe(false);
    expect(w.find('[data-action="mixer"]').exists()).toBe(false);
  });

  it("the footer alone shows the default hint with no drag model mounted", async () => {
    await openInspectorProject(sample());
    const w = mount(TimelineFooter);
    expect(w.get('[data-testid="timeline-footer-hint"]').text()).toBe(DEFAULT_HINT);
  });
});
