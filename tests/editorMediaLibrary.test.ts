/**
 * `MediaLibrary.vue` (Task 25; F-02; visual-parity Task 9, concept spec
 * §3.2): the actions row (Import media / Webcam), the search box, the
 * "SOURCE MEDIA" heading with its asset-count pill, the asset rows
 * (thumbnail, meta text, add/reconnect) and their asset context menu, and
 * "+"/Enter — insert at the playhead onto the first compatible, unlocked
 * track FREE for the clip's span, else onto a new one (`placeOnFreeTrack`,
 * visual-parity Task 7; its own suite, `editorPlaceOnFreeTrack.test.ts`,
 * pins the rule and the addTrack-then-insert sequence).
 */
import { clearMocks, mockConvertFileSrc } from "@tauri-apps/api/mocks";
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

afterEach(() => {
  clearMocks();
});

// Asymmetric: the FIRST video track is locked and an audio track sits
// between, so "the first video track" and "the first unlocked compatible
// track" are different answers.
function track(id: string, kind: "video" | "audio", locked = false): Track {
  return { id, kind, name: id, visible: true, locked, muted: false, solo: false, volume: 1 };
}

// `vid` carries dimensions (the meta line's "W × H" case); `pic` does not
// (the fallback case — an image or a video whose probe hasn't reported
// dimensions yet); `aud` is audio ("Local audio"); `gone` is missing
// ("Missing source", overriding everything else).
const ASSETS: Asset[] = [
  { id: "vid", kind: "video", name: "Screen take.mp4", duration_ms: 12_000, width: 1920, height: 1080 },
  { id: "aud", kind: "audio", name: "Voice over.mp3", duration_ms: 7_500 },
  { id: "pic", kind: "video", name: "Diagram.png", duration_ms: 5_000, media_type: "image" },
  { id: "gone", kind: "video", name: "Old clip.mov", duration_ms: 3_000 },
];

function project(tracks: Track[], assets: Asset[] = ASSETS): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets,
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
  assets: Asset[] = ASSETS,
) {
  executed = [];
  const p = project(tracks, assets);
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
    mediaThumbnail: () => Promise.reject(new Error("no thumbnail stubbed")),
    ...extra,
  });
  const store = useEditorProjectStore();
  store.setPort(port);
  await store.openStaged("base");
  useEditorWorkspaceStore().playheadMs = 4_200;
  const w = mount(MediaLibrary, { attachTo: document.body });
  await flushPromises();
  return w;
}

describe("MediaLibrary — insert", () => {
  it("library + inserts at the playhead on the first unlocked compatible track", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="library-asset-vid-add"]').trigger("click");
    await w.get('[data-testid="library-asset-aud-add"]').trigger("click");
    await w.get('[data-testid="library-asset-pic-add"]').trigger("click");
    expect(executed).toEqual([
      { kind: "insertClip", assetId: "vid", trackId: "v2", startMs: 4_200, inMs: 0, outMs: 12_000 },
      { kind: "insertClip", assetId: "aud", trackId: "a1", startMs: 4_200, inMs: 0, outMs: 7_500 },
      { kind: "insertClip", assetId: "pic", trackId: "v2", startMs: 4_200, inMs: 0, outMs: 5_000 },
    ]);
  });

  // Audit finding 1a: "+" used to be refused here ("Add an unlocked video
  // track first") — now it adds the track itself. This fake never mints
  // the track, so only the `addTrack` is sent: an insert never follows a
  // track that did not land.
  it("+ adds a track above the top video track when no unlocked one is free", async () => {
    const w = await mountLibrary([track("v-locked", "video", true), track("a1", "audio")]);
    const button = w.get('[data-testid="library-asset-vid-add"]');
    expect(button.attributes("title")).toBe("Add at the playhead on a new video track");
    await button.trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "addTrack", trackKind: "video", name: "Video 2", index: 0 }]);
  });

  it("Enter on the focused row runs its own primary action", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="library-asset-vid"]').trigger("keydown", { key: "Enter" });
    expect(executed).toEqual([
      { kind: "insertClip", assetId: "vid", trackId: "v2", startMs: 4_200, inMs: 0, outMs: 12_000 },
    ]);
  });
});

