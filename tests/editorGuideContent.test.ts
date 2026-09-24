/**
 * The guide's shipped content and its typed targets (Task 55; F-46; ADR
 * R18): 22 lessons in seven chapters, copied VERBATIM from the concept
 * bundle, each lesson pointing at a `GuideTargetKey` rather than a CSS
 * selector — and every one of those keys resolved by a control the mounted
 * editor shell actually registered. The last test is the one that fails,
 * naming the key, the day a component stops binding its target.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h } from "vue";

vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import FadesSection from "../src/components/editor/inspector/FadesSection.vue";
import InspectorPanel from "../src/components/editor/inspector/InspectorPanel.vue";
import LayoutSection from "../src/components/editor/inspector/LayoutSection.vue";
import LibraryPanel from "../src/components/editor/library/LibraryPanel.vue";
import PreviewSurface from "../src/components/editor/preview/PreviewSurface.vue";
import EditorShell from "../src/components/editor/shell/EditorShell.vue";
import PreviewToolbar from "../src/components/editor/shell/PreviewToolbar.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import { useGuideOverflow, useGuideTarget } from "../src/composables/useGuideTarget";
import {
  CONTENT_REVISION,
  GUIDE_CHAPTERS,
  GUIDE_STEPS,
  resolveStepId,
  RETIRED_STEP_MAP,
  STEP_TARGETS,
} from "../src/editor/guide/content";
import type { GuideTargetKey } from "../src/editor/guide/targets";
import { GUIDE_TARGET_KEYS, resolve } from "../src/editor/guide/targets";
import type { Clip, EditorOpenResult, Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (p: string) => readFileSync(path.join(ROOT, p), "utf8");

/** The concept bundle's order, and ADR R18's key list in the same order. */
const STEP_IDS = [
  "welcome", "media", "preview", "timeline", "select", "split", "undo", "arrange", "context",
  "tracks", "webcam", "layout", "fades", "audio", "callouts", "captions", "chapters",
  "checks", "save", "render", "products", "help",
];
const R18_KEYS = [
  "projectbar", "library.import", "transport", "timeline.toolbar", "clip.selected", "timeline.split",
  "timeline.undo", "inspector", "timeline.more", "track.menu", "library.webcam", "inspector.layout",
  "inspector.fades", "mixer", "preview.toolstrip", "library.captions", "library.chapters",
  "header.checks", "header.save", "header.render", "library.products", "header.help",
];

describe("guide content", () => {
  it("there are 22 steps in 7 chapters", () => {
    // Verbatim: byte-identical to the concept bundle, not a re-typed copy.
    expect(read("src/editor/guide/steps.json")).toBe(
      read("docs/concepts/vault-buddy-editor/contracts/onboarding.steps.json"),
    );
    expect(read("src/editor/guide/chapters.json")).toBe(
      read("docs/concepts/vault-buddy-editor/contracts/onboarding.chapters.json"),
    );
    expect(GUIDE_STEPS.map((s) => s.id)).toEqual(STEP_IDS);
    expect(GUIDE_CHAPTERS.map((c) => c.id)).toEqual(["orient", "edit", "layers", "polish", "teach", "finish", "return"]);
    // Chapters appear in chapter order, each a contiguous run of lessons.
    const runs = GUIDE_STEPS.map((s) => s.chapter).filter((c, i, all) => i === 0 || all[i - 1] !== c);
    expect(runs).toEqual(GUIDE_CHAPTERS.map((c) => c.id));
    expect(CONTENT_REVISION).toBe(1);
  });

  it("every step maps to a target key", () => {
    expect(GUIDE_STEPS.map((s) => STEP_TARGETS[s.id])).toEqual(R18_KEYS);
    expect([...GUIDE_TARGET_KEYS]).toEqual(R18_KEYS);
  });

  it("unknown ids fall back to the chapter's first step", () => {
    const retired = { trim: "edit", ruler: "orient", overlay: "teach" };
    expect(resolveStepId("undo", retired)).toBe("undo");
    expect(resolveStepId("trim", retired)).toBe("select");
    expect(resolveStepId("ruler", retired)).toBe("welcome");
    expect(resolveStepId("overlay", retired)).toBe("callouts");
    // No chapter claims it: the guide's first lesson.
    expect(resolveStepId("gone", retired)).toBe("welcome");
    // The shipped map only ever names real chapters and no live lesson.
    for (const [old, chapter] of Object.entries(RETIRED_STEP_MAP)) {
      expect(STEP_IDS).not.toContain(old);
      expect(GUIDE_CHAPTERS.map((c) => c.id)).toContain(chapter);
    }
  });
});

describe("guide target registry", () => {
  it("resolves the More item when the owning control is in the overflow", async () => {
    let overflowed = false;
    const Harness = defineComponent({
      setup() {
        const target = useGuideTarget("preview.toolstrip");
        const more = useGuideOverflow("preview.toolstrip", () => overflowed);
        return () => h("div", [h("div", { ref: target, id: "strip" }), h("button", { ref: more, id: "more" })]);
      },
    });
    const w = mount(Harness, { attachTo: document.body });

    expect(resolve("preview.toolstrip")).toEqual({ element: w.get("#strip").element, revealed: "direct" });
    overflowed = true;
    expect(resolve("preview.toolstrip")).toEqual({ element: w.get("#more").element, revealed: "overflow" });

    w.unmount();
    expect(resolve("preview.toolstrip")).toBeNull();
  });

  it("prefers the owning panel over its tab, and falls back to the tab", async () => {
    const Harness = defineComponent({
      props: { open: Boolean },
      setup(props) {
        const tab = useGuideTarget("library.captions", { fallback: true });
        return () => h("div", [h("button", { ref: tab, id: "tab" }), props.open ? h(Panel) : null]);
      },
    });
    const Panel = defineComponent({
      setup() {
        const panel = useGuideTarget("library.captions");
        return () => h("section", { ref: panel, id: "panel" });
      },
    });
    const w = mount(Harness, { props: { open: true }, attachTo: document.body });
    expect(resolve("library.captions")?.element).toBe(w.get("#panel").element);
    expect(w.get("#panel").attributes("data-guide-target")).toBe("library.captions");

    await w.setProps({ open: false });
    expect(resolve("library.captions")?.element).toBe(w.get("#tab").element);
  });
});

