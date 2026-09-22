/**
 * `InspectorPanel.vue` (the six-category shell) and `useInspectorDraft`
 * (the shared draft-buffer composable both this panel's own tests and every
 * later per-category section will build on) — Task 19.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import InspectorPanel from "../src/components/editor/inspector/InspectorPanel.vue";
import { numberField, useInspectorDraft } from "../src/composables/useInspectorDraft";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

describe("InspectorPanel", () => {
  it("no selection shows guidance, not disabled controls", () => {
    const w = mount(InspectorPanel);

    expect(w.get('[data-testid="inspector-empty"]').text()).toBe("Select a clip to adjust it…");
    expect(w.find('[data-testid="inspector-scope"]').exists()).toBe(false);
    // SCREENS-AND-INTERACTIONS.md §02: "Unselected states teach what to do
    // next rather than filling the inspector with disabled controls" -- no
    // element anywhere in the panel is disabled while nothing is selected.
    expect(w.findAll("[disabled]")).toHaveLength(0);
    // The category chrome itself is unaffected by selection -- it is a view
    // preference (`propertyTab`), not a selected-object control.
    expect(w.get('[data-testid="inspector-tab-clip"]').attributes("aria-disabled")).toBeUndefined();
  });

  it("multi-selection states its scope", () => {
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1", "c2", "c3"]);

    const w = mount(InspectorPanel);

    expect(w.get('[data-testid="inspector-scope"]').text()).toBe("3 clips selected");
    expect(w.find('[data-testid="inspector-empty"]').exists()).toBe(false);
  });

  it("a single selection scopes the active slot without a banner", () => {
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);

    const w = mount(InspectorPanel, {
      slots: { clip: `<template #clip="{ clipIds }"><p data-testid="clip-scope">{{ clipIds.join(",") }}</p></template>` },
    });

    expect(w.find('[data-testid="inspector-scope"]').exists()).toBe(false);
    expect(w.find('[data-testid="inspector-empty"]').exists()).toBe(false);
    expect(w.get('[data-testid="clip-scope"]').text()).toBe("c1");
  });

  it("renders all six categories, defaults to Clip, and switches on click", async () => {
    const w = mount(InspectorPanel);
    const ids = ["clip", "layout", "fades", "audio", "speed", "color"];
    for (const id of ids) {
      expect(w.find(`[data-testid="inspector-tab-${id}"]`).exists()).toBe(true);
    }
    expect(w.get('[data-testid="inspector-tab-clip"]').attributes("aria-selected")).toBe("true");
    expect(w.get('[data-testid="inspector-tablist"]').attributes("role")).toBe("tablist");

    await w.get('[data-testid="inspector-tab-speed"]').trigger("click");
    expect(w.get('[data-testid="inspector-tab-speed"]').attributes("aria-selected")).toBe("true");
    expect(w.get('[data-testid="inspector-tab-clip"]').attributes("aria-selected")).toBe("false");

    const workspace = useEditorWorkspaceStore();
    expect(workspace.propertyTab).toBe("speed");
  });

  it("falls back to Clip when the persisted tab names an unknown category", () => {
    const workspace = useEditorWorkspaceStore();
    workspace.setPropertyTab("caption-settings"); // not one of the six inspector categories
    const w = mount(InspectorPanel);
    expect(w.get('[data-testid="inspector-tab-clip"]').attributes("aria-selected")).toBe("true");
  });

  it("arrow keys and Home/End move the active tab (roving tabindex)", async () => {
    const w = mount(InspectorPanel, { attachTo: document.body });
    const tablist = w.get('[data-testid="inspector-tablist"]');

    await tablist.trigger("keydown", { key: "ArrowRight" });
    expect(w.get('[data-testid="inspector-tab-layout"]').attributes("aria-selected")).toBe("true");

    await tablist.trigger("keydown", { key: "ArrowLeft" });
    expect(w.get('[data-testid="inspector-tab-clip"]').attributes("aria-selected")).toBe("true");

    // Wraps backward from the first tab to the last.
    await tablist.trigger("keydown", { key: "ArrowLeft" });
    expect(w.get('[data-testid="inspector-tab-color"]').attributes("aria-selected")).toBe("true");

    await tablist.trigger("keydown", { key: "Home" });
    expect(w.get('[data-testid="inspector-tab-clip"]').attributes("aria-selected")).toBe("true");
    await tablist.trigger("keydown", { key: "End" });
    expect(w.get('[data-testid="inspector-tab-color"]').attributes("aria-selected")).toBe("true");

    // An unrelated key is a no-op.
    await tablist.trigger("keydown", { key: "a" });
    expect(w.get('[data-testid="inspector-tab-color"]').attributes("aria-selected")).toBe("true");
    await flushPromises();
  });
});

describe("useInspectorDraft", () => {
  // A speed field mirroring `core::editor::mod::limits::{SPEED_MIN,
  // SPEED_MAX}` (0.25..=4.0) -- the Rust bound this composable's own field
  // definitions are meant to reproduce, read from the source rather than
  // guessed.
  function speedField(current: () => number) {
    return numberField({
      value: current,
      label: "Speed",
      min: 0.25,
      max: 4,
      rangeLabel: "0.25× and 4×",
    });
  }

  it("draft commits once on Enter and reverts on Escape", () => {
    let committed = 1;
    const commits: number[] = [];
    const d = useInspectorDraft(speedField(() => committed), (v) => {
      commits.push(v);
      committed = v;
    });
    expect(d.draft.value).toBe("1");

    // Enter: one command, exactly once.
    d.draft.value = "2";
    d.submit();
    expect(commits).toEqual([2]);
    expect(d.draft.value).toBe("2");
    expect(d.error.value).toBeNull();

    // A blur immediately following an already-handled Enter (same,
    // unchanged draft) must not resend the identical edit as a second
    // command -- this is the regression the "commits ONCE" wording guards.
    d.submit();
    expect(commits).toEqual([2]);

    // Escape: reverts the in-progress edit, discards it, no command sent.
    d.draft.value = "3";
    d.revert();
    expect(d.draft.value).toBe("2"); // back to the last committed value
    expect(commits).toEqual([2]); // nothing further was ever sent
    expect(d.error.value).toBeNull();
  });

  it('invalid draft stays visible with its correction (speed "9")', () => {
    const commits: number[] = [];
    const d = useInspectorDraft(speedField(() => 1), (v) => commits.push(v));

    d.draft.value = "9";
    d.submit();

    expect(d.error.value).toBe("Speed must be between 0.25× and 4×");
    expect(d.draft.value).toBe("9"); // the invalid text stays exactly as typed
    expect(commits).toEqual([]); // no command sent
  });

  it("a non-numeric draft is refused before the range check", () => {
    const commits: number[] = [];
    const d = useInspectorDraft(speedField(() => 1), (v) => commits.push(v));

    d.draft.value = "fast";
    d.submit();

    expect(d.error.value).toBe("Speed must be a number.");
    expect(commits).toEqual([]);
  });
});
