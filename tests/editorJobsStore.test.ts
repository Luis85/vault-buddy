/**
 * `editorJobs` (Task 25; IPC-CONTRACTS.md "Progress, cancellation and
 * reconciliation"): a job's Channel messages are installed only for the
 * job and session they belong to, only with a strictly increasing
 * `sequence`, and never after a terminal; `reconcile()` (the Rust job
 * registry) is authoritative over whatever the Channel said.
 *
 * The fake port hands the test the `onProgress` callback the store gave
 * `importMedia`, so a test can deliver messages in any order — including
 * BEFORE `importMedia` resolves, which a real Channel can do.
 */
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { EditorPort } from "../src/editor/port";
import type {
  EditorOpenResult,
  JobProgressDto,
  JobRecordDto,
  Project,
} from "../src/editorTypes";
import { useEditorJobsStore } from "../src/stores/editorJobs";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

beforeEach(() => {
  setActivePinia(createPinia());
});

function project(): Project {
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
  };
}

function openResult(sessionId = "ses-a"): EditorOpenResult {
  return {
    snapshot: {
      sessionId,
      projectId: "project-a",
      revision: 3,
      persistedRevision: 3,
      title: "Tutorial",
      durationMs: 0,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    },
    project: project(),
    workspace: {},
    missing: [],
    sourceBase: "base",
    recovered: false,
  };
}

function msg(overrides: Partial<JobProgressDto> = {}): JobProgressDto {
  return {
    sessionId: "ses-a",
    jobId: "job-1",
    kind: "import",
    sequence: 1,
    phase: "preparing",
    fraction: 0.25,
    terminal: null,
    ...overrides,
  };
}

/** Opens session `ses-a` and returns the captured `onProgress` plus a
 * resolver for `importMedia`'s `{ jobId }` reply. */
async function setup(extra: Partial<EditorPort> = {}) {
  let deliver: ((m: JobProgressDto) => void) | null = null;
  let resolveStart: ((v: { jobId: string }) => void) | null = null;
  const getSnapshot = vi.fn(() =>
    Promise.resolve({ snapshot: { ...openResult().snapshot, revision: 4 }, project: project() }),
  );
  const port = fakeEditorPort({
    openStaged: () => Promise.resolve(openResult()),
    getSnapshot,
    importMedia: (_sessionId, onProgress) => {
      deliver = onProgress;
      return new Promise((resolve) => {
        resolveStart = resolve;
      });
    },
    ...extra,
  });
  const project$ = useEditorProjectStore();
  project$.setPort(port);
  await project$.openStaged("base");
  const jobs = useEditorJobsStore();
  const started = jobs.importMedia();
  return {
    jobs,
    getSnapshot,
    send: (m: JobProgressDto) => deliver?.(m),
    start: async (jobId = "job-1") => {
      resolveStart?.({ jobId });
      await started;
    },
  };
}

describe("editorJobs — channel messages", () => {
  it("store ignores a non-increasing sequence", async () => {
    const { jobs, send, start } = await setup();
    await start();
    send(msg({ sequence: 2, fraction: 0.5 }));
    send(msg({ sequence: 2, fraction: 0.9 })); // a duplicate
    send(msg({ sequence: 1, fraction: 0.1 })); // an older one, late
    expect(jobs.jobs["job-1"].fraction).toBe(0.5);
    send(msg({ sequence: 3, fraction: 0.75 }));
    expect(jobs.jobs["job-1"].fraction).toBe(0.75);
  });

  // The mutation check's other half: Rust emitting a SECOND terminal must
  // not change what the first one said.
  it("store ignores messages after terminal", async () => {
    const { jobs, send, start } = await setup();
    await start();
    const done = { assetIds: ["asset-1"], perFile: [] };
    send(msg({ sequence: 2, phase: "complete", fraction: 1, terminal: done }));
    send(msg({ sequence: 3, phase: "preparing", fraction: 0.2 }));
    send(msg({ sequence: 4, phase: "failed", fraction: 1, terminal: { perFile: [] } }));
    expect(jobs.jobs["job-1"].phase).toBe("complete");
    expect(jobs.jobs["job-1"].terminal).toEqual(done);
  });

  it("store ignores another session's job", async () => {
    const { jobs, send, start } = await setup();
    await start();
    send(msg({ sessionId: "ses-other", sequence: 2, fraction: 0.9 }));
    send(msg({ jobId: "job-stranger", sequence: 2, fraction: 0.9 }));
    expect(jobs.jobs["job-1"].fraction).toBe(0);
    expect(jobs.jobs["job-stranger"]).toBeUndefined();
  });

  it("applies messages that arrived before importMedia resolved", async () => {
    const { jobs, send, start } = await setup();
    send(msg({ sequence: 1, phase: "queued", fraction: 0 }));
    send(msg({ sequence: 2, fraction: 0.5 }));
    send(msg({ jobId: "job-stranger", sequence: 3, fraction: 0.9 }));
    await start("job-1");
    expect(jobs.jobs["job-1"].fraction).toBe(0.5);
    expect(jobs.jobs["job-stranger"]).toBeUndefined();
  });

  it("a terminal that imported assets refreshes the project once", async () => {
    const { jobs, send, start, getSnapshot } = await setup();
    await start();
    send(msg({ sequence: 2, phase: "complete", fraction: 1, terminal: { assetIds: ["a1"], perFile: [] } }));
    await Promise.resolve();
    expect(getSnapshot).toHaveBeenCalledTimes(1);
    expect(jobs.activeImport).toBeNull();
    expect(jobs.lastImport?.terminal?.assetIds).toEqual(["a1"]);
  });
});

