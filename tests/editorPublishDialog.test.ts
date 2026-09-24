/**
 * Publish to vault (Task 48; F-43, F-35; ADR R13): the Publish dialog — a
 * vault picker defaulting to the CAPTURE's vault (F-01), the folder, the
 * dated toggle and the companion-note choice; the receipt's landed names
 * and Open; a dialog that never closes onto a publish still in flight —
 * plus the port's wire for the two new commands, the subtitle export, and
 * Task 47's carry: the Render and Review dialogs cannot be closed while
 * their START request is still in flight.
 */
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import PublishDialog from "../src/components/editor/dialogs/PublishDialog.vue";
import RenderDialog from "../src/components/editor/dialogs/RenderDialog.vue";
import ReviewDialog from "../src/components/editor/dialogs/ReviewDialog.vue";
import CaptionsExport from "../src/components/editor/library/CaptionsExport.vue";
import ProductLibrary from "../src/components/editor/library/ProductLibrary.vue";
import { decodeNullableFileName, decodePublishReceipt } from "../src/editor/decodeRender";
import type { EditorPort } from "../src/editor/port";
import { createTauriEditorPort, EditorPortError } from "../src/editor/port";
import type { PublishReceipt, RenderStarted } from "../src/editorTypes";
import { useVaultsStore } from "../src/stores/vaults";
import { openResult, openWithRenders, product, SESSION } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

afterEach(() => {
  clearMocks();
});

const VAULTS = [
  { id: "vault-a", name: "Panel Pick" },
  { id: "vault-b", name: "Capture Vault" },
];

function receipt(overrides: Partial<PublishReceipt> = {}): PublishReceipt {
  return {
    videoPath: "C:\\Vaults\\B\\Tutorials\\2026\\09\\2026-09-24 1000 Walkthrough v1 (2).mp4",
    notePath: "C:\\Vaults\\B\\Tutorials\\2026\\09\\2026-09-24 1000 Walkthrough v1 (2).md",
    vaultId: "vault-b",
    vaultName: "Capture Vault",
    warning: null,
    ...overrides,
  };
}

/** A session whose project was captured into `vault-b`, with a publish
 * port whose replies the test controls. */
async function openPublish(extra: Partial<EditorPort> = {}) {
  const opened = openResult();
  opened.project.destination = { vault: "vault-b", folder: "Tutorials", dated: true };
  const publishProduct = vi.fn(() => Promise.resolve(receipt()));
  const openScreenCapture = vi.fn(() => Promise.resolve());
  const env = await openWithRenders({
    openStaged: () => Promise.resolve(opened),
    listVaults: () => Promise.resolve(VAULTS),
    publishProduct,
    openScreenCapture,
    ...extra,
  });
  const w = mount(PublishDialog, { props: { open: true, productId: "prod-a", productName: "Walkthrough v1" } });
  await flushPromises();
  return { ...env, w, publishProduct, openScreenCapture };
}

