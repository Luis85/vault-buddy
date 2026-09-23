/**
 * `ContextMenu.vue` and `PreviewToolbar.vue` (Task 17) — the two components
 * built on the `actions.ts` registry. Grouped in this one file per the
 * task's own "Files" list (no separate toolbar spec file is named).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ContextMenu from "../src/components/editor/menus/ContextMenu.vue";
import PreviewToolbar from "../src/components/editor/shell/PreviewToolbar.vue";
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

describe("ContextMenu — arrow navigation wraps and Escape returns focus", () => {
  it("wraps ArrowUp from the first item to the last and back, and Escape refocuses the invoker", async () => {
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    trigger.focus();
    expect(document.activeElement).toBe(trigger);

    const w = mount(ContextMenu, {
      attachTo: document.body,
      props: {
        open: false,
        items: ["split", "delete", "copy"],
        context: ctx(),
        x: 0,
        y: 0,
      },
    });

    await w.setProps({ open: true });
    await flushPromises();

    // Opening focuses the first item.
    expect(document.activeElement?.getAttribute("data-testid")).toBe("editor-context-menu-item-split");

    const menu = w.get('[data-testid="editor-context-menu"]');
    await menu.trigger("keydown", { key: "ArrowUp" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("editor-context-menu-item-copy");

    await menu.trigger("keydown", { key: "ArrowDown" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("editor-context-menu-item-split");

    await menu.trigger("keydown", { key: "End" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("editor-context-menu-item-copy");
    await menu.trigger("keydown", { key: "Home" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe("editor-context-menu-item-split");

    await menu.trigger("keydown", { key: "Escape" });
    await flushPromises();

    expect(w.emitted("close")).toHaveLength(1);
    expect(document.activeElement).toBe(trigger);

    trigger.remove();
  });

  it("Enter activates the focused item only when it is enabled", async () => {
    // "delete" is enabled (a real clip target); "split" at time 0 sits
    // exactly on the clip boundary and is refused — Enter on it must be a
    // no-op, not an activation of a disabled action.
    const w = mount(ContextMenu, {
      attachTo: document.body,
      props: {
        open: true,
        items: ["split", "delete"],
        context: ctx({ pointerTarget: { kind: "clip", id: "c1", timeMs: 0 } }),
        x: 0,
        y: 0,
      },
    });
    await flushPromises();

    const menu = w.get('[data-testid="editor-context-menu"]');
    expect(w.get('[data-testid="editor-context-menu-item-split"]').attributes("aria-disabled")).toBe("true");

    await menu.trigger("keydown", { key: "Enter" });
    expect(w.emitted("activate")).toBeUndefined();

    await menu.trigger("keydown", { key: "ArrowDown" });
    await menu.trigger("keydown", { key: "Enter" });
    expect(w.emitted("activate")).toEqual([["delete"]]);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("shows a disabled item's reason as its title", () => {
    const w = mount(ContextMenu, {
      props: {
        open: true,
        items: ["split"],
        context: ctx({ pointerTarget: { kind: "clip", id: "c1", timeMs: 0 } }),
        x: 0,
        y: 0,
      },
    });
    expect(w.get('[data-testid="editor-context-menu-item-split"]').attributes("title")).toBe(
      "The playhead is at a clip boundary",
    );
  });

  it("clicking an item activates it directly, and an unrelated key is a no-op", async () => {
    const w = mount(ContextMenu, {
      props: { open: true, items: ["delete", "copy"], context: ctx(), x: 0, y: 0 },
    });
    await w.get('[data-testid="editor-context-menu-item-copy"]').trigger("click");
    expect(w.emitted("activate")).toEqual([["copy"]]);

    await w.get('[data-testid="editor-context-menu"]').trigger("keydown", { key: "a" });
    expect(w.emitted("activate")).toEqual([["copy"]]); // unchanged
  });

  it("Space also activates the focused item", async () => {
    const w = mount(ContextMenu, {
      attachTo: document.body,
      props: { open: true, items: ["delete"], context: ctx(), x: 0, y: 0 },
    });
    await flushPromises();
    await w.get('[data-testid="editor-context-menu"]').trigger("keydown", { key: " " });
    expect(w.emitted("activate")).toEqual([["delete"]]);
  });

  it("a click outside the menu closes it WITHOUT returning focus to the invoker", async () => {
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    trigger.focus();

    const outside = document.createElement("div");
    document.body.appendChild(outside);

    const w = mount(ContextMenu, {
      attachTo: document.body,
      props: { open: false, items: ["delete"], context: ctx(), x: 0, y: 0 },
    });
    // Open via a prop transition (not the initial value) so the open-watcher
    // actually fires and moves focus into the menu first -- otherwise
    // `document.activeElement` never leaves `trigger` regardless of what the
    // outside click does, and the assertion below would prove nothing.
    await w.setProps({ open: true });
    await flushPromises();
    expect(document.activeElement).not.toBe(trigger);

    outside.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await flushPromises();

    expect(w.emitted("close")).toHaveLength(1);
    // The invoker is NOT refocused on an outside click (the user aimed
    // elsewhere on purpose) -- distinct from Escape's focus-return.
    expect(document.activeElement).not.toBe(trigger);

    trigger.remove();
    outside.remove();
  });

  it("labels the menu by the pointer target's kind, and falls back to a generic label without one", () => {
    const withTarget = mount(ContextMenu, {
      props: { open: true, items: ["delete"], context: ctx({ pointerTarget: { kind: "track", id: "v1", timeMs: null } }), x: 0, y: 0 },
    });
    expect(withTarget.get('[data-testid="editor-context-menu"]').attributes("aria-label")).toBe("Actions for track");

    const withoutTarget = mount(ContextMenu, {
      props: { open: true, items: ["delete"], context: ctx({ pointerTarget: null }), x: 0, y: 0 },
    });
    expect(withoutTarget.get('[data-testid="editor-context-menu"]').attributes("aria-label")).toBe("Actions");
  });

  it("ignores keys and stays closed while `open` is false", () => {
    const w = mount(ContextMenu, {
      props: { open: false, items: ["delete"], context: ctx(), x: 0, y: 0 },
    });
    expect(w.find('[data-testid="editor-context-menu"]').exists()).toBe(false);
  });
});

describe("PreviewToolbar — overflow moves into More, never a second toolbar", () => {
  it("renders exactly one role=toolbar element with the full row and no More button when nothing overflows", () => {
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    expect(w.findAll('[role="toolbar"]')).toHaveLength(1);
    expect(w.get('[data-testid="preview-toolbar"]').attributes("role")).toBe("toolbar");
    expect(w.find('[data-testid="preview-toolbar-more"]').exists()).toBe(false);
    expect(w.find('[data-testid="preview-toolbar-toggleInspector"]').exists()).toBe(true);
  });

  it("moves the trailing items into a labelled More menu, keeping exactly one toolbar row", async () => {
    const w = mount(PreviewToolbar, { props: { overflowCount: 3 } });

    expect(w.findAll('[role="toolbar"]')).toHaveLength(1);
    // The three trailing items (focusPreview, toggleLibrary, toggleInspector)
    // are overflowed -- absent from the row until More is opened.
    expect(w.find('[data-testid="preview-toolbar-toggleLibrary"]').exists()).toBe(false);
    expect(w.get('[data-testid="preview-toolbar-more"]').attributes("aria-haspopup")).toBe("menu");

    await w.get('[data-testid="preview-toolbar-more"]').trigger("click");

    expect(w.findAll('[role="toolbar"]')).toHaveLength(1); // still exactly one, even with More open
    const moreMenu = w.get('[data-testid="preview-toolbar-more-menu"]');
    expect(moreMenu.attributes("role")).toBe("menu");
    expect(w.find('[data-testid="preview-toolbar-toggleLibrary"]').exists()).toBe(true);
    expect(w.find('[data-testid="preview-toolbar-toggleInspector"]').exists()).toBe(true);

    // An overflowed item still activates through the SAME handler as a
    // visible one -- clicking it inside the open More menu.
    await w.get('[data-testid="preview-toolbar-toggleLibrary"]').trigger("click");
    expect(w.emitted("toggle-library")).toHaveLength(1);
    // Fix round 1, finding 4a: toggleLibrary/toggleInspector/focusPreview
    // are the FIRST items to overflow, and clicking one used to leave the
    // More menu open (their early `return`s skipped `closeMore()`).
    expect(w.find('[data-testid="preview-toolbar-more-menu"]').exists()).toBe(false);
  });

  it("emits toggle-library/toggle-inspector/focus-preview instead of sending a command", async () => {
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    await w.get('[data-testid="preview-toolbar-toggleLibrary"]').trigger("click");
    await w.get('[data-testid="preview-toolbar-toggleInspector"]').trigger("click");
    await w.get('[data-testid="preview-toolbar-focusPreview"]').trigger("click");
    expect(w.emitted("toggle-library")).toHaveLength(1);
    expect(w.emitted("toggle-inspector")).toHaveLength(1);
    expect(w.emitted("focus-preview")).toHaveLength(1);
  });

  it("a disabled tool (e.g. addText with no project) renders disabled with its reason as the title", () => {
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    const addText = w.get('[data-testid="preview-toolbar-addText"]');
    expect(addText.attributes("aria-disabled")).toBe("true");
    // Fix round 1, finding 1: human copy, never the raw Rust wire kind.
    // Task 35: the teaching tools are real now, so the reason is their own
    // resolver's ("arrives in a later update" was Task 34's placeholder).
    expect(addText.attributes("title")).toBe("No project is open.");
    expect(w.get('[data-testid="preview-toolbar-render"]').attributes("title")).toBe(
      "Rendering a video arrives in a later update.",
    );
  });

  it("reflects libraryOpen/inspectorOpen as aria-pressed on their own toggle buttons only", () => {
    const w = mount(PreviewToolbar, { props: { overflowCount: 0, libraryOpen: true, inspectorOpen: false } });
    expect(w.get('[data-testid="preview-toolbar-toggleLibrary"]').attributes("aria-pressed")).toBe("true");
    expect(w.get('[data-testid="preview-toolbar-toggleInspector"]').attributes("aria-pressed")).toBe("false");
    // Every other item carries no aria-pressed at all -- it isn't a toggle.
    expect(w.get('[data-testid="preview-toolbar-render"]').attributes("aria-pressed")).toBeUndefined();
  });

  it("roving tabindex: ArrowRight/Left wrap across the row, including the More button", async () => {
    const w = mount(PreviewToolbar, { attachTo: document.body, props: { overflowCount: 1 } });
    const row = w.get('[data-testid="preview-toolbar"]');
    const testids = () =>
      w.findAll("button").map((b) => b.attributes("data-testid"));
    const firstVisible = testids()[0];
    const moreTestid = "preview-toolbar-more";

    await row.trigger("keydown", { key: "ArrowLeft" }); // wraps from index 0 to the last (More)
    expect(document.activeElement?.getAttribute("data-testid")).toBe(moreTestid);

    await row.trigger("keydown", { key: "ArrowRight" }); // wraps back to the first visible item
    expect(document.activeElement?.getAttribute("data-testid")).toBe(firstVisible);

    await row.trigger("keydown", { key: "End" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe(moreTestid);
    await row.trigger("keydown", { key: "Home" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe(firstVisible);

    // An unrelated key is a no-op.
    await row.trigger("keydown", { key: "a" });
    expect(document.activeElement?.getAttribute("data-testid")).toBe(firstVisible);
  });

  it("clamps the active index when a narrowing resize shrinks the focusable count (fix round 1, finding 3)", async () => {
    // At overflowCount 0 there are 12 focusable buttons (no More); End
    // moves activeIndex to 11. Shrinking to overflowCount 5 leaves only 7
    // visible + 1 More = 8 focusable slots -- an unclamped activeIndex of
    // 11 would match NEITHER the visible row's own indices (0..6) NOR the
    // More button's check (`activeIndex === visibleItems.length`, 7),
    // leaving zero controls with tabindex="0" and nothing in the Tab order.
    const w = mount(PreviewToolbar, { attachTo: document.body, props: { overflowCount: 0 } });
    const row = w.get('[data-testid="preview-toolbar"]');
    await row.trigger("keydown", { key: "End" });
    expect(w.findAll('button[tabindex="0"]')).toHaveLength(1);

    await w.setProps({ overflowCount: 5 });
    await flushPromises();

    expect(w.findAll('button[tabindex="0"]')).toHaveLength(1);
  });

  it("a click outside closes the open More menu", async () => {
    const outside = document.createElement("div");
    document.body.appendChild(outside);
    const w = mount(PreviewToolbar, { attachTo: document.body, props: { overflowCount: 2 } });

    await w.get('[data-testid="preview-toolbar-more"]').trigger("click");
    expect(w.find('[data-testid="preview-toolbar-more-menu"]').exists()).toBe(true);

    outside.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await flushPromises();
    expect(w.find('[data-testid="preview-toolbar-more-menu"]').exists()).toBe(false);

    outside.remove();
  });

  it("wires a ResizeObserver in production and recomputes overflow from the measured width", async () => {
    // happy-dom implements no `ResizeObserver`, so this test installs a
    // minimal fake to exercise the `onMounted` branch that only runs when
    // one exists — production's actual overflow math, as opposed to the
    // test-only `overflowCount` prop every other test drives directly.
    // A mutable holder OBJECT, not a bare `let` -- TS narrows a `let`
    // reassigned only inside a nested closure (the constructor below) down
    // to its initial `null` at every later read in this outer scope, which
    // turns the optional call below into a build error ("Type 'never' has
    // no call signatures"). A property on an object is never narrowed that
    // way, which is exactly why this indirection is the fix, not a style
    // preference.
    type Callback = (entries: { contentRect: { width: number } }[]) => void;
    const captured: { cb: Callback | null } = { cb: null };
    class FakeResizeObserver {
      constructor(cb: Callback) {
        captured.cb = cb;
      }
      observe() {
        /* no-op */
      }
      disconnect() {
        /* no-op */
      }
    }
    const original = (globalThis as { ResizeObserver?: unknown }).ResizeObserver;
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = FakeResizeObserver;

    try {
      const w = mount(PreviewToolbar, { attachTo: document.body });
      expect(captured.cb).not.toBeNull();
      // A narrow width forces every item into overflow (TOOLBAR_ITEMS has
      // 12 entries; a handful of pixels fits none of them).
      captured.cb?.([{ contentRect: { width: 10 } }]);
      await flushPromises();
      expect(w.find('[data-testid="preview-toolbar-more"]').exists()).toBe(true);
      w.unmount();
    } finally {
      (globalThis as { ResizeObserver?: unknown }).ResizeObserver = original;
    }
  });

  it("does not reserve a More slot when the measured width fits every item (fix round 1, finding 4b)", async () => {
    // Fix round 1, finding 4b: the overflow math used to subtract one item
    // width for "More" UNCONDITIONALLY, so even a row wide enough to fit
    // every item still reserved a slot and showed an empty More menu. The
    // width here is deliberately EXACTLY 12 * ITEM_WIDTH (84): a naive
    // "always subtract one" formula computes maxFit = 12 - 1 = 11 and
    // wrongly overflows the 12th item, while the correct one recognizes
    // all 12 already fit and reserves nothing. A generously wide fixture
    // (e.g. 1100px) would pass under BOTH the old and the new formula and
    // prove nothing -- this exact width is what actually distinguishes them.
    type Callback = (entries: { contentRect: { width: number } }[]) => void;
    const captured: { cb: Callback | null } = { cb: null };
    class FakeResizeObserver {
      constructor(cb: Callback) {
        captured.cb = cb;
      }
      observe() {
        /* no-op */
      }
      disconnect() {
        /* no-op */
      }
    }
    const original = (globalThis as { ResizeObserver?: unknown }).ResizeObserver;
    (globalThis as { ResizeObserver?: unknown }).ResizeObserver = FakeResizeObserver;

    try {
      const w = mount(PreviewToolbar, { attachTo: document.body });
      captured.cb?.([{ contentRect: { width: 1_008 } }]);
      await flushPromises();
      expect(w.find('[data-testid="preview-toolbar-more"]').exists()).toBe(false);
      expect(w.find('[data-testid="preview-toolbar-toggleInspector"]').exists()).toBe(true);
      w.unmount();
    } finally {
      (globalThis as { ResizeObserver?: unknown }).ResizeObserver = original;
    }
  });
});
