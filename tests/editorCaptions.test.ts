/**
 * `CaptionsLibrary.vue` + `src/editor/captionRules.ts` (Task 36; F-34,
 * F-35): the cue list in OUTPUT time, Import / Add at playhead / Split at
 * playhead, the caption settings, and the density/overlap notices with
 * their "Select cue" action.
 *
 * One asymmetric fixture (the "fixture flaw" rule): clip `c1` sits at
 * output 1000, plays source [2000, 12000) at speed 2 -- so a cue's SOURCE
 * time (what the wire carries) and its OUTPUT time (what the list shows)
 * are never the same number, and confusing them shows.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import CaptionsLibrary from "../src/components/editor/library/CaptionsLibrary.vue";
import LibraryPanel from "../src/components/editor/library/LibraryPanel.vue";
import CaptionOverlay from "../src/components/editor/preview/CaptionOverlay.vue";
import {
  addCaptionAt,
  addMarkerAt,
  captionNotices,
  captionRows,
  DENSITY_LIMIT_CPS,
} from "../src/editor/captionRules";
import type {
  CaptionCue,
  CaptionImportResult,
  Clip,
  EditorCommand,
  EditorOpenResult,
  EditorProjection,
  Project,
} from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function clip(overrides: Partial<Clip> = {}): Clip {
  return {
    id: "c1",
    asset_id: "av",
    track_id: "v1",
    name: "Screen",
    start_ms: 1_000,
    in_ms: 2_000,
    out_ms: 12_000,
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
    speed: 2,
    ...overrides,
  } as Clip;
}

function cue(id: string, start_ms: number, end_ms: number, text: string): CaptionCue {
  return { id, clip_id: "c1", start_ms, end_ms, text };
}

// Output spans at speed 2 from output 1000:
// capA [2000,4000) -> [1000,2000), 11 chars in 1 s
// capB [4000,6000) -> [2000,3000), 21 chars in 1 s: over the 20 cps line
// capC [5000,7000) -> [2500,3500): overlaps capB
const DENSE_TEXT = "abcdefghijklmnopqrstu"; // 21 characters
function cues(): CaptionCue[] {
  return [
    cue("capA", 2_000, 4_000, "Hello there"),
    cue("capB", 4_000, 6_000, DENSE_TEXT),
    cue("capC", 5_000, 7_000, "Overlap"),
  ];
}

function project(captionCues: CaptionCue[] = cues(), clips: Clip[] = [clip()]): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "av", kind: "video", name: "demo.mp4", duration_ms: 60_000 }],
    tracks: [{ id: "v1", kind: "video", name: "Screen", visible: true, locked: false, muted: false, solo: false, volume: 1 }],
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: {
      enabled: true,
      burn_in: true,
      font_size: 30,
      position: "bottom",
      background: true,
      cues: captionCues,
    },
    destination: { vault: "vault-a", folder: "", dated: false },
  } as Project;
}

const REFUSAL = { code: "invalidRequest", message: "Refused by Rust", retryable: false, operationId: "op-1" };

let executed: EditorCommand[] = [];
/** When set, every `execute` is refused the way Rust refuses one. */
let refuse = false;
let importCalls: { sessionId: string; clipId: string; replace: boolean }[] = [];

function snapshot(revision: number): EditorOpenResult["snapshot"] {
  return {
    sessionId: "ses-a",
    projectId: "project-a",
    revision,
    persistedRevision: 1,
    title: "Tutorial",
    durationMs: 6_000,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
  };
}

async function open(
  p: Project,
  importReply: CaptionImportResult | null = null,
  playheadMs = 1_500,
) {
  // A fresh store per open: `openStaged` short-circuits a re-open of the
  // same base, which a second mount in one test would otherwise hit.
  setActivePinia(createPinia());
  executed = [];
  importCalls = [];
  refuse = false;
  const port = fakeEditorPort({
    openStaged: () =>
      Promise.resolve({
        snapshot: snapshot(1),
        project: p,
        workspace: {},
        missing: [],
        sourceBase: "base",
        recovered: false,
      }),
    execute: (req): Promise<EditorProjection> => {
      executed.push(req.command);
      if (refuse) return Promise.reject(REFUSAL);
      return Promise.resolve({ snapshot: snapshot(2), project: p });
    },
    importCaptions: (sessionId, clipId, replace) => {
      importCalls.push({ sessionId, clipId, replace });
      return Promise.resolve(importReply);
    },
  });
  const store = useEditorProjectStore();
  store.setPort(port);
  await store.openStaged("base");
  useEditorWorkspaceStore().playheadMs = playheadMs;
}

