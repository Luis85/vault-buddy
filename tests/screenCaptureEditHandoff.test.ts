/**
 * The hand-off from the screen-capture domain into the tutorial editor
 * (tutorial-editor Task 15) — two properties that must both hold before a
 * user can trust the Edit affordance on a screen capture:
 *
 *  1. `screenCapture.lastStaged` (what makes the panel/buddy's Edit button
 *     appear at all — AGENTS.md's screen-capture section, `ScreenCaptureBar`)
 *     must not be set from a `stop_screen_capture` reply alone: finalize is
 *     unbounded, and a `stillSaving` reply means the capture has not
 *     actually landed as a staged file yet. Only `screen:stopped` may set it.
 *  2. Once the editor window opens that staged capture, the project it gets
 *     back is authoritative for which VAULT it targets (A01,
 *     `open_staged_resolves_the_vault_from_the_sidecar`, Rust-tested) — this
 *     window must display that, not `screenCapture.vaultId`, which is a
 *     DIFFERENT fact (the vault of the last capture the buddy/panel windows
 *     recorded, mirrored by a store the editor window never `init()`s —
 *     AGENTS.md's Frontend state section).
 */
import { mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => undefined),
}));
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import type { EditorPort } from "../src/editor/port";
import type { EditorOpenResult, EditorSnapshot, Project } from "../src/editorTypes";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useScreenCaptureStore } from "../src/stores/screenCapture";
import { DETAIL } from "./helpers/editorMount";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
});

afterEach(() => vi.restoreAllMocks());

describe("screenCapture store — the Edit handoff gate", () => {
  // A `stillSaving` reply means the bounded wait for `stop_screen_capture`
  // expired while finalize was still running — NOT that anything failed, and
  // NOT that the capture landed. Only `screen:stopped` (never fired here)
  // may populate `lastStaged`, which is what makes `ScreenCaptureBar` offer
  // Edit at all: exposing it off a `stillSaving` reply alone would open the
  // editor on a capture that is not actually staged yet.
  it("stillSaving does not expose Edit", async () => {
    mockIPC((cmd) => {
      if (cmd === "stop_screen_capture") return { stillSaving: true };
      return undefined;
    });
    const store = useScreenCaptureStore();

    await store.stop();

    expect(store.lastStaged).toBeNull();
  });
});

describe("EditorRoot — never reads a vault id from the screenCapture store", () => {
  function snapshot(overrides: Partial<EditorSnapshot> = {}): EditorSnapshot {
    return {
      sessionId: "ses-a",
      projectId: "project-a",
      revision: 1,
      persistedRevision: null,
      title: "Tutorial",
      durationMs: 6000,
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

  function openResult(overrides: Partial<EditorOpenResult> = {}): EditorOpenResult {
    return {
      snapshot: snapshot(),
      project: project(),
      workspace: {},
      missing: [],
      sourceBase: "cap one",
      recovered: false,
      ...overrides,
    };
  }

  it("shows the opened project's OWN sidecar vault, never the screenCapture store's", async () => {
    // A DIFFERENT vault in each store is the point (the "fixture flaw" rule):
    // if both happened to agree, reading the wrong one would render the
    // right text for the wrong reason.
    const screenCapture = useScreenCaptureStore();
    screenCapture.$patch({ vaultId: "vault-b" });

    const editorProject = useEditorProjectStore();
    editorProject.setPort(
      fakePort({
        openStaged: (base) =>
          Promise.resolve(openResult({ sourceBase: base, project: project({ destination: { vault: "vault-a", folder: "", dated: false } }) })),
      }),
    );

    mockIPC((cmd) => {
      if (cmd === "take_editor_request") return "cap one";
      if (cmd === "load_staged_capture") return DETAIL;
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();

    expect(w.get('[data-testid="editor-shell-vault"]').text()).toBe("vault-a");
    expect(w.text()).not.toContain("vault-b");
  });
});
