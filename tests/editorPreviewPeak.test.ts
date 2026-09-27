/**
 * The preview's sample peak, read from outside the preview
 * (`src/editor/previewPeak.ts`, visual-parity Task 20): the mixer moved to
 * the timeline footer, a sibling `PreviewSurface` cannot hand a prop to, so
 * the surface registers its reader while it is mounted and the footer's
 * mixer reads through it.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import PreviewSurface from "../src/components/editor/preview/PreviewSurface.vue";
import TimelineFooter from "../src/components/editor/timeline/TimelineFooter.vue";
import { formatDb } from "../src/editor/mixRules";
import type { AudioContextLike } from "../src/editor/previewController";
import { clearPreviewPeakReader, readPreviewPeak, setPreviewPeakReader } from "../src/editor/previewPeak";
import type { EditorOpenResult } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";
import { project } from "./helpers/inspectorProject";

enableAutoUnmount(afterEach);

describe("previewPeak", () => {
  it("reads nothing until a reader is set, then whatever it answers", () => {
    const mine = () => 0.25;
    expect(readPreviewPeak()).toBeNull();
    setPreviewPeakReader(mine);
    expect(readPreviewPeak()).toBe(0.25);
    clearPreviewPeakReader(mine);
  });

  it("clearing forgets only the caller's own reader", () => {
    const old = () => 0.1;
    const current = () => 0.9;
    setPreviewPeakReader(old);
    setPreviewPeakReader(current);
    // A surface unmounting after a newer one mounted must not blank it.
    clearPreviewPeakReader(old);
    expect(readPreviewPeak()).toBe(0.9);
    clearPreviewPeakReader(current);
    expect(readPreviewPeak()).toBeNull();
  });
});

/** Web Audio with the one analyser the controller routes every layer
 * through, answering a fixed buffer whose peak is 0.5. */
function meteredContext(): AudioContextLike {
  return {
    destination: {},
    createGain: () => ({ gain: { value: 1 }, connect: () => undefined }),
    createMediaElementSource: () => ({ connect: () => undefined }),
    createAnalyser: () => ({
      fftSize: 3,
      connect: () => undefined,
      getFloatTimeDomainData: (buf: Float32Array) => buf.set([0.2, -0.5, 0.1]),
    }),
    resume: () => Promise.resolve(),
  };
}

async function openProject(): Promise<void> {
  const opened: EditorOpenResult = {
    snapshot: {
      sessionId: "ses-a", projectId: "project-a", revision: 1, persistedRevision: null, title: "Tutorial",
      durationMs: 33_500, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
    },
    project: project(),
    workspace: {},
    missing: [],
    sourceBase: "base",
    recovered: false,
  };
  const store = useEditorProjectStore();
  store.setPort(
    fakeEditorPort({
      openStaged: () => Promise.resolve(opened),
      mediaUrl: () => Promise.resolve("C:\\x\\media.mp4"),
      getWorkspace: () => Promise.resolve({}),
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged("base");
}

describe("the footer's mixer reads the mounted preview's peak", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    mockConvertFileSrc("windows");
  });

  it("the surface registers its reader on mount, and nothing is read once it unmounts", async () => {
    await openProject();
    const surface = mount(PreviewSurface, { props: { createAudioContext: meteredContext }, attachTo: document.body });
    await flushPromises();
    expect(readPreviewPeak()).toBeCloseTo(0.5, 6);
    surface.unmount();
    expect(readPreviewPeak()).toBeNull();
  });

  it("the meter in the footer's mixer shows that peak", async () => {
    await openProject();
    mount(PreviewSurface, { props: { createAudioContext: meteredContext }, attachTo: document.body });
    const footer = mount(TimelineFooter, { attachTo: document.body });
    await flushPromises();
    await footer.get('[data-testid="mixer-toggle"]').trigger("click");
    await flushPromises();
    expect(footer.get('[data-testid="mixer-peak"]').text()).toContain(`${formatDb(0.5)}FS`);
  });
});
