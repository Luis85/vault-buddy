/**
 * The editor's close guard (Task 37 Part A; F-44). Rust answers the editor
 * window's X with `editor:closeRequested` instead of hiding; the guard then
 * decides: a dirty session or a live render/publish job gets a dialog,
 * anything else hides at once through `editor_hide_window`.
 *
 * Every test drives the guard through an injected fake `EditorPort` (the
 * `editorProjectStore.test.ts` posture) — the jobs it reads come from
 * `getJobs`, the authoritative registry, never a local guess.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logBreadcrumb: vi.fn(), logWarning: vi.fn() }));

// Captured so the EditorRoot wiring tests can deliver `editor:closeRequested`
// the way Rust's `emit_to("editor", …)` does.
const listeners: Record<string, (e?: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e?: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));

import CloseGuardDialog from "../src/components/editor/dialogs/CloseGuardDialog.vue";
import { decodeJobRecords } from "../src/editor/decode";
import type { EditorPort } from "../src/editor/port";
import { EditorPortError } from "../src/editor/port";
import { noteTakeOpen, noteTakeSettled, openWebcamTakes } from "../src/editor/webcamTakes";
import type { EditorOpenResult, EditorSnapshot, JobRecordDto } from "../src/editorTypes";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { useEditorOnboardingStore } from "../src/stores/editorOnboarding";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { mockEditor } from "./helpers/editorMount";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  for (const key of Object.keys(listeners)) delete listeners[key];
  mockConvertFileSrc("windows");
  setActivePinia(createPinia());
});

function snapshot(revision: number, persistedRevision: number): EditorSnapshot {
  return {
    sessionId: "ses-7",
    projectId: "proj-3",
    revision,
    persistedRevision,
    title: "Tutorial",
    durationMs: 61_500,
    canUndo: true,
    canRedo: false,
    undoLabel: "Rename",
    redoLabel: null,
  };
}

function job(kind: JobRecordDto["kind"], phase: JobRecordDto["phase"], jobId = `job-${kind}`): JobRecordDto {
  return { jobId, kind, phase, fraction: 0.4, terminal: null };
}

/** A session at `revision`, saved at `persisted`, with `jobs` in its
 * registry. Returns the port's spies and the mounted guard. */
async function setup(opts: {
  revision?: number;
  persisted?: number;
  jobs?: JobRecordDto[];
  session?: boolean;
  overrides?: Partial<EditorPort>;
}) {
  const hideWindow = vi.fn(async () => {});
  const cancelJob = vi.fn(async () => {});
  const closeSession = vi.fn(async () => {});
  const getJobs = vi.fn(async () => opts.jobs ?? []);
  const store = useEditorProjectStore();
  store.setPort(fakeEditorPort({ hideWindow, cancelJob, closeSession, getJobs, ...opts.overrides }));
  if (opts.session !== false) {
    store.sessionId = "ses-7";
    store.snapshot = snapshot(opts.revision ?? 3, opts.persisted ?? 2);
  }
  const w = mount(CloseGuardDialog, { attachTo: document.body });
  await (w.vm as unknown as { request(): Promise<void> }).request();
  await flushPromises();
  return { w, store, hideWindow, cancelJob, closeSession, getJobs };
}

function button(w: ReturnType<typeof mount>, label: string) {
  const found = w.findAll("button").find((b) => b.text() === label);
  if (!found) throw new Error(`no button "${label}" in: ${w.text()}`);
  return found;
}

