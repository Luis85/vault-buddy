/**
 * `ChaptersLibrary.vue` + `captionRules.chapterRows`/`addMarkerAt` (Task 36;
 * F-36): chapter markers listed by DERIVED output time, added at the
 * playhead on the topmost clip, renamed, deleted and jumped to.
 *
 * Asymmetric fixture: `c1` (bottom track) sits at output 1000, plays
 * source [2000, 12000) at speed 2; `c2` (top track, index 0) sits at
 * output 3000 playing [0, 4000) at speed 1 -- so a marker's source time,
 * its output time and "which clip is on top" all disagree.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ChaptersLibrary from "../src/components/editor/library/ChaptersLibrary.vue";
import { addMarkerAt, chapterRows } from "../src/editor/captionRules";
import type { Clip, EditorCommand, EditorProjection, Marker, Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function clip(id: string, track_id: string, start_ms: number, in_ms: number, out_ms: number, speed: number): Clip {
  return {
    id,
    asset_id: "av",
    track_id,
    name: id,
    start_ms,
    in_ms,
    out_ms,
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
    speed,
  } as Clip;
}

function marker(id: string, clip_id: string, source_ms: number, title: string): Marker {
  return { id, clip_id, source_ms, title };
}

function project(markers: Marker[]): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "av", kind: "video", name: "demo.mp4", duration_ms: 60_000 }],
    tracks: [
      { id: "top", kind: "video", name: "Camera", visible: true, locked: false, muted: false, solo: false, volume: 1 },
      { id: "v1", kind: "video", name: "Screen", visible: true, locked: false, muted: false, solo: false, volume: 1 },
    ],
    clips: [clip("c1", "v1", 1_000, 2_000, 12_000, 2), clip("c2", "top", 3_000, 0, 4_000, 1)],
    effects: [],
    markers,
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  } as Project;
}

// m1 on c1 at source 10000 -> output 1000 + 8000/2 = 5000
// m2 on c1 at source 3000  -> output 1000 + 1000/2 = 1500
// m3 on c1 at source 1000  -> before in_ms: trimmed out, not a chapter
const MARKERS = [
  marker("m1", "c1", 10_000, "Wrap up"),
  marker("m2", "c1", 3_000, "Intro"),
  marker("m3", "c1", 1_000, "Trimmed away"),
];

const REFUSAL = { code: "invalidRequest", message: "Refused by Rust", retryable: false, operationId: "op-1" };

let executed: EditorCommand[] = [];
let refuse = false;

async function mountLibrary(markers: Marker[] = MARKERS, playheadMs = 3_500) {
  executed = [];
  refuse = false;
  const p = project(markers);
  const snapshot = {
    sessionId: "ses-a",
    projectId: "project-a",
    revision: 1,
    persistedRevision: 1,
    title: "Tutorial",
    durationMs: 7_000,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
  };
  const store = useEditorProjectStore();
  store.setPort(
    fakeEditorPort({
      openStaged: () =>
        Promise.resolve({ snapshot, project: p, workspace: {}, missing: [], sourceBase: "b", recovered: false }),
      execute: (req): Promise<EditorProjection> => {
        executed.push(req.command);
        if (refuse) return Promise.reject(REFUSAL);
        return Promise.resolve({ snapshot: { ...snapshot, revision: 2 }, project: p });
      },
    }),
  );
  await store.openStaged("b");
  useEditorWorkspaceStore().playheadMs = playheadMs;
  const w = mount(ChaptersLibrary);
  await flushPromises();
  return w;
}

describe("chapterRows", () => {
  it("resolves source markers to OUTPUT time, sorted, trimmed ones omitted", () => {
    expect(chapterRows(project(MARKERS)).map((r) => [r.marker.id, r.outputMs])).toEqual([
      ["m2", 1_500],
      ["m1", 5_000],
    ]);
  });

  it("adds on the TOPMOST clip under the playhead, at its source instant", () => {
    // At output 3500 both clips play; `top` (tracks[0]) wins: c2's source
    // at 3500 is 0 + 500 * 1 = 500. The title counts on from the list.
    expect(addMarkerAt(project(MARKERS), 3_500)).toEqual({
      command: { kind: "addMarker", clipId: "c2", sourceMs: 500, title: "Chapter 3" },
    });
    // Before c2 starts only c1 plays: 2000 + (2000 - 1000) * 2 = 4000.
    expect(addMarkerAt(project([]), 2_000)).toEqual({
      command: { kind: "addMarker", clipId: "c1", sourceMs: 4_000, title: "Chapter 1" },
    });
    expect("reason" in addMarkerAt(project([]), 60_000)).toBe(true);
  });
});

describe("ChaptersLibrary", () => {
  it("lists chapters by output time", async () => {
    const w = await mountLibrary();
    const ids = w.findAll('[data-testid^="chapter-row-"]').map((r) => r.attributes("data-testid"));
    expect(ids).toEqual(["chapter-row-m2", "chapter-row-m1"]);
    expect(w.get('[data-testid="chapter-row-m2"]').text()).toContain("0:01.5");
  });

  it("adds at the playhead", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="chapter-add"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "addMarker", clipId: "c2", sourceMs: 500, title: "Chapter 3" }]);
  });

  it("disables Add with a reason when nothing plays at the playhead", async () => {
    const w = await mountLibrary(MARKERS, 60_000);
    const add = w.get('[data-testid="chapter-add"]');
    expect(add.attributes("disabled")).toBeDefined();
    expect(add.attributes("title")).toMatch(/clip/i);
  });

  it("renames, deletes and jumps", async () => {
    const w = await mountLibrary();
    const title = w.get('[data-testid="chapter-title-m1"]');
    await title.setValue("Summary");
    // An unchanged or blank title is never sent.
    const other = w.get('[data-testid="chapter-title-m2"]');
    await other.setValue("   ");
    await w.get('[data-testid="chapter-delete-m2"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([
      { kind: "updateMarker", markerId: "m1", title: "Summary" },
      { kind: "removeMarker", markerId: "m2" },
    ]);
    await w.get('[data-testid="chapter-jump-m1"]').trigger("click");
    expect(useEditorWorkspaceStore().playheadMs).toBe(5_000);
  });

  // Fix round 1: a title Rust refused goes back to the stored one.
  it("a refused rename puts the stored title back", async () => {
    const w = await mountLibrary();
    refuse = true;
    const title = w.get('[data-testid="chapter-title-m1"]');
    await title.setValue("Too long for Rust, say");
    await flushPromises();
    expect(executed).toEqual([{ kind: "updateMarker", markerId: "m1", title: "Too long for Rust, say" }]);
    expect((title.element as HTMLInputElement).value).toBe("Wrap up");
  });
});
