/**
 * The library's Project section (visual-parity Task 10; design D9; concept
 * spec §3.6): not a tab but a section the Project menu's "Workspace &
 * rendered products", the status bar and the Render dialog's completion
 * open — "YOUR WORKSPACE" with the revision, the project summary, the
 * rendered products (Watch, Restore) or an empty state, "Render a new
 * video", and "Back to media".
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import LibraryPanel from "../src/components/editor/library/LibraryPanel.vue";
import ProjectSection from "../src/components/editor/library/ProjectSection.vue";
import { requestReveal, revealSerial } from "../src/editor/revealBus";
import { revealWorkspaceProducts } from "../src/editor/revealProducts";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";
import { openWithRenders, product } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
});

async function section(products = [product()]) {
  await openWithRenders({ getProducts: vi.fn(() => Promise.resolve(products)) });
  const w = mount(ProjectSection, { attachTo: document.body });
  await flushPromises();
  return w;
}

describe("ProjectSection", () => {
  it("reads YOUR WORKSPACE with the project's revision in an accent pill", async () => {
    const w = await section();
    expect(w.get('[data-testid="project-section-heading"]').text()).toBe("YOUR WORKSPACE");
    const pill = w.get('[data-testid="project-section-pill"]');
    expect(pill.text()).toBe("r7");
    expect(pill.classes()).toContain("bg-accent-bg");
    expect(w.get('[data-testid="project-summary"]').text()).toContain("Walkthrough");
  });

  it("lists each rendered product with Watch and Restore, and says whether it matches this edit", async () => {
    const w = await section([product(), product({ id: "prod-now", name: "Current cut", revision: 7 })]);
    const earlier = w.get('[data-testid="product-card-prod-a"]');
    expect(earlier.get('[data-testid="product-match-prod-a"]').text()).toBe("Earlier edit");
    expect(earlier.find('[data-testid="product-watch-prod-a"]').exists()).toBe(true);
    expect(earlier.get('[data-testid="product-restore-prod-a"]').attributes("aria-label")).toBe(
      "Restore the edit Walkthrough v1 was rendered from",
    );
    expect(w.get('[data-testid="product-match-prod-now"]').text()).toBe("Matches this edit");
  });

  it("with no products shows No renders yet", async () => {
    const w = await section([]);
    const empty = w.get('[data-testid="product-library-empty"]');
    expect(empty.text()).toContain("No renders yet");
    expect(empty.text()).toContain("Your project stays editable either way.");
  });

  it("Render a new video opens the Render dialog", async () => {
    const w = await section();
    const before = revealSerial("render");
    await w.get('[data-testid="project-section-render"]').trigger("click");
    expect(revealSerial("render")).toBe(before + 1);
  });

  it("Render a new video is disabled, and says why, with nothing on the timeline", async () => {
    await openWithRenders({ getProducts: () => Promise.resolve([]) });
    useEditorProjectStore().snapshot!.durationMs = 0;
    const w = mount(ProjectSection);
    await flushPromises();
    const render = w.get('[data-testid="project-section-render"]');
    expect(render.attributes("disabled")).toBeDefined();
    expect(render.attributes("title")).toBe("Place a clip on the timeline to render a video.");
  });

  it("Back to media returns the library to its Media tab", async () => {
    const w = await section();
    useEditorWorkspaceStore().setLibraryTab("project");
    await w.get('[data-testid="library-project-back"]').trigger("click");
    expect(useEditorWorkspaceStore().libraryTab).toBe("media");
  });
});

describe("LibraryPanel and the Project section", () => {
  beforeEach(() => {
    useEditorProjectStore().setPort(fakeEditorPort({ getProducts: () => Promise.resolve([]) }));
  });

  it("the tabs are exactly Media / Titles / Captions / Chapters — Products left the tabs (D9)", () => {
    const w = mount(LibraryPanel);
    expect(w.findAll('[role="tab"]').map((t) => t.text())).toEqual(["Media", "Titles", "Captions", "Chapters"]);
  });

  it("a projectSection reveal opens the section, with no tab selected and Media still reachable", async () => {
    const w = mount(LibraryPanel);
    requestReveal("projectSection");
    await flushPromises();
    expect(w.find('[data-testid="library-project-section"]').exists()).toBe(true);
    expect(w.find('[data-testid="media-library"]').exists()).toBe(false);
    const tabs = w.findAll('[role="tab"]');
    expect(tabs.every((t) => t.attributes("aria-selected") === "false")).toBe(true);
    expect(w.get('[data-testid="library-tab-media"]').attributes("tabindex")).toBe("0");

    await w.get('[data-testid="library-project-back"]').trigger("click");
    expect(w.find('[data-testid="media-library"]').exists()).toBe(true);
  });

  it("revealWorkspaceProducts asks for the section and for the library column", () => {
    const section = revealSerial("projectSection");
    const library = revealSerial("library");
    revealWorkspaceProducts();
    expect(revealSerial("projectSection")).toBe(section + 1);
    expect(revealSerial("library")).toBe(library + 1);
  });
});
