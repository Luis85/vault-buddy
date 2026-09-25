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
import { clearClipboardForTest } from "../src/editor/clipboard";
import type { Clip, EditorOpenResult, EditorSnapshot, Project, SaveReceipt } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

function setViewportWidth(width: number) {
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: width });
}

beforeEach(() => {
  setActivePinia(createPinia());
  clearClipboardForTest();
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

describe("EditorShell — header buttons", () => {
  it("shows Save project and Render video as separate buttons", () => {
    const w = mount(EditorShell);

    const save = w.get('[data-testid="editor-header-save"]');
    const render = w.get('[data-testid="editor-header-render"]');

    expect(save.element).not.toBe(render.element);
    expect(save.text()).toBe("Save project");
    expect(render.text()).toBe("Render video");
    // No project is open here, so Render (Task 47) is disabled with a
    // visible reason (R20: "a disabled control carries a reason string"),
    // not just a hover title.
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

describe("EditorShell — keyboard shortcut dispatcher (Task 21)", () => {
  it("Ctrl+Z sends undo through the SAME registry the toolbar/menu read", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ snapshot: snapshot({ canUndo: true, undoLabel: "Split clip" }) })),
        execute: (req) => {
          executed.push(req.command);
          return Promise.resolve({ snapshot: snapshot({ revision: 2 }), project: project() });
        },
      }),
    );
    await store.openStaged("cap one");
    const w = mount(EditorShell, { attachTo: document.body });

    await w.get('[data-testid="editor-shell"]').trigger("keydown", { key: "z", ctrlKey: true });
    await flushPromises();

    expect(executed).toEqual([{ kind: "undo" }]);
  });

  it("a disabled shortcut (nothing to undo) sends nothing", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ canUndo: false }) })),
        execute: (req) => {
          executed.push(req.command);
          return Promise.resolve({ snapshot: snapshot({ revision: 2 }), project: project() });
        },
      }),
    );
    await store.openStaged("cap one");
    const w = mount(EditorShell, { attachTo: document.body });

    await w.get('[data-testid="editor-shell"]').trigger("keydown", { key: "z", ctrlKey: true });
    await flushPromises();

    expect(executed).toEqual([]);
  });

  it("a shortcut typed into a text field is ignored (shouldHandle's input gate)", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ title: "Old", canUndo: true }) })),
        execute: (req) => {
          executed.push(req.command);
          return Promise.resolve({ snapshot: snapshot({ revision: 2 }), project: project() });
        },
      }),
    );
    await store.openStaged("cap one");
    const w = mount(EditorShell, { attachTo: document.body });

    await w.get('[data-testid="editor-shell-title"]').trigger("click");
    const input = w.get('[data-testid="editor-header-title-input"]');
    await input.trigger("keydown", { key: "z", ctrlKey: true });
    await flushPromises();

    expect(executed).toEqual([]);
  });
});

describe("EditorShell — dispatcher ownership (Task 21)", () => {
  function undoablePort(executed: unknown[]) {
    return fakePort({
      openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ canUndo: true, undoLabel: "Move" }) })),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: snapshot({ revision: 2 }), project: project() });
      },
    });
  }
  function keydown(target: Element, init: KeyboardEventInit): KeyboardEvent {
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    target.dispatchEvent(event);
    return event;
  }

  // The legacy capture editor (still mounted beside this shell) binds its
  // own Ctrl+Z on `window`. A keystroke this dispatcher CLAIMS must never
  // also reach that listener -- two undo stacks stepping on one keypress.
  it("a claimed shortcut never reaches a window-level listener (the legacy editor's)", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    store.setPort(undoablePort(executed));
    await store.openStaged("cap one");
    const w = mount(EditorShell, { attachTo: document.body });
    const legacy: string[] = [];
    const onWindowKeydown = (e: KeyboardEvent) => legacy.push(e.key);
    window.addEventListener("keydown", onWindowKeydown);
    try {
      const event = keydown(w.get('[data-testid="editor-shell"]').element, { key: "z", ctrlKey: true });
      await flushPromises();

      expect(executed).toEqual([{ kind: "undo" }]);
      expect(event.defaultPrevented).toBe(true);
      expect(legacy).toEqual([]);
    } finally {
      window.removeEventListener("keydown", onWindowKeydown);
    }
  });

  // The other half of the same rule: a key this dispatcher does NOT act on
  // must not be claimed -- preventDefault + stopPropagation with nothing
  // done would swallow it silently (R20). Ctrl+S used to be that key; since
  // Task 57's fix round it runs the header's Save (every key the learning
  // center lists does something -- tests/editorShortcutsWired.test.ts), so
  // the pin moved to F6 with the guide's coach closed, which still bubbles
  // untouched (F1/? are always claimed: they start or resume the guide).
  it("a listed key with nothing to do right now (F6, no coach) is left to bubble, not swallowed", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    store.setPort(undoablePort(executed));
    await store.openStaged("cap one");
    const w = mount(EditorShell, { attachTo: document.body });
    const reached: string[] = [];
    const onWindowKeydown = (e: KeyboardEvent) => reached.push(e.key);
    window.addEventListener("keydown", onWindowKeydown);
    try {
      const event = keydown(w.get('[data-testid="editor-shell"]').element, { key: "F6" });
      await flushPromises();

      expect(executed).toEqual([]);
      expect(event.defaultPrevented).toBe(false);
      expect(reached).toEqual(["F6"]);
    } finally {
      window.removeEventListener("keydown", onWindowKeydown);
    }
  });

  // Ctrl+S (Task 57 fix round): the header's Save, claimed so WebView2 does
  // nothing of its own and the legacy surface never sees it.
  it("Ctrl+S saves the project like the header's Save, and is claimed", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    const saved: number[] = [];
    store.setPort(
      fakePort({
        ...undoablePort(executed),
        save: (_sessionId, expectedRevision) => {
          saved.push(expectedRevision);
          return Promise.resolve({ sessionId: "ses-a", savedRevision: expectedRevision, projectFileId: "p" });
        },
      }),
    );
    await store.openStaged("cap one");
    const w = mount(EditorShell, { attachTo: document.body });
    const reached: string[] = [];
    const onWindowKeydown = (e: KeyboardEvent) => reached.push(e.key);
    window.addEventListener("keydown", onWindowKeydown);
    try {
      const event = keydown(w.get('[data-testid="editor-shell"]').element, { key: "s", ctrlKey: true });
      await flushPromises();

      expect(saved).toEqual([store.snapshot?.revision]);
      expect(executed).toEqual([]);
      expect(event.defaultPrevented).toBe(true);
      expect(reached).toEqual([]);
    } finally {
      window.removeEventListener("keydown", onWindowKeydown);
    }
  });

  // `shouldHandle`'s `menuOwnsKeys`: an open menu or dialog inside the shell
  // owns the keyboard. Pressing Delete or Ctrl+Z while a context menu has
  // focus must not act on the timeline selection behind it.
  it("a shortcut pressed inside an open menu or dialog is left to it", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    store.setPort(undoablePort(executed));
    await store.openStaged("cap one");
    const w = mount(EditorShell, {
      attachTo: document.body,
      slots: {
        timeline:
          '<div role="menu"><button data-testid="in-menu">Split</button></div>' +
          '<div role="dialog"><button data-testid="in-dialog">OK</button></div>',
      },
    });

    keydown(w.get('[data-testid="in-menu"]').element, { key: "z", ctrlKey: true });
    keydown(w.get('[data-testid="in-dialog"]').element, { key: "z", ctrlKey: true });
    await flushPromises();

    expect(executed).toEqual([]);
  });
});

