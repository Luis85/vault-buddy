/**
 * The concept's share dialogs (visual-parity Task 21; concept spec §9.4–9.6,
 * screens 07/08/09; design D6, D8, D10, D14): Before you share, Save a copy
 * as project file, Render a video and Publish to vault — their anatomy and
 * the native meaning of every control the concept draws.
 *
 * - Checks: the summary box, NOTE/REVIEW/FIX rows (a title, the finding's
 *   sentence, a reveal button with a chevron), the closing help, and a footer
 *   of Export diagnostics · Back to edit · Continue to render. No
 *   "not downloaded" note (D10).
 * - Save a copy (D8): the intro, the Project name (a changed name renames the
 *   tutorial before the copy is written — the name the file dialog suggests
 *   comes from the title), two option cards, the checklist, and Keep editing
 *   · Save copy. There is NO "include rendered videos" checkbox: a project
 *   file never carries rendered videos natively, so the slot says so (D14).
 * - Render: the review range box, the render-parent card, the name, the
 *   destination by vault NAME (D6), the quality profile card, the "new
 *   output" callout, the checks, and Save project instead · Render video.
 * - Publish: the vault select lists names; its actions sit in the footer.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ChecksDialog from "../src/components/editor/dialogs/ChecksDialog.vue";
import PublishDialog from "../src/components/editor/dialogs/PublishDialog.vue";
import RenderDialog from "../src/components/editor/dialogs/RenderDialog.vue";
import SaveProjectDialog from "../src/components/editor/dialogs/SaveProjectDialog.vue";
import type { EditorPort } from "../src/editor/port";
import type { CheckFinding, PackageReceipt } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useNotificationsStore } from "../src/stores/notifications";
import { openResult, openWithRenders, progress, SESSION } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function finding(overrides: Partial<CheckFinding>): CheckFinding {
  return {
    id: "chk-gap-c1",
    severity: "warning",
    code: "gap",
    message: "No clip on V1 from 0:04 to 0:06.",
    target: { kind: "clip", id: "c1" },
    action: "select",
    ...overrides,
  };
}

/** A session whose project names `vault-b` as its destination. */
async function openWithVault(extra: Partial<EditorPort> = {}) {
  const opened = openResult();
  opened.project.destination = { vault: "vault-b", folder: "Tutorials", dated: false };
  return openWithRenders({
    openStaged: () => Promise.resolve(opened),
    listVaults: () =>
      Promise.resolve([
        { id: "vault-a", name: "Panel Pick" },
        { id: "vault-b", name: "Knowledge vault" },
      ]),
    getProducts: () => Promise.resolve([]),
    ...extra,
  });
}

const footerLabels = (w: ReturnType<typeof mount>) =>
  w.get("footer").findAll("button").map((b) => b.text());

