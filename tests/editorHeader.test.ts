/**
 * The editor's header (visual-parity Task 8; design D6, D10; concept spec
 * §2, screens 01–02): the brand, the Project menu trigger, the document
 * title (click → Rename tutorial), the save state as a dot and words, and
 * Help / Checks / Save project / Render video with their icons. The pixel
 * geometry (56 / 52 tall) is measured in `tests/e2e/editorParity.spec.ts`;
 * happy-dom has no layout engine.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import EditorShell from "../src/components/editor/shell/EditorShell.vue";
import type { CheckFinding, EditorCommand, EditorOpenResult, EditorSnapshot, Project } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

function setViewportWidth(width: number) {
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: width });
}

beforeEach(() => {
  setActivePinia(createPinia());
  setViewportWidth(1600);
});

function snapshot(overrides: Partial<EditorSnapshot> = {}): EditorSnapshot {
  return {
    sessionId: "ses-a",
    projectId: "project-a",
    revision: 2,
    persistedRevision: 2,
    title: "Create your first project",
    durationMs: 6000,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
    ...overrides,
  };
}

function project(): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-a",
    title: "Create your first project",
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
  };
}

function openResult(snap: Partial<EditorSnapshot> = {}): EditorOpenResult {
  return { snapshot: snapshot(snap), project: project(), workspace: {}, missing: [], sourceBase: "base", recovered: false };
}

function finding(code: CheckFinding["code"]): CheckFinding {
  return { id: `chk-${code}`, severity: "warning", code, message: "A finding", target: { kind: "project", id: null }, action: null };
}

async function openShell(opts: { snap?: Partial<EditorSnapshot>; checks?: CheckFinding[]; executed?: EditorCommand[] } = {}) {
  const store = useEditorProjectStore();
  store.setPort(
    fakeEditorPort({
      openStaged: () => Promise.resolve(openResult(opts.snap)),
      getChecks: () => Promise.resolve(opts.checks ?? []),
      getProducts: () => Promise.resolve([]),
      execute: (req) => {
        opts.executed?.push(req.command);
        return Promise.resolve({ snapshot: snapshot({ revision: 3, title: "Renamed" }), project: project() });
      },
    }),
  );
  await store.openStaged("base");
  const w = mount(EditorShell, { attachTo: document.body });
  await flushPromises();
  return w;
}

describe("the brand", () => {
  it("is the CSS mark (two eyes) and the wordmark, with the concept's tooltip", async () => {
    const w = await openShell();
    const brand = w.get('[data-testid="editor-header-brand"]');
    expect(brand.attributes("title")).toBe("Vault Buddy · Tutorial Editor");
    expect(w.get('[data-testid="editor-header-brand-mark"]').text()).toBe("··");
    // A drawing, not an image file.
    expect(brand.find("img").exists()).toBe(false);
    expect(w.get('[data-testid="editor-header-wordmark"]').text()).toBe("vault-buddy");
    expect(w.get('[data-testid="editor-header-wordmark"]').isVisible()).toBe(true);
  });

  it("drops the wordmark at or below 1350px wide (§1.4)", async () => {
    setViewportWidth(1350);
    const w = await openShell();
    expect(w.get('[data-testid="editor-header-brand-mark"]').isVisible()).toBe(true);
    expect(w.get('[data-testid="editor-header-wordmark"]').isVisible()).toBe(false);
  });
});

describe("the document title", () => {
  it("is a button naming the tutorial; a click opens Rename tutorial, and Apply renames", async () => {
    const executed: EditorCommand[] = [];
    const w = await openShell({ executed });
    const title = w.get('[data-testid="editor-header-title"]');
    expect(title.element.tagName).toBe("BUTTON");
    expect(title.text()).toBe("Create your first project");
    // The pencil that appears on hover.
    expect(title.find("svg").exists()).toBe(true);

    await title.trigger("click");
    await flushPromises();
    const dialog = w.get('[data-testid="rename-dialog"]');
    expect(document.querySelector('[role="dialog"]')?.textContent).toContain("Rename tutorial");
    const input = dialog.get('[data-testid="rename-dialog-input"]');
    expect((input.element as HTMLInputElement).value).toBe("Create your first project");
    await input.setValue("  Renamed  ");
    await w.get('[data-testid="rename-dialog-apply"]').trigger("click");
    await flushPromises();

    expect(executed).toEqual([{ kind: "rename", title: "Renamed" }]);
    expect(w.find('[data-testid="rename-dialog"]').exists()).toBe(false);
  });

  it("Cancel renames nothing; an empty title cannot be applied and says why", async () => {
    const executed: EditorCommand[] = [];
    const w = await openShell({ executed });
    await w.get('[data-testid="editor-header-title"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="rename-dialog-input"]').setValue("   ");
    const apply = w.get('[data-testid="rename-dialog-apply"]');
    expect(apply.attributes("disabled")).toBeDefined();
    expect(apply.attributes("title")).toBe("A title can't be empty.");

    await w.get('[data-testid="rename-dialog-cancel"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([]);
    expect(w.find('[data-testid="rename-dialog"]').exists()).toBe(false);
  });

  // Fix round 1: a refused rename keeps the dialog and the person's draft
  // (the refusal itself is the shell's toast, visual-parity Task 7).
  it("a refused rename keeps the dialog open with the draft", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: () => Promise.resolve(openResult()),
        getChecks: () => Promise.resolve([]),
        getProducts: () => Promise.resolve([]),
        execute: () => Promise.reject(new Error("That title cannot be used.")),
      }),
    );
    await store.openStaged("base");
    const w = mount(EditorShell, { attachTo: document.body });
    await flushPromises();
    await w.get('[data-testid="editor-header-title"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="rename-dialog-input"]').setValue("Refused name");
    await w.get('[data-testid="rename-dialog-apply"]').trigger("click");
    await flushPromises();

    expect(w.find('[data-testid="rename-dialog"]').exists()).toBe(true);
    expect((w.get('[data-testid="rename-dialog-input"]').element as HTMLInputElement).value).toBe("Refused name");
    expect(w.get('[data-testid="rename-dialog-apply"]').attributes("disabled")).toBeUndefined();
    expect(w.text()).toContain("That title cannot be used.");
  });

  it("Enter in the Title field applies it", async () => {
    const executed: EditorCommand[] = [];
    const w = await openShell({ executed });
    await w.get('[data-testid="editor-header-title"]').trigger("click");
    await flushPromises();
    const input = w.get('[data-testid="rename-dialog-input"]');
    await input.setValue("Renamed");
    await input.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(executed).toEqual([{ kind: "rename", title: "Renamed" }]);
  });
});

describe("the save state (D10)", () => {
  it("is a gold dot and 'Unsaved changes' when dirty", async () => {
    const w = await openShell({ snap: { revision: 3, persistedRevision: 2 } });
    const state = w.get('[data-testid="editor-header-save-state"]');
    expect(state.text()).toBe("Unsaved changes");
    expect(state.get('[data-testid="editor-header-save-dot"]').classes()).toContain("bg-gold");
  });

  it("is a teal dot and 'Saved' when clean", async () => {
    const w = await openShell();
    const state = w.get('[data-testid="editor-header-save-state"]');
    expect(state.text()).toBe("Saved");
    expect(state.get('[data-testid="editor-header-save-dot"]').classes()).toContain("bg-audio");
  });
});

describe("what the header carries", () => {
  it("never shows the destination vault's id (D6)", async () => {
    const w = await openShell();
    expect(w.get('[data-testid="editor-header"]').text()).not.toContain("vault-a");
    expect(w.find('[data-testid="editor-shell-vault"]').exists()).toBe(false);
  });

  it("Help, Checks, Save project and Render video each carry their icon", async () => {
    const w = await openShell();
    const withIcon = ["editor-header-help", "editor-header-checks", "editor-header-save", "editor-header-render"].filter(
      (id) => w.get(`[data-testid="${id}"]`).find("svg").exists(),
    );
    expect(withIcon).toHaveLength(4);
    expect(w.get('[data-testid="editor-header-save"]').text()).toBe("Save project");
    expect(w.get('[data-testid="editor-header-render"]').text()).toBe("Render video");
  });

  // Fix round 1: a disabled Render video keeps its fill under the pointer —
  // the hover colour is for a button that can act (the base rule's
  // `:hover:not(:disabled)` posture).
  it("the primary fill only takes its hover colour while enabled", async () => {
    const w = await openShell({ snap: { durationMs: 0 } });
    const render = w.get('[data-testid="editor-header-render"]');
    expect(render.attributes("disabled")).toBeDefined();
    expect(render.classes()).toContain("enabled:hover:bg-primary-hover");
    expect(render.classes()).not.toContain("hover:bg-primary-hover");
  });

  it("the Save ▾ split button is gone; its items live in the Project menu", async () => {
    const w = await openShell();
    expect(w.find('[data-testid="editor-header-save-menu-toggle"]').exists()).toBe(false);
    expect(w.find('[data-testid="editor-header-project-menu"]').exists()).toBe(true);
  });

  it("the theme toggle left the header for the View menu's Light theme (ruling P5)", async () => {
    const w = await openShell();
    expect(w.find('[data-testid="editor-header-theme-toggle"]').exists()).toBe(false);
    expect(w.get('[data-testid="editor-header"]').find('[aria-label$="theme"]').exists()).toBe(false);
  });

  it("the library and inspector toggles left the header; the preview header opens the drawers", async () => {
    setViewportWidth(960);
    const w = await openShell();
    expect(w.find('[data-testid="editor-header-library-toggle"]').exists()).toBe(false);
    expect(w.find('[data-testid="editor-header-inspector-toggle"]').exists()).toBe(false);
    expect(w.get('[data-testid="editor-shell-inspector"]').isVisible()).toBe(false);
    await w.get('[data-testid="preview-properties-toggle"]').trigger("click");
    expect(w.get('[data-testid="editor-shell-inspector"]').isVisible()).toBe(true);
  });
});

describe("the Checks count chip (ruling T3-2)", () => {
  it("is hidden with no findings", async () => {
    const w = await openShell({ checks: [] });
    expect(w.find('[data-testid="editor-header-checks-count"]').exists()).toBe(false);
  });

  it("is a mono gold chip with the count when there are findings", async () => {
    const w = await openShell({ checks: [finding("gap"), finding("clipping")] });
    const chip = w.get('[data-testid="editor-header-checks-count"]');
    expect(chip.text()).toBe("2");
    expect(chip.classes()).toEqual(expect.arrayContaining(["vb-mono", "text-gold", "bg-gold-bg"]));
  });

  // Fix round 1: the chip caps like the shared CountBadge it replaced.
  it("caps at 99+", async () => {
    const many = Array.from({ length: 120 }, (_, i) => ({ ...finding("gap"), id: `chk-gap-${i}` }));
    const w = await openShell({ checks: many });
    expect(w.get('[data-testid="editor-header-checks-count"]').text()).toBe("99+");
    expect(w.get('[data-testid="editor-header-checks"]').attributes("aria-label")).toBe("Checks, 120 to review");
  });
});
