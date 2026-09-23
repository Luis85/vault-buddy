/**
 * `editorWorkspace` store (Task 18; P03, F-48/F-25/F-14). Every test drives
 * the store through an injected FAKE `EditorPort` (`setPort`), the
 * `editorProjectStore.test.ts` precedent — this store's job is deciding
 * WHEN to persist and WHAT to send, not decoding the wire (that is
 * `port.ts`/`decode.ts`, covered by `editorPort.test.ts`/
 * `editorDecode.test.ts`).
 */
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { EditorPort } from "../src/editor/port";
import type {
  EditorOpenResult,
  EditorProjection,
  EditorSnapshot,
  Project,
  Workspace,
} from "../src/editorTypes";
import * as logging from "../src/logging";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";

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

function clip(id: string, trackId = "t1"): Project["clips"][number] {
  return {
    id,
    asset_id: "a1",
    track_id: trackId,
    name: id,
    start_ms: 0,
    in_ms: 0,
    out_ms: 1000,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 1,
    volume: 1,
    muted: false,
    x: 0,
    y: 0,
    w: 1,
    h: 1,
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
    destination: { vault: "", folder: "", dated: false },
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

/** A minimal fake `EditorPort` — every method a test doesn't override
 * rejects loudly, so a test exercising one path never silently passes
 * through a code path it forgot to stub (the `editorProjectStore.test.ts`
 * precedent). */
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
    mediaUrl: unimplemented("mediaUrl"),
    importMedia: unimplemented("importMedia"),
    cancelJob: unimplemented("cancelJob"),
    getJobs: unimplemented("getJobs"),
    ...overrides,
  };
}

beforeEach(() => {
  setActivePinia(createPinia());
});

describe("editorWorkspace — persist debouncing", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("persist is debounced to one save per burst", async () => {
    vi.useFakeTimers();
    const saveCalls: { sessionId: string; workspace: Workspace }[] = [];
    const project = useEditorProjectStore();
    project.setPort(
      fakePort({ openStaged: () => Promise.resolve(openResult({ snapshot: snapshot() })) }),
    );
    await project.openStaged("cap one");

    const workspace = useEditorWorkspaceStore();
    workspace.setPort(
      fakePort({
        saveWorkspace: (sessionId, ws) => {
          saveCalls.push({ sessionId, workspace: ws });
          return Promise.resolve();
        },
      }),
    );
    await workspace.hydrate("ses-a");

    // A rapid burst of unrelated edits -- each one schedules a persist, but
    // none of them lets 750ms elapse before the next fires.
    workspace.toggleSnap();
    vi.advanceTimersByTime(200);
    workspace.setZoom(4);
    vi.advanceTimersByTime(200);
    workspace.select(["c1", "c2"]);

    // MUTATION CHECK (this task's brief): a `persist` that does not clear
    // the previous timer (or that saves synchronously on every mutator
    // call) makes this assertion fail -- either zero calls yet at this
    // point would be wrong for the opposite reason, or more than one call
    // would already have landed.
    expect(saveCalls).toHaveLength(0);

    vi.advanceTimersByTime(750);
    await vi.waitFor(() => expect(saveCalls).toHaveLength(1));

    expect(saveCalls[0].sessionId).toBe("ses-a");
    expect(saveCalls[0].workspace.snap).toBe(false);
    expect(saveCalls[0].workspace.timeline_zoom).toBe(4);
    expect(saveCalls[0].workspace.selection_clip_ids).toEqual(["c1", "c2"]);
  });
});

