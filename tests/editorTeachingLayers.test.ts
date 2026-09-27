/**
 * The timeline's Teaching layers and Captions rows (visual-parity Task 19;
 * concept spec §6.4, design D11, D14, D16). Mounted as the whole
 * `TimelineView`, because what matters crosses components: a cue's
 * right-click must reach the ONE context menu, a click must put the
 * inspector in its cue state, a caption must open the Captions tab. The
 * packing and drag arithmetic have their own suite (`editorCueLanes`);
 * the heights and the pinned label column are measured in real Chromium
 * (`tests/e2e/editorParity.spec.ts`).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import InspectorPanel from "../src/components/editor/inspector/InspectorPanel.vue";
import CaptionsLibrary from "../src/components/editor/library/CaptionsLibrary.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { revealSerial } from "../src/editor/revealBus";
import type { Effect, Project } from "../src/editorTypes";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { useNotificationsStore } from "../src/stores/notifications";
import { clip, openInspectorProject, project, track } from "./helpers/inspectorProject";

enableAutoUnmount(afterEach);

function fx(id: string, clipId: string, start: number, end: number, extra: Partial<Effect> = {}): Effect {
  return { id, clip_id: clipId, kind: "text", start_ms: start, end_ms: end, x: 0.5, y: 0.5, color: "#fff", ...extra };
}

/** Two lanes (fx1 and fx2 overlap), a zoom, and a cue on a LOCKED track;
 * two captions on c1. Zoom 1 is 50 px a second. */
function sample(overrides: Partial<Project> = {}): Project {
  return project({
    tracks: [track("v1", "Screen recording"), track("vL", "Locked overlay", "video", { locked: true }), track("a1", "Guide cues", "audio")],
    clips: [
      clip("c1", "capture", "v1", 0, 10_000, "One"),
      clip("c2", "capture", "v1", 10_000, 10_000, "Two"),
      clip("cL", "capture", "vL", 0, 20_000, "Pinned"),
      clip("c6", "music", "a1", 0, 20_000, "Bed"),
    ],
    effects: [
      fx("fx1", "c1", 500, 6_500, { text: "A little structure." }),
      fx("fx2", "c1", 2_700, 8_000, { kind: "highlight" }),
      fx("fx3", "c2", 2_000, 5_000, { kind: "zoom", factor: 1.65 }),
      fx("fxL", "cL", 16_000, 18_000, { kind: "arrow" }),
    ],
    captions: {
      enabled: true,
      burn_in: false,
      font_size: 28,
      position: "bottom",
      background: true,
      cues: [
        { id: "cap2", clip_id: "c1", start_ms: 5_000, end_ms: 7_000, text: "Second line" },
        { id: "cap1", clip_id: "c1", start_ms: 1_000, end_ms: 3_000, text: "Hi there" },
      ],
    },
    ...overrides,
  });
}

let executed: EditorCommand[] = [];

beforeEach(() => {
  setActivePinia(createPinia());
});

async function timeline(p: Project = sample()) {
  executed = await openInspectorProject(p);
  const workspace = useEditorWorkspaceStore();
  workspace.setViewport(1600, 1000);
  workspace.select([]);
  workspace.setSelected(null);
  const w = mount(TimelineView, { attachTo: document.body, props: { viewportWidth: 1_000 } });
  await flushPromises();
  return w;
}

const heading = () => document.querySelector('[data-testid="editor-context-menu-heading"]')?.textContent ?? null;

function testIds(w: Awaited<ReturnType<typeof timeline>>): string[] {
  return [...w.get('[data-testid="timeline-content"]').element.children].map((el) => el.getAttribute("data-testid") ?? "");
}

