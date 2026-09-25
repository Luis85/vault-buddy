/**
 * Every key the learning center's shortcut table lists DOES something
 * (Task 57 fix round 1; R20: no control that silently succeeds). The table
 * is built from `shortcuts.ts`' `SHORTCUTS`, so this walks that same map:
 * for each combo it mounts the editor shell, sets up the state the action
 * needs, presses the key on the shell and asserts the action's observable
 * effect — the command sent, the clipboard written, the save requested, the
 * Review dialog opened, the guide started, focus moved. A binding added to
 * `SHORTCUTS` with no effect here fails by name ("no effect is known").
 *
 * Ctrl+S and Ctrl+E were listed and unhandled until this round: they now
 * run the header's Save and the preview toolbar's Review.
 */
import { mockConvertFileSrc } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import type { ActionId } from "../src/editor/actionMeta";
import { clearClipboardForTest, clipboardFor } from "../src/editor/clipboard";
import { SHORTCUT_TABLE } from "../src/editor/guide/answers";
import type { EditorPort } from "../src/editor/port";
import { shortcutKey,SHORTCUTS } from "../src/editor/shortcuts";
import type { EditorOpenResult, SaveReceipt } from "../src/editorTypes";
import { useEditorOnboardingStore } from "../src/stores/editorOnboarding";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { MountedShell, shellPort } from "./helpers/guideShell";

enableAutoUnmount(afterEach);

/** What the shell sent, and what the port saw asked of it. */
let sent: string[];
let saves: number;
let saveResolve: (() => void) | null;

async function opened(): Promise<EditorOpenResult> {
  const base = await shellPort().openStaged("cap one");
  return {
    ...base,
    // Undo and Redo need history to act on; ungroup needs a group.
    snapshot: { ...base.snapshot, canUndo: true, canRedo: true },
    project: { ...base.project, clips: base.project.clips.map((c) => ({ ...c, group_id: "g1" })) },
  };
}

async function install(overrides: Partial<EditorPort> = {}): Promise<void> {
  sent = [];
  saves = 0;
  saveResolve = null;
  const open = await opened();
  const receipt: SaveReceipt = { sessionId: "ses-a", savedRevision: 1, projectFileId: "project-a" };
  const port = shellPort({
    openStaged: () => Promise.resolve(open),
    execute: (req) => {
      sent.push(req.command.kind);
      return Promise.resolve({ snapshot: open.snapshot, project: open.project });
    },
    save: () => {
      saves += 1;
      return new Promise((resolve) => {
        saveResolve = () => resolve(receipt);
      });
    },
    ...overrides,
  });
  const project = useEditorProjectStore();
  project.setPort(port);
  useEditorWorkspaceStore().setPort(port);
  await project.openStaged("cap one");
}

async function mountEditor(): Promise<VueWrapper> {
  const w = mount(MountedShell, { attachTo: document.body });
  await flushPromises();
  return w;
}

/** A `SHORTCUTS` combo as the key event a keyboard produces. */
function eventInit(combo: string): KeyboardEventInit {
  const parts = combo.split("+");
  const last = parts[parts.length - 1];
  const key = last.length > 1 ? last[0].toUpperCase() + last.slice(1) : last;
  return { key, ctrlKey: parts.includes("ctrl"), shiftKey: parts.includes("shift") };
}

function press(target: Element, combo: string): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...eventInit(combo) });
  target.dispatchEvent(event);
  return event;
}

const shell = (w: VueWrapper) => w.get('[data-testid="editor-shell"]').element;

interface Effect {
  /** The state the action needs before its key is pressed. */
  prepare?: (w: VueWrapper) => Promise<void> | void;
  /** The action's observable effect. */
  check: (w: VueWrapper) => void;
}

const sends = (kind: string): Effect => ({ check: () => expect(sent).toContain(kind) });

