/**
 * The inspector suites' shared sample (visual-parity Task 13): the concept's
 * screen-02 shapes — a 32 s presenter clip on "Webcam · presenter", two
 * screen clips, an audio bed and an empty overlay track — and a project
 * store opened over a fake port that records every command.
 */
import type { EditorCommand } from "../../src/editor/editorCommandTypes";
import type { Clip, EditorOpenResult, Project, Track } from "../../src/editorTypes";
import { useEditorProjectStore } from "../../src/stores/editorProject";
import { fakeEditorPort } from "./fakeEditorPort";

export function track(id: string, name: string, kind: "video" | "audio" = "video", extra: Partial<Track> = {}): Track {
  return { id, kind, name, visible: true, locked: false, muted: false, solo: false, volume: 1, ...extra };
}
export function clip(id: string, assetId: string, trackId: string, start: number, len: number, name: string, extra: Partial<Clip> = {}): Clip {
  return {
    id, asset_id: assetId, track_id: trackId, name, start_ms: start, in_ms: 0, out_ms: len,
    fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false,
    x: 0, y: 0, w: 1, h: 1, ...extra,
  };
}
export function project(overrides: Partial<Project> = {}): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [
      { id: "presenter", kind: "video", name: "Presenter · demo", duration_ms: 33_000 },
      { id: "capture", kind: "video", name: "Getting started.capture", duration_ms: 36_000 },
      { id: "music", kind: "audio", name: "Guide cues · synth", duration_ms: 36_000 },
    ],
    tracks: [
      track("v3", "Webcam · presenter"),
      track("v1", "Screen recording"),
      track("a1", "Guide cues", "audio"),
      track("v9", "Empty overlay"),
    ],
    clips: [
      clip("c5", "presenter", "v3", 1_500, 32_000, "Presenter · demo"),
      clip("c1", "capture", "v1", 0, 9_500, "Open your workspace"),
      clip("c2", "capture", "v1", 9_500, 14_000, "Create a project"),
      clip("c6", "music", "a1", 300, 33_000, "Chapter cues · demo"),
    ],
    effects: [{ id: "fx1", clip_id: "c1", kind: "text", start_ms: 0, end_ms: 1_000, x: 0.1, y: 0.1, text: "Hi", color: "#ffd279" }],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
    ...overrides,
  };
}

/** Opens `p` in the project store over a fake port whose `execute` records
 * every command (and answers with `p` unchanged). Returns that record. */
export async function openInspectorProject(p: Project = project(), sessionId = "ses-a"): Promise<EditorCommand[]> {
  const executed: EditorCommand[] = [];
  const store = useEditorProjectStore();
  const snapshot = {
    sessionId, projectId: "project-a", revision: 1, persistedRevision: null, title: "Tutorial",
    durationMs: 33_500, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  };
  const opened: EditorOpenResult = { snapshot, project: p, workspace: {}, missing: [], sourceBase: `base-${sessionId}`, recovered: false };
  let revision = 1;
  store.setPort(
    fakeEditorPort({
      openStaged: () => Promise.resolve(opened),
      execute: (req) => {
        executed.push(req.command);
        revision += 1;
        return Promise.resolve({ snapshot: { ...snapshot, revision }, project: p });
      },
      getWorkspace: () => Promise.resolve({}),
      saveWorkspace: () => Promise.resolve(),
    }),
  );
  await store.openStaged(`base-${sessionId}`);
  return executed;
}