describe("Before you share (§9.4, screen 07)", () => {
  async function openChecks(findings: CheckFinding[], extra: Partial<EditorPort> = {}) {
    const env = await openWithRenders({ getChecks: () => Promise.resolve(findings), ...extra });
    const w = mount(ChecksDialog, { props: { open: true }, attachTo: document.body });
    await flushPromises();
    return { ...env, w };
  }

  it("is 680 wide, with the summary box and its two lines", async () => {
    const { w } = await openChecks([]);
    expect((w.get('[data-testid="dialog-host-content"]').element as HTMLElement).style.width).toBe("680px");
    const summary = w.get('[data-testid="checks-summary"]');
    expect(summary.classes()).toEqual(expect.arrayContaining(["border-line", "bg-app", "rounded-[9px]"]));
    expect(summary.get("b").text()).toBe("0 blockers · 0 review warnings");
    expect(summary.text()).toContain("These checks inspect the edit, not the meaning of your tutorial.");
  });

  it("draws each finding as a row: a kind chip, a title, the sentence and a reveal button with a chevron", async () => {
    const { w } = await openChecks([
      finding({ id: "chk-note", severity: "info", code: "captionDensity", message: "Caption 2 reads fast.", action: "openCaptions" }),
      finding({ id: "chk-fix", severity: "blocking", code: "missingMedia", message: '"capture.mp4" is missing.', action: "reconnect" }),
      finding({ id: "chk-review", severity: "warning" }),
    ]);
    const chips = ["chk-fix", "chk-review", "chk-note"].map((id) => w.get(`[data-testid="check-kind-${id}"]`));
    expect(chips.map((c) => c.text())).toEqual(["FIX", "REVIEW", "NOTE"]);
    expect(chips[0].classes()).toContain("text-danger-fg");
    expect(chips[1].classes()).toEqual(expect.arrayContaining(["bg-gold-bg", "text-gold"]));
    expect(chips[2].classes()).toEqual(expect.arrayContaining(["bg-accent-bg", "text-accent-ink"]));
    expect(chips[0].classes()).toContain("vb-mono");

    const row = w.get('[data-testid="check-chk-note"]');
    expect(row.get("h3").text()).toBe("A caption reads fast");
    expect(row.get("p").text()).toBe("Caption 2 reads fast.");
    const action = w.get('[data-testid="check-action-chk-note"]');
    expect(action.text()).toBe("Open Captions");
    expect(action.find("svg").exists()).toBe(true);
    // The rows run FIX, REVIEW, NOTE in the one list.
    const order = w.findAll('[data-testid^="check-kind-"]').map((c) => c.text());
    expect(order).toEqual(["FIX", "REVIEW", "NOTE"]);
  });

  it("never says the project has not been downloaded (D10), and closes with the help line", async () => {
    const { w } = await openChecks([finding({})]);
    expect(w.text()).not.toMatch(/download/i);
    expect(w.get('[data-testid="checks-help"]').text()).toBe(
      "No automatic speech transcription, content review or privacy detection is performed. This is not an accessibility certification.",
    );
  });

  it("an empty list is the concept's callout", async () => {
    const { w } = await openChecks([]);
    const empty = w.get('[data-testid="checks-empty"]');
    expect(empty.classes()).toEqual(expect.arrayContaining(["bg-accent-bg"]));
    expect(empty.text()).toMatch(/No structural issues found/);
  });

  it("the footer is Export diagnostics · Back to edit · Continue to render (primary)", async () => {
    const { w } = await openChecks([]);
    expect(footerLabels(w)).toEqual(["Export diagnostics", "Back to edit", "Continue to render"]);
    expect(w.get('[data-testid="checks-render"]').classes()).toContain("bg-primary");
  });

  it("Export diagnostics takes Help's own path: Rust's dialog, then a toast naming the file", async () => {
    const exportDiagnostics = vi.fn(() => Promise.resolve("diagnostics.json"));
    const { w } = await openChecks([], { exportDiagnostics });
    await w.get('[data-testid="checks-diagnostics"]').trigger("click");
    await flushPromises();
    expect(exportDiagnostics).toHaveBeenCalledTimes(1);
    expect(useNotificationsStore().items.map((n) => n.message)).toEqual([
      "Saved diagnostics to diagnostics.json. It holds counts and error codes, never project content.",
    ]);
  });
});