const EFFECTS: Record<string, Effect> = {
  split: sends("splitClip"),
  delete: sends("deleteClips"),
  deleteClose: sends("deleteClips"),
  undo: sends("undo"),
  redo: sends("redo"),
  copy: { check: () => expect(clipboardFor("project-a")).not.toBeNull() },
  cut: sends("cutClips"),
  paste: {
    prepare: async (w) => {
      press(shell(w), "ctrl+c");
      await flushPromises();
    },
    check: () => expect(sent).toContain("pasteFragment"),
  },
  duplicate: sends("duplicateClips"),
  group: {
    prepare: () => useEditorWorkspaceStore().select(["intro", "body"]),
    check: () => expect(sent).toContain("groupClips"),
  },
  ungroup: sends("ungroupClips"),
  save: { check: () => expect(saves).toBe(1) },
  render: { check: (w) => expect(w.find('[data-testid="review-dialog"]').exists()).toBe(true) },
  help: { check: (w) => expect(w.find('[data-testid="guide-coach"]').exists()).toBe(true) },
  guideFocus: {
    prepare: async (w) => {
      useEditorOnboardingStore().start();
      await flushPromises();
      (w.get('[data-testid="guide-coach"]').element as HTMLElement).focus();
    },
    check: (w) => expect(w.get('[data-testid="guide-coach"]').element.contains(document.activeElement)).toBe(false),
  },
};

beforeEach(async () => {
  setActivePinia(createPinia());
  mockConvertFileSrc("windows");
  clearClipboardForTest();
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(120, 90, 200, 40));
  await install();
});
afterEach(() => {
  vi.restoreAllMocks();
});

describe("every listed shortcut", () => {
  it("every key the shortcut table lists does what the table says", async () => {
    // The table and the map are the same bindings (answers.ts builds one
    // from the other), so walking the map walks every listed key.
    expect(SHORTCUT_TABLE.flatMap((r) => r.keys)).toHaveLength(SHORTCUTS.size);
    for (const [combo, actionId] of SHORTCUTS) {
      expect({ combo, normalized: shortcutKey(new KeyboardEvent("keydown", eventInit(combo))) }).toEqual({ combo, normalized: combo });
      const effect = EFFECTS[actionId as ActionId];
      expect({ combo, actionId, effectKnown: effect !== undefined }).toEqual({ combo, actionId, effectKnown: true });
      setActivePinia(createPinia());
      clearClipboardForTest();
      await install();
      const w = await mountEditor();
      useEditorWorkspaceStore().select(["intro"]);
      useEditorWorkspaceStore().setPlayhead(1_500);
      await flushPromises();
      await effect.prepare?.(w);
      sent = [];

      const event = press(shell(w), combo);
      await flushPromises();

      expect({ combo, actionId, claimed: event.defaultPrevented }).toEqual({ combo, actionId, claimed: true });
      effect.check(w);
      w.unmount();
    }
  });

  it("Ctrl+S is the header's Save: never a second save while one is in flight", async () => {
    const w = await mountEditor();

    const first = press(shell(w), "ctrl+s");
    await flushPromises();
    const second = press(shell(w), "ctrl+s");
    await flushPromises();

    expect(first.defaultPrevented).toBe(true);
    expect(second.defaultPrevented).toBe(true);
    expect(saves).toBe(1);
    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Saving…");
    saveResolve?.();
    await flushPromises();
    expect(w.get('[data-testid="editor-header-status"]').text()).toBe("Saved");
  });

  it("Ctrl+S and Ctrl+E in a text field are the field's", async () => {
    const w = await mountEditor();
    await w.get('[data-testid="editor-shell-title"]').trigger("click");
    const input = w.get('[data-testid="editor-header-title-input"]').element;

    const save = press(input, "ctrl+s");
    const review = press(input, "ctrl+e");
    await flushPromises();

    expect([save.defaultPrevented, review.defaultPrevented]).toEqual([false, false]);
    expect(saves).toBe(0);
    expect(w.find('[data-testid="review-dialog"]').exists()).toBe(false);
  });

  it("Ctrl+E with nothing on the timeline is not claimed and opens nothing", async () => {
    const empty = await opened();
    setActivePinia(createPinia());
    await install({
      openStaged: () =>
        Promise.resolve({ ...empty, snapshot: { ...empty.snapshot, durationMs: 0 }, project: { ...empty.project, clips: [] } }),
    });
    const w = await mountEditor();

    const event = press(shell(w), "ctrl+e");
    await flushPromises();

    expect(event.defaultPrevented).toBe(false);
    expect(w.find('[data-testid="review-dialog"]').exists()).toBe(false);
  });
});
