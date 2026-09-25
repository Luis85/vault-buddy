/**
 * The mounted editor the guide suites drive (Tasks 55, 56): `EditorRoot`'s
 * own slot filling of `EditorShell` minus the legacy surface, over a small
 * project with two video clips on `v1` and an audio track, opened through a
 * fake port. Shared so the content suite (every key resolves) and the coach
 * suite (the walkthrough itself) mount the same editor.
 */
import { defineComponent, h } from "vue";

import FadesSection from "../../src/components/editor/inspector/FadesSection.vue";
import InspectorPanel from "../../src/components/editor/inspector/InspectorPanel.vue";
import LayoutSection from "../../src/components/editor/inspector/LayoutSection.vue";
import LibraryPanel from "../../src/components/editor/library/LibraryPanel.vue";
import PreviewSurface from "../../src/components/editor/preview/PreviewSurface.vue";
import EditorShell from "../../src/components/editor/shell/EditorShell.vue";
import TimelineView from "../../src/components/editor/timeline/TimelineView.vue";
import type { EditorPort } from "../../src/editor/port";
import type { Clip, EditorOpenResult, Project } from "../../src/editorTypes";
import { fakeEditorPort } from "./fakeEditorPort";

function clip(id: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id, asset_id: "capture", track_id: "v1", name: id, start_ms: 0, in_ms: 0, out_ms: 3_000,
    fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false,
    x: 0, y: 0, w: 1, h: 1, ...overrides,
  };
}

const PROJECT: Project = {
  schema: "vault-buddy-video-project/3",
  id: "project-a",
  title: "Tutorial",
  canvas: { width: 1280, height: 720, fps: 30 },
  master_gain: 1,
  assets: [{ id: "capture", kind: "video", name: "cap one", duration_ms: 10_000 }],
  tracks: [
    { id: "v1", kind: "video", name: "Video", visible: true, locked: false, muted: false, solo: false, volume: 1 },
    { id: "a1", kind: "audio", name: "Audio", visible: true, locked: false, muted: false, solo: false, volume: 1 },
  ],
  clips: [clip("intro"), clip("body", { start_ms: 3_000, in_ms: 3_000, out_ms: 7_000 })],
  effects: [],
  markers: [],
  transitions: [],
  captions: null,
  destination: { vault: "vault-a", folder: "", dated: false },
};

const OPENED: EditorOpenResult = {
  snapshot: {
    sessionId: "ses-a", projectId: "project-a", revision: 1, persistedRevision: 1, title: "Tutorial",
    durationMs: 7_000, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  },
  project: PROJECT,
  workspace: {},
  missing: [],
  sourceBase: "cap one",
  recovered: false,
};

/** The read-only replies a mounted editor asks for; media lookups fail
 * (there is no media in a unit test). */
export function shellPort(overrides: Partial<EditorPort> = {}): EditorPort {
  return fakeEditorPort({
    openStaged: () => Promise.resolve(OPENED),
    getWorkspace: () => Promise.resolve({}),
    saveWorkspace: () => Promise.resolve(),
    getChecks: () => Promise.resolve([]),
    getProducts: () => Promise.resolve([]),
    getJobs: () => Promise.resolve([]),
    mediaUrl: () => Promise.reject(new Error("no media in this test")),
    mediaPeaks: () => Promise.reject(new Error("no media in this test")),
    mediaThumbnail: () => Promise.reject(new Error("no media in this test")),
    getGuideProgress: () => Promise.reject(new Error("not read here")),
    ...overrides,
  });
}

/** `EditorRoot`'s own slot filling, minus the legacy surface. */
export const MountedShell = defineComponent({
  setup() {
    return () =>
      h(EditorShell, null, {
        library: () => h(LibraryPanel),
        preview: () => h(PreviewSurface),
        inspector: () =>
          h(InspectorPanel, null, {
            layout: ({ clipIds }: { clipIds: string[] }) => h(LayoutSection, { clipIds }),
            fades: ({ clipIds }: { clipIds: string[] }) => h(FadesSection, { clipIds }),
          }),
        timeline: () => h(TimelineView),
      });
  },
});
