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
    store.openImportPicker("C:/x/Report.docx");
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.text()).toContain("Report.docx");
    const rows = wrapper.findAll('[data-testid="import-picker-vault"]');
    expect(rows).toHaveLength(2);
    expect(wrapper.text()).toContain("Personal");
    expect(wrapper.text()).toContain("Work");
  });

  it("picking a vault converts the document, toasts success, and returns to the list", async () => {
    const convertArgs: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "detect_pandoc") return installed();
      if (cmd === "convert_document") {
        convertArgs.push(args);
        return "Documents/2026/07/2026-07-10 Report.md";
      }
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.openImportPicker("C:/x/Report.docx");
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    await wrapper.findAll('[data-testid="import-picker-vault"]')[0].trigger("click");
    await flushPromises();

    expect(convertArgs).toEqual([
      { id: "d4e5f6", sourcePath: "C:/x/Report.docx" },
    ]);
    const notes = useNotificationsStore();
    expect(
      notes.items.some((i) => i.kind === "success" && i.message.includes("Imported")),
    ).toBe(true);
    expect(store.view).toBe("list");
    expect(store.pendingImportPath).toBeNull();
  });

  it("shows an install hint and no vault list when Pandoc is not installed", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") return NOT_INSTALLED;
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.openImportPicker("C:/x/Report.docx");
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.find('[data-testid="import-picker-gate-hint"]').text()).toContain(
      "Pandoc isn't installed",
    );
    expect(wrapper.find('[data-testid="import-picker-vault"]').exists()).toBe(false);
  });

  it("routes to settings from the install hint", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") return NOT_INSTALLED;
    });
    const store = useVaultsStore();
    store.openImportPicker("C:/x/Report.docx");
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    await wrapper.get('[data-testid="import-picker-settings"]').trigger("click");
    expect(store.view).toBe("settings");
  });

  it("shows an update hint when Pandoc is too old for the sandbox", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc")
        return installed({ version: "pandoc 2.14", sandboxSupported: false });
    });
    const store = useVaultsStore();
    store.openImportPicker("C:/x/Report.docx");
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
    store.openImportPicker("C:/x/Report.docx");
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
    store.openImportPicker("C:/x/Report.docx");
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
    store.openImportPicker("C:/x/Report.docx");
    const wrapper = mount(ImportVaultPicker);
    await flushPromises();
    expect(wrapper.find('[data-testid="import-picker-gate-hint"]').exists()).toBe(true);
  });
});
