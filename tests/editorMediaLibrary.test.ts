/**
 * `MediaLibrary.vue` (Task 25; F-02): search, asset cards (kind, duration,
 * availability), Import (Rust opens its own dialog), the running job's
 * progress + Cancel, the last import's per-file errors, and "+" — insert
 * at the playhead onto the FIRST compatible, unlocked track
 * (`trackCompat.firstAcceptingTrack`, the same rule the timeline drag
 * applies).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import MediaLibrary from "../src/components/editor/library/MediaLibrary.vue";
import type { EditorPort } from "../src/editor/port";
import type {
  Asset,
  EditorCommand,
  EditorOpenResult,
  JobProgressDto,
  MissingMedia,
  Project,
  Track,
} from "../src/editorTypes";
import { useEditorJobsStore } from "../src/stores/editorJobs";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

// Asymmetric: the FIRST video track is locked and an audio track sits
// between, so "the first video track" and "the first unlocked compatible
// track" are different answers.
function track(id: string, kind: "video" | "audio", locked = false): Track {
  return { id, kind, name: id, visible: true, locked, muted: false, solo: false, volume: 1 };
}

const ASSETS: Asset[] = [
  { id: "vid", kind: "video", name: "Screen take.mp4", duration_ms: 12_000 },
  { id: "aud", kind: "audio", name: "Voice over.mp3", duration_ms: 7_500 },
  { id: "pic", kind: "video", name: "Diagram.png", duration_ms: 5_000, media_type: "image" },
  { id: "gone", kind: "video", name: "Old clip.mov", duration_ms: 3_000 },
];

function project(tracks: Track[]): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: ASSETS,
    tracks,
    clips: [],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}

let executed: EditorCommand[] = [];

async function mountLibrary(
  tracks: Track[] = [track("v-locked", "video", true), track("a1", "audio"), track("v2", "video")],
  extra: Partial<EditorPort> = {},
  missing: MissingMedia[] = [{ assetId: "gone", name: "Old clip.mov", expectedSize: 10, expectedDurationMs: 3_000 }],
) {
  executed = [];
  const p = project(tracks);
  const open: EditorOpenResult = {
    snapshot: {
      sessionId: "ses-a",
      projectId: "project-a",
      revision: 5,
      persistedRevision: 5,
      title: "Tutorial",
      durationMs: 60_000,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    },
    project: p,
    workspace: {},
    missing,
    sourceBase: "base",
    recovered: false,
  };
  const port = fakeEditorPort({
    openStaged: () => Promise.resolve(open),
    execute: (req) => {
      executed.push(req.command);
      return Promise.resolve({ snapshot: { ...open.snapshot, revision: 6 }, project: p });
    },
    getJobs: () => Promise.resolve([]),
    ...extra,
  });
  const store = useEditorProjectStore();
  store.setPort(port);
  await store.openStaged("base");
  useEditorWorkspaceStore().playheadMs = 4_200;
  const w = mount(MediaLibrary);
  await flushPromises();
  return w;
}

describe("MediaLibrary — insert", () => {
  it("library + inserts at the playhead on the first unlocked compatible track", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="library-asset-vid-insert"]').trigger("click");
    await w.get('[data-testid="library-asset-aud-insert"]').trigger("click");
    await w.get('[data-testid="library-asset-pic-insert"]').trigger("click");
    expect(executed).toEqual([
      { kind: "insertClip", assetId: "vid", trackId: "v2", startMs: 4_200, inMs: 0, outMs: 12_000 },
      { kind: "insertClip", assetId: "aud", trackId: "a1", startMs: 4_200, inMs: 0, outMs: 7_500 },
      { kind: "insertClip", assetId: "pic", trackId: "v2", startMs: 4_200, inMs: 0, outMs: 5_000 },
    ]);
  });

  it("+ is refused, with a reason, when no unlocked track of that kind exists", async () => {
    const w = await mountLibrary([track("v-locked", "video", true), track("a1", "audio")]);
    const button = w.get('[data-testid="library-asset-vid-insert"]');
    expect(button.attributes("aria-disabled")).toBe("true");
    expect(button.attributes("title")).toMatch(/unlocked video track/i);
    await button.trigger("click");
    expect(executed).toEqual([]);
  });

  it("a missing asset says so and cannot be inserted", async () => {
    const w = await mountLibrary();
    const card = w.get('[data-testid="library-asset-gone"]');
    expect(card.text()).toContain("Missing");
    await w.get('[data-testid="library-asset-gone-insert"]').trigger("click");
    expect(executed).toEqual([]);
  });
});

describe("MediaLibrary — cards and search", () => {
  it("shows each asset's kind and duration, and filters by name", async () => {
    const w = await mountLibrary();
    expect(w.get('[data-testid="library-asset-vid"]').text()).toContain("Video");
    expect(w.get('[data-testid="library-asset-vid"]').text()).toContain("0:12");
    expect(w.get('[data-testid="library-asset-aud"]').text()).toContain("Audio");
    expect(w.get('[data-testid="library-asset-pic"]').text()).toContain("Image");

    await w.get('[data-testid="library-search"]').setValue("voice");
    expect(w.find('[data-testid="library-asset-aud"]').exists()).toBe(true);
    expect(w.find('[data-testid="library-asset-vid"]').exists()).toBe(false);
  });
});

describe("MediaLibrary — import", () => {
  it("Import starts a job, shows its progress, cancels it, and lists per-file errors at the end", async () => {
    let deliver: (m: JobProgressDto) => void = () => {};
    const cancelJob = vi.fn(() => Promise.resolve());
    const getSnapshot = vi.fn(() => new Promise<never>(() => {}));
    const w = await mountLibrary(undefined, {
      importMedia: (_s, onProgress) => {
        deliver = onProgress;
        return Promise.resolve({ jobId: "job-1" });
      },
      cancelJob,
      getSnapshot,
    });
    await w.get('[data-testid="library-import"]').trigger("click");
    await flushPromises();
    const base = { sessionId: "ses-a", jobId: "job-1", kind: "import" as const, terminal: null };
    deliver({ ...base, sequence: 2, phase: "preparing", fraction: 0.5 });
    await flushPromises();
    expect(w.get('[data-testid="library-import-progress"]').attributes("aria-valuenow")).toBe("50");
    expect(w.get('[data-testid="library-import"]').attributes("aria-disabled")).toBe("true");

    await w.get('[data-testid="library-import-cancel"]').trigger("click");
    expect(cancelJob).toHaveBeenCalledWith("ses-a", "job-1");

    deliver({
      ...base,
      sequence: 3,
      phase: "cancelled",
      fraction: 1,
      terminal: { assetIds: ["new-1"], perFile: [{ name: "broken.mov", error: "It may be damaged." }] },
    });
    await flushPromises();
    expect(w.find('[data-testid="library-import-progress"]').exists()).toBe(false);
    const summary = w.get('[data-testid="library-import-summary"]').text();
    expect(summary).toContain("1");
    expect(summary).toContain("broken.mov");
    expect(summary).toContain("It may be damaged.");
    expect(useEditorJobsStore().lastImport?.phase).toBe("cancelled");
  });

  it("summarises a completed import (singular) and a failed one (its error)", async () => {
    let deliver: (m: JobProgressDto) => void = () => {};
    let started = 0;
    const w = await mountLibrary(undefined, {
      importMedia: (_s, onProgress) => {
        deliver = onProgress;
        started += 1;
        return Promise.resolve({ jobId: `job-${started}` });
      },
      getSnapshot: () => new Promise<never>(() => {}),
    });
    await w.get('[data-testid="library-import"]').trigger("click");
    await flushPromises();
    const base = { sessionId: "ses-a", jobId: "job-1", kind: "import" as const };
    deliver({ ...base, sequence: 2, phase: "complete", fraction: 1, terminal: { assetIds: ["n1"], perFile: [] } });
    await flushPromises();
    expect(w.get('[data-testid="library-import-summary"]').text()).toContain("Imported 1 file.");

    // A second import, which fails as a whole (the session ended under it).
    await w.get('[data-testid="library-import"]').trigger("click");
    await flushPromises();
    deliver({
      ...base,
      jobId: "job-2",
      sequence: 2,
      phase: "failed",
      fraction: 1,
      terminal: {
        assetIds: [],
        perFile: [],
        error: { code: "sessionGone", message: "This editing session has ended.", retryable: false, operationId: "op-1" },
      },
    });
    await flushPromises();
    expect(w.get('[data-testid="library-import-summary"]').text()).toContain(
      "Import failed: This editing session has ended.",
    );
  });

  it("a refused start surfaces its error", async () => {
    const w = await mountLibrary(undefined, {
      importMedia: () =>
        Promise.reject(new Error("This editing session has ended.")),
    });
    await w.get('[data-testid="library-import"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="library-error"]').text()).toContain("session has ended");
  });
});