describe("editorWorkspace — selection pruning", () => {
  it("selection is pruned when a clip disappears", async () => {
    const projectStore = useEditorProjectStore();
    projectStore.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(
            openResult({
              snapshot: snapshot({ revision: 1 }),
              project: project({ clips: [clip("c1"), clip("c2")] }),
            }),
          ),
      }),
    );
    await projectStore.openStaged("cap one");

    const workspace = useEditorWorkspaceStore();
    workspace.setPort(fakePort({ saveWorkspace: () => Promise.resolve() }));
    workspace.select(["c1", "c2"]);
    expect(workspace.selectionClipIds).toEqual(["c1", "c2"]);

    // A command reply that installs a NEW projection (revision advances)
    // with "c2" deleted -- the same shape `execute()` installs via
    // `applyExecuteResult`.
    const reply: EditorProjection = {
      snapshot: snapshot({ revision: 2 }),
      project: project({ clips: [clip("c1")] }),
    };
    projectStore.setPort(fakePort({ execute: () => Promise.resolve(reply) }));
    await projectStore.execute({ kind: "deleteClips", clipIds: ["c2"], closeGap: false });

    // MUTATION CHECK (this task's brief): removing the watch on
    // `editorProject.snapshot.revision` leaves the stale "c2" behind --
    // this assertion is exactly what goes red.
    expect(workspace.selectionClipIds).toEqual(["c1"]);
  });

  it("does not touch a selection that still resolves after a projection install", async () => {
    const projectStore = useEditorProjectStore();
    projectStore.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(
            openResult({
              snapshot: snapshot({ revision: 1 }),
              project: project({ clips: [clip("c1"), clip("c2")] }),
            }),
          ),
      }),
    );
    await projectStore.openStaged("cap one");

    const workspace = useEditorWorkspaceStore();
    workspace.setPort(fakePort({ saveWorkspace: () => Promise.resolve() }));
    workspace.select(["c1"]);

    const reply: EditorProjection = {
      snapshot: snapshot({ revision: 2, title: "Renamed" }),
      project: project({ title: "Renamed", clips: [clip("c1"), clip("c2")] }),
    };
    projectStore.setPort(fakePort({ execute: () => Promise.resolve(reply) }));
    await projectStore.execute({ kind: "rename", title: "Renamed" });

    expect(workspace.selectionClipIds).toEqual(["c1"]);
  });
});

describe("editorWorkspace — monitor mute", () => {
  it("monitor mute is workspace state only (no editor_execute call when toggled)", async () => {
    const projectStore = useEditorProjectStore();
    projectStore.setPort(
      fakePort({ openStaged: () => Promise.resolve(openResult({ snapshot: snapshot() })) }),
    );
    await projectStore.openStaged("cap one");
    const executeSpy = vi.spyOn(projectStore, "execute");

    const workspace = useEditorWorkspaceStore();
    workspace.setPort(fakePort({ saveWorkspace: () => Promise.resolve() }));

    expect(workspace.monitorMuted).toBe(false);
    workspace.toggleMonitorMute();
    expect(workspace.monitorMuted).toBe(true);
    workspace.toggleMonitorMute();
    expect(workspace.monitorMuted).toBe(false);

    expect(executeSpy).not.toHaveBeenCalled();
  });
});

describe("editorWorkspace — playhead clamping", () => {
  it("playhead clamps to project duration", async () => {
    const projectStore = useEditorProjectStore();
    projectStore.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ snapshot: snapshot({ durationMs: 5_000 }) })),
      }),
    );
    await projectStore.openStaged("cap one");

    const workspace = useEditorWorkspaceStore();
    workspace.setPort(fakePort({ saveWorkspace: () => Promise.resolve() }));

    workspace.setPlayhead(9_000);
    expect(workspace.playheadMs).toBe(5_000);

    workspace.setPlayhead(-500);
    expect(workspace.playheadMs).toBe(0);

    workspace.setPlayhead(2_500);
    expect(workspace.playheadMs).toBe(2_500);
  });
});