describe("MediaLibrary — actions row (concept §3.2)", () => {
  it("Import media and Webcam are 40px tall with icons", async () => {
    const w = await mountLibrary();
    const importBtn = w.get('[data-testid="library-import"]');
    const webcamBtn = w.get('[data-testid="library-webcam"]');
    expect(importBtn.classes()).toContain("h-10");
    expect(webcamBtn.classes()).toContain("h-10");
    expect(importBtn.text()).toContain("Import media");
    expect(webcamBtn.text()).toContain("Webcam");
    expect(importBtn.find("svg").exists()).toBe(true);
    expect(webcamBtn.find("svg").exists()).toBe(true);
  });

  it("the search input has a search icon and the concept's placeholder, and filters by name", async () => {
    const w = await mountLibrary();
    const search = w.get('[data-testid="library-search"]');
    expect(search.attributes("placeholder")).toBe("Find media…");
    expect(search.attributes("aria-label")).toBe("Find media");
    expect(search.element.parentElement?.querySelector("svg")).not.toBeNull();

    await search.setValue("voice");
    expect(w.find('[data-testid="library-asset-aud"]').exists()).toBe(true);
    expect(w.find('[data-testid="library-asset-vid"]').exists()).toBe(false);
  });
});

describe("MediaLibrary — heading (concept §3.2)", () => {
  it('shows "SOURCE MEDIA" with the asset-count pill, unaffected by the search filter', async () => {
    const w = await mountLibrary();
    expect(w.text()).toContain("SOURCE MEDIA");
    expect(w.text()).toContain("4 assets");
    await w.get('[data-testid="library-search"]').setValue("voice");
    expect(w.text()).toContain("4 assets");
  });
});

describe("MediaLibrary — rows: thumbnails, meta and trailing action", () => {
  it("renders a 58×40 thumbnail for a video asset, at 1 000 ms", async () => {
    mockConvertFileSrc("windows");
    const mediaThumbnail = vi.fn(() => Promise.resolve("C:\\cache\\vid-1000.jpg"));
    const w = await mountLibrary(undefined, { mediaThumbnail });
    await flushPromises();
    expect(mediaThumbnail).toHaveBeenCalledWith("ses-a", "vid", 1_000);
    const thumb = w.get('[data-testid="library-asset-vid-thumb"]');
    expect(thumb.classes()).toContain("w-[58px]");
    expect(thumb.classes()).toContain("h-10");
    expect(thumb.find("img").attributes("src")).toContain("vid-1000.jpg");
  });

  // A 1 200 ms asset is shorter than the 2 000 ms the 1 000 ms instant needs
  // headroom for, so the rule falls back to the asset's own midpoint: 600.
  it("thumbnails at the asset's own midpoint when it is shorter than 2 s", async () => {
    const shortAssets = [{ ...ASSETS[0], duration_ms: 1_200 }, ...ASSETS.slice(1)];
    const mediaThumbnail = vi.fn(() => Promise.reject(new Error("unused")));
    await mountLibrary(undefined, { mediaThumbnail }, [], shortAssets);
    await flushPromises();
    expect(mediaThumbnail).toHaveBeenCalledWith("ses-a", "vid", 600);
  });

  it("shows a teal audio tile for an audio asset and never asks for a thumbnail", async () => {
    const mediaThumbnail = vi.fn(() => Promise.reject(new Error("unused")));
    const w = await mountLibrary(undefined, { mediaThumbnail });
    await flushPromises();
    expect(mediaThumbnail).not.toHaveBeenCalledWith("ses-a", "aud", expect.anything());
    const card = w.get('[data-testid="library-asset-aud"]');
    expect(card.find("img").exists()).toBe(false);
    expect(card.find(".bg-audio-bg").exists()).toBe(true);
  });

  it("shows W × H for video, Local audio for audio, and Missing source for a missing asset", async () => {
    const w = await mountLibrary();
    expect(w.get('[data-testid="library-asset-vid"]').text()).toContain("0:12 · 1920 × 1080");
    expect(w.get('[data-testid="library-asset-aud"]').text()).toContain("0:07 · Local audio");
    expect(w.get('[data-testid="library-asset-gone"]').text()).toContain("Missing source");
  });

  it("has an Add button for an available asset and a Reconnect button that opens the dialog for a missing one", async () => {
    const w = await mountLibrary();
    const add = w.get('[data-testid="library-asset-vid-add"]');
    expect(add.attributes("aria-label")).toBe("Add Screen take.mp4 to timeline");

    const reconnect = w.get('[data-testid="library-asset-gone-add"]');
    expect(reconnect.attributes("aria-label")).toBe("Reconnect Old clip.mov");
    expect(w.find('[data-testid="reconnect-row-gone"]').exists()).toBe(false);
    await reconnect.trigger("click");
    await flushPromises();
    expect(document.body.querySelector('[data-testid="reconnect-row-gone"]')).not.toBeNull();
  });

  it("a 120-character asset name ellipsizes (never widens the row): the name carries the truncate class", async () => {
    const longName = `${"A".repeat(120)}.mp4`;
    const longAssets = [...ASSETS, { id: "long", kind: "video" as const, name: longName, duration_ms: 4_000 }];
    const w = await mountLibrary(undefined, {}, [], longAssets);
    const nameEl = w.get('[data-testid="library-asset-long"] b');
    expect(nameEl.classes()).toContain("truncate");
    expect(nameEl.text()).toBe(longName);
    // The name column shrinks, never the fixed-size thumbnail or button.
    expect(w.get('[data-testid="library-asset-long-thumb"]').classes()).toContain("w-[58px]");
    expect(w.get('[data-testid="library-asset-long-add"]').classes()).toContain("w-7");
  });
});