async function mountLibrary(p: Project = project(), importReply: CaptionImportResult | null = null, playheadMs = 1_500) {
  await open(p, importReply, playheadMs);
  const w = mount(CaptionsLibrary, { attachTo: document.body });
  await flushPromises();
  return w;
}

// ---- the brief's named tests ------------------------------------------------

describe("CaptionsLibrary notices", () => {
  it("density notice appears above 20 chars per second", async () => {
    expect(DENSITY_LIMIT_CPS).toBe(20);
    const w = await mountLibrary();
    expect(w.find('[data-testid="caption-notice-density-capB"]').exists()).toBe(true);
    expect(w.find('[data-testid="caption-notice-density-capA"]').exists()).toBe(false);

    // Exactly 20 characters in the same one second: AT the line, not above it.
    const atLimit = project([cue("capB", 4_000, 6_000, DENSE_TEXT.slice(0, 20))]);
    const w2 = await mountLibrary(atLimit);
    expect(w2.find('[data-testid="caption-notice-density-capB"]').exists()).toBe(false);
  });

  it("overlap notice selects the cue", async () => {
    const w = await mountLibrary();
    const notice = w.get('[data-testid="caption-notice-overlap-capC"]');
    expect(notice.text()).toContain("overlap");
    await notice.get("button").trigger("click");
    const workspace = useEditorWorkspaceStore();
    expect(workspace.selected).toEqual({ type: "caption", id: "capC" });
    // The playhead jumps to the cue's OUTPUT start (source 5000 at speed 2
    // from output 1000 = 2500), and the row says it is the selected one.
    expect(workspace.playheadMs).toBe(2_500);
    expect(w.get('[data-testid="caption-row-capC"]').attributes("aria-current")).toBe("true");
    expect(w.get('[data-testid="caption-row-capB"]').attributes("aria-current")).toBeUndefined();
  });

  it("library never claims automatic transcription", async () => {
    const w = await mountLibrary();
    expect(w.get('[data-testid="caption-transcription-note"]').text()).toContain(
      "Automatic transcription is not available",
    );
    for (const control of w.findAll("button, a, [role='button'], label")) {
      expect(control.text()).not.toMatch(/transcri/i);
    }
    // And it says so even with nothing selected and no captions yet.
    const empty = await mountLibrary(project([]));
    expect(empty.text()).toContain("Automatic transcription is not available");
  });
});

// ---- the pure rules ---------------------------------------------------------

describe("captionRules", () => {
  it("rows are in OUTPUT time, sorted, with their reading speed", () => {
    const rows = captionRows(project([cues()[2], cues()[0], cues()[1]]));
    expect(rows.map((r) => [r.cue.id, r.startMs, r.endMs])).toEqual([
      ["capA", 1_000, 2_000],
      ["capB", 2_000, 3_000],
      ["capC", 2_500, 3_500],
    ]);
    expect(rows[1].cps).toBeCloseTo(21);
  });

  it("a cue trimmed out of its clip, or on a missing clip, is not a row", () => {
    const orphan = { ...cue("gone", 0, 500, "x"), clip_id: "nope" };
    const trimmed = cue("trimmed", 12_500, 13_000, "after the clip's source end");
    expect(captionRows(project([orphan, trimmed, cues()[0]])).map((r) => r.cue.id)).toEqual(["capA"]);
  });

  it("an overlap names the LATER cue and the earlier one it runs into", () => {
    const notices = captionNotices(captionRows(project()));
    const overlap = notices.find((n) => n.kind === "overlap");
    expect(overlap?.row.cue.id).toBe("capC");
    expect(overlap?.message).toMatch(/2 and 3/);
    // Touching is not overlapping: capA ends exactly where capB starts.
    expect(notices.filter((n) => n.kind === "overlap")).toHaveLength(1);
  });
});

// Fix round 1: the frontend's MAX_CAPTIONS/MAX_MARKERS are hand copies of
// `core::editor::limits`; this reads the Rust source so the two can never
// drift -- the button must disable exactly where Rust starts refusing.
function rustLimit(name: string): number {
  const file = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../src-tauri/core/src/editor/mod.rs");
  const match = new RegExp(`pub const ${name}: usize = ([0-9_]+);`).exec(readFileSync(file, "utf8"));
  if (!match) throw new Error(`${name} not found in core::editor::limits`);
  return Number(match[1].replace(/_/g, ""));
}