describe("PublishDialog", () => {
  // F-01: a publish goes where the capture came from. The panel's own
  // vault choices (its record/capture pickers) are a different window's
  // state and must never leak into this default.
  it("publish defaults to the capture's vault, not the panel's selection", async () => {
    const vaults = useVaultsStore();
    vaults.recordModeVaultId = "vault-a";
    vaults.screenCaptureVaultId = "vault-a";
    const { w, publishProduct } = await openPublish();
    const select = w.get('[data-testid="publish-vault"]').element as HTMLSelectElement;
    expect(select.value).toBe("vault-b");
    expect((w.get('[data-testid="publish-folder"]').element as HTMLInputElement).value).toBe("Tutorials");
    expect((w.get('[data-testid="publish-dated"]').element as HTMLInputElement).checked).toBe(true);
    expect((w.get('[data-testid="publish-create-note"]').element as HTMLInputElement).checked).toBe(true);
    await w.get('[data-testid="publish-start"]').trigger("click");
    await flushPromises();
    expect(publishProduct).toHaveBeenCalledWith(SESSION, "prod-a", {
      vaultId: "vault-b",
      folder: "Tutorials",
      dated: true,
      createNote: true,
    });
  });

  it("shows the landed names and opens the note in Obsidian", async () => {
    const { w, openScreenCapture } = await openPublish();
    await w.get('[data-testid="publish-start"]').trigger("click");
    await flushPromises();
    const result = w.get('[data-testid="publish-result"]').text();
    expect(result).toContain("Capture Vault");
    expect(w.get('[data-testid="publish-video-name"]').text()).toBe("2026-09-24 1000 Walkthrough v1 (2).mp4");
    expect(w.get('[data-testid="publish-note-name"]').text()).toBe("2026-09-24 1000 Walkthrough v1 (2).md");
    await w.get('[data-testid="publish-open"]').trigger("click");
    expect(openScreenCapture).toHaveBeenCalledWith("vault-b", receipt().notePath);
  });

  it("a note that could not be written is a warning, and Open goes to the video", async () => {
    const { w, openScreenCapture } = await openPublish({
      publishProduct: () =>
        Promise.resolve(receipt({ notePath: null, warning: "The video was published, but its note was not." })),
    });
    await w.get('[data-testid="publish-start"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="publish-warning"]').text()).toContain("note");
    expect(w.find('[data-testid="publish-note-name"]').exists()).toBe(false);
    await w.get('[data-testid="publish-open"]').trigger("click");
    expect(openScreenCapture).toHaveBeenCalledWith("vault-b", receipt().videoPath);
  });

  // Never close onto an untracked publish: while the request is in flight
  // Close is disabled, Escape/backdrop are refused, and a close request is
  // ignored until the receipt lands.
  it("cannot be closed while the publish is in flight", async () => {
    let land: (r: PublishReceipt) => void = () => undefined;
    const pending = new Promise<PublishReceipt>((resolve) => {
      land = resolve;
    });
    const { w } = await openPublish({ publishProduct: () => pending });
    await w.get('[data-testid="publish-start"]').trigger("click");
    await flushPromises();
    const close = w.get('[data-testid="publish-close"]');
    expect(close.attributes("disabled")).toBeDefined();
    await close.trigger("click");
    await w.get('[role="dialog"]').trigger("keydown", { key: "Escape" });
    expect(w.emitted("close")).toBeUndefined();
    expect(w.get('[data-testid="publish-start"]').attributes("disabled")).toBeDefined();
    land(receipt());
    await flushPromises();
    await w.get('[data-testid="publish-close"]').trigger("click");
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("a refused publish says why and keeps the form", async () => {
    const { w } = await openPublish({
      publishProduct: () =>
        Promise.reject(
          new EditorPortError({
            code: "diskFull",
            message: "Not enough disk space in that vault.",
            retryable: false,
            operationId: "op",
          }),
        ),
    });
    await w.get('[data-testid="publish-start"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="publish-error"]').text()).toContain("Not enough disk space");
    expect(w.find('[data-testid="publish-vault"]').exists()).toBe(true);
    expect(w.find('[data-testid="publish-result"]').exists()).toBe(false);
  });

  // A capture whose vault is no longer in Obsidian has no default: the
  // user must pick one, and the reason says so (R20).
  it("a vault Obsidian no longer lists is not silently replaced", async () => {
    const { w, publishProduct } = await openPublish({ listVaults: () => Promise.resolve([VAULTS[0]]) });
    expect((w.get('[data-testid="publish-vault"]').element as HTMLSelectElement).value).toBe("");
    expect(w.get('[data-testid="publish-start"]').attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="publish-start-reason"]').text()).toMatch(/choose a vault/i);
    await w.get('[data-testid="publish-start"]').trigger("click");
    expect(publishProduct).not.toHaveBeenCalled();
  });
});

describe("PublishDialog — the form and its failures", () => {
  it("sends what the user changed: another vault, a folder, undated, no note", async () => {
    const { w, publishProduct } = await openPublish();
    await w.get('[data-testid="publish-vault"]').setValue("vault-a");
    await w.get('[data-testid="publish-folder"]').setValue("  Guides/Video  ");
    await w.get('[data-testid="publish-dated"]').setValue(false);
    await w.get('[data-testid="publish-create-note"]').setValue(false);
    await w.get('[data-testid="publish-start"]').trigger("click");
    await flushPromises();
    expect(publishProduct).toHaveBeenCalledWith(SESSION, "prod-a", {
      vaultId: "vault-a",
      folder: "Guides/Video",
      dated: false,
      createNote: false,
    });
  });

  it("a vault list that cannot be read is said, and nothing is preselected", async () => {
    const { w } = await openPublish({
      listVaults: () =>
        Promise.reject(
          new EditorPortError({ code: "internal", message: "registry unreadable", retryable: false, operationId: "op" }),
        ),
    });
    expect(w.get('[data-testid="publish-error"]').text()).toContain("registry unreadable");
    expect((w.get('[data-testid="publish-vault"]').element as HTMLSelectElement).value).toBe("");
  });

  it("an Open that Obsidian refuses is said, not swallowed", async () => {
    const { w } = await openPublish({
      openScreenCapture: () =>
        Promise.reject(
          new EditorPortError({ code: "internal", message: "outside its vault", retryable: false, operationId: "op" }),
        ),
    });
    await w.get('[data-testid="publish-start"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="publish-open"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="publish-error"]').text()).toContain("outside its vault");
  });

  it("without a product there is nothing to publish, and it says so", async () => {
    await openPublish();
    const w = mount(PublishDialog, { props: { open: true, productId: null, productName: "" } });
    await flushPromises();
    expect(w.get('[data-testid="publish-start-reason"]').text()).toMatch(/render a video first/i);
  });

  it("reopening resets a finished publish back to the form", async () => {
    const { w } = await openPublish();
    await w.get('[data-testid="publish-start"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="publish-result"]').exists()).toBe(true);
    await w.setProps({ open: false });
    await w.setProps({ open: true });
    await flushPromises();
    expect(w.find('[data-testid="publish-result"]').exists()).toBe(false);
    expect((w.get('[data-testid="publish-vault"]').element as HTMLSelectElement).value).toBe("vault-b");
  });
});

