/**
 * The editor's status bar (visual-parity Task 4; concept spec §7, design
 * D10): three slots, each in native words — the concept's browser-only
 * copy ("Recovery cached in this browser", "Browser reference · downloads
 * stay local") has no meaning in a desktop app. The centre reads the
 * session's own dirty state and saves through the header's save path; the
 * right counts rendered products and reveals where they live.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import EditorStatusBar from "../src/components/editor/shell/EditorStatusBar.vue";
import { EditorPortError } from "../src/editor/port";
import { revealSerial } from "../src/editor/revealBus";
import type { EditorOpenResult, EditorSnapshot, ProductDto, SaveReceipt } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { fakeEditorPort as fakePort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function snapshot(overrides: Partial<EditorSnapshot> = {}): EditorSnapshot {
  return {
    sessionId: "ses-a", projectId: "project-a", revision: 2, persistedRevision: 2, title: "Tutorial",
    durationMs: 0, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null, ...overrides,
  };
}

function openResult(snap: EditorSnapshot): EditorOpenResult {
  return {
    snapshot: snap,
    project: {
      schema: "vault-buddy-video-project/3", id: "project-a", title: "Tutorial",
      canvas: { width: 1280, height: 720, fps: 30 }, master_gain: 1, assets: [], tracks: [], clips: [],
      effects: [], markers: [], transitions: [], captions: null,
      destination: { vault: "vault-a", folder: "", dated: false },
    },
    workspace: {}, missing: [], sourceBase: "base", recovered: false,
  };
}

function product(id: string): ProductDto {
  return {
    id, projectId: "project-a", name: id, filename: `${id}.mp4`, mime: "video/mp4", revision: 2, durationMs: 1000,
    createdAt: "2026-09-26T10:00:00Z", editFingerprint: "f", renderRange: null, available: true,
  };
}

async function open(
  opts: { dirty?: boolean; products?: ProductDto[]; saved?: number[]; saveFails?: boolean } = {},
) {
  const store = useEditorProjectStore();
  store.setPort(
    fakePort({
      openStaged: () => Promise.resolve(openResult(snapshot({ persistedRevision: opts.dirty ? 1 : 2 }))),
      getProducts: () => Promise.resolve(opts.products ?? []),
      save: (sessionId, expectedRevision) => {
        opts.saved?.push(expectedRevision);
        if (opts.saveFails) {
          return Promise.reject(
            new EditorPortError({
              code: "diskFull",
              message: "Cannot write the project file <path:#0a1b2c3d>. The disk is full.",
              retryable: true,
              operationId: "op-save",
            }),
          );
        }
        return Promise.resolve<SaveReceipt>({ sessionId, savedRevision: expectedRevision, projectFileId: "project-a" });
      },
    }),
  );
  await store.openStaged("cap one");
  const w = mount(EditorStatusBar);
  await flushPromises();
  return w;
}

describe("EditorStatusBar", () => {
  it("says the work stays on this computer, on the left", async () => {
    const w = await open();
    expect(w.get('[data-testid="editor-statusbar-local"]').text()).toBe("Local only. No media is uploaded.");
  });

  it("reads the session's dirty state in the centre, and a click saves the project", async () => {
    const saved: number[] = [];
    const w = await open({ dirty: true, saved });
    const centre = w.get('[data-testid="editor-statusbar-recovery"]');
    expect(centre.text()).toBe("Unsaved changes are journaled for recovery");

    await centre.trigger("click");
    await flushPromises();

    expect(saved).toEqual([2]);
    expect(w.get('[data-testid="editor-statusbar-recovery"]').text()).toBe("All changes saved");
  });

  // Ruling T4-1: a refused save is never invisible. "Save failed" takes
  // precedence over the dirty/clean copy, its reason (without the log's
  // redaction handle) is in the tooltip, and a click retries the one save.
  it("says Save failed when the last save was refused, and a click retries it", async () => {
    const saved: number[] = [];
    const w = await open({ dirty: true, saved, saveFails: true });
    const centre = () => w.get('[data-testid="editor-statusbar-recovery"]');

    await centre().trigger("click");
    await flushPromises();

    expect(centre().text()).toBe("Save failed");
    expect(centre().attributes("title")).toBe("Cannot write the project file. The disk is full.");
    expect(centre().attributes("disabled")).toBeUndefined();

    await centre().trigger("click");
    await flushPromises();
    expect(saved).toEqual([2, 2]);
  });

  it("counts the rendered products on the right", async () => {
    const one = await open({ products: [product("p1")] });
    expect(one.get('[data-testid="editor-statusbar-products"]').text()).toBe(
      "1 rendered video · Workspace & rendered products",
    );
    one.unmount();
    setActivePinia(createPinia());
    const none = await open({ products: [] });
    expect(none.get('[data-testid="editor-statusbar-products"]').text()).toBe(
      "0 rendered videos · Workspace & rendered products",
    );
  });

  it("the products slot opens the library on its products", async () => {
    const w = await open({ products: [product("p1"), product("p2")] });
    const before = revealSerial("library");

    await w.get('[data-testid="editor-statusbar-products"]').trigger("click");

    expect(useEditorWorkspaceStore().libraryTab).toBe("products");
    expect(revealSerial("library")).toBe(before + 1);
  });
});
