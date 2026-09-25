/**
 * The Render dialog (Task 47; F-41, F-42; SCREENS 09): name, quality,
 * whole-or-range, the checks summary and the originals statement; progress
 * with its phases and Cancel; and completion's Watch / Publish / Render
 * another. Progress reaches 100 % ONLY from a `complete` terminal (R20: no
 * progress that completes without a terminal record), a cancel is not a
 * failure, and a render's refusal never reads as a failed SAVE (Task 46's
 * carry: render errors stay out of the header's save error).
 *
 * Also the preview toolbar's Review (F18): a short range render of the
 * selection, or of +/- 5 s around the playhead, played back as the real
 * encoded file.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import RenderDialog from "../src/components/editor/dialogs/RenderDialog.vue";
import EditorHeader from "../src/components/editor/shell/EditorHeader.vue";
import PreviewToolbar from "../src/components/editor/shell/PreviewToolbar.vue";
import { EditorPortError } from "../src/editor/port";
import { useEditorJobsStore } from "../src/stores/editorJobs";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { openWithRenders, product, progress, SESSION } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
});

const HEADER_PROPS = { isCompact: false, libraryOpen: false, inspectorOpen: false, theme: "dark" as const };

async function openDialog(extra = {}) {
  const env = await openWithRenders({ getProducts: () => Promise.resolve([product()]), ...extra });
  const w = mount(RenderDialog, { props: { open: true, initialRange: null } });
  await flushPromises();
  return { ...env, w };
}

async function start(w: ReturnType<typeof mount>) {
  await w.get('[data-testid="render-dialog-start"]').trigger("click");
  await flushPromises();
}

describe("RenderDialog — the form", () => {
  it("defaults to '<title> v<n>', balanced, the whole project, and says the originals are untouched", async () => {
    const { w, requests } = await openDialog();
    const name = w.get('[data-testid="render-dialog-name"]').element as HTMLInputElement;
    // One product already exists, so this is the second.
    expect(name.value).toBe("Walkthrough v2");
    expect((w.get('[data-testid="render-dialog-quality-balanced"]').element as HTMLInputElement).checked).toBe(true);
    expect((w.get('[data-testid="render-dialog-scope-whole"]').element as HTMLInputElement).checked).toBe(true);
    expect(w.get('[data-testid="render-dialog-originals"]').text()).toContain(
      "Your originals and this project are not changed",
    );
    // Task 54: the real checks — none here, and the list says so.
    expect(w.get('[data-testid="render-dialog-checks-summary"]').text()).toBe("0 blockers · 0 review warnings");
    expect(w.get('[data-testid="render-dialog-checks"]').text()).toBe("Nothing blocks this render.");
    await start(w);
    expect(requests).toEqual([
      { sessionId: SESSION, expectedRevision: 7, name: "Walkthrough v2", range: null, quality: "balanced" },
    ]);
  });

  it("a range defaults to the given in/out range and is sent in output milliseconds", async () => {
    const env = await openWithRenders({ getProducts: () => Promise.resolve([]) });
    const w = mount(RenderDialog, { props: { open: true, initialRange: { startMs: 1_500, endMs: 12_500 } } });
    await flushPromises();
    await w.get('[data-testid="render-dialog-scope-range"]').setValue(true);
    const startField = w.get('[data-testid="render-dialog-range-start"]');
    const endField = w.get('[data-testid="render-dialog-range-end"]');
    expect((startField.element as HTMLInputElement).value).toBe("1.5");
    expect((endField.element as HTMLInputElement).value).toBe("12.5");
    await endField.setValue("4.25");
    await w.get('[data-testid="render-dialog-quality-high"]').setValue(true);
    await start(w);
    expect(env.requests[0]).toMatchObject({
      name: "Walkthrough v1",
      range: { startMs: 1_500, endMs: 4_250 },
      quality: "high",
    });
  });

  it("an empty or inverted range cannot start, and says why", async () => {
    const { w, startRender } = await openDialog();
    await w.get('[data-testid="render-dialog-scope-range"]').setValue(true);
    await w.get('[data-testid="render-dialog-range-start"]').setValue("9");
    await w.get('[data-testid="render-dialog-range-end"]').setValue("3");
    const button = w.get('[data-testid="render-dialog-start"]');
    expect(button.attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="render-dialog-start-reason"]').text()).toMatch(/end after/i);
    await button.trigger("click");
    expect(startRender).not.toHaveBeenCalled();
  });
});

describe("RenderDialog — progress", () => {
  // R20: the bar is a promise about a FILE. `fraction: 1` only says ffmpeg
  // reached the end -- the product is not moved or recorded yet, and either
  // step can still fail. Only the `complete` terminal is completion.
  it("fraction 1 without a terminal does not show complete", async () => {
    const { w, deliver } = await openDialog();
    await start(w);
    deliver(0, progress("job-0", { sequence: 2, phase: "publishing", fraction: 1 }));
    await flushPromises();
    const bar = w.get('[data-testid="render-progress-bar"]');
    expect(bar.attributes("aria-valuenow")).not.toBe("100");
    expect(w.find('[data-testid="render-dialog-watch"]').exists()).toBe(false);
    expect(w.get('[data-testid="render-progress-phase"]').text()).toMatch(/saving/i);
    deliver(0, progress("job-0", { sequence: 3, phase: "complete", fraction: 1, terminal: { productId: "prod-b" } }));
    await flushPromises();
    expect(bar.attributes("aria-valuenow")).toBe("100");
    expect(w.find('[data-testid="render-dialog-watch"]').exists()).toBe(true);
  });

  it("cancel sends editor_cancel_job and shows cancelled, not failed", async () => {
    const cancelJob = vi.fn(() => Promise.resolve());
    const { w, deliver } = await openDialog({ cancelJob });
    await start(w);
    deliver(0, progress("job-0", { sequence: 2, fraction: 0.4 }));
    await flushPromises();
    await w.get('[data-testid="render-dialog-cancel"]').trigger("click");
    await flushPromises();
    expect(cancelJob).toHaveBeenCalledWith(SESSION, "job-0");
    const cancelled = {
      code: "cancelled" as const,
      message: "The render was cancelled.",
      retryable: false,
      operationId: "op-1",
    };
    deliver(0, progress("job-0", { sequence: 3, phase: "cancelled", fraction: 0.4, terminal: { error: cancelled } }));
    await flushPromises();
    const status = w.get('[data-testid="render-dialog-status"]');
    expect(status.text()).toMatch(/cancelled/i);
    expect(status.text()).not.toMatch(/failed/i);
    expect(status.attributes("role")).not.toBe("alert");
    expect(w.find('[data-testid="render-dialog-watch"]').exists()).toBe(false);
  });

  it("a failed render says what failed and offers another try", async () => {
    const { w, deliver } = await openDialog();
    await start(w);
    const error = { code: "internal" as const, message: "ffmpeg exited with status 1", retryable: false, operationId: "o" };
    deliver(0, progress("job-0", { sequence: 2, phase: "failed", terminal: { error } }));
    await flushPromises();
    const status = w.get('[data-testid="render-dialog-status"]');
    expect(status.attributes("role")).toBe("alert");
    expect(status.text()).toContain("ffmpeg exited with status 1");
    expect(w.find('[data-testid="render-dialog-another"]').exists()).toBe(true);
  });
});

describe("RenderDialog — completion", () => {
  it("offers Watch rendered file, Publish to vault… and Render another", async () => {
    const mediaUrl = vi.fn(() => Promise.resolve("C:\\data\\products\\prod-b.mp4"));
    const { w, deliver, startRender } = await openDialog({ mediaUrl });
    await start(w);
    deliver(0, progress("job-0", { sequence: 2, phase: "complete", fraction: 1, terminal: { productId: "prod-b" } }));
    await flushPromises();
    await w.get('[data-testid="render-dialog-watch"]').trigger("click");
    await flushPromises();
    expect(mediaUrl).toHaveBeenCalledWith(SESSION, { productId: "prod-b" });
    expect(w.get('[data-testid="product-player-video"]').attributes("src")).toContain("prod-b.mp4");
    // Task 48: publishing is live (its dialog is `editorPublishDialog.test.ts`'s).
    expect(w.get('[data-testid="render-dialog-publish"]').attributes("disabled")).toBeUndefined();
    await w.get('[data-testid="render-dialog-another"]').trigger("click");
    expect(w.find('[data-testid="render-dialog-start"]').exists()).toBe(true);
    await start(w);
    expect(startRender).toHaveBeenCalledTimes(2);
  });

  // Task 46's carry: a refused render is the RENDER's error. It must not
  // read as "Save failed" in the header, nor surface in the media
  // library's import error line.
  it("a refused render never marks the header Save failed", async () => {
    const refusal = new EditorPortError({
      code: "encoderUnavailable",
      message: "Rendering needs ffmpeg, which is not installed.",
      retryable: false,
      operationId: "o",
    });
    const { w } = await openDialog({ startRender: () => Promise.reject(refusal) });
    const header = mount(EditorHeader, { props: HEADER_PROPS });
    await start(w);
    expect(w.get('[data-testid="render-dialog-status"]').text()).toContain("Rendering needs ffmpeg");
    expect(useEditorProjectStore().saveError).toBeNull();
    expect(useEditorProjectStore().lastError).toBeNull();
    expect(useEditorJobsStore().lastError).toBeNull();
    expect(header.get('[data-testid="editor-header-status"]').text()).toBe("Saved");
  });
});

describe("EditorHeader — Render video", () => {
  it("is enabled for an open project and opens the Render dialog", async () => {
    await openWithRenders({ getProducts: () => Promise.resolve([]) });
    const header = mount(EditorHeader, { props: HEADER_PROPS });
    const button = header.get('[data-testid="editor-header-render"]');
    expect(button.attributes("disabled")).toBeUndefined();
    expect(header.find('[data-testid="render-dialog"]').exists()).toBe(false);
    await button.trigger("click");
    await flushPromises();
    expect(header.find('[data-testid="render-dialog"]').exists()).toBe(true);
  });
});

describe("PreviewToolbar — Review (F18)", () => {
  async function review(select: string[], playheadMs: number) {
    const mediaUrl = vi.fn(() => Promise.resolve("C:\\data\\cache\\review-job-0.mp4"));
    const env = await openWithRenders({ mediaUrl });
    const workspace = useEditorWorkspaceStore();
    workspace.select(select);
    workspace.setPlayhead(playheadMs);
    const w = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    await w.get('[data-testid="preview-toolbar-render"]').trigger("click");
    await flushPromises();
    return { ...env, w, mediaUrl };
  }

  it("renders the selection's output span as a review, never a product", async () => {
    const { requests } = await review(["c2", "c1"], 0);
    // c1 starts at 1500; c2 ends at 6000 + (14000 - 1000) / 2 = 12500.
    expect(requests).toEqual([
      expect.objectContaining({ range: { startMs: 1_500, endMs: 12_500 }, review: true }),
    ]);
  });

  it("with nothing selected renders 5 s either side of the playhead, clamped to the project", async () => {
    const { requests } = await review([], 11_000);
    expect(requests[0]).toMatchObject({ range: { startMs: 6_000, endMs: 12_500 }, review: true });
  });

  it("plays the finished review's own file", async () => {
    const { w, deliver, mediaUrl } = await review([], 3_000);
    deliver(0, progress("job-0", { sequence: 2, phase: "complete", fraction: 1, terminal: {} }));
    await flushPromises();
    expect(mediaUrl).toHaveBeenCalledWith(SESSION, { reviewJobId: "job-0" });
    expect(w.get('[data-testid="product-player-video"]').attributes("src")).toContain("review-job-0.mp4");
  });
});