describe("the Teaching layers row (§6.4)", () => {
  it("sits after the ruler and the Captions row, above every track", async () => {
    const w = await timeline();
    const order = testIds(w).filter((id) => /^(timeline-ruler|captions-row|teaching-layers-row|track-lane-)/.test(id));
    expect(order).toEqual(["timeline-ruler", "captions-row", "teaching-layers-row", "track-lane-v1", "track-lane-vL", "track-lane-a1"]);
  });

  it("is max(44, lanes×22+5) tall: 49 for two lanes", async () => {
    const w = await timeline();
    expect(w.get('[data-testid="teaching-layers-row"]').attributes("style")).toContain("height: 49px");
  });

  it("is 44 tall, and still there, with no cues at all", async () => {
    const empty = await timeline(sample({ effects: [] }));
    expect(empty.get('[data-testid="teaching-layers-row"]').attributes("style")).toContain("height: 44px");
    expect(empty.findAll('[data-testid^="cue-"]')).toHaveLength(0);
  });

  it("pins its label cell like a track header: the text badge on accent, Teaching layers, Attached to video", async () => {
    const w = await timeline();
    const cell = w.get('[data-testid="teaching-layers-label"]');
    expect(cell.classes()).toEqual(expect.arrayContaining(["sticky", "left-0", "z-[15]", "bg-panel"]));
    expect(cell.attributes("style")).toContain("width: 196px");
    const badge = w.get('[data-testid="teaching-layers-badge"]');
    expect(badge.classes()).toEqual(expect.arrayContaining(["bg-accent-bg", "text-accent-ink"]));
    expect(badge.find("svg").exists()).toBe(true);
    expect(cell.text()).toContain("Teaching layers");
    expect(w.get('[data-testid="teaching-layers-sub"]').text()).toBe("Attached to video");
  });

  it("draws each cue 19px tall at lane×22+3, where its clip plays it, labelled with its text or kind", async () => {
    const w = await timeline();
    const one = w.get('[data-testid="cue-fx1"]');
    expect(one.classes()).toContain("h-[19px]");
    expect(one.attributes("style")).toMatch(/left: 25px;.*width: 300px;.*top: 3px/);
    expect(one.text()).toBe("A little structure.");
    const two = w.get('[data-testid="cue-fx2"]');
    expect(two.attributes("style")).toContain("top: 25px");
    expect(two.text()).toBe("Highlight");
  });

  it("a zoom cue is gold and reads its factor", async () => {
    const w = await timeline();
    const zoom = w.get('[data-testid="cue-fx3"]');
    expect(zoom.text()).toBe("1.65× Focus");
    expect(zoom.classes()).toEqual(expect.arrayContaining(["bg-gold-bg", "text-gold", "border-gold"]));
    expect(w.get('[data-testid="cue-fx1"]').classes()).toEqual(expect.arrayContaining(["bg-accent-bg", "text-accent-ink"]));
  });
});

describe("selecting a cue (the inspector's cue state)", () => {
  it("a click selects the cue and its clip, and brings the inspector in on Teaching properties", async () => {
    const inspector = mount(InspectorPanel);
    const w = await timeline();
    const before = revealSerial("inspector");
    await w.get('[data-testid="cue-fx2"]').trigger("click");
    await flushPromises();
    const workspace = useEditorWorkspaceStore();
    expect(workspace.selected).toEqual({ type: "effect", id: "fx2" });
    expect(workspace.selectionClipIds).toEqual(["c1"]);
    expect(revealSerial("inspector")).toBe(before + 1);
    expect(inspector.get('[data-testid="inspector-title"]').text()).toBe("Teaching properties");
    expect(w.get('[data-testid="cue-fx2"]').attributes("aria-selected")).toBe("true");
    expect(w.get('[data-testid="cue-fx2"]').classes()).toContain("outline-accent");
  });

  it("Enter selects the focused cue too", async () => {
    const w = await timeline();
    await w.get('[data-testid="cue-fx1"]').trigger("keydown", { key: "Enter" });
    expect(useEditorWorkspaceStore().selected).toEqual({ type: "effect", id: "fx1" });
  });
});