describe("EditorShell — the clipboard across a session change (fix round 1)", () => {
  function capture(projectId: string, clipId: string): Project {
    const c: Clip = {
      id: clipId, asset_id: "src", track_id: "v1", name: clipId, start_ms: 0, in_ms: 2_000, out_ms: 3_000,
      fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false, x: 0, y: 0, w: 1, h: 1,
    };
    return project({
      id: projectId,
      // Every migrated project names its capture asset "src" -- which is
      // why Rust's asset check cannot catch a cross-project paste.
      assets: [{ id: "src", kind: "video", name: "capture", duration_ms: 60_000 }],
      tracks: [{ id: "v1", kind: "video", name: "Video", visible: true, locked: false, muted: false, solo: false, volume: 1 }],
      clips: [c],
    });
  }

  it("a clip copied in capture A cannot be pasted into capture B; in A it still can", async () => {
    const store = useEditorProjectStore();
    const executed: unknown[] = [];
    store.setPort(
      fakePort({
        openStaged: (base) => {
          const id = base === "cap A" ? "project-a" : "project-b";
          return Promise.resolve(
            openResult({
              snapshot: snapshot({ projectId: id, sessionId: `ses-${id}`, durationMs: 3_000 }),
              project: capture(id, base === "cap A" ? "clip-a" : "clip-b"),
              sourceBase: base,
            }),
          );
        },
        execute: (req) => {
          executed.push(req.command);
          return Promise.resolve({ snapshot: snapshot({ revision: 2 }), project: store.project as Project });
        },
      }),
    );
    const workspace = useEditorWorkspaceStore();
    const w = mount(EditorShell, { attachTo: document.body });
    const shell = () => w.get('[data-testid="editor-shell"]');

    await store.openStaged("cap A");
    workspace.select(["clip-a"]);
    await shell().trigger("keydown", { key: "c", ctrlKey: true });
    await shell().trigger("keydown", { key: "v", ctrlKey: true }); // positive control: same project
    await flushPromises();
    expect(executed).toEqual([expect.objectContaining({ kind: "pasteFragment", trackId: "v1" })]);

    executed.length = 0;
    await store.openStaged("cap B");
    await flushPromises();
    workspace.select(["clip-b"]);
    await shell().trigger("keydown", { key: "v", ctrlKey: true });
    await flushPromises();

    expect(executed).toEqual([]);
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

describe("EditorShell — guide progress storage", () => {
  // Task 55 (ONBOARDING.md § State and persistence): progress that cannot be
  // stored lasts for this session only, and the header SAYS so rather than
  // letting the next start silently forget where the user was.
  it("says Session only beside Help when guide progress cannot be read", async () => {
    const store = useEditorProjectStore();
    store.setPort(fakePort({ getGuideProgress: () => Promise.reject(new Error("locked")) }));
    const w = mount(EditorShell);
    await flushPromises();

    const note = w.get('[data-testid="editor-header-guide-session-only"]');
    expect(note.text()).toBe("Session only");
    expect(note.attributes("title")).toBe(
      "Guide progress cannot be stored on this device right now. It lasts until the editor closes; Help → Learning center can save it to a file.",
    );
  });

  it("shows nothing when guide progress was read", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        getGuideProgress: () =>
          Promise.resolve({
            contentRevision: 1,
            currentStepId: null,
            reviewed: [],
            explored: [],
            invitationDismissed: false,
            active: false,
            collapsed: false,
            completed: false,
            preferences: { dimming: true, motion: "system" },
          }),
      }),
    );
    const w = mount(EditorShell);
    await flushPromises();

    expect(w.find('[data-testid="editor-header-guide-session-only"]').exists()).toBe(false);
  });
});
