/**
 * The splitter between the workspace and the timeline (visual-parity Task
 * 4; concept spec §6.1): the frame's own 8 px grid row, a focusable
 * `role="separator"` that resizes the timeline by dragging or by ±25 px
 * per ArrowUp/ArrowDown, always within 170 … min(540, innerHeight − 370).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import TimelineSplitter from "../src/components/editor/timeline/TimelineSplitter.vue";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  useEditorWorkspaceStore().setViewport(1600, 1000);
});

describe("TimelineSplitter", () => {
  it("is a focusable separator that states the timeline height and its range", () => {
    const w = mount(TimelineSplitter);
    const el = w.get('[data-testid="editor-splitter"]');
    expect(el.attributes("role")).toBe("separator");
    expect(el.attributes("aria-orientation")).toBe("horizontal");
    expect(el.attributes("tabindex")).toBe("0");
    expect(el.attributes("aria-valuenow")).toBe("400");
    expect(el.attributes("aria-valuemin")).toBe("170");
    expect(el.attributes("aria-valuemax")).toBe("540");
  });

  it("ArrowUp grows the timeline by 25 and ArrowDown shrinks it by 25", async () => {
    const ws = useEditorWorkspaceStore();
    const w = mount(TimelineSplitter);
    const el = w.get('[data-testid="editor-splitter"]');

    await el.trigger("keydown", { key: "ArrowUp" });
    expect(ws.timelineHeight).toBe(425);
    await el.trigger("keydown", { key: "ArrowDown" });
    await el.trigger("keydown", { key: "ArrowDown" });
    expect(ws.timelineHeight).toBe(375);
  });

  it("the keyboard steps from the height on screen, and stays inside the range", async () => {
    const ws = useEditorWorkspaceStore();
    ws.setViewport(960, 640); // the stored 400 shows as 270 here
    const w = mount(TimelineSplitter);
    const el = w.get('[data-testid="editor-splitter"]');

    await el.trigger("keydown", { key: "ArrowDown" });
    expect(ws.timelineHeight).toBe(245);
    await el.trigger("keydown", { key: "ArrowUp" });
    await el.trigger("keydown", { key: "ArrowUp" });
    expect(ws.timelineHeight).toBe(270);
  });

  it("dragging up grows the timeline, clamped to the window", async () => {
    const ws = useEditorWorkspaceStore();
    const w = mount(TimelineSplitter, { attachTo: document.body });

    await w.get('[data-testid="editor-splitter"]').trigger("pointerdown", { clientY: 500, pointerId: 1 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientY: 440 }));
    expect(ws.timelineHeight).toBe(460);
    window.dispatchEvent(new PointerEvent("pointermove", { clientY: 0 }));
    expect(ws.timelineHeight).toBe(540);
    window.dispatchEvent(new PointerEvent("pointerup"));
    window.dispatchEvent(new PointerEvent("pointermove", { clientY: 490 }));
    await flushPromises();
    // Released: a later move changes nothing.
    expect(ws.timelineHeight).toBe(540);
  });
});
