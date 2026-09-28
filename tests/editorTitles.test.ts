/**
 * `TitlesLibrary.vue` (Task 33; F-37): four one-click title-card presets
 * (Intro/Chapter/Outro/Blank) plus a title/subtitle pair the next insert
 * uses -- `addCard` through `editorProject.execute`, the `MediaLibrary.vue`
 * "+" precedent (Rust stays the authority; an overlap or a locked track
 * surfaces as the store's error).
 *
 * Track choice is `MediaLibrary`'s "+" rule (`placeOnFreeTrack`, visual-
 * parity Task 7): the first unlocked VIDEO track free for the card's span,
 * else a new video track above the topmost one — `addCard`'s own
 * `trackId: null` when that is the top (`editorPlaceOnFreeTrack.test.ts`
 * pins both sequences).
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import TitlesLibrary from "../src/components/editor/library/TitlesLibrary.vue";
import type { EditorPort } from "../src/editor/port";
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

async function mountLibrary(tracks: Track[] = [track("v1", "video")], extra: Partial<EditorPort> = {}) {
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
    ...extra,
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

  it("inserts on a new top video track (trackId: null) when no unlocked video track exists", async () => {
    const w = await mountLibrary([track("v1", "video", true), track("a1", "audio")]);
    await w.get('[data-testid="titles-add-intro"]').trigger("click");
    expect(executed).toEqual([
      { kind: "addCard", preset: "intro", trackId: null, startMs: 5_000, durationMs: 3_000, title: "Intro", subtitle: "" },
    ]);
  });

  it("does nothing when no project is open", async () => {
    executed = [];
    const w = mount(TitlesLibrary);
    await flushPromises();
    await w.get('[data-testid="titles-add-blank"]').trigger("click");
    expect(executed).toEqual([]);
  });
});

// visual-parity Task 10 (concept spec §3.3): the concept's Titles tab — the
// heading, the intro insert, four 16:9 template cards and the still-image
// import (a real backend: `editor_import_media` accepts PNG/JPEG/WebP).
describe("TitlesLibrary — concept §3.3", () => {
  it("reads GIVE IT STRUCTURE with a Local pill and the concept's intro line", async () => {
    const w = await mountLibrary();
    expect(w.get('[data-testid="titles-heading"]').text()).toBe("GIVE IT STRUCTURE");
    expect(w.get('[data-testid="titles-pill"]').text()).toBe("Local");
    expect(w.text()).toContain("A clear beginning, useful chapters, and a next step.");
  });

  it("Insert intro sends ONE insertIntro, with the card text when one is typed", async () => {
    const w = await mountLibrary();
    const intro = w.get('[data-testid="titles-insert-intro"]');
    expect(intro.text()).toContain("Insert intro at the beginning");
    expect(intro.text()).toContain("Move every existing track together");
    await intro.trigger("click");
    await w.get('[data-testid="titles-title"]').setValue("Welcome");
    await w.get('[data-testid="titles-subtitle"]').setValue("Part 1");
    await intro.trigger("click");
    expect(executed).toEqual([
      { kind: "insertIntro", durationMs: 3_000, title: "Intro", subtitle: "" },
      { kind: "insertIntro", durationMs: 3_000, title: "Welcome", subtitle: "Part 1" },
    ]);
  });

  it("offers four 16:9 template cards with the concept's texts and canvas colours", async () => {
    const w = await mountLibrary();
    const expected = [
      ["intro", "A clear beginning", "Intro card", "bg-card-intro"],
      ["chapter", "One step at a time", "Chapter card", "bg-card-chapter"],
      ["outro", "What happens next?", "Closing card", "bg-card-outro"],
      ["blank", "Room for your idea", "Plain background", "bg-card-blank"],
    ];
    for (const [id, title, label, colour] of expected) {
      const card = w.get(`[data-testid="titles-add-${id}"]`);
      const canvas = card.get(`[data-testid="titles-canvas-${id}"]`);
      expect(canvas.classes()).toContain("aspect-video");
      expect(canvas.classes()).toContain(colour);
      expect(canvas.text()).toContain(title);
      expect(canvas.text()).toContain("VAULT BUDDY · YOUR TUTORIAL");
      expect(card.get(`[data-testid="titles-label-${id}"]`).text()).toBe(label);
    }
  });

  it("Import a still image starts the media import and shows it on the Media tab", async () => {
    let imports = 0;
    const w = await mountLibrary([track("v1", "video")], {
      importMedia: () => {
        imports += 1;
        return Promise.resolve({ jobId: "job-1" });
      },
      getJobs: () => Promise.resolve([]),
    });
    useEditorWorkspaceStore().setLibraryTab("titles");
    await w.get('[data-testid="titles-import-image"]').trigger("click");
    await flushPromises();
    expect(imports).toBe(1);
    expect(useEditorWorkspaceStore().libraryTab).toBe("media");
  });

  it("with no project open every insert is disabled and says why", async () => {
    executed = [];
    const w = mount(TitlesLibrary);
    await flushPromises();
    for (const id of ["titles-insert-intro", "titles-add-intro", "titles-add-blank", "titles-import-image"]) {
      const button = w.get(`[data-testid="${id}"]`);
      expect(button.attributes("disabled")).toBeDefined();
      expect(button.attributes("title")).toBe("Open a project first.");
    }
  });
});
