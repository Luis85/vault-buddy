/**
 * The header's Project menu (visual-parity Task 8; design D7; concept spec
 * §2): the native items in the concept's slots, each with its icon and its
 * effect. Mounted through `EditorRoot`, which owns which project the shell
 * shows — opening another project, opening a project file and discarding
 * are its to do.
 */
import { mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({ listen: () => Promise.resolve(() => {}) }));
vi.mock("../src/logging", () => ({ logBreadcrumb: vi.fn(), logWarning: vi.fn() }));

import type { EditorPort } from "../src/editor/port";
import { revealSerial } from "../src/editor/revealBus";
import type { EditorCommand, EditorOpenResult, ProjectSummaryDto } from "../src/editorTypes";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { chooseProjectMenuItem, openProjectMenu } from "./helpers/editorMount";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  mockConvertFileSrc("windows");
  setActivePinia(createPinia());
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1600 });
});

function result(projectId: string, title: string, sessionId = `ses-${projectId}`): EditorOpenResult {
  return {
    snapshot: {
      sessionId, projectId, revision: 1, persistedRevision: 1, title, durationMs: 6000,
      canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
    },
    project: {
      schema: "vault-buddy-video-project/3", id: projectId, title, canvas: { width: 1280, height: 720, fps: 30 },
      master_gain: 1, assets: [], tracks: [], clips: [], effects: [], markers: [], transitions: [], captions: null,
      destination: { vault: "vault-a", folder: "", dated: false },
    },
    workspace: {}, missing: [], sourceBase: null, recovered: false,
  };
}

function summary(projectFileId: string, title: string): ProjectSummaryDto {
  return { projectFileId, title, updatedAt: "2026-09-25T10:05:00.000Z", persistedRevision: 1, hasRecovery: false, sourceBase: null };
}

const CURRENT = summary("proj-a", "First");
const OTHER = summary("proj-b", "Second");

async function mountRoot(overrides: Partial<EditorPort> = {}): Promise<VueWrapper> {
  const port = fakeEditorPort({
    openProject: (id) => Promise.resolve(id === "proj-b" ? result("proj-b", "Second") : result("proj-a", "First")),
    listProjects: () => Promise.resolve([CURRENT, OTHER]),
    getWorkspace: () => Promise.resolve({}),
    getJobs: () => Promise.resolve([]),
    getChecks: () => Promise.resolve([]),
    getProducts: () => Promise.resolve([]),
    ...overrides,
  });
  useEditorProjectStore().setPort(port);
  useEditorWorkspaceStore().setPort(port);
  mockIPC((cmd) => (cmd === "take_editor_request" ? { kind: "project", value: "proj-a" } : undefined));
  const w = mount(EditorRoot, { attachTo: document.body });
  await flushPromises();
  return w;
}

const LABELS = [
  "Open project…",
  "Open a project file…",
  "Rename tutorial…",
  "Workspace & rendered products",
  "Save a copy as project file…",
  "Discard project…",
];

describe("the Project menu's items (D7)", () => {
  it("lists exactly the native items, in order, each with an icon, under the 'Project' heading", async () => {
    const w = await mountRoot();
    const menu = await openProjectMenu(w);
    expect(menu.get('[data-testid="editor-project-menu-heading"]').text()).toBe("Project");
    expect(menu.get('[data-testid="editor-project-menu-subtitle"]').text()).toBe(
      "Working files, originals and rendered products",
    );
    const items = menu.findAll('[role="menuitem"]');
    expect(items.map((i) => i.text())).toEqual(LABELS);
    expect(items.filter((item) => !item.find("svg").exists()).map((i) => i.text())).toEqual([]);
    // Two separators: before the workspace group and before the danger item.
    expect(menu.findAll('[role="separator"]')).toHaveLength(2);
    expect(menu.get('[data-testid="editor-project-menu-item-discard"]').classes()).toContain("danger");
  });

  it("omits Open project… when the only stored project is the open one", async () => {
    const w = await mountRoot({ listProjects: () => Promise.resolve([CURRENT]) });
    const menu = await openProjectMenu(w);
    expect(menu.findAll('[role="menuitem"]').map((i) => i.text())).toEqual(LABELS.slice(1));
  });

  it("omits Open project… when the stored projects cannot be listed", async () => {
    const w = await mountRoot({ listProjects: () => Promise.reject(new Error("store unreadable")) });
    const menu = await openProjectMenu(w);
    expect(menu.find('[data-testid="editor-project-menu-item-open"]').exists()).toBe(false);
  });

  it("the trigger reads 'Project' with its folder icon and says it opens a menu", async () => {
    const w = await mountRoot();
    const trigger = w.get('[data-testid="editor-header-project-menu"]');
    expect(trigger.text()).toBe("Project");
    expect(trigger.attributes("aria-haspopup")).toBe("menu");
    expect(trigger.attributes("aria-expanded")).toBe("false");
    expect(trigger.findAll("svg")).toHaveLength(2);
    await openProjectMenu(w);
    expect(trigger.attributes("aria-expanded")).toBe("true");
  });

  // The open menu closes on any press outside it, and the trigger is
  // outside it: without the trigger's own guard, a click there would close
  // the menu on pointerdown and reopen it on click.
  it("a second click on the trigger closes the menu", async () => {
    const w = await mountRoot();
    await openProjectMenu(w);
    const trigger = w.get('[data-testid="editor-header-project-menu"]');
    trigger.element.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await trigger.trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="editor-project-menu"]').exists()).toBe(false);
    expect(trigger.attributes("aria-expanded")).toBe("false");
  });
});