describe("CloseGuardDialog", () => {
  it("dirty close offers Save, Keep and Discard", async () => {
    const { w, hideWindow } = await setup({ revision: 3, persisted: 2 });
    const labels = w.findAll("button").map((b) => b.text());
    expect(labels).toEqual(
      expect.arrayContaining(["Save project", "Keep for later", "Discard changes", "Cancel"]),
    );
    expect(hideWindow).not.toHaveBeenCalled();
  });

  it("clean close hides immediately", async () => {
    const { w, hideWindow } = await setup({ revision: 4, persisted: 4 });
    expect(hideWindow).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="close-guard"]').exists()).toBe(false);
  });

  // Task 56: the guide's progress is saved on a 400 ms debounce; a window
  // hidden inside that window may never run the timer again, so the last
  // lesson change is written BEFORE the hide, not lost.
  it("hiding flushes a pending guide progress save first", async () => {
    const order: string[] = [];
    const guide = useEditorOnboardingStore();
    guide.loaded = true;
    guide.start(); // a lesson change, still inside the save debounce
    const { hideWindow } = await setup({
      revision: 4,
      persisted: 4,
      overrides: {
        saveGuideProgress: async (p) => {
          order.push(`save:${p.currentStepId}`);
        },
        hideWindow: async () => {
          order.push("hide");
        },
      },
    });
    expect(hideWindow).not.toHaveBeenCalled(); // the override replaced it
    expect(order).toEqual(["save:welcome", "hide"]);
  });

  it("a live render changes the close copy and never cancels by itself", async () => {
    const { w, hideWindow, cancelJob } = await setup({
      revision: 4,
      persisted: 4,
      jobs: [job("render", "rendering")],
    });
    expect(w.get('[data-testid="close-guard"]').text()).toContain(
      "A render is running — keep it running in the background, or cancel it",
    );
    expect(hideWindow).not.toHaveBeenCalled();
    expect(cancelJob).not.toHaveBeenCalled();
    // Keeping it running hides the window and still cancels nothing, and
    // neither does the guard going away.
    await button(w, "Keep it running").trigger("click");
    await flushPromises();
    expect(hideWindow).toHaveBeenCalledTimes(1);
    w.unmount();
    expect(cancelJob).not.toHaveBeenCalled();
  });

  // Task 46: the PAIR. Rust serializes a render job's kind as "render"
  // (`media_jobs.rs`' `job_progress_wire_shape_is_pinned`); a registry row
  // in that literal spelling, through the real decoder, is what flips the
  // close copy — in either phase a render blocks shutdown in.
  it("a render row in Rust's own spelling flips the close copy", async () => {
    for (const phase of ["rendering", "publishing"]) {
      const jobs = decodeJobRecords([{ jobId: "job-r", kind: "render", phase, fraction: 1, terminal: null }]);
      const { w } = await setup({ revision: 3, persisted: 3, jobs });
      expect(w.get('[data-testid="close-guard"]').text()).toContain("A render is running");
      w.unmount();
    }
  });

  it("a publish job is a render for the close copy too", async () => {
    const { w } = await setup({ revision: 4, persisted: 4, jobs: [job("publish", "publishing")] });
    expect(w.get('[data-testid="close-guard"]').text()).toContain("A render is running");
  });

  // Imports and waveform decodes are not the long, irreplaceable work the
  // render copy is about; a finished render is not live either.
  it("import, peaks and finished render jobs never change the close copy", async () => {
    const { hideWindow, w } = await setup({
      revision: 4,
      persisted: 4,
      jobs: [job("import", "preparing"), job("peaks", "queued"), job("render", "complete", "job-old")],
    });
    expect(hideWindow).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="close-guard"]').exists()).toBe(false);
  });

  it("Cancel the render cancels only the live render jobs, then closes by the usual rule", async () => {
    const { w, cancelJob, hideWindow } = await setup({
      revision: 3,
      persisted: 2,
      jobs: [job("import", "preparing"), job("render", "rendering", "job-r1")],
    });
    await button(w, "Cancel the render").trigger("click");
    await flushPromises();
    expect(cancelJob).toHaveBeenCalledTimes(1);
    expect(cancelJob).toHaveBeenCalledWith("ses-7", "job-r1");
    // Still dirty, so the unsaved-changes choice follows.
    expect(hideWindow).not.toHaveBeenCalled();
    expect(w.findAll("button").map((b) => b.text())).toContain("Keep for later");
  });

  // Task 49: a take still recording (begun, never finished) exists only as
  // a `.part` the closing session would lose — the guard says so first,
  // and "Discard the take" discards it natively before the usual rule.
  it("an unsaved webcam take asks before closing", async () => {
    noteTakeOpen("ses-7", "take-1");
    const webcamDiscard = vi.fn(async (_sessionId: string, takeId: string) => {
      noteTakeSettled(takeId);
    });
    const { w, hideWindow } = await setup({ revision: 4, persisted: 4, overrides: { webcamDiscard } });
    expect(w.get('[data-testid="close-guard"]').text()).toContain("You have an unsaved webcam take");
    expect(hideWindow).not.toHaveBeenCalled();
    await button(w, "Discard the take").trigger("click");
    await flushPromises();
    expect(webcamDiscard).toHaveBeenCalledWith("ses-7", "take-1");
    expect(hideWindow).toHaveBeenCalledTimes(1);
  });

  it("Cancel on an unsaved take keeps the take and the editor", async () => {
    noteTakeOpen("ses-7", "take-2");
    const webcamDiscard = vi.fn(async () => {});
    const { w, hideWindow } = await setup({ revision: 4, persisted: 4, overrides: { webcamDiscard } });
    await button(w, "Cancel").trigger("click");
    await flushPromises();
    expect(webcamDiscard).not.toHaveBeenCalled();
    expect(hideWindow).not.toHaveBeenCalled();
    expect(openWebcamTakes("ses-7")).toEqual(["take-2"]);
    noteTakeSettled("take-2");
  });

  it("Keep for later hides without closing the session", async () => {
    const { w, hideWindow, closeSession, store } = await setup({ revision: 3, persisted: 2 });
    await button(w, "Keep for later").trigger("click");
    await flushPromises();
    expect(hideWindow).toHaveBeenCalledTimes(1);
    expect(closeSession).not.toHaveBeenCalled();
    expect(store.sessionId).toBe("ses-7");
  });

  it("Discard changes discards the recovery journal, then hides", async () => {
    const { w, hideWindow, closeSession } = await setup({ revision: 3, persisted: 2 });
    await button(w, "Discard changes").trigger("click");
    await flushPromises();
    expect(closeSession).toHaveBeenCalledWith("ses-7", "discardRecovery");
    expect(hideWindow).toHaveBeenCalledTimes(1);
    expect(closeSession.mock.invocationCallOrder[0]).toBeLessThan(
      hideWindow.mock.invocationCallOrder[0],
    );
  });

  it("Save project saves the current revision, then hides", async () => {
    const save = vi.fn(async () => ({ sessionId: "ses-7", savedRevision: 3, projectFileId: "proj-3" }));
    const { w, hideWindow } = await setup({ revision: 3, persisted: 2, overrides: { save } });
    await button(w, "Save project").trigger("click");
    await flushPromises();
    expect(save).toHaveBeenCalledWith("ses-7", 3);
    expect(hideWindow).toHaveBeenCalledTimes(1);
  });

  it("a failed save keeps the dialog open and says why", async () => {
    const save = vi.fn(async () => {
      throw new EditorPortError({
        code: "diskFull",
        message: "Not enough disk space to save the project",
        retryable: true,
        operationId: "op-1",
      });
    });
    const { w, hideWindow } = await setup({ revision: 3, persisted: 2, overrides: { save } });
    await button(w, "Save project").trigger("click");
    await flushPromises();
    expect(hideWindow).not.toHaveBeenCalled();
    expect(w.get('[role="alert"]').text()).toContain("Not enough disk space");
  });

  it("Cancel keeps the editor open", async () => {
    const { w, hideWindow, closeSession } = await setup({ revision: 3, persisted: 2 });
    await button(w, "Cancel").trigger("click");
    await flushPromises();
    expect(hideWindow).not.toHaveBeenCalled();
    expect(closeSession).not.toHaveBeenCalled();
    expect(w.find('[data-testid="close-guard"]').exists()).toBe(false);
  });

  // The legacy phase-4 surface (still mounted until Task 59) has no
  // new-editor session: a close behaves exactly as it did before.
  it("with no session open, a close hides as before and reads no jobs", async () => {
    const { hideWindow, getJobs } = await setup({ session: false });
    expect(hideWindow).toHaveBeenCalledTimes(1);
    expect(getJobs).not.toHaveBeenCalled();
  });

  // The job registry being unreadable must not strand the window: the
  // guard still decides on what it does know (here: clean).
  it("an unreadable job registry falls back to the dirty/clean rule", async () => {
    const getJobs = vi.fn(async () => {
      throw new Error("ipc down");
    });
    const { hideWindow } = await setup({ revision: 4, persisted: 4, overrides: { getJobs } });
    expect(hideWindow).toHaveBeenCalledTimes(1);
  });
});