describe("Render and Review dialogs (Task 47 carry)", () => {
  function pendingStart() {
    let land: (r: RenderStarted) => void = () => undefined;
    const startRender = vi.fn(
      () =>
        new Promise<RenderStarted>((resolve) => {
          land = resolve;
        }),
    );
    return { startRender, land: (r: RenderStarted) => land(r) };
  }

  it("the Render dialog cannot be closed while its start request is in flight", async () => {
    const { startRender, land } = pendingStart();
    await openWithRenders({ getProducts: () => Promise.resolve([product()]), startRender });
    const w = mount(RenderDialog, { props: { open: true, initialRange: null } });
    await flushPromises();
    await w.get('[data-testid="render-dialog-start"]').trigger("click");
    const close = w.get('[data-testid="render-dialog-close"]');
    expect(close.attributes("disabled")).toBeDefined();
    await close.trigger("click");
    await w.get('[role="dialog"]').trigger("keydown", { key: "Escape" });
    expect(w.emitted("close")).toBeUndefined();
    land({ jobId: "job-0", revision: 7 });
    await flushPromises();
    expect(w.get('[data-testid="render-dialog-close"]').attributes("disabled")).toBeDefined();
  });

  it("the Review dialog cannot be closed while its start request is in flight", async () => {
    const { startRender } = pendingStart();
    await openWithRenders({ startRender });
    const w = mount(ReviewDialog, { props: { open: true, range: { startMs: 0, endMs: 5_000 } } });
    await flushPromises();
    const close = w.get('[data-testid="review-dialog-close"]');
    expect(close.attributes("disabled")).toBeDefined();
    await close.trigger("click");
    expect(w.emitted("close")).toBeUndefined();
  });

  it("a refused Review start still releases the dialog", async () => {
    const startRender = vi.fn(() =>
      Promise.reject(
        new EditorPortError({ code: "encoderUnavailable", message: "no ffmpeg", retryable: false, operationId: "op" }),
      ),
    );
    await openWithRenders({ startRender });
    const w = mount(ReviewDialog, { props: { open: true, range: { startMs: 0, endMs: 5_000 } } });
    await flushPromises();
    expect(w.get('[data-testid="review-dialog-problem"]').text()).toContain("no ffmpeg");
    await w.get('[data-testid="review-dialog-close"]').trigger("click");
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("Publish to vault… is offered on a completed render and opens the Publish dialog", async () => {
    const env = await openWithRenders({
      getProducts: () => Promise.resolve([product()]),
      listVaults: () => Promise.resolve(VAULTS),
    });
    const w = mount(RenderDialog, { props: { open: true, initialRange: null } });
    await flushPromises();
    await w.get('[data-testid="render-dialog-start"]').trigger("click");
    await flushPromises();
    env.deliver(0, {
      sessionId: SESSION,
      jobId: "job-0",
      kind: "render",
      sequence: 2,
      phase: "complete",
      fraction: 1,
      terminal: { productId: "prod-b" },
    });
    await flushPromises();
    const publish = w.get('[data-testid="render-dialog-publish"]');
    expect(publish.attributes("disabled")).toBeUndefined();
    await publish.trigger("click");
    await flushPromises();
    expect(w.findComponent(PublishDialog).props("productId")).toBe("prod-b");
    expect(w.findComponent(PublishDialog).props("open")).toBe(true);
  });
});

describe("ProductLibrary", () => {
  // Every available product can be published, not only the one a render
  // just finished; a product whose file is gone cannot, and says why.
  it("a product card offers Publish to vault… for that product", async () => {
    await openWithRenders({
      getProducts: () =>
        Promise.resolve([product(), product({ id: "prod-gone", name: "Old cut", available: false })]),
      listVaults: () => Promise.resolve(VAULTS),
    });
    const w = mount(ProductLibrary);
    await flushPromises();
    const gone = w.get('[data-testid="product-publish-prod-gone"]');
    expect(gone.attributes("disabled")).toBeDefined();
    expect(gone.attributes("title")).toMatch(/no longer on disk/i);
    await w.get('[data-testid="product-publish-prod-a"]').trigger("click");
    await flushPromises();
    const dialog = w.findComponent(PublishDialog);
    expect(dialog.props("open")).toBe(true);
    expect(dialog.props("productId")).toBe("prod-a");
    expect(dialog.props("productName")).toBe("Walkthrough v1");
  });
});

describe("CaptionsExport", () => {
  it("exports the chosen format through the port and names the file", async () => {
    const exportSubtitles = vi.fn(() => Promise.resolve("walkthrough.srt"));
    await openWithRenders({ exportSubtitles });
    const w = mount(CaptionsExport, { props: { reason: null } });
    await w.get('[data-testid="caption-export-srt"]').trigger("click");
    await flushPromises();
    expect(exportSubtitles).toHaveBeenCalledWith(SESSION, "srt");
    expect(w.get('[data-testid="caption-export-status"]').text()).toContain("walkthrough.srt");
    await w.get('[data-testid="caption-export-vtt"]').trigger("click");
    await flushPromises();
    expect(exportSubtitles).toHaveBeenLastCalledWith(SESSION, "vtt");
  });

  it("a refused export is an alert, and a dismissed dialog says nothing", async () => {
    const exportSubtitles = vi
      .fn()
      .mockResolvedValueOnce(null)
      .mockRejectedValueOnce(
        new EditorPortError({ code: "writeDenied", message: "already exists", retryable: false, operationId: "op" }),
      );
    await openWithRenders({ exportSubtitles });
    const w = mount(CaptionsExport, { props: { reason: null } });
    await w.get('[data-testid="caption-export-srt"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="caption-export-status"]').exists()).toBe(false);
    await w.get('[data-testid="caption-export-srt"]').trigger("click");
    await flushPromises();
    const status = w.get('[data-testid="caption-export-status"]');
    expect(status.attributes("role")).toBe("alert");
    expect(status.text()).toContain("already exists");
  });

  it("is disabled with its reason when there is nothing to export", async () => {
    const exportSubtitles = vi.fn(() => Promise.resolve(null));
    await openWithRenders({ exportSubtitles });
    const w = mount(CaptionsExport, { props: { reason: "No captions to export yet." } });
    const button = w.get('[data-testid="caption-export-srt"]');
    expect(button.attributes("disabled")).toBeDefined();
    expect(button.attributes("title")).toBe("No captions to export yet.");
    await button.trigger("click");
    expect(exportSubtitles).not.toHaveBeenCalled();
  });
});

describe("the publish and subtitle wire", () => {
  it("publishProduct, exportSubtitles, listVaults and openScreenCapture send their exact arguments", async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "editor_publish_product") return receipt();
      if (cmd === "editor_export_subtitles") return null;
      if (cmd === "list_vaults") return [{ id: "v1", name: "Notes", path: "C:\\N", open: false }];
      if (cmd === "open_screen_capture") return null;
      throw new Error(`unexpected command ${cmd}`);
    });
    const port = createTauriEditorPort();
    const destination = { vaultId: "v1", folder: "", dated: false, createNote: true };
    expect((await port.publishProduct("ses-1", "prod-a", destination)).vaultName).toBe("Capture Vault");
    await expect(port.exportSubtitles("ses-1", "vtt")).resolves.toBeNull();
    await expect(port.listVaults()).resolves.toEqual([{ id: "v1", name: "Notes" }]);
    await port.openScreenCapture("v1", "C:\\N\\a.md");
    expect(calls).toEqual([
      { cmd: "editor_publish_product", args: { sessionId: "ses-1", productId: "prod-a", destination } },
      { cmd: "editor_export_subtitles", args: { sessionId: "ses-1", format: "vtt" } },
      { cmd: "list_vaults", args: {} },
      { cmd: "open_screen_capture", args: { id: "v1", path: "C:\\N\\a.md" } },
    ]);
  });

  // The Rust literal `publish_tests.rs` pins: notePath/warning are
  // present-even-when-null, and a missing key is a protocol error.
  it("decodes the PublishReceipt literal and refuses a missing key", () => {
    const literal = {
      videoPath: "C:\\v\\a.mp4",
      notePath: null,
      vaultId: "v",
      vaultName: "Notes",
      warning: null,
    };
    expect(decodePublishReceipt(literal)).toEqual(literal);
    const { notePath: _dropped, ...missing } = literal;
    expect(() => decodePublishReceipt(missing)).toThrow(/notePath/);
    expect(() => decodePublishReceipt({ ...literal, videoPath: "" })).toThrow(/videoPath/);
    expect(decodeNullableFileName(null)).toBeNull();
    expect(() => decodeNullableFileName("")).toThrow();
  });
});