describe("moving and trimming a cue: ONE updateEffect on release", () => {
  it("a body drag previews, then sends one updateEffect with the shifted source range", async () => {
    const w = await timeline();
    const chip = w.get('[data-testid="cue-fx1"]');
    await chip.trigger("pointerdown", { button: 0, clientX: 100, pointerId: 1 });
    await chip.trigger("pointermove", { clientX: 150, pointerId: 1 });
    expect(chip.attributes("style")).toContain("left: 75px");
    expect(executed).toEqual([]);
    await chip.trigger("pointerup", { clientX: 150, pointerId: 1 });
    await flushPromises();
    expect(executed).toEqual([{ kind: "updateEffect", effectId: "fx1", startMs: 1_500, endMs: 7_500 }]);
  });

  it("Escape mid-drag discards the preview and sends nothing", async () => {
    const w = await timeline();
    const chip = w.get('[data-testid="cue-fx1"]');
    await chip.trigger("pointerdown", { button: 0, clientX: 100, pointerId: 1 });
    await chip.trigger("pointermove", { clientX: 150, pointerId: 1 });
    await chip.trigger("keydown", { key: "Escape" });
    expect(chip.attributes("style")).toContain("left: 25px");
    await chip.trigger("pointerup", { clientX: 150, pointerId: 1 });
    await flushPromises();
    expect(executed).toEqual([]);
  });

  it("the end grip trims the end only", async () => {
    const w = await timeline();
    const grip = w.get('[data-testid="cue-fx1-grip-end"]');
    await grip.trigger("pointerdown", { button: 0, clientX: 320, pointerId: 1 });
    await grip.trigger("pointermove", { clientX: 370, pointerId: 1 });
    await grip.trigger("pointerup", { clientX: 370, pointerId: 1 });
    await flushPromises();
    expect(executed).toEqual([{ kind: "updateEffect", effectId: "fx1", startMs: 500, endMs: 7_500 }]);
  });

  it("the start grip trims the start only", async () => {
    const w = await timeline();
    const grip = w.get('[data-testid="cue-fx2-grip-start"]');
    await grip.trigger("pointerdown", { button: 0, clientX: 140, pointerId: 1 });
    await grip.trigger("pointermove", { clientX: 115, pointerId: 1 });
    await grip.trigger("pointerup", { clientX: 115, pointerId: 1 });
    await flushPromises();
    expect(executed).toEqual([{ kind: "updateEffect", effectId: "fx2", startMs: 2_200, endMs: 8_000 }]);
  });

  it("→ nudges the focused cue one frame, Shift+→ one second, each ONE updateEffect", async () => {
    const w = await timeline();
    const chip = w.get('[data-testid="cue-fx1"]');
    await chip.trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    await chip.trigger("keydown", { key: "ArrowLeft", shiftKey: true });
    await flushPromises();
    expect(executed).toEqual([
      { kind: "updateEffect", effectId: "fx1", startMs: 533, endMs: 6_533 },
      { kind: "updateEffect", effectId: "fx1", startMs: 0, endMs: 6_000 },
    ]);
  });

  it("a cue already at its clip's end says so instead of doing nothing", async () => {
    const w = await timeline(sample({ effects: [fx("fx1", "c1", 4_000, 10_000)] }));
    await w.get('[data-testid="cue-fx1"]').trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    expect(executed).toEqual([]);
    expect(useNotificationsStore().items.map((n) => n.message)).toContain("The cue is at the end of its clip.");
  });

  it("Delete removes the focused cue — never its clip — and stops there", async () => {
    const w = await timeline();
    let reachedWindow = false;
    const onWindow = () => (reachedWindow = true);
    window.addEventListener("keydown", onWindow);
    await w.get('[data-testid="cue-fx2"]').trigger("keydown", { key: "Delete" });
    window.removeEventListener("keydown", onWindow);
    await flushPromises();
    expect(executed).toEqual([{ kind: "removeEffect", effectId: "fx2" }]);
    expect(reachedWindow).toBe(false);
  });
});

describe("a cue on a locked track (D14: refused visibly)", () => {
  it("says why in its title, and a drag sends nothing", async () => {
    const w = await timeline();
    const chip = w.get('[data-testid="cue-fxL"]');
    expect(chip.attributes("title")).toBe("Track Locked overlay is locked");
    await chip.trigger("pointerdown", { button: 0, clientX: 820, pointerId: 1 });
    await chip.trigger("pointermove", { clientX: 900, pointerId: 1 });
    await chip.trigger("pointerup", { clientX: 900, pointerId: 1 });
    await flushPromises();
    expect(chip.attributes("style")).toContain("left: 800px");
    expect(executed).toEqual([]);
  });

  it("a key that would move or delete it toasts the reason instead", async () => {
    const w = await timeline();
    const chip = w.get('[data-testid="cue-fxL"]');
    await chip.trigger("keydown", { key: "ArrowRight" });
    await chip.trigger("keydown", { key: "Delete" });
    await flushPromises();
    expect(executed).toEqual([]);
    expect(useNotificationsStore().items.map((n) => n.message)).toContain("Track Locked overlay is locked");
  });
});

