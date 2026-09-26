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

/** A promise plus its own `resolve`, never reassigned after construction —
 * `editorProjectStore.test.ts`'s own precedent, used here instead of a
 * nullable `let` resolver captured by a closure (the latter trips a
 * TypeScript control-flow narrowing quirk when read back in the SAME
 * function body — verified in isolation, not this repo's bug to fix). */
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
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

  // Task 28 fix round 1: a peaks decode's record is FORGOTTEN by Rust once
  // its command replies (it has no Channel to recover). A reconcile that
  // caught it mid-decode must not leave it "preparing" forever: the next
  // reconcile, not finding it in the registry, drops it. An import the
  // registry still lists is untouched.
  it("a job the registry has forgotten is dropped on the next reconcile", async () => {
    const peaks: JobRecordDto = { jobId: "job-p", kind: "peaks", phase: "preparing", fraction: 0, terminal: null };
    const importRow: JobRecordDto = { jobId: "job-1", kind: "import", phase: "preparing", fraction: 0.2, terminal: null };
    let rows: JobRecordDto[] = [importRow, peaks];
    const { jobs, start } = await setup({ getJobs: () => Promise.resolve(rows) });
    await start();
    await jobs.reconcile();
    expect(jobs.jobs["job-p"]?.phase).toBe("preparing");

    rows = [importRow];
    await jobs.reconcile();
    expect(jobs.jobs["job-p"]).toBeUndefined();
    expect(jobs.jobs["job-1"].phase).toBe("preparing");
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

// Hardening Task 15 (GAP-208): after a reload of the editor webview a render
// Rust is still running has no Channel here. The reconcile that runs for the
// new session adopts it; a render this webview started itself never is —
// its own dialog already follows it.
describe("editorJobs — renders nobody follows", () => {
  const running = (jobId: string): JobRecordDto => ({
    jobId,
    kind: "render",
    phase: "rendering",
    fraction: 0.5,
    terminal: null,
  });

  it("adopts a running render only when this webview did not start it", async () => {
    let rows: JobRecordDto[] = [running("job-mine"), running("job-orphan")];
    const { jobs } = await setup({
      startRender: () => Promise.resolve({ jobId: "job-mine", revision: 3 }),
      getJobs: () => Promise.resolve(rows),
    });
    await jobs.startRender({ name: "v1", range: null, quality: "balanced" });
    await jobs.reconcile();
    expect(jobs.adoptedRender).toBe("job-orphan");
    // An import or a finished render is never adopted.
    rows = [
      { ...running("job-imp"), kind: "import" },
      { ...running("job-done"), phase: "complete", terminal: { productId: "p" } },
    ];
    jobs.adopted = [];
    await jobs.reconcile();
    expect(jobs.adoptedRender).toBeNull();
  });

  it("follow polls the registry until the render's terminal, retrying a failed read", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      const reads: (JobRecordDto[] | Error)[] = [
        [running("job-7")],
        new Error("transport down"),
        [{ ...running("job-7"), phase: "complete", fraction: 1, terminal: { productId: "p" } }],
      ];
      const getJobs = vi.fn(() => {
        const next = reads.length > 1 ? reads.shift() : reads[0];
        return next instanceof Error ? Promise.reject(next) : Promise.resolve(next ?? []);
      });
      const { jobs } = await setup({ getJobs, getProducts: () => Promise.resolve([]) });
      await jobs.reconcile();
      expect(jobs.adoptedRender).toBe("job-7");
      const followed = jobs.follow("job-7");
      expect(jobs.adoptedRender, "a followed render is no longer waiting for a follower").toBeNull();
      await vi.advanceTimersByTimeAsync(1000);
      expect(jobs.jobs["job-7"].terminal).toBeNull();
      await vi.advanceTimersByTimeAsync(1000);
      await followed;
      expect(jobs.jobs["job-7"].phase).toBe("complete");
      expect(getJobs).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  // Carried from Task 15's review: `track` only installs a job once `start()`
  // replies with its id, but Rust can already list the row in its registry
  // before that reply lands (`editor_start_render` "answers at once", per
  // AGENTS.md) — a `reconcile()` racing that window used to adopt this
  // webview's OWN render, which for a Review started from `ReviewDialog`
  // would stack `RenderDialog` on top of it.
  it("does not adopt a render this webview is currently starting", async () => {
    const start = deferred<{ jobId: string; revision: number }>();
    const { jobs } = await setup({
      startRender: () => start.promise,
      getJobs: () => Promise.resolve([running("job-mine")]),
    });
    const starting = jobs.startRender({ name: "v1", range: null, quality: "balanced" });
    // Rust's registry already lists the job; this webview's own Channel
    // just has not been wired to it yet.
    await jobs.reconcile();
    expect(jobs.adoptedRender).toBeNull();

    start.resolve({ jobId: "job-mine", revision: 3 });
    await starting;

    expect(jobs.jobs["job-mine"]).toBeDefined();
    expect(jobs.adoptedRender).toBeNull();
  });

  // Carried from Task 15's review: `follow`'s own polling reads called
  // `reconcile()` directly, which sets `this.lastError` on a failed read —
  // the MEDIA LIBRARY's import line, overwritten once a second for as long
  // as a stalled registry stayed unreachable, over a render nobody's
  // library card is even about (F-M... the render's errors belong in
  // `renderError`, never `lastError`, and `follow` files neither).
  it("a failed poll while following a render never touches lastError", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      const reads: (JobRecordDto[] | Error)[] = [
        [running("job-7")],
        new Error("transport down"),
        [{ ...running("job-7"), phase: "complete", fraction: 1, terminal: { productId: "p" } }],
      ];
      const getJobs = vi.fn(() => {
        const next = reads.length > 1 ? reads.shift() : reads[0];
        return next instanceof Error ? Promise.reject(next) : Promise.resolve(next ?? []);
      });
      const { jobs } = await setup({ getJobs, getProducts: () => Promise.resolve([]) });
      await jobs.reconcile();
      const followed = jobs.follow("job-7");
      await vi.advanceTimersByTimeAsync(1000); // the failing read
      expect(jobs.lastError).toBeNull();
      await vi.advanceTimersByTimeAsync(1000); // the terminal read
      await followed;
      expect(jobs.jobs["job-7"].terminal).toEqual({ productId: "p" });
      expect(jobs.lastError).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});

// Hardening Task 16 (F-M6): a registry read can race a Channel message
// already applied at a higher sequence — the module doc's "the registry is
// updated before every message is sent" holds at Rust's SEND, not at this
// reply's ARRIVAL — so a reconcile must never regress a job's own progress
// backwards under a stale row.
describe("editorJobs — reconcile never regresses a held job's progress", () => {
  it("a registry row behind a Channel-tracked job's own progress is never installed over it", async () => {
    const rows: JobRecordDto[] = [
      { jobId: "job-1", kind: "render", phase: "preparing", fraction: 0, terminal: null },
    ];
    const { jobs, send, start } = await setup({ getJobs: () => Promise.resolve(rows) });
    await start();
    send(msg({ kind: "render", sequence: 2, phase: "rendering", fraction: 0.4 }));
    expect(jobs.jobs["job-1"].fraction).toBe(0.4);

    await jobs.reconcile();

    expect(jobs.jobs["job-1"].phase).toBe("rendering");
    expect(jobs.jobs["job-1"].fraction).toBe(0.4);
  });

  // The mutation check's other half: a registry row AHEAD of (or equal to)
  // the held progress still installs — this is a regression guard, not a
  // blanket "never trust the registry".
  it("a registry row at or ahead of the held progress still installs", async () => {
    const rows: JobRecordDto[] = [
      { jobId: "job-1", kind: "render", phase: "rendering", fraction: 0.6, terminal: null },
    ];
    const { jobs, send, start } = await setup({ getJobs: () => Promise.resolve(rows) });
    await start();
    send(msg({ kind: "render", sequence: 2, phase: "rendering", fraction: 0.4 }));

    await jobs.reconcile();

    expect(jobs.jobs["job-1"].fraction).toBe(0.6);
  });
});

// Hardening Task 16 (C-4): a job's session must still be the CURRENT one
// when its install actually happens — `track()`'s `{jobId}` reply can land
// after the session that started it has since closed or been superseded by
// a new open — and closing (or opening a new) session must not leave the
// old one's rows sitting in this store forever: they can never be adopted,
// shown or reconciled again (nothing ever reads a stale session's rows),
// so left alone they are a pure, permanent leak.
describe("editorJobs — session hygiene", () => {
  it("a start reply that arrives after its session closed installs nothing", async () => {
    const { jobs, start } = await setup({ closeSession: () => Promise.resolve() });
    await useEditorProjectStore().close("keep");
    await start();
    expect(jobs.jobs["job-1"]).toBeUndefined();
  });

  // The wiring that actually CALLS `forgetSession` from a session's end
  // lives in `EditorRoot`'s `wireJobHygiene` (an `editorProject.$onAction`
  // subscription, deliberately not a static import here — see this file's
  // own module doc and `editorRoot.test.ts`'s "job hygiene on session end"
  // tests for that integration).
  it("forgetSession drops only that session's jobs and its adoption record", () => {
    const jobs = useEditorJobsStore();
    jobs.jobs = {
      "job-old": {
        jobId: "job-old",
        sessionId: "ses-old",
        kind: "render",
        phase: "rendering",
        fraction: 0.4,
        terminal: null,
        sequence: 3,
      },
      "job-current": {
        jobId: "job-current",
        sessionId: "ses-new",
        kind: "import",
        phase: "preparing",
        fraction: 0.1,
        terminal: null,
        sequence: 1,
      },
    };
    jobs.adopted = ["job-old"];

    jobs.forgetSession("ses-old");

    expect(jobs.jobs["job-old"]).toBeUndefined();
    expect(jobs.jobs["job-current"]).toBeDefined();
    expect(jobs.adopted).toEqual([]);
  });
});