describe("captionRules limits match core::editor::limits", () => {
  it("Add caption disables exactly at MAX_CAPTIONS", () => {
    const max = rustLimit("MAX_CAPTIONS");
    const filled = (n: number) => project(Array.from({ length: n }, (_, i) => cue(`k${i}`, 2_000, 2_100, "x")));
    expect("command" in addCaptionAt(filled(max - 1), [], 1_500)).toBe(true);
    expect(addCaptionAt(filled(max), [], 1_500)).toEqual({
      reason: `This project already has the maximum of ${max} captions`,
    });
  });

  it("Add chapter disables exactly at MAX_MARKERS", () => {
    const max = rustLimit("MAX_MARKERS");
    const withMarkers = (n: number) => ({
      ...project([]),
      markers: Array.from({ length: n }, (_, i) => ({ id: `m${i}`, clip_id: "c1", source_ms: 2_000, title: "M" })),
    });
    expect("command" in addMarkerAt(withMarkers(max - 1), 1_500)).toBe(true);
    expect(addMarkerAt(withMarkers(max), 1_500)).toEqual({
      reason: `This project already has the maximum of ${max} chapters`,
    });
  });
});

// ---- authoring --------------------------------------------------------------

describe("CaptionsLibrary authoring", () => {
  it("adds a caption at the playhead in the clip's SOURCE time", async () => {
    const w = await mountLibrary(project([]), null, 1_500);
    await w.get('[data-testid="caption-add"]').trigger("click");
    await flushPromises();
    // Output 1500 on c1 = source 2000 + 500 * 2 = 3000; the default 3 s of
    // OUTPUT is 6 s of source at speed 2.
    expect(executed).toEqual([
      { kind: "addCaption", clipId: "c1", startMs: 3_000, endMs: 9_000, text: "New caption" },
    ]);
  });

  it("disables Add with a reason when no clip is under the playhead", async () => {
    const w = await mountLibrary(project([]), null, 9_000);
    const add = w.get('[data-testid="caption-add"]');
    expect(add.attributes("disabled")).toBeDefined();
    expect(add.attributes("title")).toMatch(/clip/i);
  });

  it("splits the caption under the playhead at its SOURCE instant", async () => {
    const w = await mountLibrary(project(), null, 1_500);
    await w.get('[data-testid="caption-split"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "splitCaption", captionId: "capA", atMs: 3_000 }]);
  });

  it("refuses Split with a reason when no caption is under the playhead", async () => {
    const w = await mountLibrary(project(), null, 5_000);
    const split = w.get('[data-testid="caption-split"]');
    expect(split.attributes("disabled")).toBeDefined();
    expect(split.attributes("title")).toMatch(/caption/i);
  });

  it("edits text, and timing typed in OUTPUT seconds becomes SOURCE time", async () => {
    const w = await mountLibrary();
    const text = w.get('[data-testid="caption-text-capA"]');
    await text.setValue("Hello, world");
    const start = w.get('[data-testid="caption-start-capA"]');
    expect((start.element as HTMLInputElement).value).toBe("1.000");
    await start.setValue("1.250");
    await flushPromises();
    // Output 1250 -> source 2000 + 250 * 2 = 2500.
    expect(executed).toEqual([
      { kind: "updateCaption", captionId: "capA", text: "Hello, world" },
      { kind: "updateCaption", captionId: "capA", startMs: 2_500 },
    ]);
  });

  // Fix round 1: a value Rust refused must not stay in the field -- the
  // projection did not change, so Vue would never re-patch `:value`, and
  // the field would show a caption that is not the one stored.
  it("a refused edit puts the committed text and time back in the field", async () => {
    const w = await mountLibrary();
    refuse = true;
    const text = w.get('[data-testid="caption-text-capA"]');
    await text.setValue("Rejected words");
    const end = w.get('[data-testid="caption-end-capA"]');
    await end.setValue("0.500");
    await flushPromises();
    expect(executed).toHaveLength(2);
    expect((text.element as HTMLTextAreaElement).value).toBe("Hello there");
    expect((end.element as HTMLInputElement).value).toBe("2.000");
  });

  it("deletes a cue", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="caption-delete-capB"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "removeCaptions", captionIds: ["capB"] }]);
  });

  it("burn-in and placement send setCaptionSettings", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="caption-burn-in"]').setValue(false);
    await w.get('[data-testid="caption-position"]').setValue("top");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "setCaptionSettings", burnIn: false },
      { kind: "setCaptionSettings", position: "top" },
    ]);
  });

  it("a font size outside 18-56 is never sent", async () => {
    const w = await mountLibrary();
    const font = w.get('[data-testid="caption-font-size"]');
    await font.setValue("12");
    await font.setValue("40");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setCaptionSettings", fontSize: 40 }]);
  });

  it("imports onto the target clip and reports what was skipped", async () => {
    const reply: CaptionImportResult = {
      projection: { snapshot: snapshot(2), project: project() },
      imported: 2,
      skipped: 1,
    };
    const w = await mountLibrary(project(), reply);
    await w.get('[data-testid="caption-import-replace"]').setValue(true);
    await w.get('[data-testid="caption-import"]').trigger("click");
    await flushPromises();
    expect(importCalls).toEqual([{ sessionId: "ses-a", clipId: "c1", replace: true }]);
    const status = w.get('[data-testid="caption-import-status"]').text();
    expect(status).toContain("Imported 2 captions");
    expect(status).toContain("1 cue fell outside the clip");
    expect(useEditorProjectStore().snapshot?.revision).toBe(2);
  });

  it("a cancelled import dialog changes nothing and says nothing", async () => {
    const w = await mountLibrary(project(), null);
    await w.get('[data-testid="caption-import"]').trigger("click");
    await flushPromises();
    expect(importCalls).toHaveLength(1);
    expect(w.find('[data-testid="caption-import-status"]').exists()).toBe(false);
    expect(useEditorProjectStore().snapshot?.revision).toBe(1);
  });

  it("renders a window of a long cue list, not every row", async () => {
    const many = Array.from({ length: 300 }, (_, i) =>
      cue(`k${i}`, 2_000 + i * 20, 2_000 + i * 20 + 10, `Cue ${i}`),
    );
    const w = await mountLibrary(project(many));
    const rendered = w.findAll('[data-testid^="caption-row-"]').length;
    expect(rendered).toBeGreaterThan(0);
    expect(rendered).toBeLessThan(60);
  });
});