describe("Save a copy as project file (§9.5, screen 08; D8)", () => {
  function receipt(revision: number, format: "portable" | "lightweight" = "portable"): PackageReceipt {
    return { sessionId: SESSION, savedRevision: revision, fileName: "Walkthrough.vbproject.zip", format };
  }

  async function openSave(extra: Partial<EditorPort> = {}) {
    const exportPackage = vi.fn((_s: string, revision: number) => Promise.resolve(receipt(revision)));
    const env = await openWithRenders({ exportPackage, ...extra });
    const w = mount(SaveProjectDialog, { props: { open: true, initialFormat: "portable" }, attachTo: document.body });
    await flushPromises();
    return { ...env, w, exportPackage };
  }

  it("shows the intro tile, the heading and its sentence", async () => {
    const { w } = await openSave();
    const intro = w.get('[data-testid="save-project-intro"]');
    expect(intro.find("svg").exists()).toBe(true);
    expect(intro.get("h3").text()).toBe("Your workspace. Ready to continue.");
    expect(intro.get("p").text()).toContain("Nothing is flattened, and your original media is never changed.");
  });

  it("offers two option cards, the portable one Recommended and selected", async () => {
    const { w } = await openSave();
    const portable = w.get('[data-testid="save-project-option-portable"]');
    const light = w.get('[data-testid="save-project-option-lightweight"]');
    expect(portable.get("b").text()).toContain("Portable project");
    expect(portable.get('[data-testid="save-project-recommended"]').text()).toBe("Recommended");
    expect(light.get("b").text()).toBe("Lightweight project file");
    expect(portable.classes()).toEqual(expect.arrayContaining(["border-accent", "bg-accent-bg"]));
    expect(light.classes()).not.toContain("border-accent");
    await w.get('[data-testid="save-project-format-lightweight"]').setValue(true);
    expect(light.classes()).toContain("border-accent");
  });

  it("says rendered videos stay in the workspace — there is no checkbox that could not be honoured", async () => {
    const { w } = await openSave();
    expect(w.findAll('input[type="checkbox"]')).toHaveLength(0);
    expect(w.get('[data-testid="save-project-products"]').text()).toMatch(/Rendered videos stay in this project's workspace/);
  });

  it("lists what the copy keeps in a 2×2 checklist", async () => {
    const { w } = await openSave();
    const items = w.get('[data-testid="save-project-checklist"]').findAll("li");
    expect(items.map((i) => i.text())).toEqual([
      "All tracks & clips",
      "Fades & teaching layers",
      "Mixer & chapter markers",
      "Playhead & workspace layout",
    ]);
    expect(w.get('[data-testid="save-project-checklist"]').classes()).toContain("grid-cols-2");
  });

  it("the footer is Keep editing · Save copy (primary)", async () => {
    const { w } = await openSave();
    expect(footerLabels(w)).toEqual(["Keep editing", "Save copy"]);
    expect(w.get('[data-testid="save-project-confirm"]').classes()).toContain("bg-primary");
  });

  it("the Project name is the title; an unchanged one sends no rename", async () => {
    const execute = vi.fn();
    const { w, exportPackage } = await openSave({ execute });
    expect((w.get('[data-testid="save-project-name"]').element as HTMLInputElement).value).toBe("Walkthrough");
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    expect(execute).not.toHaveBeenCalled();
    expect(exportPackage).toHaveBeenCalledWith(SESSION, 7, "portable");
  });

  it("a changed name renames the tutorial first, then saves the renamed revision", async () => {
    const execute = vi.fn((req: { expectedRevision: number; command: unknown }) =>
      Promise.resolve({ snapshot: { ...openResult().snapshot, revision: req.expectedRevision + 1, title: "Atlas" }, project: openResult().project }),
    );
    const { w, exportPackage } = await openSave({ execute: execute as unknown as EditorPort["execute"] });
    await w.get('[data-testid="save-project-name"]').setValue("  Atlas ");
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    expect(execute.mock.calls[0][0].command).toEqual({ kind: "rename", title: "Atlas" });
    expect(exportPackage).toHaveBeenCalledWith(SESSION, 8, "portable");
    expect(w.get('[data-testid="save-project-status"]').text()).toBe("Saved to Walkthrough.vbproject.zip");
  });

  it("an empty name cannot be saved, and says why", async () => {
    const { w, exportPackage } = await openSave();
    await w.get('[data-testid="save-project-name"]').setValue("   ");
    const confirm = w.get('[data-testid="save-project-confirm"]');
    expect(confirm.attributes("disabled")).toBeDefined();
    expect(confirm.attributes("title")).toBe("Give the project a name.");
    expect(w.get('[data-testid="save-project-reason"]').text()).toBe("Give the project a name.");
    await confirm.trigger("click");
    expect(exportPackage).not.toHaveBeenCalled();
  });
});

describe("Render a video (§9.6, screen 09)", () => {
  async function openRender(extra: Partial<EditorPort> = {}) {
    const env = await openWithVault(extra);
    const w = mount(RenderDialog, { props: { open: true, initialRange: null }, attachTo: document.body });
    await flushPromises();
    return { ...env, w };
  }

  it("draws the review range box: a checkbox, its help and From/To in seconds", async () => {
    const { w } = await openRender();
    const box = w.get('[data-testid="render-dialog-range-box"]');
    const toggle = box.get('[data-testid="render-dialog-scope-range"]');
    expect(toggle.attributes("type")).toBe("checkbox");
    expect(box.text()).toContain("Render a short review range");
    expect(box.text()).toContain("Test a section before rendering everything.");
    expect(box.find('[data-testid="render-dialog-range-start"]').exists()).toBe(false);
    await toggle.setValue(true);
    expect(box.find('[data-testid="render-dialog-range-start"]').exists()).toBe(true);
    expect(box.text()).toContain("From (seconds)");
    expect(box.text()).toContain("To (seconds)");
  });

  it("names the render's parent: the PROJECT · r{revision} pill, the title and what stays editable", async () => {
    const { w } = await openRender();
    const parent = w.get('[data-testid="render-dialog-parent"]');
    expect(parent.get('[data-testid="render-dialog-revision"]').text()).toBe("PROJECT · r7");
    expect(parent.get("b").text()).toBe("Walkthrough");
    expect(parent.text()).toContain("This render becomes a product. The project stays editable.");
  });

  it("shows the destination vault by NAME, never its id (D6)", async () => {
    const { w } = await openRender();
    const destination = w.get('[data-testid="render-dialog-destination"]');
    expect(destination.text()).toContain("Knowledge vault");
    expect(destination.text()).toContain("Tutorials");
    expect(w.text()).not.toContain("vault-b");
  });

  it("an unreadable vault list reads as the capture's vault, never the id", async () => {
    const { w } = await openRender({ listVaults: () => Promise.reject(new Error("offline")) });
    expect(w.get('[data-testid="render-dialog-destination"]').text()).toContain("the capture's vault");
    expect(w.text()).not.toContain("vault-b");
  });

  it("the quality radios live in the profile card with the canvas size — no browser review label (D10)", async () => {
    const { w } = await openRender();
    const profile = w.get('[data-testid="render-dialog-profile"]');
    expect(profile.find('[data-testid="render-dialog-quality-balanced"]').exists()).toBe(true);
    expect(profile.get("b").text()).toBe("1280 × 720 · 30 fps · MP4");
    expect(w.text()).not.toMatch(/browser/i);
    expect(w.text()).not.toMatch(/WebM/);
  });

  it("the accent callout: a new output, never an overwrite", async () => {
    const { w } = await openRender();
    const callout = w.get('[data-testid="render-dialog-originals"]');
    expect(callout.get("b").text()).toBe("A new output, never an overwrite.");
    expect(callout.classes()).toContain("bg-accent-bg");
  });

  it("the footer is Save project instead · Render video (primary, with its icon)", async () => {
    const { w } = await openRender();
    expect(footerLabels(w)).toEqual(["Save project instead", "Render video"]);
    const start = w.get('[data-testid="render-dialog-start"]');
    expect(start.classes()).toContain("bg-primary");
    expect(start.find("svg").exists()).toBe(true);
  });

  it("Save project instead runs Save project and steps aside", async () => {
    const save = vi.fn(() => Promise.resolve({ sessionId: SESSION, savedRevision: 7, projectFileId: "project-r" }));
    const { w } = await openRender({ save });
    await w.get('[data-testid="render-dialog-save-instead"]').trigger("click");
    await flushPromises();
    expect(save).toHaveBeenCalledWith(SESSION, 7);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("Save project instead is disabled, with its reason, while a save runs (no second save)", async () => {
    const save = vi.fn(() => new Promise<never>(() => {}));
    const { w } = await openRender({ save });
    void useEditorProjectStore().save();
    await flushPromises();
    const instead = w.get('[data-testid="render-dialog-save-instead"]');
    expect(instead.attributes("disabled")).toBeDefined();
    expect(instead.attributes("title")).toBe("Saving…");
    await instead.trigger("click");
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("the completion line names the vault a publish goes to", async () => {
    const { w, deliver } = await openRender();
    await w.get('[data-testid="render-dialog-start"]').trigger("click");
    await flushPromises();
    deliver(0, progress("job-0", { sequence: 2, phase: "complete", fraction: 1, terminal: { productId: "prod-b" } }));
    await flushPromises();
    expect(w.get('[data-testid="render-dialog-publish-line"]').text()).toContain("Knowledge vault");
    expect(w.text()).not.toContain("vault-b");
  });
});

describe("Publish to vault", () => {
  it("lists vaults by name, with its actions in the footer", async () => {
    await openWithVault({ publishDefaults: () => Promise.resolve({ dated: false, createNote: true }) });
    const w = mount(PublishDialog, {
      props: { open: true, productId: "prod-a", productName: "Walkthrough v1" },
      attachTo: document.body,
    });
    await flushPromises();
    const options = w.get('[data-testid="publish-vault"]').findAll("option").map((o) => o.text());
    expect(options).toEqual(["Choose a vault…", "Panel Pick", "Knowledge vault"]);
    expect(footerLabels(w)).toEqual(["Cancel", "Publish"]);
    expect(w.get('[data-testid="publish-start"]').classes()).toContain("bg-primary");
    expect(w.text()).not.toContain("vault-b");
  });
});