describe("editorJobs — no session", () => {
  it("import, cancel and reconcile are no-ops without an open session", async () => {
    const jobs = useEditorJobsStore();
    const importMedia = vi.fn();
    const store = useEditorProjectStore();
    store.setPort({ importMedia, cancelJob: importMedia, getJobs: importMedia } as unknown as EditorPort);
    await expect(jobs.importMedia()).resolves.toBeNull();
    await jobs.cancel("job-1");
    await jobs.reconcile();
    expect(importMedia).not.toHaveBeenCalled();
    expect(jobs.activeImport).toBeNull();
    expect(jobs.lastImport).toBeNull();
  });

  it("a refused cancel surfaces its error", async () => {
    const { jobs, start } = await setup({ cancelJob: () => Promise.reject(new Error("no such job")) });
    await start();
    await jobs.cancel("job-1");
    expect(jobs.lastError?.message).toBe("no such job");
  });
});

describe("editorProject.refresh", () => {
  it("never installs a projection BEHIND the revision already shown, and surfaces a failure", async () => {
    const older = { snapshot: { ...openResult().snapshot, revision: 2 }, project: project() };
    const getSnapshot = vi
      .fn()
      .mockResolvedValueOnce(older)
      .mockRejectedValueOnce(new Error("gone"));
    const { start } = await setup({ getSnapshot });
    await start();
    const store = useEditorProjectStore();
    await store.refresh();
    expect(store.snapshot?.revision).toBe(3);
    await store.refresh();
    expect(store.lastError?.message).toBe("gone");
  });
});

describe("editorJobs — reconcile", () => {
  it("reconcile replaces stale event state", async () => {
    const rows: JobRecordDto[] = [
      {
        jobId: "job-1",
        kind: "import",
        phase: "complete",
        fraction: 1,
        terminal: { assetIds: ["a1", "a2"], perFile: [] },
      },
    ];
    const { jobs, send, start } = await setup({ getJobs: () => Promise.resolve(rows) });
    await start();
    send(msg({ sequence: 2, fraction: 0.3 }));
    expect(jobs.jobs["job-1"].phase).toBe("preparing");

    await jobs.reconcile();
    expect(jobs.jobs["job-1"].phase).toBe("complete");
    expect(jobs.jobs["job-1"].terminal?.assetIds).toEqual(["a1", "a2"]);
    // The Channel's stale tail can no longer overwrite the registry's answer.
    send(msg({ sequence: 3, fraction: 0.6 }));
    expect(jobs.jobs["job-1"].phase).toBe("complete");
  });

  // Fix round 1: the registry reply and the Channel travel separately, so
  // a reply read BEFORE the terminal can land AFTER it. It must not revive
  // the finished job (that would pin "An import is already running").
  it("a late reconcile reply cannot revive a job the store already holds as terminal", async () => {
    const stale: JobRecordDto[] = [
      { jobId: "job-1", kind: "import", phase: "preparing", fraction: 0.4, terminal: null },
    ];
    const { jobs, send, start } = await setup({ getJobs: () => Promise.resolve(stale) });
    await start();
    const done = { assetIds: [], perFile: [] };
    send(msg({ sequence: 2, phase: "complete", fraction: 1, terminal: done }));
    await jobs.reconcile();
    expect(jobs.jobs["job-1"].phase).toBe("complete");
    expect(jobs.jobs["job-1"].terminal).toEqual(done);
    expect(jobs.activeImport).toBeNull();
  });

  it("a failed reconcile keeps what the store had and surfaces the error", async () => {
    const { jobs, send, start } = await setup({
      getJobs: () => Promise.reject(new Error("transport down")),
    });
    await start();
    send(msg({ sequence: 2, fraction: 0.3 }));
    await jobs.reconcile();
    expect(jobs.jobs["job-1"].fraction).toBe(0.3);
    expect(jobs.lastError?.message).toContain("transport down");
  });

  it("cancel asks Rust for the current session's job", async () => {
    const cancelJob = vi.fn(() => Promise.resolve());
    const { jobs, start } = await setup({ cancelJob });
    await start();
    await jobs.cancel("job-1");
    expect(cancelJob).toHaveBeenCalledWith("ses-a", "job-1");
  });
});
