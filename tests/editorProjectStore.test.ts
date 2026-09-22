/**
 * `editorProject` store (Task 14; P03, F-13/F-40/F-44). Every test drives
 * the store through an injected FAKE `EditorPort` (`setPort`) — never
 * `mockIPC` — because this store's whole job is deciding which ALREADY-
 * DECODED Rust replies to install, not decoding the wire itself (that's
 * `port.ts`/`decode.ts`, Task 13's job, covered by `editorPort.test.ts`
 * and `editorDecode.test.ts`).
 *
 * Fixtures are deliberately asymmetric per test (the global "fixture flaw"
 * rule): each test varies exactly the ONE thing its guard checks, so a
 * mutation that deletes a single guard reddens exactly the test that names
 * it and nothing else.
 */
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it } from "vitest";

import type { EditorPort } from "../src/editor/port";
import { EditorPortError } from "../src/editor/port";
import type {
  EditorCommand,
  EditorError,
  EditorOpenResult,
  EditorProjection,
  EditorSnapshot,
  Project,
  SaveReceipt,
} from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";

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

function editorError(overrides: Partial<EditorError> = {}): EditorError {
  return {
    code: "internal",
    message: "boom",
    retryable: false,
    operationId: "op-1",
    ...overrides,
  };
}

/** A minimal fake `EditorPort` — every method a test doesn't override
 * rejects loudly, so a test exercising one path can never accidentally
 * pass through a code path it forgot to stub. */
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
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => setActivePinia(createPinia()));

