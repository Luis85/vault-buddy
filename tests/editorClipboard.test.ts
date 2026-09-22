/**
 * The tutorial editor's clipboard (Task 21; F-12) — `src/editor/clipboard.ts`
 * and `baseActionContext`'s real wiring of it (`actionContext.ts`'s own
 * module doc: "A later task that wires the clipboard replaces these two
 * literals, not the callers").
 */
import { describe, expect, it, vi } from "vitest";

import { baseActionContext } from "../src/editor/actionContext";
import { activateEditorAction, clearClipboardForTest, clipboardFragment, setClipboard } from "../src/editor/clipboard";
import type { Clip, Project } from "../src/editorTypes";

function clip(id: string, overrides: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "a1",
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
    id: "p1",
    title: "T",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "a1", kind: "video", name: "a1", duration_ms: 60_000 }],
    tracks: [{ id: "v1", kind: "video", name: "v1", visible: true, locked: false, muted: false, solo: false, volume: 1 }],
    clips,
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}

describe("baseActionContext — clipboard wiring", () => {
  it("reflects the real clipboard, not a hardcoded empty default", () => {
    clearClipboardForTest();
    let ctx = baseActionContext(null, null, 0, []);
    expect(ctx.hasClipboard).toBe(false);
    expect(ctx.clipboardFragment).toBeNull();

    const fragment = { clips: [clip("c1")], effects: [], captions: [], markers: [], originMs: 0 };
    setClipboard(fragment);
    ctx = baseActionContext(null, null, 0, []);
    expect(ctx.hasClipboard).toBe(true);
    expect(ctx.clipboardFragment).toEqual(fragment);
    expect(clipboardFragment.value).toEqual(fragment);
  });
});

describe("activateEditorAction — copy", () => {
  it("copies the target clip into the clipboard and sends NO command", () => {
    clearClipboardForTest();
    const c1 = clip("c1", { start_ms: 100, out_ms: 600 });
    const ctx = baseActionContext(project([c1]), { sessionId: "s", projectId: "p1", revision: 1, persistedRevision: null, title: "T", durationMs: 600, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null }, 0, ["c1"]);
    const execute = vi.fn();

    expect(activateEditorAction("copy", ctx, execute)).toBe(true);

    expect(execute).not.toHaveBeenCalled();
    expect(clipboardFragment.value?.clips.map((c) => c.id)).toEqual(["c1"]);
  });
});

describe("activateEditorAction — cut", () => {
  it("cut copies then deletes as one undo step", () => {
    clearClipboardForTest();
    const c1 = clip("c1", { start_ms: 100, out_ms: 600 });
    const snapshot = {
      sessionId: "s",
      projectId: "p1",
      revision: 1,
      persistedRevision: null,
      title: "T",
      durationMs: 600,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    };
    const ctx = baseActionContext(project([c1]), snapshot, 0, ["c1"]);
    const execute = vi.fn();

    expect(activateEditorAction("cut", ctx, execute)).toBe(true);

    // The copy half: the clipboard now holds the cut clip, entirely local.
    expect(clipboardFragment.value?.clips.map((c) => c.id)).toEqual(["c1"]);
    // The delete half: exactly ONE command reaches Rust -- one undo step,
    // not two (a separate "copy" commit followed by a "delete" commit).
    expect(execute).toHaveBeenCalledTimes(1);
    expect(execute).toHaveBeenCalledWith({ kind: "cutClips", clipIds: ["c1"], closeGap: true });
  });

  it("a DISABLED cut (e.g. a locked track) copies nothing and sends nothing", () => {
    clearClipboardForTest();
    const lockedProject: Project = {
      ...project([clip("c1")]),
      tracks: [{ id: "v1", kind: "video", name: "v1", visible: true, locked: true, muted: false, solo: false, volume: 1 }],
    };
    const snapshot = {
      sessionId: "s",
      projectId: "p1",
      revision: 1,
      persistedRevision: null,
      title: "T",
      durationMs: 600,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    };
    const ctx = baseActionContext(lockedProject, snapshot, 0, ["c1"]);
    const execute = vi.fn();

    expect(activateEditorAction("cut", ctx, execute)).toBe(false);

    expect(clipboardFragment.value).toBeNull();
    expect(execute).not.toHaveBeenCalled();
  });
});
