/**
 * `ContextMenu.vue` (Task 17) — built on the `actions.ts` registry, and
 * rendering the concept's item sets through `MenuPanel` since visual-parity
 * Task 5. The preview toolbar's tests that used to live here moved to
 * `editorPreviewHeader.test.ts` with its replacement (visual-parity Task 11).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ContextMenu from "../src/components/editor/menus/ContextMenu.vue";
import type { ActionContext } from "../src/editor/actions";
import type { EditorSnapshot, Project } from "../src/editorTypes";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

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

function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [],
    tracks: [{ id: "v1", kind: "video", name: "Screen", visible: true, locked: false, muted: false, solo: false, volume: 1 }],
    clips: [
      { id: "c1", asset_id: "a1", track_id: "v1", name: "c1", start_ms: 0, in_ms: 0, out_ms: 1_000, fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false, x: 0, y: 0, w: 1, h: 1 },
    ],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

function ctx(overrides: Partial<ActionContext> = {}): ActionContext {
  return {
    project: project(),
    snapshot: snapshot(),
    playheadMs: 0,
    selectedClipIds: [],
    pointerTarget: { kind: "clip", id: "c1", timeMs: 200 },
    hasClipboard: false,
    clipboardFragment: null,
    ...overrides,
  };
}

const view = () => ({ fitRange: vi.fn(), fitTimeline: vi.fn() });

function mountMenu(props: Partial<{ open: boolean; context: ActionContext; x: number; y: number }> = {}) {
  return mount(ContextMenu, {
    attachTo: document.body,
    props: { open: true, context: ctx(), x: 0, y: 0, view: view(), ...props },
  });
}

// Visual-parity Task 5: the menu renders the concept's item sets through
// MenuPanel; the panel's own keyboard/submenu rules live in
// editorMenuPanel.test.ts, the item sets in editorMenuSets.test.ts.
describe("ContextMenu — which menu a target opens", () => {
  it("a clip target opens the clip menu under the clip's name, first item focused", async () => {
    const w = mountMenu();
    await flushPromises();
    expect(w.get('[data-testid="editor-context-menu-heading"]').text()).toBe("c1");
    expect(w.get('[data-testid="editor-context-menu"]').attributes("aria-label")).toBe("c1");
    expect(document.activeElement?.getAttribute("data-testid")).toBe("editor-context-menu-item-goTo");
  });

  it("a lane target opens the gap menu, and no target the editor actions", () => {
    const lane = mountMenu({ context: ctx({ pointerTarget: { kind: "gap", id: "v1", timeMs: 1_500 } }) });
    expect(lane.get('[data-testid="editor-context-menu-heading"]').text()).toBe("Timeline gap");
    expect(lane.find('[data-testid="editor-context-menu-item-lane-close-gap"]').exists()).toBe(true);
    const none = mountMenu({ context: ctx({ pointerTarget: null }) });
    expect(none.get('[data-testid="editor-context-menu-heading"]').text()).toBe("Editor actions");
    expect(none.find('[data-testid="editor-context-menu-item-lane-close-gap"]').exists()).toBe(false);
  });

  it("stays closed while `open` is false", () => {
    const w = mountMenu({ open: false });
    expect(w.find('[data-testid="editor-context-menu"]').exists()).toBe(false);
  });
});

describe("ContextMenu — reasons, focus and close", () => {
  it("a disabled item keeps its reason as its title and in the hint, and does not close the menu", async () => {
    const w = mountMenu({ context: ctx({ pointerTarget: { kind: "clip", id: "c1", timeMs: 0 } }) });
    await flushPromises();
    const split = w.get('[data-testid="editor-context-menu-item-split"]');
    expect(split.attributes("aria-disabled")).toBe("true");
    expect(split.attributes("title")).toBe("The playhead is at a clip boundary");
    await split.trigger("click");
    expect(w.get('[data-testid="editor-context-menu-hint"]').text()).toBe("The playhead is at a clip boundary");
    expect(w.emitted("close")).toBeUndefined();
  });

  it("Escape closes and returns focus to what had focus when it opened", async () => {
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    trigger.focus();
    const w = mountMenu({ open: false });
    await w.setProps({ open: true });
    await flushPromises();
    expect(document.activeElement).not.toBe(trigger);
    await w.get('[data-testid="editor-context-menu"]').trigger("keydown", { key: "Escape" });
    await flushPromises();
    expect(w.emitted("close")).toHaveLength(1);
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });

  it("a click outside closes it without returning focus", async () => {
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    trigger.focus();
    const w = mountMenu({ open: false });
    await w.setProps({ open: true });
    await flushPromises();
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await flushPromises();
    expect(w.emitted("close")).toHaveLength(1);
    expect(document.activeElement).not.toBe(trigger);
    trigger.remove();
  });

  it("the timeline's view changes reach the items (Fit this clip, Fit timeline)", async () => {
    const ops = view();
    const clipMenu = mount(ContextMenu, { attachTo: document.body, props: { open: true, context: ctx(), x: 0, y: 0, view: ops } });
    await clipMenu.get('[data-testid="editor-context-menu-item-fitClip"]').trigger("click");
    expect(ops.fitRange).toHaveBeenCalledWith(0, 1_000);
    expect(clipMenu.emitted("close")).toHaveLength(1);
    const lane = mount(ContextMenu, {
      attachTo: document.body,
      props: { open: true, context: ctx({ pointerTarget: null }), x: 0, y: 0, view: ops },
    });
    await lane.get('[data-testid="editor-context-menu-item-lane-fit"]').trigger("click");
    expect(ops.fitTimeline).toHaveBeenCalledTimes(1);
  });
});

