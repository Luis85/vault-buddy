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

import PreviewToolbar from "../src/components/editor/shell/PreviewToolbar.vue";
import { useGuideOverflow, useGuideTarget } from "../src/composables/useGuideTarget";
import {
  CONTENT_REVISION,
  GUIDE_CHAPTERS,
  GUIDE_STEPS,
  LESSON_COPY_OVERRIDES,
  lessonCopy,
  resolveStepId,
  RETIRED_STEP_MAP,
  STEP_TARGETS,
} from "../src/editor/guide/content";
import type { GuideTargetKey } from "../src/editor/guide/targets";
import { GUIDE_TARGET_KEYS, resolve } from "../src/editor/guide/targets";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { MountedShell, shellPort } from "./helpers/guideShell";

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

// GAP-203: `steps.json` stays a verbatim copy of the concept file, but that
// file was written for the BROWSER reference. The coach renders
// `lessonCopy(id)`, which replaces exactly the sentences that are untrue
// in this app. Each override is pinned whole, so a lost word, a stray
// space or a quietly reworded promise fails here.
describe("native lesson copy", () => {
  it("overrides exactly the sentences the reference gets wrong", () => {
    expect(LESSON_COPY_OVERRIDES).toEqual({
      media: {
        tip: "Imported files are copied into this project; the file you pick is never changed. Importing is optional: the whole guide works with the recording already open.",
        task: "Open the import picker if you want to add a file, or continue.",
      },
      select: {
        tip: "Selection is not an edit: choosing which clip is selected never changes the video.",
      },
      split: {
        tip: "Leave gap keeps other clips in place. Close gap on this track closes the gap on this track only, which can change its alignment with other tracks.",
      },
      context: {
        body: "Right-click a clip for the actions that apply to it. Edit actions in the timeline opens the same menu for the selected clips without a right click.",
      },
      tracks: {
        label: "Track menu",
        tip: "Adding a track is optional: dropping media below the last track makes a new one.",
        task: "Open the top track's menu to see what it offers. You do not need to change anything.",
      },
      fades: {
        body: "Set Fade in and Fade out, or drag the gold handles on a timeline clip. Video fades reveal the layer beneath; audio fades change volume.",
      },
      audio: {
        tip: "Listen to the actual rendered file before sharing.",
      },
      chapters: {
        task: "Look through the chapter list. Adding a chapter is optional.",
      },
      save: {
        tip: "Save project stores the editable project in Vault Buddy on this computer, and unsaved edits are kept for recovery if the editor closes. A portable project file may include uncensored originals.",
        task: "Open the menu beside Save project to see the file formats. Saving is optional.",
      },
      render: {
        tip: "Rendering runs on this computer with your installed ffmpeg and adds the video to this project's Products. Publishing a product copies it, with an optional companion note, into a vault. Keep the editable project for later changes.",
      },
      products: {
        label: "Rendered products",
        body: "Rendered videos are listed in the library's Products tab, separate from the editable project. Watch one, or restore the edit snapshot behind an earlier output.",
        task: "Look through Products, then continue.",
      },
      help: {
        label: "Help",
        body: "Help is always here: it resumes a paused walkthrough at the same lesson, and so do F1 and ? when you are not typing in a field. Back revisits any earlier lesson.",
      },
    });
  });

  it("every other sentence is the verbatim concept text", () => {
    for (const step of GUIDE_STEPS) {
      const over: Partial<Record<"label" | "body" | "tip" | "task", string>> =
        (LESSON_COPY_OVERRIDES as Record<string, Partial<Record<"label" | "body" | "tip" | "task", string>>>)[step.id] ?? {};
      expect(lessonCopy(step.id), `lesson ${step.id}`).toEqual({
        label: over.label ?? step.label,
        body: over.body ?? step.body,
        tip: over.tip ?? step.tip,
        task: over.task ?? step.task ?? null,
      });
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

describe("every lesson's target in the mounted editor", () => {
  beforeEach(async () => {
    setActivePinia(createPinia());
    mockConvertFileSrc("windows");
    const port = shellPort();
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

  // Task 55 review (carried into Task 56): a key whose owning panel lost its
  // binding still resolves through its tab fallback, so "every key
  // resolves" alone cannot see it. Open each owner and require the OWNER,
  // not its tab, to be what resolves.
  const OWNERS: [GuideTargetKey, "library" | "property", string, string][] = [
    ["library.import", "library", "media", "library-import"],
    ["library.webcam", "library", "media", "library-webcam"],
    ["library.captions", "library", "captions", "captions-library"],
    ["library.chapters", "library", "chapters", "chapters-library"],
    ["library.products", "library", "products", "product-library"],
    ["inspector.layout", "property", "layout", "layout-section"],
    ["inspector.fades", "property", "fades", "fades-section"],
  ];

  it.each(OWNERS)("%s resolves to its owning control once its tab is open", async (key, where, tab, owner) => {
    const workspace = useEditorWorkspaceStore();
    if (where === "library") workspace.setLibraryTab(tab);
    else workspace.setPropertyTab(tab);
    mount(MountedShell, { attachTo: document.body });
    await flushPromises();

    expect(resolve(key)?.element.getAttribute("data-testid")).toBe(owner);
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
