/**
 * The editor window's root: draining the Rust-owned stash, opening a
 * tutorial-editor session over what it held (a staged capture or a
 * project), gating the shell on the store's own reply, hydrating the
 * workspace, the project file import — and, since Task 59, the window's
 * own words when nothing is open and the project menu's Discard project.
 *
 * Task 59 retired the phase-4 editor this file used to test at length (its
 * strip, its preview clocks, its export bar); the tutorial editor's own
 * suites own those surfaces now.
 */
import { mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Captured so a test can drive `editor:open` the way Rust does — from the
// same main-thread closure that shows the editor window, immediately before
// the show. The editor window is hidden and reused, never destroyed, so its
// webview mounts exactly ONCE per process and the event is the only thing
// that makes an already-mounted editor re-read the stash.
const listeners: Record<string, (e?: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e?: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));

// `mockIPC` installs `__TAURI_INTERNALS__`, which stops `logging.ts` being a
// no-op and routes it through the log plugin — harmless, but it would put
// `plugin:log|log` calls in the recorded array the assertions read.
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { EditorPortError } from "../src/editor/port";
import type { EditorCommand, EditorOpenResult, EditorSnapshot, Project } from "../src/editorTypes";
import { logWarning } from "../src/logging";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { open } from "./helpers/editorMount";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  for (const key of Object.keys(listeners)) delete listeners[key];
  mockConvertFileSrc("windows");
  // `EditorRoot` calls `useEditorProjectStore()`, which needs an active
  // Pinia at mount time — a fresh instance per test.
  setActivePinia(createPinia());
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("EditorRoot", () => {
  function snapshotFixture(overrides: Partial<EditorSnapshot> = {}): EditorSnapshot {
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

  function projectFixture(overrides: Partial<Project> = {}): Project {
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

  function openResultFixture(overrides: Partial<EditorOpenResult> = {}): EditorOpenResult {
    return {
      snapshot: snapshotFixture(),
      project: projectFixture(),
      workspace: {},
      missing: [],
      sourceBase: "cap one",
      recovered: false,
      ...overrides,
    };
  }


  // ---- The window's own words when no session is open (Task 59). The
  // retired phase-4 surface used to say "No capture open" or show its load
  // error; without it a failed or empty open would be a blank window. ----

  it("says so plainly when no capture was requested", async () => {
    const w = await open([null]);
    expect(w.get('[data-testid="editor-empty"]').text()).toContain("No capture open");
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(false);
  });

  // The drain itself failing is not the same as an empty stash, but it has
  // the same remedy: say nothing is open and leave a log line behind, rather
  // than throwing out of `onMounted` into an empty window.
  it("logs and stays empty when the stash cannot be drained", async () => {
    mockIPC((cmd) => {
      if (cmd === "take_editor_request") throw new Error("ipc is down");
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.get('[data-testid="editor-empty"]').text()).toContain("No capture open");
    expect(vi.mocked(logWarning)).toHaveBeenCalledWith(
      expect.stringContaining("take_editor_request failed"),
    );
  });

  // A refused open says WHY, in the role wording Rust gives it — never the
  // `<path:#hash8>` handle its log line carries (Task 58's redaction).
  it("names a failed open instead of showing a blank window", async () => {
    useEditorProjectStore().setPort(
      fakeEditorPort({
        openStaged: () =>
          Promise.reject(
            new EditorPortError({
              code: "sourceMissing",
              message: "Cannot read the sidecar <path:#1a2b3c4d>: The system cannot find the file.",
              retryable: false,
              operationId: "op-1",
            }),
          ),
      }),
    );
    const w = await open();
    const shown = w.get('[data-testid="editor-open-failed"]').text();
    expect(shown).toContain("could not be opened");
    expect(shown).toContain("Cannot read the sidecar: The system cannot find the file.");
    expect(shown).not.toContain("<path:#");
    expect(w.find('[data-testid="editor-empty"]').exists()).toBe(false);
  });

  // The stash alone is not enough: this webview mounts once per process, so
  // a second `open_capture_editor` would leave the FIRST capture on screen
  // forever without the event (`editor_commands.rs`'s module doc).
  it("re-reads the stash on editor:open, not only on mount", async () => {
    const opened: string[] = [];
    useEditorProjectStore().setPort(
      fakeEditorPort({
        openStaged: (base) => {
          opened.push(base);
          return Promise.resolve(
            openResultFixture({
              sourceBase: base,
              snapshot: snapshotFixture({
                sessionId: `ses-${opened.length}`,
                title: base === "cap one" ? "Capture A" : "Capture B",
              }),
            }),
          );
        },
      }),
    );
    const w = await open(["cap one", "cap two"]);
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("Capture A");
    listeners["editor:open"]();
    await flushPromises();
    expect(opened).toEqual(["cap one", "cap two"]);
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("Capture B");
  });

  // A drain that comes back empty means "nothing new", never "close what you
  // are showing" — blanking a live edit on a spurious event would be worse
  // than doing nothing.
  it("keeps the capture on screen when a re-open drains an empty stash", async () => {
    useEditorProjectStore().setPort(
      fakeEditorPort({
        openStaged: (base) => Promise.resolve(openResultFixture({ sourceBase: base })),
      }),
    );
    const w = await open(["cap one"]);
    listeners["editor:open"]();
    await flushPromises();
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);
    expect(w.find('[data-testid="editor-empty"]').exists()).toBe(false);
  });

  // Fix round 1: the shell's duration must use the shared
  // `src/utils/formatDuration.ts` (h:mm:ss, negative-clamped), not a local
  // mm:ss-only copy that overflows past an hour — 2h read "120:00" instead
  // of "2:00:00" before this fix.
  it("renders the shell's duration past an hour as h:mm:ss, not overflowed mm:ss", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: (base) =>
          Promise.resolve(
            openResultFixture({
              sourceBase: base,
              snapshot: snapshotFixture({ durationMs: 7_200_000 }), // 2h
            }),
          ),
      }),
    );
    const w = await open();

    expect(w.get('[data-testid="editor-shell-duration"]').text()).toBe("2:00:00");
  });

  it("opens the stashed base through editor_open_staged", async () => {
    const calls: string[] = [];
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => {
          calls.push(base);
          return Promise.resolve(openResultFixture({ sourceBase: base }));
        },
      }),
    );
    await open();

    expect(calls).toEqual(["cap one"]);
    expect(store.sessionId).toBe("ses-a");
  });

  // Task 18 fix round 1 (controller ruling): a successful open must hydrate
  // `editorWorkspace` with the new session id, or `editor_get_workspace`/
  // `editor_save_workspace` never get a production caller at all.
  it("hydrates the workspace store with the new session id after a successful open", async () => {
    const project = useEditorProjectStore();
    project.setPort(
      fakeEditorPort({
        openStaged: (base) => Promise.resolve(openResultFixture({ sourceBase: base })),
      }),
    );
    const workspace = useEditorWorkspaceStore();
    const hydrateCalls: string[] = [];
    workspace.setPort(
      fakeEditorPort({
        getWorkspace: (id) => {
          hydrateCalls.push(id);
          return Promise.resolve({});
        },
      }),
    );

    await open();

    expect(hydrateCalls).toEqual(["ses-a"]);
  });

  it("does not hydrate the workspace when the open fails", async () => {
    const project = useEditorProjectStore();
    project.setPort(
      fakeEditorPort({
        openStaged: () =>
          Promise.reject(
            new EditorPortError({
              code: "sourceMissing",
              message: "gone",
              retryable: false,
              operationId: "op-1",
            }),
          ),
      }),
    );
    const workspace = useEditorWorkspaceStore();
    const hydrateCalls: string[] = [];
    workspace.setPort(
      fakeEditorPort({
        getWorkspace: (id) => {
          hydrateCalls.push(id);
          return Promise.resolve({});
        },
      }),
    );

    await open();

    expect(hydrateCalls).toEqual([]);
  });

  // Task 18 fix round 2 (controller ruling): `editorProject.openStaged`
  // SHORT-CIRCUITS for a duplicate open of the capture already showing
  // (`editorProject.ts`'s own same-base guard) without touching
  // `sessionId`, so re-hydrating on EVERY resolved `openStaged` call —
  // rather than only on a genuinely NEW session — re-fetches the (up to
  // 750ms stale) persisted workspace and resets fields to defaults first,
  // silently discarding a change made since the last debounce flush.
  it("a duplicate editor:open for the same base does not re-hydrate or discard an unpersisted change", async () => {
    const project = useEditorProjectStore();
    project.setPort(
      fakeEditorPort({
        openStaged: (base) => Promise.resolve(openResultFixture({ sourceBase: base })),
      }),
    );
    const workspace = useEditorWorkspaceStore();
    const hydrateCalls: string[] = [];
    workspace.setPort(
      fakeEditorPort({
        getWorkspace: (id) => {
          hydrateCalls.push(id);
          return Promise.resolve({});
        },
        // The `setPlayhead` below arms the store's 750ms persist debounce,
        // which outlives this test: unstubbed, it threw "saveWorkspace not
        // stubbed" into whichever later test was running when it fired —
        // an unhandled error that surfaced only when the file ran slowly
        // enough (Task 27's heavier editor mount made that common).
        saveWorkspace: () => Promise.resolve(),
      }),
    );

    // `open()`'s default queue is `["cap one"]`; a second `editor:open`
    // with nothing further queued still resolves to "cap one" here,
    // modelling Rust re-stashing the same base for a re-Edit click on the
    // capture already open (the "a second editor:open..." test's own
    // precedent, immediately below).
    await open(["cap one", "cap one"]);
    // MUTATION CHECK: without the fix, the first mount-time open already
    // hydrates once here — this assertion is the baseline, not yet the red
    // one.
    expect(hydrateCalls).toEqual(["ses-a"]);

    // An unpersisted, in-flight view-state change -- the user moved the
    // playhead -- that has not yet reached its 750ms debounce flush.
    workspace.setPlayhead(4_000);
    expect(workspace.playheadMs).toBe(4_000);

    listeners["editor:open"]();
    await flushPromises();

    // MUTATION CHECK: dropping the "only hydrate a genuinely NEW session"
    // guard makes BOTH of these fail -- `hydrateCalls` gains a second
    // "ses-a" entry, and the re-hydrate's `applyDefaults` resets
    // `playheadMs` back to 0 before re-applying the stale persisted `{}`.
    expect(hydrateCalls).toEqual(["ses-a"]);
    expect(workspace.playheadMs).toBe(4_000);
  });

  it("opening a genuinely different base still hydrates the workspace", async () => {
    const project = useEditorProjectStore();
    project.setPort(
      fakeEditorPort({
        openStaged: (base) =>
          Promise.resolve(
            openResultFixture({
              sourceBase: base,
              snapshot: snapshotFixture({ sessionId: base === "cap one" ? "ses-a" : "ses-b" }),
            }),
          ),
      }),
    );
    const workspace = useEditorWorkspaceStore();
    const hydrateCalls: string[] = [];
    workspace.setPort(
      fakeEditorPort({
        getWorkspace: (id) => {
          hydrateCalls.push(id);
          return Promise.resolve({});
        },
      }),
    );

    await open(["cap one", "cap two"]);
    listeners["editor:open"]();
    await flushPromises();

    expect(hydrateCalls).toEqual(["ses-a", "ses-b"]);
  });

  // A second `editor:open` for the SAME base while that session is open must
  // not mint a second one — the store-level guard Task 15 adds to
  // `openStaged` (`editorProjectStore.test.ts` pins the guard itself; this
  // is the scenario it exists for: a duplicate Edit click, or a re-emitted
  // `editor:open`, for the capture already showing).
  it("a second editor:open for the same base reuses the session", async () => {
    const calls: string[] = [];
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => {
          calls.push(base);
          return Promise.resolve(openResultFixture({ sourceBase: base }));
        },
      }),
    );
    // `open()`'s default request queue is `["cap one"]`; a second
    // `editor:open` with nothing further queued still resolves to "cap one"
    // here because Rust re-stashes the SAME base for a re-Edit click on a
    // capture that is already open — modelled by handing `open()` the base
    // twice.
    const w = await open(["cap one", "cap one"]);
    listeners["editor:open"]();
    await flushPromises();

    // MUTATION CHECK (this task's brief): drop the same-base short-circuit
    // in `editorProject.openStaged` and this reads 2, red for the reason
    // this test names — see `editorProjectStore.test.ts` for the guard's
    // own isolated pin.
    expect(calls).toEqual(["cap one"]);
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("Tutorial");
  });

  // ---- Fix round 1: a failed session open must not leave the shell
  // showing the PREVIOUS capture's identity under the new one. ----

  it("hides the shell rather than showing a stale capture's title or vault when a later open fails", async () => {
    const store = useEditorProjectStore();
    let openCalls = 0;
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => {
          openCalls += 1;
          if (openCalls === 1) {
            return Promise.resolve(
              openResultFixture({
                sourceBase: base,
                snapshot: snapshotFixture({ title: "Capture A" }),
                project: projectFixture({ destination: { vault: "vault-a", folder: "", dated: false } }),
              }),
            );
          }
          return Promise.reject(
            new EditorPortError({
              code: "sourceMissing",
              message: "That capture's video file is missing.",
              retryable: false,
              operationId: "op-1",
            }),
          );
        },
      }),
    );
    const w = await open(["cap one", "cap two"]);
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("Capture A");
    expect(w.get('[data-testid="editor-shell-vault"]').text()).toBe("vault-a");

    listeners["editor:open"]();
    await flushPromises();

    // The shell must not go on attributing capture A's title and vault to
    // "cap two" — whose own open FAILED — and the window says so instead.
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(false);
    expect(w.get('[data-testid="editor-open-failed"]').text()).toContain("video file is missing");
    expect(w.text()).not.toContain("Capture A");
    expect(w.text()).not.toContain("vault-a");
    expect(vi.mocked(logWarning)).toHaveBeenCalledWith(
      expect.stringContaining("editor_open_staged"),
    );
  });

  // ---- Task 37 Part B, fix round 1 (review Critical #1): a Resume click
  // from the panel opened a real session with nothing on screen, because
  // the shell's gate only understood a staged open. It now compares the
  // store's own reply with whichever KIND of request was drained. ----

  it("renders the shell for a project-kind drain, with the project's own snapshot", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openProject: (id) =>
          Promise.resolve(
            openResultFixture({
              sourceBase: null,
              snapshot: snapshotFixture({ projectId: id, title: "My Tutorial" }),
              project: projectFixture({
                id,
                destination: { vault: "vault-a", folder: "", dated: false },
              }),
            }),
          ),
      }),
    );
    mockIPC((cmd) => {
      if (cmd === "take_editor_request") return { kind: "project", value: "proj1" };
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();

    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("My Tutorial");
    expect(w.get('[data-testid="editor-shell-vault"]').text()).toBe("vault-a");
    // The empty-window line must not show beside a real, open session.
    expect(w.find('[data-testid="editor-empty"]').exists()).toBe(false);
  });

  it("hides the shell when a project-kind open fails, exactly like a failed staged open", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openProject: () =>
          Promise.reject(
            new EditorPortError({
              code: "invalidProject",
              message: "That project could not be opened.",
              retryable: false,
              operationId: "op-1",
            }),
          ),
      }),
    );
    mockIPC((cmd) => {
      if (cmd === "take_editor_request") return { kind: "project", value: "proj1" };
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();

    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(false);
    expect(vi.mocked(logWarning)).toHaveBeenCalledWith(
      expect.stringContaining("editor_open_project"),
    );
  });

  // ---- Task 39: "Open a project file" from the header's Save menu. The
  // shell's gate names the project this root opened, so the import must set
  // that id in the SAME tick the store installs the imported session — or
  // the shell (and the menu that asked) vanishes. ----

  function importPort(importPackage: () => Promise<EditorOpenResult | null>, reads: string[] = []) {
    return fakeEditorPort({
      openProject: (id) =>
        Promise.resolve(
          openResultFixture({
            sourceBase: null,
            snapshot: snapshotFixture({ projectId: id, title: "First" }),
            project: projectFixture({ id }),
          }),
        ),
      importPackage,
      getWorkspace: (sessionId) => {
        reads.push(sessionId);
        return Promise.resolve({});
      },
      listProjects: () => Promise.resolve([]),
    });
  }

  async function openFromMenu() {
    mockIPC((cmd) => (cmd === "take_editor_request" ? { kind: "project", value: "proj1" } : undefined));
    const w = mount(EditorRoot, { attachTo: document.body });
    await flushPromises();
    await w.get('[data-testid="editor-header-save-menu-toggle"]').trigger("click");
    await w.get('[data-testid="editor-header-menu-open"]').trigger("click");
    await flushPromises();
    return w;
  }

  it("opens an imported project file into the shell and hydrates its new session", async () => {
    const store = useEditorProjectStore();
    const reads: string[] = [];
    const imported = openResultFixture({
      sourceBase: null,
      snapshot: snapshotFixture({ sessionId: "ses-b", projectId: "imported1", title: "Imported" }),
      project: projectFixture({ id: "imported1", title: "Imported" }),
    });
    const port = importPort(() => Promise.resolve(imported), reads);
    store.setPort(port);
    useEditorWorkspaceStore().setPort(port);
    const w = await openFromMenu();

    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("Imported");
    expect(store.snapshot?.projectId).toBe("imported1");
    expect(reads).toEqual(["ses-a", "ses-b"]);
  });

  it("a refused or cancelled project file leaves the open project exactly as it was", async () => {
    const store = useEditorProjectStore();
    const message = 'package entry "media/src.mp4" does not match its manifest';
    store.setPort(
      importPort(() =>
        Promise.reject(
          new EditorPortError({ code: "invalidProject", message, retryable: false, operationId: "op-1" }),
        ),
      ),
    );
    const w = await openFromMenu();
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("First");
    expect(store.snapshot?.projectId).toBe("proj1");
    expect(w.text()).toContain(message);

    store.setPort(importPort(() => Promise.resolve(null)));
    await w.get('[data-testid="editor-header-save-menu-toggle"]').trigger("click");
    await w.get('[data-testid="editor-header-menu-open"]').trigger("click");
    await flushPromises();
    expect(store.snapshot?.projectId).toBe("proj1");
  });

  // The paired negative for the fix above: an ordinary STAGED open renders
  // the shell through the staged arm of the same gate.
  it("leaves an ordinary staged open's rendering unchanged", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: (base) =>
          Promise.resolve(
            openResultFixture({
              sourceBase: base,
              project: projectFixture({ destination: { vault: "vault-a", folder: "", dated: false } }),
            }),
          ),
      }),
    );
    const w = await open();

    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("Tutorial");
    expect(w.get('[data-testid="editor-shell-vault"]').text()).toBe("vault-a");
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);
    expect(w.find('[data-testid="editor-empty"]').exists()).toBe(false);
  });

  // ---- Task 59: Discard project. The retired phase-4 editor's Discard was
  // the only way to discard a tutorial project (and so to unpin its staged
  // capture, which the panel then lets the user discard). The project menu
  // carries it now: confirm-gated, `discardProject` (ADR invariant 6: the
  // RECORDING is never deleted by it), then the window hides. ----

  function discardPort(closeSession: (id: string, disposition: string) => Promise<void>) {
    const hideWindow = vi.fn(async () => {});
    const port = fakeEditorPort({
      openStaged: (base) => Promise.resolve(openResultFixture({ sourceBase: base })),
      closeSession,
      hideWindow,
      getJobs: async () => [],
    });
    useEditorProjectStore().setPort(port);
    return hideWindow;
  }

  async function askToDiscard() {
    const w = await open();
    await w.get('[data-testid="editor-header-save-menu-toggle"]').trigger("click");
    await w.get('[data-testid="editor-header-menu-discard"]').trigger("click");
    await flushPromises();
    return w;
  }

  it("Discard project asks first, then discards the project and hides the window", async () => {
    const closes: string[] = [];
    const hideWindow = discardPort(async (id, disposition) => {
      closes.push(`${id}:${disposition}`);
    });
    const w = await askToDiscard();
    const dialog = w.get('[data-testid="discard-project-dialog"]');
    expect(dialog.text()).toContain("recording stays");
    // Nothing is discarded by opening the confirm.
    expect(closes).toEqual([]);

    await w.get('[data-testid="discard-project-confirm"]').trigger("click");
    await flushPromises();
    expect(closes).toEqual(["ses-a:discardProject"]);
    expect(hideWindow).toHaveBeenCalledTimes(1);
    expect(closes.length).toBe(1);
  });

  it("Cancel keeps the project", async () => {
    const closes: string[] = [];
    discardPort(async (id, disposition) => {
      closes.push(`${id}:${disposition}`);
    });
    const w = await askToDiscard();
    await w.get('[data-testid="discard-project-cancel"]').trigger("click");
    await flushPromises();
    expect(closes).toEqual([]);
    expect(w.find('[data-testid="discard-project-dialog"]').exists()).toBe(false);
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);
  });

  // A refused discard (the store's owned-file checks, a locked file) says
  // why in role wording — never the `<path:#hash8>` handle — and does not
  // hide the window on a failure the user has to read.
  it("a refused discard says why without a redaction handle and keeps the window", async () => {
    const hideWindow = discardPort(async () => {
      throw new EditorPortError({
        code: "internal",
        message: "Could not remove the project folder <path:#1a2b3c4d>: Access is denied.",
        retryable: true,
        operationId: "op-3",
      });
    });
    const w = await askToDiscard();
    await w.get('[data-testid="discard-project-confirm"]').trigger("click");
    await flushPromises();
    expect(hideWindow).not.toHaveBeenCalled();
    const alert = w.get('[data-testid="discard-project-dialog"] [role="alert"]').text();
    expect(alert).toContain("Could not remove the project folder: Access is denied.");
    expect(alert).not.toContain("<path:#");
  });

  // Task 33 fix round 1 (review finding, Important #1): `TitlesLibrary.vue`
  // was fully built and tested in isolation but never reachable from the
  // running editor — `EditorRoot`'s `library` slot rendered only
  // `MediaLibrary`. `LibraryPanel.vue` now fills that slot with a Media/
  // Titles tablist; this test proves the switch, and the insert it gates,
  // work end to end through the REAL root, not just `TitlesLibrary.vue`
  // mounted standalone (`editorTitles.test.ts`'s own job).
  it("reaches Titles from the editor root's library panel and inserts a card through editorProject.execute", async () => {
    const executed: EditorCommand[] = [];
    const store = useEditorProjectStore();
    const withTrack = projectFixture({
      tracks: [
        { id: "v1", kind: "video", name: "v1", visible: true, locked: false, muted: false, solo: false, volume: 1 },
      ],
    });
    store.setPort(
      fakeEditorPort({
        openStaged: (base) =>
          Promise.resolve(openResultFixture({ sourceBase: base, project: withTrack })),
        execute: (req) => {
          executed.push(req.command);
          return Promise.resolve({
            snapshot: snapshotFixture({ revision: 2 }),
            project: withTrack,
          });
        },
      }),
    );
    const w = await open();
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);

    // MediaLibrary is the default tab; Titles is not shown until selected.
    expect(w.find('[data-testid="titles-library"]').exists()).toBe(false);
    await w.get('[data-testid="library-tab-titles"]').trigger("click");
    expect(w.get('[data-testid="library-tab-titles"]').attributes("aria-selected")).toBe("true");
    expect(w.find('[data-testid="titles-library"]').exists()).toBe(true);

    await w.get('[data-testid="titles-add-chapter"]').trigger("click");
    expect(executed).toEqual([
      expect.objectContaining({ kind: "addCard", preset: "chapter", trackId: "v1" }),
    ]);
  });
});