// ---- the mounted shell ---------------------------------------------------

function clip(id: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id, asset_id: "capture", track_id: "v1", name: id, start_ms: 0, in_ms: 0, out_ms: 3_000,
    fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false,
    x: 0, y: 0, w: 1, h: 1, ...overrides,
  };
}

const PROJECT: Project = {
  schema: "vault-buddy-video-project/3",
  id: "project-a",
  title: "Tutorial",
  canvas: { width: 1280, height: 720, fps: 30 },
  master_gain: 1,
  assets: [{ id: "capture", kind: "video", name: "cap one", duration_ms: 10_000 }],
  tracks: [
    { id: "v1", kind: "video", name: "Video", visible: true, locked: false, muted: false, solo: false, volume: 1 },
    { id: "a1", kind: "audio", name: "Audio", visible: true, locked: false, muted: false, solo: false, volume: 1 },
  ],
  clips: [clip("intro"), clip("body", { start_ms: 3_000, in_ms: 3_000, out_ms: 7_000 })],
  effects: [],
  markers: [],
  transitions: [],
  captions: null,
  destination: { vault: "vault-a", folder: "", dated: false },
};

const OPENED: EditorOpenResult = {
  snapshot: {
    sessionId: "ses-a", projectId: "project-a", revision: 1, persistedRevision: 1, title: "Tutorial",
    durationMs: 7_000, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  },
  project: PROJECT,
  workspace: {},
  missing: [],
  sourceBase: "cap one",
  recovered: false,
};

/** `EditorRoot`'s own slot filling, minus the legacy surface. */
const MountedShell = defineComponent({
  setup() {
    return () =>
      h(EditorShell, null, {
        library: () => h(LibraryPanel),
        preview: () => h(PreviewSurface),
        inspector: () =>
          h(InspectorPanel, null, {
            layout: ({ clipIds }: { clipIds: string[] }) => h(LayoutSection, { clipIds }),
            fades: ({ clipIds }: { clipIds: string[] }) => h(FadesSection, { clipIds }),
          }),
        timeline: () => h(TimelineView),
      });
  },
});

describe("every lesson's target in the mounted editor", () => {
  beforeEach(async () => {
    setActivePinia(createPinia());
    mockConvertFileSrc("windows");
    const port = fakeEditorPort({
      openStaged: () => Promise.resolve(OPENED),
      getWorkspace: () => Promise.resolve({}),
      saveWorkspace: () => Promise.resolve(),
      getChecks: () => Promise.resolve([]),
      getProducts: () => Promise.resolve([]),
      getJobs: () => Promise.resolve([]),
      mediaUrl: () => Promise.reject(new Error("no media in this test")),
      mediaPeaks: () => Promise.reject(new Error("no media in this test")),
      mediaThumbnail: () => Promise.reject(new Error("no media in this test")),
      getGuideProgress: () => Promise.reject(new Error("not read here")),
    });
    const project = useEditorProjectStore();
    project.setPort(port);
    useEditorWorkspaceStore().setPort(port);
    await project.openStaged("cap one");
    useEditorWorkspaceStore().select(["body"]);
  });

  function unresolved(): GuideTargetKey[] {
    return GUIDE_TARGET_KEYS.filter((key) => resolve(key) === null);
  }

  it("every target key is registered by the mounted shell", async () => {
    mount(MountedShell, { attachTo: document.body });
    await flushPromises();

    expect(unresolved()).toEqual([]);
    // The selected clip, not merely some clip.
    expect(resolve("clip.selected")?.element.getAttribute("data-testid")).toBe("clip-body");
  });

  it("a lesson in a closed library tab or inspector section resolves to its tab", async () => {
    const workspace = useEditorWorkspaceStore();
    workspace.setLibraryTab("media");
    workspace.setPropertyTab("clip");
    mount(MountedShell, { attachTo: document.body });
    await flushPromises();

    expect(resolve("library.captions")?.element.getAttribute("data-testid")).toBe("library-tab-captions");
    expect(resolve("inspector.layout")?.element.getAttribute("data-testid")).toBe("inspector-tab-layout");
    expect(resolve("library.import")?.element.getAttribute("data-testid")).toBe("library-import");

    workspace.setPropertyTab("layout");
    workspace.setLibraryTab("captions");
    await flushPromises();
    expect(resolve("inspector.layout")?.element.getAttribute("data-testid")).toBe("layout-section");
    expect(resolve("library.captions")?.element.getAttribute("data-testid")).toBe("captions-library");
    expect(resolve("library.import")?.element.getAttribute("data-testid")).toBe("library-tab-media");
  });

  it("the teaching tools point at More once the arrow has overflowed", async () => {
    const w = mount(PreviewToolbar, { props: { overflowCount: 11 }, attachTo: document.body });
    expect(resolve("preview.toolstrip")).toEqual({
      element: w.get('[data-testid="preview-toolbar-more"]').element,
      revealed: "overflow",
    });
    await w.setProps({ overflowCount: 0 });
    expect(resolve("preview.toolstrip")).toEqual({
      element: w.get('[data-testid="preview-toolbar"]').element,
      revealed: "direct",
    });
  });
});
