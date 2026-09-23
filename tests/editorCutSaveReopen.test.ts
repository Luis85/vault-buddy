/**
 * Task 21's acceptance: "capture -> cut -> save -> reopen works through the
 * UI in the Vitest integration harness (mock port backed by an in-memory
 * fake that applies nothing locally -- the fake returns canned
 * projections)".
 *
 * The whole editor window (`EditorRoot`) is mounted, a staged capture is
 * opened through the store's `openStaged` seam, a clip is cut with the
 * keyboard (Ctrl+X through `EditorShell`'s dispatcher), the project is saved
 * with the header's own Save button, and the window is mounted AGAIN from a
 * fresh Pinia -- the reopen -- to prove what comes back is what was saved.
 *
 * The fake is deliberately dumb: it never interprets a command. It hands
 * back one of two CANNED projections (before/after the cut) keyed on the
 * command KIND it was sent, and `save` snapshots whatever projection is live.
 * So every assertion below is about what the UI SENT and what it RENDERS
 * from Rust's reply -- the frontend applying the cut to its own copy of the
 * project (an R14 violation) would show up as a render the fake never
 * returned.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { clearClipboardForTest, clipboardFragment } from "../src/editor/clipboard";
import type { EditorPort } from "../src/editor/port";
import type { Clip, EditorCommand, EditorOpenResult, EditorSnapshot, Project } from "../src/editorTypes";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { mockEditor } from "./helpers/editorMount";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
  clearClipboardForTest();
});

function clip(id: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "capture",
    track_id: "v1",
    name: id,
    start_ms: 0,
    in_ms: 0,
    out_ms: 1_000,
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
    ...overrides,
  };
}

function project(clips: Clip[]): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "capture", kind: "video", name: "cap one", duration_ms: 10_000 }],
    tracks: [{ id: "v1", kind: "video", name: "Video", visible: true, locked: false, muted: false, solo: false, volume: 1 }],
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}

function snapshot(revision: number, persistedRevision: number | null, durationMs: number): EditorSnapshot {
  return {
    sessionId: "ses-a",
    projectId: "project-a",
    revision,
    persistedRevision,
    title: "Tutorial",
    durationMs,
    canUndo: revision > 1,
    canRedo: false,
    undoLabel: revision > 1 ? "Cut" : null,
    redoLabel: null,
  };
}

// The capture split into an intro (0..1200) and the body (1200..4000); the
// cut removes the intro with the gap closed, so the body ripples to 0.
const BEFORE = project([
  clip("intro", { start_ms: 0, in_ms: 0, out_ms: 1_200 }),
  clip("body", { start_ms: 1_200, in_ms: 1_200, out_ms: 4_000 }),
]);
const AFTER_CUT = project([clip("body", { start_ms: 0, in_ms: 1_200, out_ms: 4_000 })]);

/** The in-memory fake: canned projections, a record of every call, and a
 * "disk" that holds whatever was live at the last save. */
function cannedPort() {
  const sent: EditorCommand[] = [];
  const saves: { sessionId: string; expectedRevision: number }[] = [];
  let live = { snapshot: snapshot(1, null, 4_000), project: BEFORE };
  let disk: typeof live | null = null;
  const port: EditorPort = {
    openStaged: (base) =>
      Promise.resolve<EditorOpenResult>({
        ...(disk ?? live),
        workspace: {},
        missing: [],
        sourceBase: base,
        recovered: false,
      }),
    execute: (req) => {
      sent.push(req.command);
      if (req.command.kind === "cutClips") live = { snapshot: snapshot(2, null, 2_800), project: AFTER_CUT };
      return Promise.resolve(live);
    },
    save: (sessionId, expectedRevision) => {
      saves.push({ sessionId, expectedRevision });
      live = { ...live, snapshot: { ...live.snapshot, persistedRevision: expectedRevision } };
      disk = live;
      return Promise.resolve({ sessionId, savedRevision: expectedRevision, projectFileId: "project-a" });
    },
    openProject: () => Promise.reject(new Error("not used")),
    listProjects: () => Promise.reject(new Error("not used")),
    getSnapshot: () => Promise.reject(new Error("not used")),
    closeSession: () => Promise.reject(new Error("not used")),
    hideWindow: () => Promise.reject(new Error("not used")),
    getWorkspace: () => Promise.resolve({}),
    saveWorkspace: () => Promise.resolve(),
    mediaUrl: () => Promise.reject(new Error("no media in this test")),
    importMedia: () => Promise.reject(new Error("not used")),
    cancelJob: () => Promise.reject(new Error("not used")),
    getJobs: () => Promise.resolve([]),
  };
  return { port, sent, saves };
}

async function openWindow(port: EditorPort) {
  mockEditor();
  useEditorProjectStore().setPort(port);
  const w = mount(EditorRoot, { attachTo: document.body });
  await flushPromises();
  return w;
}

describe("capture -> cut -> save -> reopen (Task 21 acceptance)", () => {
  it("cuts through the UI, saves, and reopens to exactly what Rust acknowledged", async () => {
    const fake = cannedPort();
    const w = await openWindow(fake.port);

    // The capture opened: both clips on the timeline, nothing saved yet.
    expect(w.find('[data-testid="clip-intro"]').exists()).toBe(true);
    expect(w.find('[data-testid="clip-body"]').exists()).toBe(true);

    // Select the intro and cut it from the keyboard.
    const intro = w.get('[data-testid="clip-intro"]');
    await intro.trigger("click");
    (intro.element as HTMLElement).focus();
    await intro.trigger("keydown", { key: "x", ctrlKey: true });
    await flushPromises();

    // ONE command reached the fake -- the cut, one undo step -- and the
    // clipboard (window-local, never sent) holds the intro.
    expect(fake.sent).toEqual([{ kind: "cutClips", clipIds: ["intro"], closeGap: true }]);
    expect(clipboardFragment.value?.fragment.clips.map((c) => c.id)).toEqual(["intro"]);
    // The timeline renders Rust's canned reply, not a local edit.
    expect(w.find('[data-testid="clip-intro"]').exists()).toBe(false);
    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Unsaved changes");

    // Save through the header's own button.
    await w.get('[data-testid="editor-header-save"]').trigger("click");
    await flushPromises();
    expect(fake.saves).toEqual([{ sessionId: "ses-a", expectedRevision: 2 }]);
    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Saved");

    // Reopen: a brand-new window (fresh Pinia) over the same fake "disk".
    w.unmount();
    setActivePinia(createPinia());
    const reopened = await openWindow(fake.port);

    expect(reopened.find('[data-testid="clip-intro"]').exists()).toBe(false);
    const body = reopened.get('[data-testid="clip-body"]');
    expect(body.attributes("aria-label")).toBe("Clip body, 0:00–0:02");
    expect(reopened.get('[data-testid="editor-header-status"]').text()).toBe("Saved");
    expect(fake.sent).toHaveLength(1); // reopening sent no command of its own
  });
});
