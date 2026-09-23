/**
 * The recovery dialog (Task 37 Part A; F-44; A27). When the editor opens a
 * CLEAN session over a project that still has a `recovery.json` from an
 * earlier process, the user chooses — Resume (reopen with the journal as
 * the working copy) or Discard (delete only the journal) — before editing,
 * so no new edit can journal over what was left behind.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logBreadcrumb: vi.fn(), logWarning: vi.fn() }));

import RecoveryDialog from "../src/components/editor/dialogs/RecoveryDialog.vue";
import type { EditorPort } from "../src/editor/port";
import { EditorPortError } from "../src/editor/port";
import type { EditorOpenResult, EditorSnapshot, ProjectSummaryDto } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function snapshot(overrides: Partial<EditorSnapshot> = {}): EditorSnapshot {
  return {
    sessionId: "ses-1",
    projectId: "proj-3",
    revision: 2,
    persistedRevision: 2,
    title: "Tutorial",
    durationMs: 61_500,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
    ...overrides,
  };
}

function openResult(snap: EditorSnapshot, recovered: boolean): EditorOpenResult {
  return {
    snapshot: snap,
    project: {
      schema: "vault-buddy-video-project/3",
      id: "proj-3",
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
    recovered,
  };
}

function summary(overrides: Partial<ProjectSummaryDto> = {}): ProjectSummaryDto {
  return {
    projectFileId: "proj-3",
    title: "My tutorial",
    updatedAt: "2026-09-21T10:00:00+02:00",
    persistedRevision: 2,
    hasRecovery: true,
    sourceBase: "cap one",
    ...overrides,
  };
}

async function setup(opts: {
  snap?: EditorSnapshot;
  rows?: ProjectSummaryDto[];
  overrides?: Partial<EditorPort>;
}) {
  const closeSession = vi.fn(async () => {});
  const listProjects = vi.fn(async () => opts.rows ?? [summary()]);
  const openProject = vi.fn(async (_id: string, useRecovery: boolean) =>
    openResult(
      useRecovery
        ? snapshot({ sessionId: "ses-2", revision: 5, persistedRevision: 2, title: "Recovered" })
        : snapshot({ sessionId: "ses-3" }),
      useRecovery,
    ),
  );
  const store = useEditorProjectStore();
  store.setPort(fakeEditorPort({ closeSession, listProjects, openProject, ...opts.overrides }));
  store.sessionId = (opts.snap ?? snapshot()).sessionId;
  store.snapshot = opts.snap ?? snapshot();
  const w = mount(RecoveryDialog, { attachTo: document.body });
  await (w.vm as unknown as { check(): Promise<void> }).check();
  await flushPromises();
  return { w, store, closeSession, listProjects, openProject };
}

function button(w: ReturnType<typeof mount>, label: string) {
  const found = w.findAll("button").find((b) => b.text() === label);
  if (!found) throw new Error(`no button "${label}" in: ${w.text()}`);
  return found;
}

describe("RecoveryDialog", () => {
  it("offers Resume and Discard, naming the project, for a clean session with unsaved changes left over", async () => {
    const { w } = await setup({});
    const dialog = w.get('[data-testid="recovery-dialog"]');
    expect(dialog.text()).toContain("My tutorial");
    const labels = w.findAll("button").map((b) => b.text());
    expect(labels).toEqual(expect.arrayContaining(["Resume", "Discard"]));
  });

  // A dirty session's journal is its OWN (a "Keep for later" window
  // reopened): nothing was left behind by an earlier process.
  it("stays hidden when the open session is itself dirty", async () => {
    const { w, listProjects } = await setup({ snap: snapshot({ revision: 4, persistedRevision: 2 }) });
    expect(w.find('[data-testid="recovery-dialog"]').exists()).toBe(false);
    expect(listProjects).not.toHaveBeenCalled();
  });

  // Fix round 1: an edit acknowledged while the listing was in flight makes
  // the session's journal its OWN — offering Resume then would have its
  // `keep` flush the new edits over the earlier run's file.
  it("stays hidden when the session became dirty while the projects were being listed", async () => {
    const store = useEditorProjectStore();
    const listProjects = vi.fn(async () => {
      store.snapshot = snapshot({ revision: 3, persistedRevision: 2 });
      return [summary()];
    });
    const { w } = await setup({ overrides: { listProjects } });
    expect(listProjects).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="recovery-dialog"]').exists()).toBe(false);
  });

  it("stays hidden when the project has no recovery file", async () => {
    const { w } = await setup({ rows: [summary({ hasRecovery: false }), summary({ projectFileId: "other" })] });
    expect(w.find('[data-testid="recovery-dialog"]').exists()).toBe(false);
  });

  it("Resume closes the clean session and reopens with the journal as the working copy", async () => {
    const { w, store, closeSession, openProject } = await setup({});
    await button(w, "Resume").trigger("click");
    await flushPromises();
    expect(closeSession).toHaveBeenCalledWith("ses-1", "keep");
    expect(openProject).toHaveBeenCalledWith("proj-3", true);
    expect(closeSession.mock.invocationCallOrder[0]).toBeLessThan(
      openProject.mock.invocationCallOrder[0],
    );
    expect(store.sessionId).toBe("ses-2");
    expect(store.dirty).toBe(true);
    expect(w.find('[data-testid="recovery-dialog"]').exists()).toBe(false);
    expect(w.emitted("session-changed")).toHaveLength(1);
  });

  it("Discard deletes only the journal, then reopens the saved project", async () => {
    const { w, closeSession, openProject, store } = await setup({});
    await button(w, "Discard").trigger("click");
    await flushPromises();
    expect(closeSession).toHaveBeenCalledWith("ses-1", "discardRecovery");
    expect(openProject).toHaveBeenCalledWith("proj-3", false);
    expect(store.sessionId).toBe("ses-3");
    expect(w.find('[data-testid="recovery-dialog"]').exists()).toBe(false);
    expect(w.emitted("session-changed")).toHaveLength(1);
  });

  // A27: an unreadable journal is explained as TEXT (never markup), says
  // the saved project was not changed, and still leaves a way forward.
  it("a malformed journal is reported safely and the saved project can still be opened", async () => {
    const openProject = vi.fn(async (_id: string, useRecovery: boolean) => {
      if (useRecovery) {
        throw new EditorPortError({
          code: "invalidProject",
          message: "The unsaved changes could not be read: <b>expected value</b>",
          retryable: false,
          operationId: "op-2",
        });
      }
      return openResult(snapshot({ sessionId: "ses-3" }), false);
    });
    const { w, store } = await setup({ overrides: { openProject } });
    await button(w, "Resume").trigger("click");
    await flushPromises();
    const dialog = w.get('[data-testid="recovery-dialog"]');
    expect(dialog.text()).toContain("<b>expected value</b>");
    expect(dialog.find("b").exists()).toBe(false);
    expect(dialog.text()).toContain("Your saved project was not changed.");
    await button(w, "Open saved project").trigger("click");
    await flushPromises();
    expect(openProject).toHaveBeenLastCalledWith("proj-3", false);
    expect(store.sessionId).toBe("ses-3");
    expect(w.find('[data-testid="recovery-dialog"]').exists()).toBe(false);
  });

  // After a failed Resume no session is open; Discard must still work —
  // it reopens the saved project first so there is a session to discard
  // the journal through.
  it("Discard after a failed Resume still discards the journal", async () => {
    const closeSession = vi.fn(async () => {});
    const openProject = vi.fn(async (_id: string, useRecovery: boolean) => {
      if (useRecovery) {
        throw new EditorPortError({ code: "invalidProject", message: "bad", retryable: false, operationId: "op" });
      }
      return openResult(snapshot({ sessionId: `ses-${openProject.mock.calls.length + 10}` }), false);
    });
    const { w } = await setup({ overrides: { openProject, closeSession } });
    await button(w, "Resume").trigger("click");
    await flushPromises();
    await button(w, "Discard").trigger("click");
    await flushPromises();
    expect(closeSession).toHaveBeenLastCalledWith("ses-12", "discardRecovery");
    expect(openProject).toHaveBeenLastCalledWith("proj-3", false);
  });
});