describe("editorProject store", () => {
  it("a result from a previous session is ignored", async () => {
    const openATask = deferred<EditorOpenResult>();
    const executeTask = deferred<EditorProjection>();
    const store = useEditorProjectStore();
    // Both opens resolve to the SAME sessionId on purpose: if this fixture
    // varied sessionId too, the sessionId guard alone would already reject
    // the stale reply below, and removing the generation comparison
    // (the mutation this test exists to catch) would stay green for the
    // wrong reason.
    store.setPort(
      fakePort({
        openStaged: (base) =>
          base === "A"
            ? openATask.promise
            : Promise.resolve(
                openResult({ snapshot: snapshot({ sessionId: "ses-shared", revision: 5 }) }),
              ),
        execute: () => executeTask.promise,
      }),
    );

    const openingA = store.openStaged("A");
    openATask.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-shared", revision: 1 }) }));
    await openingA;

    // Fire a slow edit against session A...
    const executing = store.execute({ kind: "rename", title: "From A" });
    // ...then open B before it resolves. This bumps generation and moves
    // the store to a revision (5) that A's reply below will still exceed —
    // so revision-monotonicity alone would NOT reject it; only generation
    // can.
    await store.openStaged("B");
    expect(store.snapshot?.revision).toBe(5);

    executeTask.resolve({
      snapshot: snapshot({ sessionId: "ses-shared", revision: 6, title: "From A" }),
      project: project({ title: "From A" }),
    });
    await executing;

    expect(store.snapshot?.revision).toBe(5);
    expect(store.project?.title).not.toBe("From A");
  });

  it("a non-increasing revision is ignored", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ revision: 3 }) })),
        execute: () =>
          Promise.resolve({
            snapshot: snapshot({ revision: 3, title: "No progress" }),
            project: project({ title: "No progress" }),
          }),
      }),
    );
    await store.openStaged("A");
    await store.execute({ kind: "undo" });

    expect(store.snapshot?.revision).toBe(3);
    expect(store.project?.title).not.toBe("No progress");
  });

  it("revisionConflict refetches and keeps the intent for retry", async () => {
    const store = useEditorProjectStore();
    const command: EditorCommand = { kind: "undo" };
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ revision: 4 }) })),
        execute: () =>
          Promise.reject(
            new EditorPortError(
              editorError({ code: "revisionConflict", retryable: true, message: "stale revision" }),
            ),
          ),
        getSnapshot: () =>
          Promise.resolve({
            snapshot: snapshot({ revision: 9, title: "Fresh" }),
            project: project({ title: "Fresh" }),
          }),
      }),
    );
    await store.openStaged("A");
    await store.execute(command);

    // Never re-sent automatically — the same command object is parked for
    // an explicit caller-driven retry.
    expect(store.conflictIntent).toEqual(command);
    expect(store.snapshot?.revision).toBe(9);
    expect(store.project?.title).toBe("Fresh");
  });

  it("a stale save receipt does not clear dirty", async () => {
    // A18: a save targeting revision R resolves after a LATER revision is
    // already current — the receipt still records that R reached disk, but
    // the later revision must stay dirty.
    const saveTask = deferred<SaveReceipt>();
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ snapshot: snapshot({ revision: 2, persistedRevision: null }) })),
        save: () => saveTask.promise,
        execute: () =>
          Promise.resolve({
            snapshot: snapshot({ revision: 3, persistedRevision: null, title: "Edited" }),
            project: project({ title: "Edited" }),
          }),
      }),
    );
    await store.openStaged("A");

    const saving = store.save();
    await store.execute({ kind: "undo" });
    expect(store.snapshot?.revision).toBe(3);

    saveTask.resolve({ sessionId: "ses-a", savedRevision: 2, projectFileId: "project-a" });
    await saving;

    expect(store.snapshot?.persistedRevision).toBe(2);
    expect(store.dirty).toBe(true);
  });

  it("a receipt from another session is ignored", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-a", revision: 2 }) })),
        save: () => Promise.resolve({ sessionId: "ses-other", savedRevision: 2, projectFileId: "project-a" }),
      }),
    );
    await store.openStaged("A");
    await store.save();

    expect(store.snapshot?.persistedRevision).toBeNull();
    expect(store.dirty).toBe(true);
  });

  it("dirty is derived, not stored", () => {
    const store = useEditorProjectStore();
    expect(Object.prototype.hasOwnProperty.call(store.$state, "dirty")).toBe(false);
  });

  it("rejection keeps the current project and surfaces lastError", async () => {
    const store = useEditorProjectStore();
    const err = editorError({ code: "invalidRequest", message: "bad command" });
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(
            openResult({
              snapshot: snapshot({ revision: 2, title: "Keep me" }),
              project: project({ title: "Keep me" }),
            }),
          ),
        execute: () => Promise.reject(new EditorPortError(err)),
      }),
    );
    await store.openStaged("A");
    await store.execute({ kind: "undo" });

    expect(store.project?.title).toBe("Keep me");
    expect(store.snapshot?.revision).toBe(2);
    expect(store.lastError).toEqual(err);
  });
});

describe("editorProject store — getters over an open project", () => {
  it("dirty, canUndo, canRedo and durationMs default sensibly with nothing open", () => {
    const store = useEditorProjectStore();
    expect(store.dirty).toBe(false);
    expect(store.canUndo).toBe(false);
    expect(store.canRedo).toBe(false);
    expect(store.durationMs).toBe(0);
    expect(store.clipById("c1")).toBeUndefined();
    expect(store.trackById("t1")).toBeUndefined();
  });

  it("canUndo/canRedo/durationMs mirror the open snapshot", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(
            openResult({ snapshot: snapshot({ canUndo: true, canRedo: true, durationMs: 4000 }) }),
          ),
      }),
    );
    await store.openStaged("A");
    expect(store.canUndo).toBe(true);
    expect(store.canRedo).toBe(true);
    expect(store.durationMs).toBe(4000);
  });

  it("clipById/trackById find entities in the open project graph", async () => {
    const store = useEditorProjectStore();
    const clip = {
      id: "c1",
      asset_id: "a1",
      track_id: "t1",
      name: "Clip 1",
      start_ms: 0,
      in_ms: 0,
      out_ms: 100,
      fade_in_ms: 0,
      fade_out_ms: 0,
      fade_curve: "linear" as const,
      opacity: 1,
      volume: 1,
      muted: false,
      x: 0,
      y: 0,
      w: 1,
      h: 1,
    };
    const track = {
      id: "t1",
      kind: "video" as const,
      name: "Track 1",
      visible: true,
      locked: false,
      muted: false,
      solo: false,
      volume: 1,
    };
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ project: project({ clips: [clip], tracks: [track] }) })),
      }),
    );
    await store.openStaged("A");

    expect(store.clipById("c1")).toEqual(clip);
    expect(store.trackById("t1")).toEqual(track);
    expect(store.clipById("missing")).toBeUndefined();
    expect(store.trackById("missing")).toBeUndefined();
  });
});