describe("editorWorkspace — every mutator persists the field it names", () => {
  it("select/setSelected/setZoom/fit/toggleSnap/setDeleteMode/tabs/scroll/height/rate/panels/theme", async () => {
    vi.useFakeTimers();
    try {
      const saveCalls: Workspace[] = [];
      const project = useEditorProjectStore();
      project.setPort(
        fakePort({ openStaged: () => Promise.resolve(openResult({ snapshot: snapshot() })) }),
      );
      await project.openStaged("cap one");

      const workspace = useEditorWorkspaceStore();
      workspace.setPort(
        fakePort({
          saveWorkspace: (_sid, ws) => {
            saveCalls.push(ws);
            return Promise.resolve();
          },
        }),
      );
      await workspace.hydrate("ses-a");

      const PERSIST_DEBOUNCE_MS_FOR_TEST = 750;
      async function settle(): Promise<Workspace> {
        vi.advanceTimersByTime(PERSIST_DEBOUNCE_MS_FOR_TEST);
        await vi.waitFor(() => expect(saveCalls.length).toBeGreaterThan(0));
        return saveCalls[saveCalls.length - 1];
      }

      workspace.setSelected({ type: "clip", id: "c1" });
      expect((await settle()).selected).toEqual({ type: "clip", id: "c1" });
      saveCalls.length = 0;

      workspace.setZoom(0.001);
      expect((await settle()).timeline_zoom).toBe(0.1);
      saveCalls.length = 0;

      workspace.setZoom(3);
      workspace.fit();
      const afterFit = await settle();
      expect(afterFit.timeline_zoom).toBe(1);
      expect(afterFit.timeline_scroll_left).toBe(0);
      saveCalls.length = 0;

      workspace.toggleSnap();
      expect((await settle()).snap).toBe(false);
      saveCalls.length = 0;

      workspace.setDeleteMode("close");
      expect((await settle()).delete_mode).toBe("close");
      saveCalls.length = 0;

      workspace.setLibraryTab("media");
      expect((await settle()).library_tab).toBe("media");
      saveCalls.length = 0;

      workspace.setPropertyTab("layout");
      expect((await settle()).property_tab).toBe("layout");
      saveCalls.length = 0;

      workspace.setTimelineScroll(120, 40);
      const afterScroll = await settle();
      expect(afterScroll.timeline_scroll_left).toBe(120);
      expect(afterScroll.timeline_scroll_top).toBe(40);
      saveCalls.length = 0;

      workspace.setTimelineHeight(5);
      expect((await settle()).timeline_height).toBe(160);
      saveCalls.length = 0;

      workspace.setPlaybackRate(10);
      expect((await settle()).playback_rate).toBe(2.0);
      saveCalls.length = 0;

      workspace.toggleLibraryHidden();
      expect((await settle()).library_hidden).toBe(true);
      saveCalls.length = 0;

      workspace.togglePropertiesHidden();
      expect((await settle()).properties_hidden).toBe(true);
      saveCalls.length = 0;

      workspace.togglePropertiesOpen();
      expect((await settle()).properties_open).toBe(true);
      saveCalls.length = 0;

      workspace.toggleFocusPreview();
      expect((await settle()).focus_preview).toBe(true);
      saveCalls.length = 0;

      workspace.toggleCaptionSettingsOpen();
      expect((await settle()).caption_settings_open).toBe(true);
      saveCalls.length = 0;

      workspace.setTheme("light");
      expect((await settle()).theme).toBe("light");
      saveCalls.length = 0;

      workspace.toggleTheme();
      expect((await settle()).theme).toBe("dark");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("editorWorkspace — hydrate", () => {
  it("applies only the fields the saved workspace carries, keeping today's defaults for the rest", async () => {
    const workspace = useEditorWorkspaceStore();
    workspace.setPort(
      fakePort({
        getWorkspace: () =>
          Promise.resolve({ snap: false, theme: "light" } as Workspace),
      }),
    );

    await workspace.hydrate("ses-a");

    expect(workspace.snap).toBe(false);
    expect(workspace.theme).toBe("light");
    // Untouched by the reply -- still this store's own default.
    expect(workspace.timelineZoom).toBe(1);
  });

  it("a hydrate failure is logged, not thrown, and leaves defaults in place", async () => {
    const workspace = useEditorWorkspaceStore();
    workspace.setPort(
      fakePort({ getWorkspace: () => Promise.reject(new Error("boom")) }),
    );

    await expect(workspace.hydrate("ses-a")).resolves.toBeUndefined();
    expect(workspace.timelineZoom).toBe(1);
  });

  it("applies every one of the 19 fields a saved workspace can carry", async () => {
    const workspace = useEditorWorkspaceStore();
    const full: Workspace = {
      selection_clip_ids: ["c1", "c2"],
      selected: { type: "clip", id: "c1" },
      playhead_ms: 4_200,
      library_tab: "media",
      property_tab: "layout",
      timeline_zoom: 2.5,
      timeline_height: 320,
      timeline_scroll_left: 15,
      timeline_scroll_top: 7,
      snap: false,
      delete_mode: "close",
      monitor_muted: true,
      playback_rate: 1.5,
      library_hidden: true,
      properties_hidden: true,
      properties_open: true,
      focus_preview: true,
      caption_settings_open: true,
      theme: "light",
    };
    workspace.setPort(fakePort({ getWorkspace: () => Promise.resolve(full) }));

    await workspace.hydrate("ses-a");

    expect(workspace.selectionClipIds).toEqual(["c1", "c2"]);
    expect(workspace.selected).toEqual({ type: "clip", id: "c1" });
    expect(workspace.playheadMs).toBe(4_200);
    expect(workspace.libraryTab).toBe("media");
    expect(workspace.propertyTab).toBe("layout");
    expect(workspace.timelineZoom).toBe(2.5);
    expect(workspace.timelineHeight).toBe(320);
    expect(workspace.timelineScrollLeft).toBe(15);
    expect(workspace.timelineScrollTop).toBe(7);
    expect(workspace.snap).toBe(false);
    expect(workspace.deleteMode).toBe("close");
    expect(workspace.monitorMuted).toBe(true);
    expect(workspace.playbackRate).toBe(1.5);
    expect(workspace.libraryHidden).toBe(true);
    expect(workspace.propertiesHidden).toBe(true);
    expect(workspace.propertiesOpen).toBe(true);
    expect(workspace.focusPreview).toBe(true);
    expect(workspace.captionSettingsOpen).toBe(true);
    expect(workspace.theme).toBe("light");
  });

  it("a key present but explicitly undefined is skipped, not applied", async () => {
    const workspace = useEditorWorkspaceStore();
    workspace.setPort(
      fakePort({
        // A hand-editable-sidecar-shaped edge case: the KEY is present (so
        // `Object.keys` still visits it) but its value is `undefined` --
        // `applyWorkspace`'s own `continue` branch, not just "absent key".
        getWorkspace: () =>
          Promise.resolve({ snap: undefined, theme: "light" } as unknown as Workspace),
      }),
    );

    await workspace.hydrate("ses-a");

    expect(workspace.snap).toBe(true); // untouched default, not overwritten to undefined
    expect(workspace.theme).toBe("light");
  });

  it("a stale hydrate reply (superseded by a second hydrate) never overwrites the newer one", async () => {
    const workspace = useEditorWorkspaceStore();
    let resolveFirst!: (ws: Workspace) => void;
    const first = new Promise<Workspace>((res) => (resolveFirst = res));
    let callCount = 0;
    workspace.setPort(
      fakePort({
        getWorkspace: () => {
          callCount += 1;
          return callCount === 1 ? first : Promise.resolve({ theme: "light" } as Workspace);
        },
      }),
    );

    const firstHydrate = workspace.hydrate("ses-a");
    const secondHydrate = workspace.hydrate("ses-b");
    await secondHydrate;
    expect(workspace.theme).toBe("light");

    // The stale first reply, resolving AFTER the second already installed,
    // must not stomp on it.
    resolveFirst({ theme: "dark" } as Workspace);
    await firstHydrate;
    expect(workspace.theme).toBe("light");
  });

  // Task 18 fix round 1, finding 3: the editor webview is reused across
  // captures, so opening project B after project A must not leak A's
  // playhead/zoom/tabs/scroll/selection into B when B's workspace.json is
  // missing or partial -- otherwise B's first edit persists A's leftover
  // values into B's own file.
  it("hydrating a new session resets to defaults before applying its own (possibly empty) workspace", async () => {
    const workspace = useEditorWorkspaceStore();
    workspace.setPort(
      fakePort({
        getWorkspace: (id) =>
          Promise.resolve(
            id === "ses-a"
              ? ({
                  selection_clip_ids: ["c1", "c2"],
                  playhead_ms: 8_000,
                  timeline_zoom: 4,
                  library_tab: "media",
                  snap: false,
                  theme: "light",
                } as Workspace)
              : ({} as Workspace),
          ),
      }),
    );

    await workspace.hydrate("ses-a");
    expect(workspace.selectionClipIds).toEqual(["c1", "c2"]);
    expect(workspace.playheadMs).toBe(8_000);
    expect(workspace.timelineZoom).toBe(4);
    expect(workspace.libraryTab).toBe("media");
    expect(workspace.snap).toBe(false);
    expect(workspace.theme).toBe("light");

    // MUTATION CHECK: dropping the reset-to-defaults step at the top of
    // `hydrate` leaves every one of these reading session A's leftover
    // values instead of the fresh defaults `createFields()` mints.
    await workspace.hydrate("ses-b");
    expect(workspace.selectionClipIds).toEqual([]);
    expect(workspace.playheadMs).toBe(0);
    expect(workspace.timelineZoom).toBe(1);
    expect(workspace.libraryTab).toBeNull();
    expect(workspace.snap).toBe(true);
  });

  it("a stale hydrate FAILURE (superseded by a second hydrate) is silently dropped", async () => {
    const workspace = useEditorWorkspaceStore();
    let rejectFirst!: (e: unknown) => void;
    const first = new Promise<Workspace>((_res, rej) => (rejectFirst = rej));
    let callCount = 0;
    workspace.setPort(
      fakePort({
        getWorkspace: () => {
          callCount += 1;
          return callCount === 1 ? first : Promise.resolve({ theme: "light" } as Workspace);
        },
      }),
    );

    const firstHydrate = workspace.hydrate("ses-a").catch(() => {
      throw new Error("hydrate must never itself throw/reject");
    });
    const secondHydrate = workspace.hydrate("ses-b");
    await secondHydrate;
    expect(workspace.theme).toBe("light");

    rejectFirst(new Error("stale failure"));
    await expect(firstHydrate).resolves.toBeUndefined();
    expect(workspace.theme).toBe("light");
  });
});

describe("editorWorkspace — persist without a session", () => {
  it("never calls saveWorkspace before hydrate() has set a session", async () => {
    vi.useFakeTimers();
    try {
      const workspace = useEditorWorkspaceStore();
      const saveWorkspace = vi.fn(() => Promise.resolve());
      workspace.setPort(fakePort({ saveWorkspace }));

      workspace.toggleSnap();
      vi.advanceTimersByTime(750);
      await Promise.resolve();

      expect(saveWorkspace).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("a save failure is logged, never thrown or left as an unhandled rejection", async () => {
    vi.useFakeTimers();
    const warnSpy = vi.spyOn(logging, "logWarning").mockImplementation(() => {});
    try {
      const project = useEditorProjectStore();
      project.setPort(
        fakePort({ openStaged: () => Promise.resolve(openResult({ snapshot: snapshot() })) }),
      );
      await project.openStaged("cap one");

      const workspace = useEditorWorkspaceStore();
      workspace.setPort(fakePort({ saveWorkspace: () => Promise.reject(new Error("disk full")) }));
      await workspace.hydrate("ses-a");

      workspace.toggleSnap();
      vi.advanceTimersByTime(750);
      // Let the rejected promise's `.catch` microtask run before the test
      // ends -- an uncaught rejection here would fail the whole run.
      await Promise.resolve();
      await Promise.resolve();

      expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining("failed to persist workspace"));
    } finally {
      warnSpy.mockRestore();
      vi.useRealTimers();
    }
  });
});
