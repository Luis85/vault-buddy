/**
 * The Save project dialog and menu (Task 39; F-40; SCREENS 08: "Package
 * preparation reports pending/success/failure/cancel … Native copy changes
 * only after a matching durable receipt"). Every test drives `editorProject`
 * through the shared fake port; the receipt is a DEFERRED promise, so what
 * the dialog shows before and after it lands is read separately — a dialog
 * that claimed success on click would pass a test that only looked after.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import SaveProjectDialog from "../src/components/editor/dialogs/SaveProjectDialog.vue";
import EditorShell from "../src/components/editor/shell/EditorShell.vue";
import { EditorPortError } from "../src/editor/port";
import type { EditorError, EditorOpenResult, PackageFormat, PackageReceipt } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { chooseProjectMenuItem, openProjectMenu } from "./helpers/editorMount";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1440 });
});

function openResult(): EditorOpenResult {
  return {
    snapshot: {
      sessionId: "ses-a",
      projectId: "project-a",
      revision: 7,
      persistedRevision: 5,
      title: "Tutorial",
      durationMs: 0,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    },
    project: {
      schema: "vault-buddy-video-project/3",
      id: "project-a",
      title: "Tutorial",
      canvas: { width: 1280, height: 720, fps: 30 },
      master_gain: 1,
      assets: [],
      tracks: [],
      clips: [],
      effects: [],
      markers: [],
      transitions: [],
      captions: null,
      destination: { vault: "vault-a", folder: "", dated: false },
    },
    workspace: {},
    missing: [],
    sourceBase: "base",
    recovered: false,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function portError(code: EditorError["code"], message: string): EditorPortError {
  return new EditorPortError({ code, message, retryable: false, operationId: "op-1" });
}

async function openStore(overrides: Parameters<typeof fakeEditorPort>[0]) {
  const store = useEditorProjectStore();
  store.setPort(fakeEditorPort({ openStaged: () => Promise.resolve(openResult()), ...overrides }));
  await store.openStaged("base");
  return store;
}

function mountDialog(initialFormat: PackageFormat = "portable") {
  return mount(SaveProjectDialog, { props: { open: true, initialFormat }, attachTo: document.body });
}

const status = (w: ReturnType<typeof mount>) => w.get('[data-testid="save-project-status"]').text();

describe("SaveProjectDialog", () => {
  it("dialog shows success only after a receipt", async () => {
    const reply = deferred<PackageReceipt | null>();
    const exportPackage = vi.fn(() => reply.promise);
    await openStore({ exportPackage });
    const w = mountDialog("portable");
    await flushPromises();

    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    expect(exportPackage).toHaveBeenCalledWith("ses-a", 7, "portable");
    expect(status(w)).toContain("Preparing");
    expect(w.text()).not.toContain("Saved to");

    reply.resolve({ sessionId: "ses-a", savedRevision: 7, fileName: "Demo.vbproject.zip", format: "portable" });
    await flushPromises();
    expect(status(w)).toBe("Saved to Demo.vbproject.zip");
  });

  // A receipt must MATCH the request (SCREENS 08: "a matching durable
  // receipt"): a reply naming another session is not this file.
  it("a receipt for a different session is not shown as success", async () => {
    await openStore({
      exportPackage: () =>
        Promise.resolve({ sessionId: "ses-b", savedRevision: 7, fileName: "Other.vbproject.zip", format: "portable" }),
    });
    const w = mountDialog("portable");
    await flushPromises();
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    expect(w.text()).not.toContain("Saved to");
    expect(status(w)).toContain("not confirmed");
  });

  it("cancelled dialog shows no success", async () => {
    await openStore({ exportPackage: () => Promise.resolve(null) });
    const w = mountDialog("lightweight");
    await flushPromises();
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    expect(w.text()).not.toContain("Saved to");
    expect(status(w)).toContain("Nothing was saved");
  });

  it("a refusal shows Rust's message and offers the save again", async () => {
    const message = "This project is too large for a portable file (limit 200 MiB). Save a lightweight copy instead.";
    await openStore({ exportPackage: () => Promise.reject(portError("invalidRequest", message)) });
    const w = mountDialog("portable");
    await flushPromises();
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    expect(status(w)).toContain(message);
    expect(w.text()).not.toContain("Saved to");
    expect(w.get('[data-testid="save-project-confirm"]').attributes("disabled")).toBeUndefined();
  });

  it("portable warns that originals are included", async () => {
    await openStore({});
    const w = mountDialog("portable");
    await flushPromises();
    const warning = w.get('[data-testid="save-project-originals-warning"]');
    expect(warning.text()).toMatch(/original/i);
    expect(w.find('[data-testid="save-project-reconnect-note"]').exists()).toBe(false);

    await w.get('[data-testid="save-project-format-lightweight"]').setValue(true);
    expect(w.find('[data-testid="save-project-originals-warning"]').exists()).toBe(false);
    expect(w.get('[data-testid="save-project-reconnect-note"]').text()).toMatch(/reconnect/i);
  });

  it("the chosen format is what is sent, and the save cannot be sent twice while pending", async () => {
    const reply = deferred<PackageReceipt | null>();
    const exportPackage = vi.fn(() => reply.promise);
    await openStore({ exportPackage });
    const w = mountDialog("portable");
    await flushPromises();
    await w.get('[data-testid="save-project-format-lightweight"]').setValue(true);
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    expect(exportPackage).toHaveBeenCalledTimes(1);
    expect(exportPackage).toHaveBeenCalledWith("ses-a", 7, "lightweight");
    reply.resolve(null);
    await flushPromises();
  });
});

describe("EditorHeader — the Project menu's save items", () => {
  // Task 16 minor, carried: "Save failed" came from the store's SHARED
  // `lastError`, so a refused EDIT read as a failed save.
  it("an edit refusal does not read Save failed; a save refusal does", async () => {
    const store = await openStore({
      execute: () => Promise.reject(portError("invalidRequest", "No such clip")),
      save: () => Promise.reject(portError("diskFull", "Disk full")),
    });
    const w = mount(EditorShell);
    await store.execute({ kind: "rename", title: "x" });
    await flushPromises();
    expect(store.lastError?.message).toBe("No such clip");
    expect(w.get('[data-testid="editor-header-save-state"]').text()).toBe("Unsaved changes");

    await store.save();
    await flushPromises();
    expect(w.get('[data-testid="editor-header-save-state"]').text()).toBe("Save failed");
  });

  // Visual-parity Task 8 (design D6/D7): the Save ▾ split button's items
  // moved into the header's Project menu. "Save a copy as project file…"
  // opens this dialog (portable first, the radios pick the format); Open a
  // project file and Discard project are `EditorRoot`'s, so the shell only
  // forwards them.
  it("the Project menu opens the dialog and forwards Open a project file and Discard", async () => {
    await openStore({ listProjects: () => Promise.resolve([]) });
    const w = mount(EditorShell, { attachTo: document.body });
    await chooseProjectMenuItem(w, "saveCopy");
    expect(
      (w.get('[data-testid="save-project-format-portable"]').element as HTMLInputElement).checked,
    ).toBe(true);
    await w.get('[data-testid="save-project-format-lightweight"]').setValue(true);
    expect(w.get('[data-testid="save-project-confirm"]').text()).toBe("Save lightweight copy…");

    await w.get('[data-testid="save-project-cancel"]').trigger("click");
    await chooseProjectMenuItem(w, "openFile");
    expect(w.emitted("open-project-file")).toHaveLength(1);

    await chooseProjectMenuItem(w, "discard");
    expect(w.emitted("discard-project")).toHaveLength(1);
  });

  // The old split button's own Save item was a second path to Save project
  // (fix round 1 had to disable it while a save ran). The Project menu has
  // none: Save project is the header button, Ctrl+S and the status bar,
  // all through `useProjectSave`.
  it("the Project menu has no second Save item", async () => {
    await openStore({ listProjects: () => Promise.resolve([]) });
    const w = mount(EditorShell, { attachTo: document.body });
    const menu = await openProjectMenu(w);
    const labels = menu.findAll('[role="menuitem"]').map((i) => i.text());
    expect(labels.filter((l) => /^Save /.test(l))).toEqual(["Save a copy as project file…"]);
  });
});

describe("the Project menu and Escape", () => {
  // Carried from the Save menu's fix round 1: Escape was stopped at the
  // menu's root even while it was CLOSED, so the shell's own Escape
  // handling never saw it.
  it("lets Escape through while closed and consumes it only to close an open menu", async () => {
    await openStore({ listProjects: () => Promise.resolve([]) });
    const w = mount(EditorShell, { attachTo: document.body });
    const seen = vi.fn();
    document.addEventListener("keydown", seen);
    try {
      const trigger = w.get('[data-testid="editor-header-project-menu"]');
      await trigger.trigger("keydown", { key: "Escape" });
      expect(seen).toHaveBeenCalledTimes(1);

      (trigger.element as HTMLElement).focus();
      const menu = await openProjectMenu(w);
      await menu.get('[role="menuitem"]').trigger("keydown", { key: "Escape" });
      await flushPromises();
      expect(w.find('[data-testid="editor-project-menu"]').exists()).toBe(false);
      expect(seen).toHaveBeenCalledTimes(1);
      expect(document.activeElement).toBe(trigger.element);
    } finally {
      document.removeEventListener("keydown", seen);
    }
  });
});