describe("editorProject store — openProject / no-op guards / close", () => {
  it("openProject opens like openStaged, through the same port method", async () => {
    const store = useEditorProjectStore();
    let received: { id: string; useRecovery: boolean } | null = null;
    store.setPort(
      fakePort({
        openProject: (id, useRecovery) => {
          received = { id, useRecovery };
          return Promise.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-p", revision: 7 }) }));
        },
      }),
    );
    await store.openProject("project-a", true);

    expect(received).toEqual({ id: "project-a", useRecovery: true });
    expect(store.sessionId).toBe("ses-p");
    expect(store.snapshot?.revision).toBe(7);
  });

  it("a failed open surfaces lastError and leaves nothing open", async () => {
    const store = useEditorProjectStore();
    const err = editorError({ code: "sourceMissing", message: "gone" });
    store.setPort(fakePort({ openStaged: () => Promise.reject(new EditorPortError(err)) }));

    await store.openStaged("A");

    expect(store.sessionId).toBeNull();
    expect(store.snapshot).toBeNull();
    expect(store.lastError).toEqual(err);
  });

  it("execute/save/close are no-ops when nothing is open — the port is never touched", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        execute: () => {
          throw new Error("execute must not be called with no open session");
        },
        save: () => {
          throw new Error("save must not be called with no open session");
        },
        closeSession: () => {
          throw new Error("closeSession must not be called with no open session");
        },
      }),
    );

    await store.execute({ kind: "undo" });
    await store.save();
    await store.close("keep");

    expect(store.lastError).toBeNull();
  });

  it("a plain successful edit installs the new snapshot and project, and clears pending", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ revision: 1 }) })),
        execute: () =>
          Promise.resolve({
            snapshot: snapshot({ revision: 2, title: "Renamed", canUndo: true }),
            project: project({ title: "Renamed" }),
          }),
      }),
    );
    await store.openStaged("A");
    await store.execute({ kind: "rename", title: "Renamed" });

    expect(store.snapshot?.revision).toBe(2);
    expect(store.project?.title).toBe("Renamed");
    expect(store.lastError).toBeNull();
    expect(store.pending.size).toBe(0);
  });

  it("an error from a superseded execute is ignored (generation guard on the error path too)", async () => {
    const executeTask = deferred<never>();
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ revision: 1 }) })),
        execute: () => executeTask.promise,
      }),
    );
    await store.openStaged("A");

    const executing = store.execute({ kind: "undo" });
    // Superseding open bumps generation before the rejection arrives.
    await store.openStaged("A");
    executeTask.reject(new EditorPortError(editorError({ code: "internal", message: "late failure" })));
    await executing;

    expect(store.lastError).toBeNull();
  });

  it("a revisionConflict refetch from a different session is ignored", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-a", revision: 1 }) })),
        execute: () =>
          Promise.reject(new EditorPortError(editorError({ code: "revisionConflict", retryable: true }))),
        getSnapshot: () =>
          Promise.resolve({
            snapshot: snapshot({ sessionId: "ses-other", revision: 5, title: "Not mine" }),
            project: project({ title: "Not mine" }),
          }),
      }),
    );
    await store.openStaged("A");
    await store.execute({ kind: "undo" });

    expect(store.snapshot?.revision).toBe(1);
    expect(store.project?.title).not.toBe("Not mine");
  });

  it("a failed refetch after revisionConflict surfaces lastError", async () => {
    const store = useEditorProjectStore();
    const refetchErr = editorError({ code: "sessionGone", message: "session gone" });
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ revision: 1 }) })),
        execute: () =>
          Promise.reject(new EditorPortError(editorError({ code: "revisionConflict", retryable: true }))),
        getSnapshot: () => Promise.reject(new EditorPortError(refetchErr)),
      }),
    );
    await store.openStaged("A");
    await store.execute({ kind: "undo" });

    expect(store.lastError).toEqual(refetchErr);
    expect(store.conflictIntent).toEqual({ kind: "undo" });
  });

  it("close tells Rust and clears every local trace of the session", async () => {
    const store = useEditorProjectStore();
    let closed: { sessionId: string; disposition: string } | null = null;
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-a" }) })),
        closeSession: (sessionId, disposition) => {
          closed = { sessionId, disposition };
          return Promise.resolve();
        },
      }),
    );
    await store.openStaged("A");
    await store.close("discardProject");

    expect(closed).toEqual({ sessionId: "ses-a", disposition: "discardProject" });
    expect(store.sessionId).toBeNull();
    expect(store.snapshot).toBeNull();
    expect(store.project).toBeNull();
  });

  it("a failed close still leaves the session cleared locally and surfaces lastError", async () => {
    const store = useEditorProjectStore();
    const err = editorError({ code: "internal", message: "close refused" });
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult()),
        closeSession: () => Promise.reject(new EditorPortError(err)),
      }),
    );
    await store.openStaged("A");
    await store.close("keep");

    expect(store.sessionId).toBeNull();
    expect(store.lastError).toEqual(err);
  });

  it("a close failure superseded by a fresh open does not surface lastError", async () => {
    const closeTask = deferred<void>();
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-a" }) })),
        closeSession: () => closeTask.promise,
      }),
    );
    await store.openStaged("A");

    const closing = store.close("keep");
    // A fresh open while the close call is still in flight bumps generation
    // again — the close's own eventual failure must not clobber the NEW
    // session's lastError.
    await store.openStaged("A");
    closeTask.reject(new Error("closeSession transport failure"));
    await closing;

    expect(store.lastError).toBeNull();
  });
});

