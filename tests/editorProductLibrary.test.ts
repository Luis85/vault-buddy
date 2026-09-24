/**
 * The product library (Task 47; F-41, F-42; SCREENS 09): each Rendered
 * Product as a card (name, revision, range, created, available/missing);
 * **Watch** plays the ACTUAL encoded file through `editor_media_url(
 * {productId})` in a plain `<video>` -- never the editable preview -- and
 * **Restore this edit** asks first, then sends `editor_restore_product`.
 * A product whose file is gone keeps its lineage (its record still says
 * which edit made it, and that edit can still be restored).
 *
 * F18: a Review render is not a product -- forty of them in a row leave
 * the library exactly as empty as it was.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import LibraryPanel from "../src/components/editor/library/LibraryPanel.vue";
import ProductLibrary from "../src/components/editor/library/ProductLibrary.vue";
import type { EditorProjection } from "../src/editorTypes";
import { useEditorJobsStore } from "../src/stores/editorJobs";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { openResult, openWithRenders, product, progress, project, SESSION } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
});

async function library(products = [product()], extra = {}) {
  const getProducts = vi.fn(() => Promise.resolve(products));
  const env = await openWithRenders({ getProducts, ...extra });
  const w = mount(ProductLibrary);
  await flushPromises();
  return { ...env, w, getProducts };
}

describe("ProductLibrary", () => {
  it("shows each product's name, revision, range and created time", async () => {
    const ranged = product({ id: "prod-b", name: "Intro only", revision: 9, renderRange: { startMs: 1_500, endMs: 33_500 } });
    const { w } = await library([product(), ranged]);
    const whole = w.get('[data-testid="product-card-prod-a"]');
    expect(whole.text()).toContain("Walkthrough v1");
    expect(whole.text()).toContain("r5");
    expect(whole.text()).toMatch(/whole project/i);
    const card = w.get('[data-testid="product-card-prod-b"]');
    expect(card.text()).toContain("Intro only");
    expect(card.text()).toContain("r9");
    expect(card.text()).toContain("0:01.5");
    expect(card.text()).toContain("0:33.5");
    expect(card.get('[data-testid="product-created-prod-b"]').attributes("datetime")).toBe(
      "2026-09-24T10:00:00+02:00",
    );
  });

  it("watch uses the product url, not the preview", async () => {
    const mediaUrl = vi.fn(() => Promise.resolve("C:\\data\\products\\prod-a.mp4"));
    const { w } = await library([product()], { mediaUrl });
    await w.get('[data-testid="product-watch-prod-a"]').trigger("click");
    await flushPromises();
    // Exactly one media lookup, for the PRODUCT -- never an asset the
    // editable preview would compose.
    expect(mediaUrl).toHaveBeenCalledTimes(1);
    expect(mediaUrl).toHaveBeenCalledWith(SESSION, { productId: "prod-a" });
    const video = w.get('[data-testid="product-player-video"]');
    expect(video.element.tagName).toBe("VIDEO");
    expect(video.attributes("src")).toBe(
      `http://asset.localhost/${encodeURIComponent("C:\\data\\products\\prod-a.mp4")}`,
    );
    expect(video.attributes("controls")).toBeDefined();
    expect(w.find('[data-testid="preview-surface"]').exists()).toBe(false);
  });

  it("restore asks for confirmation and sends restore_product", async () => {
    const restored: EditorProjection = {
      snapshot: { ...openResult(8).snapshot, persistedRevision: 7, undoLabel: "Restore render", canUndo: true },
      project: { ...project(), title: "Frozen" },
    };
    const restoreProduct = vi.fn(() => Promise.resolve(restored));
    const { w } = await library([product()], { restoreProduct });
    await w.get('[data-testid="product-restore-prod-a"]').trigger("click");
    await flushPromises();
    expect(restoreProduct).not.toHaveBeenCalled();
    expect(w.get('[data-testid="product-restore-question-prod-a"]').text()).toMatch(/undo/i);
    // Backing out sends nothing.
    await w.get('[data-testid="product-restore-cancel-prod-a"]').trigger("click");
    expect(w.find('[data-testid="product-restore-question-prod-a"]').exists()).toBe(false);
    await w.get('[data-testid="product-restore-prod-a"]').trigger("click");
    await w.get('[data-testid="product-restore-confirm-prod-a"]').trigger("click");
    await flushPromises();
    expect(restoreProduct).toHaveBeenCalledTimes(1);
    const [sessionId, expectedRevision, productId, commandId] = restoreProduct.mock.calls[0] as unknown as [
      string,
      number,
      string,
      string,
    ];
    expect([sessionId, expectedRevision, productId]).toEqual([SESSION, 7, "prod-a"]);
    expect(commandId).toMatch(/^[a-zA-Z0-9_-]{1,100}$/);
    // The restore is an ordinary acknowledged edit: the store installs it.
    expect(useEditorProjectStore().snapshot?.revision).toBe(8);
    expect(useEditorProjectStore().project?.title).toBe("Frozen");
  });

  it("a missing product file shows unavailable but keeps its lineage", async () => {
    const missing = product({ id: "prod-m", name: "Lost file", revision: 4, available: false });
    const restoreProduct = vi.fn(() => Promise.resolve({ snapshot: openResult(8).snapshot, project: project() }));
    const { w } = await library([missing], { restoreProduct });
    const card = w.get('[data-testid="product-card-prod-m"]');
    expect(card.get('[data-testid="product-unavailable-prod-m"]').text()).toMatch(/no longer on disk/i);
    // The lineage stays: which edit made it, and when.
    expect(card.text()).toContain("Lost file");
    expect(card.text()).toContain("r4");
    expect(card.find('[data-testid="product-created-prod-m"]').exists()).toBe(true);
    // Nothing to watch -- but the edit it was made from is still restorable.
    expect(w.get('[data-testid="product-watch-prod-m"]').attributes("disabled")).toBeDefined();
    await w.get('[data-testid="product-restore-prod-m"]').trigger("click");
    await w.get('[data-testid="product-restore-confirm-prod-m"]').trigger("click");
    await flushPromises();
    expect(restoreProduct).toHaveBeenCalledTimes(1);
  });

  it("says so when there are no products yet", async () => {
    const { w } = await library([]);
    expect(w.get('[data-testid="product-library-empty"]').text()).toMatch(/render/i);
  });

  // F18: a Review's terminal carries no productId, so it never asks the
  // library to refresh -- forty in a row (the product cap's worth) leave
  // it exactly as empty as it was. A real render, by contrast, does.
  it("review renders never add a ProductLibrary entry", async () => {
    const { w, getProducts, requests, deliver } = await library([]);
    const jobs = useEditorJobsStore();
    for (let i = 0; i < 40; i += 1) {
      const jobId = await jobs.startRender({ name: "Review", range: { startMs: 0, endMs: 5_000 }, quality: "balanced", review: true });
      expect(jobId).toBe(`job-${i}`);
      deliver(i, progress(`job-${i}`, { sequence: 2, phase: "complete", fraction: 1, terminal: {} }));
      await flushPromises();
    }
    expect(requests.every((r) => r.review === true)).toBe(true);
    expect(getProducts).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="product-library-empty"]').exists()).toBe(true);

    getProducts.mockResolvedValueOnce([product({ id: "prod-new" })]);
    await jobs.startRender({ name: "Walkthrough v1", range: null, quality: "balanced" });
    deliver(40, progress("job-40", { sequence: 2, phase: "complete", fraction: 1, terminal: { productId: "prod-new" } }));
    await flushPromises();
    expect(getProducts).toHaveBeenCalledTimes(2);
    expect(w.find('[data-testid="product-card-prod-new"]').exists()).toBe(true);
  });
});

describe("LibraryPanel — Products tab", () => {
  it("mounts the product library from the editor's library panel", async () => {
    await openWithRenders({ getProducts: () => Promise.resolve([product()]) });
    const w = mount(LibraryPanel);
    expect(w.find('[data-testid="product-library"]').exists()).toBe(false);
    await w.get('[data-testid="library-tab-products"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="product-library"]').exists()).toBe(true);
    expect(w.find('[data-testid="product-card-prod-a"]').exists()).toBe(true);
  });
});
