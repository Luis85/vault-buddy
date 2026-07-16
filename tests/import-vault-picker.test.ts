import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ImportVaultPicker from "../src/components/ImportVaultPicker.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import { useVaultsStore } from "../src/stores/vaults";

const NOT_INSTALLED = {
  installed: false,
  version: null,
  path: null,
  sandboxSupported: false,
  configuredPath: null,
};
const installed = (over: Record<string, unknown> = {}) => ({
  installed: true,
  version: "pandoc 3.1.9",
  path: "pandoc",
  sandboxSupported: true,
  configuredPath: null,
  ...over,
});

const sampleVaults = [
  { id: "d4e5f6", name: "Personal", path: "C:\\vaults\\Personal", open: false },
  { id: "a1b2c3", name: "Work", path: "C:\\vaults\\Work", open: false },
];

describe("ImportVaultPicker", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("shows the source filename and a row per vault when Pandoc is ready", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") return installed();
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.text()).toContain("Report.docx");
    const rows = wrapper.findAll('[data-testid="import-picker-vault"]');
    expect(rows).toHaveLength(2);
    expect(wrapper.text()).toContain("Personal");
    expect(wrapper.text()).toContain("Work");
  });

  it("picking a vault converts the document, offers to open it, and returns to the list", async () => {
    const convertArgs: unknown[] = [];
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "detect_pandoc") return installed();
      if (cmd === "convert_document") {
        convertArgs.push(args);
        return "Documents/2026/07/2026-07-10 Report.md";
      }
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();

    expect(convertArgs).toEqual([
      { id: "d4e5f6", sourcePath: "C:/x/Report.docx" },
    ]);
    const notes = useNotificationsStore();
    const toast = notes.items.find(
      (i) => i.kind === "success" && i.message.includes("Imported"),
    );
    expect(toast?.action?.label).toBe("Open in Obsidian");
    // Clicking Open launches the imported note in the vault it was imported to.
    await toast!.action!.run();
    const openCall = calls.find((c) => c.cmd === "open_imported_document");
    expect(openCall?.args).toEqual({
      id: "d4e5f6",
      path: "Documents/2026/07/2026-07-10 Report.md",
    });
    expect(store.view).toBe("list");
    expect(store.pendingImports).toEqual([]);
  });

  it("shows an install hint and no vault list when Pandoc is not installed", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") return NOT_INSTALLED;
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.find('[data-testid="import-picker-gate-hint"]').text()).toContain(
      "Pandoc isn't installed",
    );
    expect(wrapper.find('[data-testid="import-picker-vault"]').exists()).toBe(false);
  });

  it("shows a checking state before detect_pandoc resolves, not the install gate", async () => {
    // Hold the probe pending: the picker must show "Checking Pandoc…" rather
    // than the install gate, so a valid Pandoc install isn't flashed as missing
    // (and a quick click can't land on Settings) during the pre-probe window.
    let resolveDetect: (v: unknown) => void = () => {};
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        return new Promise((r) => {
          resolveDetect = r;
        });
      }
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.find('[data-testid="import-picker-checking"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="import-picker-gate-hint"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="import-picker-vault"]').exists()).toBe(false);

    resolveDetect(installed());
    await flushPromises();
    expect(wrapper.find('[data-testid="import-picker-checking"]').exists()).toBe(false);
    expect(wrapper.findAll('[data-testid="import-picker-vault"]')).toHaveLength(2);
  });

  it("routes to the document-import setup view from the install hint", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") return NOT_INSTALLED;
    });
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    await wrapper.get('[data-testid="import-picker-settings"]').trigger("click");
    expect(store.view).toBe("documentImport");
  });

  it("shows an update hint when Pandoc is too old for the sandbox", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc")
        return installed({ version: "pandoc 2.14", sandboxSupported: false });
    });
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.find('[data-testid="import-picker-gate-hint"]').text()).toContain(
      "too old for safe import",
    );
  });

  it("shows a no-vaults message when Pandoc is ready but nothing was discovered", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") return installed();
    });
    const store = useVaultsStore();
    store.vaults = [];
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.text()).toContain("No vaults found.");
  });

  it("toasts and logs an error when convert_document fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") return installed();
      if (cmd === "convert_document") throw "pandoc crashed";
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();

    const notes = useNotificationsStore();
    expect(
      notes.items.some((i) => i.kind === "error" && i.message.includes("pandoc crashed")),
    ).toBe(true);
    // stays on the picker so the user can retry a different vault
    expect(store.view).toBe("importPicker");
  });

  it("degrades to the not-installed gate when detect_pandoc fails on mount", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") throw "io error";
    });
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.find('[data-testid="import-picker-gate-hint"]').exists()).toBe(true);
  });

  it("processes a queue of dropped documents one at a time (GAP-55)", async () => {
    const convertSources: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "detect_pandoc") return installed();
      if (cmd === "convert_document") {
        convertSources.push((args as { sourcePath: string }).sourcePath);
        return "Documents/2026/07/note.md";
      }
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.enqueueImports(["/a.docx", "/b.docx", "/c.docx"]);
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    // Head shown with a "+2 more queued" indicator.
    expect(wrapper.text()).toContain("a.docx");
    expect(wrapper.get('[data-testid="import-picker-queued"]').text()).toContain("2");
    // Pick a vault for each queued doc in turn.
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("b.docx");
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("c.docx");
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();
    // All three converted, in order; queue drained → back to the list.
    expect(convertSources).toEqual(["/a.docx", "/b.docx", "/c.docx"]);
    expect(store.view).toBe("list");
    expect(store.pendingImports).toEqual([]);
  });
});