describe("what each item does", () => {
  it("Open project… opens the picker without the open project; a row opens that project", async () => {
    const openProject = vi.fn((id: string) => Promise.resolve(id === "proj-b" ? result("proj-b", "Second") : result("proj-a", "First")));
    const w = await mountRoot({ openProject });
    await chooseProjectMenuItem(w, "open");

    const dialog = w.get('[data-testid="open-project-dialog"]');
    expect(dialog.find('[data-testid="open-project-row-proj-a"]').exists()).toBe(false);
    const row = dialog.get('[data-testid="open-project-row-proj-b"]');
    expect(row.text()).toContain("Second");

    await row.trigger("click");
    await flushPromises();
    expect(openProject).toHaveBeenLastCalledWith("proj-b", false);
    expect(w.find('[data-testid="open-project-dialog"]').exists()).toBe(false);
    // The shell follows the project it now shows.
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);
    expect(w.get('[data-testid="editor-header-title"]').text()).toBe("Second");
  });

  it("a refused open keeps the open project and says why", async () => {
    const w = await mountRoot({
      openProject: (id) =>
        id === "proj-b" ? Promise.reject(new Error("That project could not be opened.")) : Promise.resolve(result("proj-a", "First")),
    });
    await chooseProjectMenuItem(w, "open");
    await w.get('[data-testid="open-project-row-proj-b"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);
    expect(useEditorProjectStore().snapshot?.projectId).toBe("proj-a");
    expect(w.text()).toContain("That project could not be opened.");
  });

  // Fix round 1: an open a newer open superseded installs nothing, so it
  // must not hydrate or recovery-check whatever session is current then —
  // that session is the newer open's to set up.
  it("a superseded pick hydrates nothing and runs no recovery check", async () => {
    let releaseB!: (r: EditorOpenResult) => void;
    const workspaceReads: string[] = [];
    const listProjects = vi.fn(() => Promise.resolve([CURRENT, OTHER]));
    const w = await mountRoot({
      openProject: (id) => {
        if (id === "proj-b") return new Promise<EditorOpenResult>((res) => (releaseB = res));
        if (id === "proj-c") return Promise.resolve(result("proj-c", "Third"));
        return Promise.resolve(result("proj-a", "First"));
      },
      getWorkspace: (sessionId) => {
        workspaceReads.push(sessionId);
        return Promise.resolve({});
      },
      listProjects,
    });
    await chooseProjectMenuItem(w, "open");
    await w.get('[data-testid="open-project-row-proj-b"]').trigger("click");
    await flushPromises();
    // A newer open lands first (here straight through the store).
    await useEditorProjectStore().openProject("proj-c", false);
    const listsBefore = listProjects.mock.calls.length;
    releaseB(result("proj-b", "Second"));
    await flushPromises();

    expect(useEditorProjectStore().snapshot?.projectId).toBe("proj-c");
    expect(workspaceReads).not.toContain("ses-proj-c");
    expect(listProjects.mock.calls.length).toBe(listsBefore);
  });

  it("Open a project file… asks Rust (editor_import_package)", async () => {
    const importPackage = vi.fn(() => Promise.resolve(null));
    const w = await mountRoot({ importPackage });
    await chooseProjectMenuItem(w, "openFile");
    expect(importPackage).toHaveBeenCalledTimes(1);
  });

  it("Rename tutorial… opens the rename dialog, whose Apply sends rename", async () => {
    const executed: EditorCommand[] = [];
    const w = await mountRoot({
      execute: (req) => {
        executed.push(req.command);
        const next = result("proj-a", "New name", "ses-proj-a");
        next.snapshot.revision = 2;
        return Promise.resolve({ snapshot: next.snapshot, project: next.project });
      },
    });
    await chooseProjectMenuItem(w, "rename");
    await w.get('[data-testid="rename-dialog-input"]').setValue("New name");
    await w.get('[data-testid="rename-dialog-apply"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "rename", title: "New name" }]);
  });

  it("Workspace & rendered products opens the library's Project section", async () => {
    const w = await mountRoot();
    const before = revealSerial("library");
    const section = revealSerial("projectSection");
    await chooseProjectMenuItem(w, "products");
    expect(revealSerial("projectSection")).toBe(section + 1);
    expect(revealSerial("library")).toBe(before + 1);
  });

  it("Save a copy as project file… opens the Save project dialog", async () => {
    const w = await mountRoot();
    await chooseProjectMenuItem(w, "saveCopy");
    expect(w.find('[data-testid="save-project-dialog"]').exists()).toBe(true);
  });

  it("Discard project… opens the discard confirm", async () => {
    const w = await mountRoot();
    await chooseProjectMenuItem(w, "discard");
    expect(w.find('[data-testid="discard-project-dialog"]').exists()).toBe(true);
  });
});