describe("the cue menu (Task 5's cueMenu)", () => {
  it("Shift+F10 on a focused cue opens it under the cue's kind; Escape gives focus back", async () => {
    const w = await timeline();
    const chip = w.get('[data-testid="cue-fx1"]');
    (chip.element as HTMLElement).focus();
    await chip.trigger("keydown", { key: "F10", shiftKey: true });
    await flushPromises();
    expect(heading()).toContain("Text");
    const items = ["cue-edit", "cue-duplicate", "cue-select-clip", "cue-delete"].filter(
      (id) => document.querySelector(`[data-testid="editor-context-menu-item-${id}"]`) !== null,
    );
    expect(items).toHaveLength(4);
    (document.activeElement ?? document.body).dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    await flushPromises();
    expect(document.querySelector('[data-testid="editor-context-menu"]')).toBeNull();
    expect(document.activeElement?.getAttribute("data-testid")).toBe("cue-fx1");
  });

  it("a right-click opens it too, and its Delete annotation removes the cue", async () => {
    const w = await timeline();
    await w.get('[data-testid="cue-fx2"]').trigger("contextmenu", { clientX: 200, clientY: 40 });
    await flushPromises();
    expect(heading()).toContain("Highlight");
    (document.querySelector('[data-testid="editor-context-menu-item-cue-delete"]') as HTMLElement).click();
    await flushPromises();
    expect(executed).toEqual([{ kind: "removeEffect", effectId: "fx2" }]);
  });

  it("the toolbar's Edit actions opens the SELECTED cue's menu", async () => {
    const w = await timeline();
    await w.get('[data-testid="cue-fx3"]').trigger("click");
    await w.get('[data-testid="timeline-toolbar-more"]').trigger("click");
    await flushPromises();
    expect(heading()).toContain("Zoom");
  });
});

describe("the Captions row (§6.4)", () => {
  it("is 35px, labelled gold with its count, its cues 24px gold at their output time", async () => {
    const w = await timeline();
    const row = w.get('[data-testid="captions-row"]');
    expect(row.attributes("style")).toContain("height: 35px");
    const label = w.get('[data-testid="captions-row-label"]');
    expect(label.classes()).toEqual(expect.arrayContaining(["sticky", "left-0", "z-[15]", "text-gold"]));
    expect(label.text().replace(/\s+/g, " ")).toBe("Captions 2");
    const cue = w.get('[data-testid="caption-cue-cap1"]');
    expect(cue.classes()).toEqual(expect.arrayContaining(["h-[24px]", "bg-gold-bg", "text-gold"]));
    expect(cue.attributes("style")).toMatch(/left: 50px;.*width: 100px/);
    expect(cue.text()).toBe("Hi there");
  });

  it("a click opens the Captions tab on that cue", async () => {
    const w = await timeline();
    const before = revealSerial("library");
    await w.get('[data-testid="caption-cue-cap2"]').trigger("click");
    const workspace = useEditorWorkspaceStore();
    expect(workspace.libraryTab).toBe("captions");
    expect(workspace.selected).toEqual({ type: "caption", id: "cap2" });
    expect(workspace.playheadMs).toBe(5_000);
    expect(revealSerial("library")).toBe(before + 1);
  });

  it("with the Captions tab already open, the chosen caption scrolls into its list", async () => {
    const many = Array.from({ length: 30 }, (_, i) => ({
      id: `k${i}`, clip_id: "c1", start_ms: i * 300, end_ms: i * 300 + 200, text: `Line ${i}`,
    }));
    const w = await timeline(sample({ captions: { ...sample().captions!, cues: many } }));
    const library = mount(CaptionsLibrary, { attachTo: document.body });
    await flushPromises();
    const root = library.get('[data-testid="captions-library"]').element as HTMLElement;
    expect(root.scrollTop).toBe(0);
    await w.get('[data-testid="caption-cue-k25"]').trigger("click");
    await flushPromises();
    expect(root.scrollTop).toBeGreaterThan(0);
  });

  it("is one tab stop: → and ← move between its cues", async () => {
    const w = await timeline();
    const first = w.get('[data-testid="caption-cue-cap1"]');
    const second = w.get('[data-testid="caption-cue-cap2"]');
    expect([first.attributes("tabindex"), second.attributes("tabindex")]).toEqual(["0", "-1"]);
    (first.element as HTMLElement).focus();
    await first.trigger("keydown", { key: "ArrowRight" });
    await flushPromises();
    expect(document.activeElement?.getAttribute("data-testid")).toBe("caption-cue-cap2");
    expect([first.attributes("tabindex"), second.attributes("tabindex")]).toEqual(["-1", "0"]);
  });

  it("is not there when the edit has no captions", async () => {
    const w = await timeline(sample({ captions: null }));
    expect(w.find('[data-testid="captions-row"]').exists()).toBe(false);
  });
});
