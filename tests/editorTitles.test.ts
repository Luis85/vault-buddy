/**
 * `TitlesLibrary.vue` (Task 33; F-37): four one-click title-card presets
 * (Intro/Chapter/Outro/Blank) plus a title/subtitle pair the next insert
 * uses -- `addCard` through `editorProject.execute`, the `MediaLibrary.vue`
 * "+" precedent (Rust stays the authority; an overlap or a locked track
 * surfaces as the store's error).
 *
 * Track choice is `MediaLibrary`'s "+" rule (`placeOnFreeTrack`, visual-
 * parity Task 7): the first unlocked VIDEO track free for the card's span,
 * else a new video track above the topmost one and the card on it
 * (`editorPlaceOnFreeTrack.test.ts` pins the full sequence).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import TitlesLibrary from "../src/components/editor/library/TitlesLibrary.vue";
import type { EditorCommand, EditorOpenResult, Project, Track } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function track(id: string, kind: "video" | "audio", locked = false): Track {
  return { id, kind, name: id, visible: true, locked, muted: false, solo: false, volume: 1 };
}

function project(tracks: Track[]): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [],
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

async function mountLibrary(tracks: Track[] = [track("v1", "video")]) {
  executed = [];
  const p = project(tracks);
  const open: EditorOpenResult = {
    snapshot: {
      sessionId: "ses-a",
      projectId: "project-a",
      revision: 1,
      persistedRevision: 1,
      title: "Tutorial",
      durationMs: 0,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    },
    project: p,
    workspace: {},
    missing: [],
    sourceBase: "base",
    recovered: false,
  };
  const port = fakeEditorPort({
    openStaged: () => Promise.resolve(open),
    execute: (req) => {
      executed.push(req.command);
      return Promise.resolve({ snapshot: { ...open.snapshot, revision: 2 }, project: p });
    },
  });
  const store = useEditorProjectStore();
  store.setPort(port);
  await store.openStaged("base");
  useEditorWorkspaceStore().playheadMs = 5_000;
  const w = mount(TitlesLibrary);
  await flushPromises();
  return w;
}

describe("TitlesLibrary", () => {
  it("titles library inserts a chapter card at the playhead", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="titles-title"]').setValue("Setting up");
    await w.get('[data-testid="titles-subtitle"]').setValue("Step 2");
    await w.get('[data-testid="titles-add-chapter"]').trigger("click");
    expect(executed).toEqual([
      {
        kind: "addCard",
        preset: "chapter",
        trackId: "v1",
        startMs: 5_000,
        durationMs: 3_000,
        title: "Setting up",
        subtitle: "Step 2",
      },
    ]);
  });

  it("an empty title falls back to the preset's own label", async () => {
    const w = await mountLibrary();
    await w.get('[data-testid="titles-add-outro"]').trigger("click");
    expect(executed).toEqual([
      {
        kind: "addCard",
        preset: "outro",
        trackId: "v1",
        startMs: 5_000,
        durationMs: 3_000,
        title: "Outro",
        subtitle: "",
      },
    ]);
  });

  // This fake never mints the track, so the card that would follow the
  // `addTrack` is not sent — an insert never follows a track that did not land.
  it("adds a video track on top when no unlocked video track exists", async () => {
    const w = await mountLibrary([track("v1", "video", true), track("a1", "audio")]);
    await w.get('[data-testid="titles-add-intro"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "addTrack", trackKind: "video", name: "Video 2", index: 0 }]);
  });

  it("does nothing when no project is open", async () => {
    executed = [];
    const w = mount(TitlesLibrary);
    await flushPromises();
    await w.get('[data-testid="titles-add-blank"]').trigger("click");
    expect(executed).toEqual([]);
  });
});
