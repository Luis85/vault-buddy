/**
 * `LibraryPanel.vue` (Task 33 fix round 1; review finding, Important #1):
 * the Media/Titles tablist that makes `TitlesLibrary.vue` reachable from
 * the running editor. `tests/editorRoot.test.ts`'s own "reaches Titles
 * from the editor root's library panel..." test covers the real
 * `EditorRoot` mount end to end; this file covers the tablist's own
 * behavior in isolation — default tab, switching, and full keyboard
 * operability (the `InspectorPanel.vue` roving-tabindex precedent, now
 * shared via `useRovingTablist`).
 */
import { enableAutoUnmount, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import LibraryPanel from "../src/components/editor/library/LibraryPanel.vue";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  // Both MediaLibrary and TitlesLibrary read `project.project`/`missing` —
  // a project store with no open session (the default) renders each in
  // its own empty state without touching the port at all, which is all
  // this suite's own tests need.
  useEditorProjectStore().setPort(fakeEditorPort());
});

describe("LibraryPanel", () => {
  it("opens on Media, and Titles is not mounted until selected", () => {
    const w = mount(LibraryPanel);
    expect(w.find('[data-testid="media-library"]').exists()).toBe(true);
    expect(w.find('[data-testid="titles-library"]').exists()).toBe(false);
    expect(w.get('[data-testid="library-tab-media"]').attributes("aria-selected")).toBe("true");
    expect(w.get('[data-testid="library-tab-titles"]').attributes("aria-selected")).toBe("false");
  });

  it("clicking Titles switches the panel and unmounts Media (don't pay for a hidden tab)", async () => {
    const w = mount(LibraryPanel);
    await w.get('[data-testid="library-tab-titles"]').trigger("click");
    expect(w.get('[data-testid="library-tab-titles"]').attributes("aria-selected")).toBe("true");
    expect(w.get('[data-testid="library-tab-media"]').attributes("aria-selected")).toBe("false");
    expect(w.find('[data-testid="titles-library"]').exists()).toBe(true);
    expect(w.find('[data-testid="media-library"]').exists()).toBe(false);
  });

  it("is fully keyboard-operable: ArrowRight/ArrowLeft/Home/End move focus and selection", async () => {
    const w = mount(LibraryPanel, { attachTo: document.body });
    const tablist = w.get('[data-testid="library-tablist"]');

    await tablist.trigger("keydown", { key: "ArrowRight" });
    expect(w.get('[data-testid="library-tab-titles"]').attributes("aria-selected")).toBe("true");
    expect(document.activeElement).toBe(w.get('[data-testid="library-tab-titles"]').element);

    await tablist.trigger("keydown", { key: "ArrowLeft" });
    expect(w.get('[data-testid="library-tab-media"]').attributes("aria-selected")).toBe("true");

    // Wraps: ArrowLeft from the first tab goes to the last (Task 36 added
    // Captions and Chapters after Titles; visual-parity Task 10 moved Task
    // 47's Products out of the tabs, into the Project section).
    await tablist.trigger("keydown", { key: "ArrowLeft" });
    expect(w.get('[data-testid="library-tab-chapters"]').attributes("aria-selected")).toBe("true");

    await tablist.trigger("keydown", { key: "Home" });
    expect(w.get('[data-testid="library-tab-media"]').attributes("aria-selected")).toBe("true");

    await tablist.trigger("keydown", { key: "End" });
    expect(w.get('[data-testid="library-tab-chapters"]').attributes("aria-selected")).toBe("true");

    w.unmount();
  });

  // visual-parity Task 9, concept spec §3.1: the concept's four tabs (Task
  // 10 moved Products into the library's Project section, D9), the active
  // one a filled pill, the whole row a fixed 48px strip.
  it("the tabs are exactly Media / Titles / Captions / Chapters, the active one a filled pill", () => {
    const w = mount(LibraryPanel);
    const labels = ["media", "titles", "captions", "chapters"].map((id) =>
      w.get(`[data-testid="library-tab-${id}"]`).text(),
    );
    expect(labels).toEqual(["Media", "Titles", "Captions", "Chapters"]);
    expect(w.find('[data-testid="library-tab-products"]').exists()).toBe(false);
    expect(w.get('[data-testid="library-tab-media"]').classes()).toContain("bg-accent-bg");
    expect(w.get('[data-testid="library-tab-media"]').classes()).toContain("text-accent-ink");
    expect(w.get('[data-testid="library-tab-titles"]').classes()).not.toContain("bg-accent-bg");
  });

  it("the tab row is a fixed 48px-tall strip (concept §3.1)", () => {
    const w = mount(LibraryPanel);
    expect(w.get('[data-testid="library-tablist"]').classes()).toContain("h-12");
  });
});