describe("MediaLibrary — empty state", () => {
  it("an empty library shows guidance instead of a blank list", async () => {
    const w = await mountLibrary(undefined, {}, [], []);
    expect(w.text()).toContain("No media yet. Import video, audio or images.");
    expect(w.text()).toContain("0 assets");
  });

  it("a search with no match says so", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="library-search"]').setValue("nothing matches this");
    expect(w.text()).toContain("No media matches the search.");
  });
});

describe("MediaLibrary — asset context menu (visual-parity Task 9, concept spec §8)", () => {
  it("right-click opens the asset menu, headed by the asset's own name", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="library-asset-vid"]').trigger("contextmenu");
    await flushPromises();
    expect(w.find('[data-testid="editor-context-menu"]').exists()).toBe(true);
    expect(w.get('[data-testid="editor-context-menu-heading"]').text()).toBe("Screen take.mp4");
  });

  it("Shift+F10 opens the same menu from the keyboard, and offers Reconnect for a missing asset", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="library-asset-gone"]').trigger("keydown", { key: "F10", shiftKey: true });
    await flushPromises();
    expect(w.get('[data-testid="editor-context-menu-heading"]').text()).toBe("Old clip.mov");
    expect(w.text()).toContain("Reconnect original…");
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
    const bar = w.get('[data-testid="library-import-progress"]');
    // Fix round 1 (review Important): the track was `bg-white/10`, ~1.1:1 on
    // the light theme's white/near-white panels — WCAG 1.4.11 wants ~3:1 for
    // a UI component boundary. e2e can't easily drive an import mid-flight
    // (per the fix-round instruction), so the token is pinned here instead:
    // `bg-track` gives 4.70:1 light / (byte-identical) 1.36:1 dark against
    // `bg-panel`, measured in the token's own style.css comment.
    expect(bar.classes()).toContain("bg-track");
    expect(bar.classes()).not.toContain("bg-white/10");
    expect(bar.attributes("aria-valuenow")).toBe("50");
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