// ---- mounting -----------------------------------------------------------------

describe("LibraryPanel captions and chapters tabs", () => {
  it("mounts CaptionsLibrary and ChaptersLibrary from their own tabs", async () => {
    await open(project());
    const w = mount(LibraryPanel);
    await w.get('[data-testid="library-tab-captions"]').trigger("click");
    expect(w.find('[data-testid="captions-library"]').exists()).toBe(true);
    expect(w.find('[data-testid="media-library"]').exists()).toBe(false);
    await w.get('[data-testid="library-tab-chapters"]').trigger("click");
    expect(w.find('[data-testid="chapters-library"]').exists()).toBe(true);
    expect(w.find('[data-testid="captions-library"]').exists()).toBe(false);
  });
});

// ---- the preview ----------------------------------------------------------------

describe("CaptionOverlay (preview placement)", () => {
  const frame = { left: 40, top: 20, width: 640, height: 360 };

  it("shows the captions playing at the preview time, where the settings put them", () => {
    const p = project();
    const bottom = mount(CaptionOverlay, { props: { project: p, timeMs: 1_500, frame } });
    const box = bottom.get('[data-testid="caption-overlay"]');
    expect(box.text()).toBe("Hello there");
    expect(box.attributes("data-position")).toBe("bottom");

    const top = { ...p, captions: { ...p.captions!, position: "top" as const } };
    const moved = mount(CaptionOverlay, { props: { project: top, timeMs: 2_700, frame } });
    // Output 2700 is inside capB [2000,3000) AND capC [2500,3500): both show.
    expect(moved.get('[data-testid="caption-overlay"]').attributes("data-position")).toBe("top");
    expect(moved.get('[data-testid="caption-overlay"]').text()).toContain("Overlap");
    expect(moved.get('[data-testid="caption-overlay"]').text()).toContain(DENSE_TEXT);
  });

  it("shows nothing when captions are off or none is playing", () => {
    const p = project();
    const off = { ...p, captions: { ...p.captions!, enabled: false } };
    expect(mount(CaptionOverlay, { props: { project: off, timeMs: 1_500, frame } }).find('[data-testid="caption-overlay"]').exists()).toBe(false);
    expect(mount(CaptionOverlay, { props: { project: p, timeMs: 5_000, frame } }).find('[data-testid="caption-overlay"]').exists()).toBe(false);
  });
});
