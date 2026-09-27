/**
 * The editor's feedback surface (visual-parity Task 7; no-op audit findings
 * 1, 4, 7, 8, 9): a refused edit, a revision conflict, a failed save, a
 * copy and a disabled shortcut each tell the person something. Before this,
 * every one of them changed nothing on screen — the store recorded the
 * refusal in `lastError`/`conflictIntent` and no always-mounted surface
 * read either (the audit counted 0 DOM mutations after 124 refusals).
 *
 * Everything goes through the one toast surface `EditorShell` mounts, fed
 * by the notifications store; `useEditorFeedback` is the watcher.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import EditorShell from "../src/components/editor/shell/EditorShell.vue";
import { baseActionContext } from "../src/editor/actionContext";
import { CLIP_BOUNDARY } from "../src/editor/actionMeta";
import { activateEditorAction, clearClipboardForTest } from "../src/editor/clipboard";
import type { EditorCommand } from "../src/editor/editorCommandTypes";
import { EditorPortError } from "../src/editor/port";
import type { Clip, EditorError, EditorOpenResult, Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { useNotificationsStore } from "../src/stores/notifications";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  clearClipboardForTest();
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
});
afterEach(() => {
  vi.useRealTimers();
});

function clip(id: string, startMs: number): Clip {
  return {
    id,
    asset_id: "a1",
    track_id: "v1",
    name: id,
    start_ms: startMs,
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
  };
}

function project(): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Tutorial",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "a1", kind: "video", name: "a1", duration_ms: 60_000 }],
    tracks: [{ id: "v1", kind: "video", name: "V1", visible: true, locked: false, muted: false, solo: false, volume: 1 }],
    clips: [clip("c1", 0), clip("c2", 2_000)],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-a", folder: "", dated: false },
  };
}

function openResult(revision = 1): EditorOpenResult {
  return {
    snapshot: {
      sessionId: "ses-a",
      projectId: "project-a",
      revision,
      persistedRevision: null,
      title: "Tutorial",
      durationMs: 3_000,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    },
    project: project(),
    workspace: {},
    missing: [],
    sourceBase: "base",
    recovered: false,
  };
}

function refusal(message: string, code: EditorError["code"] = "invalidRequest"): EditorPortError {
  return new EditorPortError({ code, message, retryable: false, operationId: "op-1" });
}

async function openShell(port: Parameters<typeof fakeEditorPort>[0] = {}) {
  const store = useEditorProjectStore();
  store.setPort(fakeEditorPort({ openStaged: () => Promise.resolve(openResult()), ...port }));
  await store.openStaged("cap");
  const w = mount(EditorShell, { attachTo: document.body });
  await flushPromises();
  return { store, w };
}

function toasts(kind?: string) {
  return useNotificationsStore().items.filter((i) => kind === undefined || i.kind === kind);
}

describe("a refused edit", () => {
  it("toasts the refusal once, without a redaction handle", async () => {
    const { store } = await openShell({
      execute: () => Promise.reject(refusal("Clips overlap on the project folder <path:#1a2b3c4d>.")),
    });
    await store.execute({ kind: "undo" });
    await flushPromises();

    expect(toasts("error").map((t) => t.message)).toEqual(["Clips overlap on the project folder."]);
  });

  it("never toasts the same error object twice", async () => {
    const { store } = await openShell();
    const error: EditorError = { code: "invalidRequest", message: "No.", retryable: false, operationId: "op" };
    store.lastError = error;
    await flushPromises();
    useNotificationsStore().clear();
    store.lastError = null;
    await flushPromises();
    store.lastError = error;
    await flushPromises();

    expect(toasts()).toEqual([]);
  });
});

describe("an error toast lives as long as its error (fix round 1)", () => {
  // Error toasts have no TTL. Before this they were never dismissed, so a
  // refusal the person had already moved past stayed over the timeline.
  it("a successful edit after a refusal leaves no error toast", async () => {
    let refuse = true;
    const { store } = await openShell({
      execute: () => {
        if (refuse) return Promise.reject(refusal("Clips overlap."));
        return Promise.resolve({ snapshot: openResult(2).snapshot, project: project() });
      },
    });
    await store.execute({ kind: "undo" });
    await flushPromises();
    expect(toasts("error")).toHaveLength(1);

    refuse = false;
    await store.execute({ kind: "undo" });
    await flushPromises();

    expect(store.lastError).toBeNull();
    expect(toasts()).toEqual([]);
  });

  it("a successful save after a failed one leaves no error toast", async () => {
    let full = true;
    const { store, w } = await openShell({
      save: (sessionId, expectedRevision) =>
        full
          ? Promise.reject(refusal("The disk is full.", "diskFull"))
          : Promise.resolve({ sessionId, savedRevision: expectedRevision, projectFileId: "project-a" }),
    });
    await store.save();
    await flushPromises();
    expect(toasts("error").map((t) => t.message)).toEqual(["The disk is full."]);

    full = false;
    await store.save();
    await flushPromises();

    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Saved");
    expect(toasts()).toEqual([]);
  });

  // A failed save's toast follows `saveError`, not `lastError`: an edit
  // that lands clears `lastError`, but the project is still unsaved and the
  // header still says "Save failed" — the reason stays with it.
  it("a failed save's toast survives a later successful edit", async () => {
    const { store, w } = await openShell({
      save: () => Promise.reject(refusal("The disk is full.", "diskFull")),
      execute: () => Promise.resolve({ snapshot: openResult(2).snapshot, project: project() }),
    });
    await store.save();
    await store.execute({ kind: "undo" });
    await flushPromises();

    expect(store.lastError).toBeNull();
    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Save failed");
    expect(toasts("error").map((t) => t.message)).toEqual(["The disk is full."]);
  });

  // The toasts belong to the session the shell shows: when the shell goes
  // (EditorRoot unmounts it with its session), so do they.
  it("unmounting the shell dismisses its error and Retry toasts", async () => {
    const { store, w } = await openShell({ execute: () => Promise.reject(refusal("Clips overlap.")) });
    await store.execute({ kind: "undo" });
    store.conflictIntent = { kind: "undo" };
    await flushPromises();
    expect(toasts()).toHaveLength(2);

    w.unmount();

    expect(toasts()).toEqual([]);
  });

  it("a newer refusal replaces the older one; closing the session clears it", async () => {
    let n = 0;
    const { store } = await openShell({
      execute: () => Promise.reject(refusal(`Refused ${++n}.`)),
      closeSession: () => Promise.resolve(),
    });
    await store.execute({ kind: "undo" });
    await store.execute({ kind: "undo" });
    await flushPromises();
    expect(toasts("error").map((t) => t.message)).toEqual(["Refused 2."]);

    await store.close("keep");
    await flushPromises();
    expect(toasts()).toEqual([]);
  });
});

describe("a revision conflict", () => {
  it("offers Retry, which resends the parked command", async () => {
    const sent: EditorCommand[] = [];
    let conflicted = false;
    const { w } = await openShell({
      execute: (req) => {
        sent.push(req.command);
        if (!conflicted) {
          conflicted = true;
          return Promise.reject(refusal("Stale.", "revisionConflict"));
        }
        return Promise.resolve({ snapshot: openResult(3).snapshot, project: project() });
      },
      getSnapshot: () => Promise.resolve({ snapshot: openResult(2).snapshot, project: project() }),
    });
    const store = useEditorProjectStore();
    await store.execute({ kind: "undo" });
    await flushPromises();

    const toast = w.get('[data-testid="notification"]');
    expect(toast.text()).toContain("Your edit wasn't applied because the project changed. Retry?");
    expect(toasts("error")).toEqual([]);
    await w.get('[data-testid="notification-action"]').trigger("click");
    await flushPromises();

    expect(sent).toEqual([{ kind: "undo" }, { kind: "undo" }]);
    expect(store.conflictIntent).toBeNull();
    expect(toasts()).toEqual([]);
  });

  it("drops a stale Retry once a later edit lands (Retry would do nothing)", async () => {
    const { store } = await openShell();
    store.conflictIntent = { kind: "undo" };
    await flushPromises();
    expect(toasts()).toHaveLength(1);
    store.conflictIntent = null;
    await flushPromises();

    expect(toasts()).toEqual([]);
  });
});

describe("a failed save", () => {
  it("puts the reason on the header's save state and toasts it once", async () => {
    const { store, w } = await openShell({ save: () => Promise.reject(refusal("The disk is full.", "diskFull")) });
    await store.save();
    await flushPromises();

    const status = w.get('[data-testid="editor-header-status"]');
    expect(status.text()).toBe("Save failed");
    expect(status.attributes("title")).toBe("The disk is full.");
    expect(toasts().map((t) => t.message)).toEqual(["The disk is full."]);
  });
});

describe("copy", () => {
  function copy(ids: string[]) {
    const ctx = baseActionContext(project(), openResult().snapshot, 0, ids);
    expect(activateEditorAction("copy", ctx, () => undefined)).toBe(true);
  }

  it("confirms what it copied", () => {
    copy(["c1", "c2"]);
    expect(toasts("info").map((t) => t.message)).toEqual(["Copied 2 clips"]);
    useNotificationsStore().clear();
    copy(["c1"]);
    expect(toasts("info").map((t) => t.message)).toEqual(["Copied 1 clip"]);
  });
});

describe("a disabled shortcut", () => {
  function press(w: Awaited<ReturnType<typeof openShell>>["w"], key: string): KeyboardEvent {
    const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
    w.get('[data-testid="editor-shell"]').element.dispatchEvent(event);
    return event;
  }

  it("S outside every clip says why, and still does not split", async () => {
    const sent: EditorCommand[] = [];
    const { w } = await openShell({
      execute: (req) => {
        sent.push(req.command);
        return Promise.resolve({ snapshot: openResult(2).snapshot, project: project() });
      },
    });
    const workspace = useEditorWorkspaceStore();
    workspace.select(["c1"]);
    workspace.setPlayhead(1_500);

    const event = press(w, "s");
    await flushPromises();

    expect(sent).toEqual([]);
    expect(event.defaultPrevented).toBe(false);
    expect(toasts("info").map((t) => t.message)).toEqual([CLIP_BOUNDARY]);
  });

  it("says so at most once every 1.5 s", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(10_000);
    const { w } = await openShell();
    press(w, "Delete");
    useNotificationsStore().clear();
    press(w, "Delete");
    vi.setSystemTime(11_400);
    press(w, "Delete");
    expect(toasts()).toEqual([]);
    vi.setSystemTime(11_600);
    press(w, "Delete");

    expect(toasts("info").map((t) => t.message)).toEqual(["Select a clip first"]);
  });

  // Fix round 1: the limit is per reason — a DIFFERENT refusal a moment
  // later still speaks (identical messages are deduped by the store).
  it("a different reason 1 s later is still said", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(10_000);
    const { w } = await openShell();
    press(w, "Delete");
    useEditorWorkspaceStore().select(["c1"]);
    useEditorWorkspaceStore().setPlayhead(1_500);
    vi.setSystemTime(11_000);
    press(w, "s");

    expect(toasts("info").map((t) => t.message)).toEqual(["Select a clip first", CLIP_BOUNDARY]);
  });

  it("an S typed into a text field says nothing", async () => {
    const { w } = await openShell();
    useEditorWorkspaceStore().select(["c1"]);
    useEditorWorkspaceStore().setPlayhead(1_500);
    await w.get('[data-testid="editor-shell-title"]').trigger("click");
    const input = w.get('[data-testid="editor-header-title-input"]');
    input.element.dispatchEvent(new KeyboardEvent("keydown", { key: "s", bubbles: true, cancelable: true }));
    await flushPromises();

    expect(toasts()).toEqual([]);
  });

  it("a combo pressed inside an open menu says nothing", async () => {
    const store = useEditorProjectStore();
    store.setPort(fakeEditorPort({ openStaged: () => Promise.resolve(openResult()) }));
    await store.openStaged("cap");
    const w = mount(EditorShell, {
      attachTo: document.body,
      slots: { timeline: '<div role="menu"><button data-testid="in-menu">Split</button></div>' },
    });
    w.get('[data-testid="in-menu"]').element.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Delete", bubbles: true, cancelable: true }),
    );
    await flushPromises();

    expect(toasts()).toEqual([]);
  });

  it("a disabled shortcut's keydown still bubbles past the shell", async () => {
    const { w } = await openShell();
    const reached: string[] = [];
    const onWindowKeydown = (e: KeyboardEvent) => reached.push(e.key);
    window.addEventListener("keydown", onWindowKeydown);
    try {
      const event = press(w, "Delete");
      await flushPromises();
      expect(toasts("info").map((t) => t.message)).toEqual(["Select a clip first"]);
      expect(event.defaultPrevented).toBe(false);
      expect(reached).toEqual(["Delete"]);
    } finally {
      window.removeEventListener("keydown", onWindowKeydown);
    }
  });
});