function opened(snap: EditorSnapshot): EditorOpenResult {
  return {
    snapshot: snap,
    project: {
      schema: "vault-buddy-video-project/3",
      id: snap.projectId,
      title: snap.title,
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
    },
    workspace: {},
    missing: [],
    sourceBase: "cap one",
    recovered: false,
  };
}

describe("EditorRoot close and recovery wiring", () => {
  // The legacy phase-4 surface alone (the store's real port, whose
  // `editor_open_staged` answer this mock cannot decode, so no session):
  // the X hides the window exactly as it did before Task 37.
  it("a close request with only the legacy editor open hides the window as before", async () => {
    const seen = mockEditor();
    mount(EditorRoot);
    await flushPromises();
    listeners["editor:closeRequested"]({ payload: {} });
    await flushPromises();
    expect(seen.filter((c) => c.cmd === "editor_hide_window")).toHaveLength(1);
  });

  it("a close request over a dirty session opens the close guard", async () => {
    mockEditor();
    const hideWindow = vi.fn(async () => {});
    useEditorProjectStore().setPort(
      fakeEditorPort({
        openStaged: async () => opened(snapshot(3, 2)),
        getJobs: async () => [],
        hideWindow,
        getWorkspace: async () => ({}),
      }),
    );
    const w = mount(EditorRoot, { attachTo: document.body });
    await flushPromises();
    listeners["editor:closeRequested"]({ payload: {} });
    await flushPromises();
    expect(w.find('[data-testid="close-guard"]').exists()).toBe(true);
    expect(hideWindow).not.toHaveBeenCalled();
  });

  it("a newly opened clean session with changes left behind offers recovery, and a resume re-hydrates", async () => {
    mockEditor();
    const project = useEditorProjectStore();
    project.setPort(
      fakeEditorPort({
        openStaged: async () => opened(snapshot(2, 2)),
        listProjects: async () => [
          {
            projectFileId: "proj-3",
            title: "My tutorial",
            updatedAt: "2026-09-21T10:00:00+02:00",
            persistedRevision: 2,
            hasRecovery: true,
            sourceBase: "cap one",
          },
        ],
        closeSession: async () => {},
        openProject: async () => opened({ ...snapshot(5, 2), sessionId: "ses-9" }),
      }),
    );
    const hydrated: string[] = [];
    useEditorWorkspaceStore().setPort(
      fakeEditorPort({
        getWorkspace: async (id) => {
          hydrated.push(id);
          return {};
        },
      }),
    );
    const w = mount(EditorRoot, { attachTo: document.body });
    await flushPromises();
    expect(w.find('[data-testid="recovery-dialog"]').exists()).toBe(true);
    await button(w, "Resume").trigger("click");
    await flushPromises();
    expect(project.sessionId).toBe("ses-9");
    expect(hydrated).toEqual(["ses-7", "ses-9"]);
  });
});