describe("editorProject store — remaining guard branches", () => {
  it("toEditorError wraps a plain Error rejection as internal (not every port failure is an EditorPortError)", async () => {
    const store = useEditorProjectStore();
    store.setPort(fakePort({ openStaged: () => Promise.reject(new Error("network down")) }));
    await store.openStaged("A");
    expect(store.lastError).toEqual({
      code: "internal",
      message: "network down",
      retryable: false,
      operationId: "store-local",
    });
  });

  it("toEditorError wraps a non-Error rejection as internal", async () => {
    const store = useEditorProjectStore();
    store.setPort(fakePort({ openStaged: () => Promise.reject("just a string") }));
    await store.openStaged("A");
    expect(store.lastError).toEqual({
      code: "internal",
      message: "just a string",
      retryable: false,
      operationId: "store-local",
    });
  });

  it("an open reply that resolves after being superseded by a newer open is ignored", async () => {
    const openATask = deferred<EditorOpenResult>();
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: (base) =>
          base === "A" ? openATask.promise : Promise.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-b" }) })),
      }),
    );
    const openingA = store.openStaged("A");
    await store.openStaged("B");
    expect(store.sessionId).toBe("ses-b");

    openATask.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-a-late" }) }));
    await openingA;

    expect(store.sessionId).toBe("ses-b");
  });

  it("an open failure that arrives after being superseded by a newer open does not surface lastError", async () => {
    const openATask = deferred<EditorOpenResult>();
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: (base) =>
          base === "A" ? openATask.promise : Promise.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-b" }) })),
      }),
    );
    const openingA = store.openStaged("A");
    await store.openStaged("B");

    openATask.reject(new EditorPortError(editorError({ code: "internal", message: "late failure" })));
    await openingA;

    expect(store.lastError).toBeNull();
    expect(store.sessionId).toBe("ses-b");
  });

  it("an execute reply naming a different session is ignored even at the same generation", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ sessionId: "ses-a", revision: 1 }) })),
        execute: () =>
          Promise.resolve({
            snapshot: snapshot({ sessionId: "ses-other", revision: 2, title: "Not mine" }),
            project: project({ title: "Not mine" }),
          }),
      }),
    );
    await store.openStaged("A");
    await store.execute({ kind: "undo" });

    expect(store.snapshot?.sessionId).toBe("ses-a");
    expect(store.snapshot?.revision).toBe(1);
    expect(store.project?.title).not.toBe("Not mine");
  });

  it("a revisionConflict refetch that resolves after being superseded is not installed", async () => {
    // `getSnapshot`'s mock supersedes the session itself, as a side effect
    // of being CALLED — the only way to land the opening open strictly
    // AFTER `refetchAfterConflict` has started (and so generation still
    // matched when the refetch began) but BEFORE its reply is installed.
    // A supersession fired from the test body instead would race the
    // rejection's own catch chain and could trip the guard in
    // `handleExecuteError` before the refetch is ever reached at all —
    // the guard this test is NOT about — which is exactly what happened
    // in an earlier draft of this test (an unhandled `getSnapshot`
    // rejection, because `refetchAfterConflict` never got called).
    const getSnapshotTask = deferred<EditorProjection>();
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ revision: 1 }) })),
        execute: () =>
          Promise.reject(new EditorPortError(editorError({ code: "revisionConflict", retryable: true }))),
        getSnapshot: async () => {
          await store.openStaged("A");
          return getSnapshotTask.promise;
        },
      }),
    );
    await store.openStaged("A");
    const executing = store.execute({ kind: "undo" });

    getSnapshotTask.resolve({
      snapshot: snapshot({ revision: 99, title: "Late refetch" }),
      project: project({ title: "Late refetch" }),
    });
    await executing;

    expect(store.snapshot?.revision).not.toBe(99);
    expect(store.project?.title).not.toBe("Late refetch");
  });

  it("a revisionConflict refetch failure superseded by a newer open does not surface lastError", async () => {
    const getSnapshotTask = deferred<EditorProjection>();
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () => Promise.resolve(openResult({ snapshot: snapshot({ revision: 1 }) })),
        execute: () =>
          Promise.reject(new EditorPortError(editorError({ code: "revisionConflict", retryable: true }))),
        getSnapshot: async () => {
          await store.openStaged("A");
          return getSnapshotTask.promise;
        },
      }),
    );
    await store.openStaged("A");
    const executing = store.execute({ kind: "undo" });

    getSnapshotTask.reject(new EditorPortError(editorError({ code: "internal", message: "refetch failed late" })));
    await executing;

    expect(store.lastError).toBeNull();
  });

  it("a save receipt that resolves after being superseded by a newer open is ignored", async () => {
    const saveTask = deferred<SaveReceipt>();
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ snapshot: snapshot({ revision: 2, persistedRevision: null }) })),
        save: () => saveTask.promise,
      }),
    );
    await store.openStaged("A");
    const saving = store.save();
    // A fresh open supersedes the in-flight save's generation.
    await store.openStaged("A");

    saveTask.resolve({ sessionId: "ses-a", savedRevision: 2, projectFileId: "project-a" });
    await saving;

    // The NEW session's own (unsaved) snapshot must be untouched by the
    // stale save receipt.
    expect(store.snapshot?.persistedRevision).toBeNull();
  });

  it("refuses a save receipt whose savedRevision is ahead of the live revision", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakePort({
        openStaged: () =>
          Promise.resolve(openResult({ snapshot: snapshot({ revision: 2, persistedRevision: null }) })),
        save: () => Promise.resolve({ sessionId: "ses-a", savedRevision: 99, projectFileId: "project-a" }),
      }),
    );
    await store.openStaged("A");
    await store.save();

    expect(store.snapshot?.persistedRevision).toBeNull();
  });
});
