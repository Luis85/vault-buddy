/**
 * `EditorShell`/`EditorHeader` (Task 16, F-48). Mounts the shell directly
 * (not through `EditorRoot`) — its own job is the responsive grid and the
 * header's command surface, independent of the Task 15 legacy-editor
 * hand-off that `tests/editorRoot.test.ts` already covers. Every test drives
 * `editorProject` through an injected fake `EditorPort`, the
 * `editorProjectStore.test.ts` precedent: this store's whole job is
 * deciding which already-decoded reply to install, not decoding the wire.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import EditorShell from "../src/components/editor/shell/EditorShell.vue";
import type { EditorPort } from "../src/editor/port";
import type { EditorOpenResult, EditorSnapshot, Project, SaveReceipt } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";

enableAutoUnmount(afterEach);

function setViewportWidth(width: number) {
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: width });
}

beforeEach(() => {
  setActivePinia(createPinia());
  // A wide default so a test that doesn't care about the responsive collapse
  // never accidentally lands in compact mode (the "fixture flaw" rule: a
  // test that only incidentally passes at the shared-document's leftover
  // width proves nothing).
  setViewportWidth(1440);
});

function snapshot(overrides: Partial<EditorSnapshot> = {}): EditorSnapshot {
  return {
    sessionId: "ses-a",
    projectId: "project-a",
    revision: 1,
    persistedRevision: null,
    title: "Tutorial",
    durationMs: 0,
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
    tracks: [],
    clips: [],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

function openResult(overrides: Partial<EditorOpenResult> = {}): EditorOpenResult {
  return {
    snapshot: snapshot(),
    project: project(),
    workspace: {},
    missing: [],
    sourceBase: "base",
    recovered: false,
    ...overrides,
  };
}

function fakePort(overrides: Partial<EditorPort> = {}): EditorPort {
  const unimplemented = (name: string) => (): never => {
    throw new Error(`fakePort.${name} not stubbed for this test`);
  };
  return {
    openStaged: unimplemented("openStaged"),
    openProject: unimplemented("openProject"),
    listProjects: unimplemented("listProjects"),
    getSnapshot: unimplemented("getSnapshot"),
    execute: unimplemented("execute"),
    save: unimplemented("save"),
    closeSession: unimplemented("closeSession"),
    hideWindow: unimplemented("hideWindow"),
    getWorkspace: unimplemented("getWorkspace"),
    saveWorkspace: unimplemented("saveWorkspace"),
    ...overrides,
  };
}

describe("EditorShell — header buttons", () => {
  it("shows Save project and Render video as separate buttons", () => {
    const w = mount(EditorShell);

    const save = w.get('[data-testid="editor-header-save"]');
    const render = w.get('[data-testid="editor-header-render"]');

    expect(save.element).not.toBe(render.element);
    expect(save.text()).toBe("Save project");
    expect(render.text()).toBe("Render video");
    // Render arrives in Task 47 — disabled with a visible reason (R20: "a
    // disabled control carries a reason string"), not just a hover title.
    expect(render.attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="editor-header-render-reason"]').text().length).toBeGreaterThan(0);
  });
});

describe("EditorShell — status text", () => {
  // This task's own mutation check: deriving status from a `setTimeout`
  // (rather than the store's own `dirty`/`saving` fields) would read
  // "Unsaved changes" as SAVED before the timer fires and would read
  // "Saving…" (or worse, stale "Saved") after the awaited receipt actually
  // lands — either way red against the assertions below, which read the
  // status synchronously right after each awaited store call and nothing
  // else.
  it("reads Unsaved changes when dirty and Saved after a matching receipt", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ snapshot: snapshot({ revision: 2, persistedRevision: null }) })),
        save: (sessionId, expectedRevision) =>
          Promise.resolve<SaveReceipt>({ sessionId, savedRevision: expectedRevision, projectFileId: "project-a" }),
      }),
    );
    await store.openStaged("cap one");
    const w = mount(EditorShell);

    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Unsaved changes");

    await store.save();
    await flushPromises();

    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Saved");
  });

  it("reads Saving… while the save round trip is outstanding", async () => {
    const store = useEditorProjectStore();
    let resolveSave!: (r: SaveReceipt) => void;
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ snapshot: snapshot({ revision: 2, persistedRevision: null }) })),
        save: () => new Promise<SaveReceipt>((res) => (resolveSave = res)),
      }),
    );
    await store.openStaged("cap one");
    const w = mount(EditorShell);

    const saving = store.save();
    await flushPromises();
    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Saving…");

    resolveSave({ sessionId: "ses-a", savedRevision: 2, projectFileId: "project-a" });
    await saving;
    await flushPromises();
    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Saved");
  });
});

describe("EditorShell — responsive collapse (SCREENS-AND-INTERACTIONS.md §12)", () => {
  it("below 1180px the library is a drawer, closed by default, and the header stays visible", async () => {
    setViewportWidth(960);
    // `isVisible()` reads `getComputedStyle`, which happy-dom only resolves
    // for a node actually attached to `document` (the `TabGroup.vue`
    // `v-show` precedent, `tests/tab-group.test.ts`) — a detached mount
    // makes every element read visible regardless of `display:none`, which
    // would make this test pass against BOTH a working and a broken drawer.
    const w = mount(EditorShell, { attachTo: document.body });

    expect(w.find('[data-testid="editor-header"]').exists()).toBe(true);
    const toggle = w.get('[data-testid="editor-header-library-toggle"]');
    expect(toggle.attributes("aria-expanded")).toBe("false");
    expect(w.get('[data-testid="editor-shell-library"]').isVisible()).toBe(false);

    await toggle.trigger("click");

    expect(toggle.attributes("aria-expanded")).toBe("true");
    expect(w.get('[data-testid="editor-shell-library"]').isVisible()).toBe(true);
    // Opening the drawer must never remove or hide the header — "the route
    // back" (this task's own Behavior section).
    expect(w.find('[data-testid="editor-header"]').exists()).toBe(true);
    expect(w.get('[data-testid="editor-header"]').isVisible()).toBe(true);
  });

  it("at or above 1180px there is no drawer toggle — library and inspector are always shown", () => {
    setViewportWidth(1180);
    const w = mount(EditorShell, { attachTo: document.body });

    expect(w.find('[data-testid="editor-header-library-toggle"]').exists()).toBe(false);
    expect(w.find('[data-testid="editor-header-inspector-toggle"]').exists()).toBe(false);
    expect(w.get('[data-testid="editor-shell-library"]').isVisible()).toBe(true);
    expect(w.get('[data-testid="editor-shell-inspector"]').isVisible()).toBe(true);
  });
});

describe("EditorShell — placeholders (later tasks fill these in)", () => {
  it("renders exactly one preview toolbar row and the timeline/library/inspector regions", () => {
    const w = mount(EditorShell);

    expect(w.findAll('[data-testid="preview-toolbar"]')).toHaveLength(1);
    expect(w.find('[data-testid="editor-shell-timeline"]').exists()).toBe(true);
    expect(w.find('[data-testid="editor-shell-library"]').exists()).toBe(true);
    expect(w.find('[data-testid="editor-shell-inspector"]').exists()).toBe(true);
  });
});

describe("EditorShell — PreviewToolbar's focus-preview (Task 17)", () => {
  it("collapses both drawers when the toolbar's Focus preview control fires", async () => {
    setViewportWidth(960); // compact, so the drawer toggles' aria-expanded is observable
    const w = mount(EditorShell, { attachTo: document.body });

    await w.get('[data-testid="editor-header-library-toggle"]').trigger("click");
    expect(w.get('[data-testid="editor-header-library-toggle"]').attributes("aria-expanded")).toBe("true");

    await w.get('[data-testid="preview-toolbar-focusPreview"]').trigger("click");

    expect(w.get('[data-testid="editor-header-library-toggle"]').attributes("aria-expanded")).toBe("false");
    expect(w.get('[data-testid="editor-header-inspector-toggle"]').attributes("aria-expanded")).toBe("false");
  });
});

describe("EditorHeader — inline rename", () => {
  it("renames through the rename command on Enter, and leaves the title untouched on Escape", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ title: "Old title" }) })),
        execute: (req) => {
          executed.push(req.command);
          return Promise.resolve({
            snapshot: snapshot({ title: "New title", revision: 2 }),
            project: project({ title: "New title" }),
          });
        },
      }),
    );
    await store.openStaged("cap one");
    const w = mount(EditorShell);

    await w.get('[data-testid="editor-shell-title"]').trigger("click");
    const input = w.get('[data-testid="editor-header-title-input"]');
    await input.setValue("New title");
    await input.trigger("keydown.enter");
    await flushPromises();

    expect(executed).toEqual([{ kind: "rename", title: "New title" }]);
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("New title");

    // Escape discards the draft without calling execute again.
    await w.get('[data-testid="editor-shell-title"]').trigger("click");
    await w.get('[data-testid="editor-header-title-input"]').setValue("Discarded");
    await w.get('[data-testid="editor-header-title-input"]').trigger("keydown.esc");

    expect(executed).toHaveLength(1);
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("New title");
  });
});
