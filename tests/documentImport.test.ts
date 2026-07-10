import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

// The suite has no Tauri runtime, so the component's `@tauri-apps/api/core`
// invoke, `@tauri-apps/plugin-dialog` open, and logging are mocked at the
// module boundary (the vi.hoisted + vi.mock pattern updates-store.test.ts
// uses) — this lets us assert the exact set_pandoc_path args and the
// logWarning fallback the mockIPC harness can't observe.
const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import DocumentImportSettings from "../src/components/DocumentImportSettings.vue";
import RecordMode from "../src/components/RecordMode.vue";
import { logWarning } from "../src/logging";
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

// Route invoke by command name. `detect` is a queue of successive
// detect_pandoc responses (defaulting to the last once drained); `setPath`
// is the set_pandoc_path handler.
function routeInvoke(opts: {
  detect: Array<unknown>;
  setPath?: (args: unknown) => unknown;
}) {
  let i = 0;
  mocks.invoke.mockImplementation((cmd: string, args: unknown) => {
    if (cmd === "detect_pandoc") {
      const idx = Math.min(i, opts.detect.length - 1);
      i += 1;
      return Promise.resolve(opts.detect[idx]);
    }
    if (cmd === "set_pandoc_path") {
      return Promise.resolve(opts.setPath ? opts.setPath(args) : undefined);
    }
    return Promise.resolve(undefined);
  });
}

describe("DocumentImportSettings", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.open.mockReset();
    vi.mocked(logWarning).mockClear();
  });

  it("shows Not Installed and re-detects on Recheck", async () => {
    routeInvoke({ detect: [NOT_INSTALLED, installed()] });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    expect(wrapper.text()).toContain("Not installed");
    await wrapper.get('[data-testid="pandoc-recheck"]').trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("3.1.9");
  });

  it("renders the install link and Browse button when not installed", async () => {
    routeInvoke({ detect: [NOT_INSTALLED] });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    const link = wrapper.get('[data-testid="pandoc-install-link"]');
    expect(link.attributes("href")).toBe("https://pandoc.org/installing.html");
    expect(wrapper.find('[data-testid="pandoc-browse"]').exists()).toBe(true);
  });

  it("opens the install link through Rust, not raw webview navigation", async () => {
    // A raw target="_blank" in a Tauri v2 webview no-ops or replaces the app;
    // the click must route through the logged open_external_url command.
    routeInvoke({ detect: [NOT_INSTALLED] });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    mocks.invoke.mockClear();
    await wrapper.get('[data-testid="pandoc-install-link"]').trigger("click");
    await flushPromises();
    expect(mocks.invoke).toHaveBeenCalledWith("open_external_url", {
      url: "https://pandoc.org/installing.html",
    });
  });

  it("keeps the error, Recheck and path override reachable when detect_pandoc fails", async () => {
    // A failed probe must not hide the whole card — those controls are the
    // recovery path for a broken Pandoc setup.
    mocks.invoke.mockImplementation((cmd: string) => {
      if (cmd === "detect_pandoc") return Promise.reject(new Error("io error"));
      return Promise.resolve(undefined);
    });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    expect(wrapper.find('[data-testid="pandoc-error"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="pandoc-recheck"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="pandoc-path-input"]').exists()).toBe(true);
    expect(wrapper.get('[data-testid="pandoc-status"]').text()).toContain(
      "Couldn't detect Pandoc",
    );
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("detect_pandoc failed"),
    );
  });

  it("shows the too-old warning when sandbox is unsupported", async () => {
    routeInvoke({
      detect: [installed({ version: "pandoc 2.14", sandboxSupported: false })],
    });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    expect(wrapper.get('[data-testid="pandoc-status"]').text()).toContain(
      "too old for safe import (need 2.15+)",
    );
    expect(wrapper.text()).toContain("2.14");
  });

  it("seeds the override field from the configured path", async () => {
    routeInvoke({ detect: [installed({ configuredPath: "/opt/pandoc/pandoc" })] });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    const input = wrapper.get('[data-testid="pandoc-path-input"]')
      .element as HTMLInputElement;
    expect(input.value).toBe("/opt/pandoc/pandoc");
  });

  it("savePath sends the trimmed path and re-detects after", async () => {
    const setArgs: unknown[] = [];
    routeInvoke({
      detect: [installed(), installed({ configuredPath: "/usr/bin/pandoc" })],
      setPath: (args) => {
        setArgs.push(args);
        return undefined;
      },
    });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    const input = wrapper.get('[data-testid="pandoc-path-input"]');
    await input.setValue("  /usr/bin/pandoc  ");
    await input.trigger("change");
    await flushPromises();
    expect(setArgs).toEqual([{ pandocPath: "/usr/bin/pandoc" }]);
    // A second detect_pandoc runs after the save (mount + post-save = 2).
    const detectCalls = mocks.invoke.mock.calls.filter(
      (c) => c[0] === "detect_pandoc",
    );
    expect(detectCalls.length).toBe(2);
  });

  it("savePath sends null when the field is cleared", async () => {
    const setArgs: unknown[] = [];
    routeInvoke({
      detect: [installed({ configuredPath: "/old/pandoc" })],
      setPath: (args) => {
        setArgs.push(args);
        return undefined;
      },
    });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    const input = wrapper.get('[data-testid="pandoc-path-input"]');
    await input.setValue("   ");
    await input.trigger("change");
    await flushPromises();
    expect(setArgs).toEqual([{ pandocPath: null }]);
  });

  it("surfaces and logs a failed savePath", async () => {
    mocks.invoke.mockImplementation((cmd: string) => {
      if (cmd === "detect_pandoc") return Promise.resolve(installed());
      if (cmd === "set_pandoc_path")
        return Promise.reject(new Error("permission denied"));
      return Promise.resolve(undefined);
    });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    const input = wrapper.get('[data-testid="pandoc-path-input"]');
    await input.setValue("/bad/pandoc");
    await input.trigger("change");
    await flushPromises();
    expect(wrapper.get('[data-testid="pandoc-error"]').text()).toContain(
      "permission denied",
    );
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("set_pandoc_path failed"),
    );
  });

  it("browse populates the field and saves the picked path", async () => {
    const setArgs: unknown[] = [];
    routeInvoke({
      detect: [installed(), installed({ configuredPath: "/picked/pandoc" })],
      setPath: (args) => {
        setArgs.push(args);
        return undefined;
      },
    });
    mocks.open.mockResolvedValue("/picked/pandoc");
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    await wrapper.get('[data-testid="pandoc-browse"]').trigger("click");
    await flushPromises();
    const input = wrapper.get('[data-testid="pandoc-path-input"]')
      .element as HTMLInputElement;
    expect(input.value).toBe("/picked/pandoc");
    expect(setArgs).toEqual([{ pandocPath: "/picked/pandoc" }]);
  });

  it("browse cancelled saves nothing and shows no error", async () => {
    routeInvoke({ detect: [installed()] });
    mocks.open.mockResolvedValue(null);
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    await wrapper.get('[data-testid="pandoc-browse"]').trigger("click");
    await flushPromises();
    expect(
      mocks.invoke.mock.calls.some((c) => c[0] === "set_pandoc_path"),
    ).toBe(false);
    expect(wrapper.find('[data-testid="pandoc-error"]').exists()).toBe(false);
  });

  it("browse surfaces and logs a picker failure", async () => {
    routeInvoke({ detect: [installed()] });
    mocks.open.mockRejectedValue(new Error("no dialog"));
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    await wrapper.get('[data-testid="pandoc-browse"]').trigger("click");
    await flushPromises();
    expect(wrapper.get('[data-testid="pandoc-error"]').text()).toContain(
      "no dialog",
    );
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("browse failed"),
    );
  });

});

describe("RecordMode — Import Document", () => {
  // Routes the three commands RecordMode itself issues on mount
  // (get_capture_config / list_recordings, unrelated to import) plus
  // detect_pandoc / convert_document for the action under test — same
  // vi.hoisted mocks.invoke/mocks.open as the DocumentImportSettings suite
  // above (this file mocks @tauri-apps/api/core and @tauri-apps/plugin-dialog
  // once at module scope).
  const routeRecordMode = (opts: {
    pandoc?: unknown;
    convert?: (args: unknown) => unknown;
  }) => {
    mocks.invoke.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === "detect_pandoc") return Promise.resolve(opts.pandoc ?? installed());
      if (cmd === "get_capture_config") return Promise.resolve({});
      if (cmd === "list_recordings") return Promise.resolve([]);
      if (cmd === "convert_document") {
        return Promise.resolve(
          opts.convert ? opts.convert(args) : "Documents/2026/07/2026-07-10 Report.md",
        );
      }
      return Promise.resolve(undefined);
    });
  };

  beforeEach(() => {
    setActivePinia(createPinia());
    mocks.invoke.mockReset();
    mocks.open.mockReset();
    vi.mocked(logWarning).mockClear();
  });

  it("imports a picked document and toasts success", async () => {
    const convertArgs: unknown[] = [];
    routeRecordMode({
      convert: (args) => {
        convertArgs.push(args);
        return "Documents/2026/07/2026-07-10 Report.md";
      },
    });
    mocks.open.mockResolvedValue("/home/user/Report.docx");
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    const button = wrapper.get('[data-testid="import-document"]');
    expect((button.element as HTMLButtonElement).disabled).toBe(false);
    await button.trigger("click");
    await flushPromises();

    expect(convertArgs).toEqual([{ id: "v1", sourcePath: "/home/user/Report.docx" }]);
    const notes = useNotificationsStore();
    expect(
      notes.items.some((i) => i.kind === "success" && i.message.includes("Imported")),
    ).toBe(true);
  });

  it("keeps the button enabled and warns when detect_pandoc fails on mount", async () => {
    // A failed probe blocks import, but the button stays clickable so it can
    // route to Settings (the recovery surface) rather than dead-ending.
    mocks.invoke.mockImplementation((cmd: string) => {
      if (cmd === "detect_pandoc") return Promise.reject(new Error("io error"));
      if (cmd === "get_capture_config") return Promise.resolve({});
      if (cmd === "list_recordings") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    const button = wrapper.get('[data-testid="import-document"]');
    expect((button.element as HTMLButtonElement).disabled).toBe(false);
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("detect_pandoc failed"),
    );
  });

  it("routes a blocked click to Settings (not installed) instead of dead-ending", async () => {
    routeRecordMode({ pandoc: NOT_INSTALLED });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    const button = wrapper.get('[data-testid="import-document"]');
    expect((button.element as HTMLButtonElement).disabled).toBe(false);
    expect(wrapper.text()).toContain("Install Pandoc in Settings to import documents");
    await button.trigger("click");
    await flushPromises();
    // Jumped to Settings; never opened the file picker or convert.
    expect(useVaultsStore().view).toBe("settings");
    expect(mocks.open).not.toHaveBeenCalled();
    expect(mocks.invoke.mock.calls.some((c) => c[0] === "convert_document")).toBe(false);
  });

  it("routes a blocked click to Settings when Pandoc is too old for the sandbox", async () => {
    routeRecordMode({
      pandoc: installed({ version: "pandoc 2.14", sandboxSupported: false }),
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    const button = wrapper.get('[data-testid="import-document"]');
    expect((button.element as HTMLButtonElement).disabled).toBe(false);
    expect(wrapper.text()).toContain("Update Pandoc (2.15+ needed)");
    await button.trigger("click");
    await flushPromises();
    expect(useVaultsStore().view).toBe("settings");
  });

  it("disables Import while detection is in flight and never jumps to Settings early", async () => {
    // Hold detect_pandoc unresolved to simulate the pre-probe window: before a
    // result exists, a blocked click must NOT route to Settings (it would with
    // a valid Pandoc that just hadn't been detected yet).
    let resolveDetect: (v: unknown) => void = () => {};
    mocks.invoke.mockImplementation((cmd: string) => {
      if (cmd === "detect_pandoc") {
        return new Promise((r) => {
          resolveDetect = r;
        });
      }
      if (cmd === "get_capture_config") return Promise.resolve({});
      if (cmd === "list_recordings") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises(); // config/recordings settle; detect stays pending

    const button = wrapper.get('[data-testid="import-document"]');
    expect((button.element as HTMLButtonElement).disabled).toBe(true);
    expect(wrapper.text()).toContain("Checking Pandoc…");
    await button.trigger("click");
    await flushPromises();
    expect(useVaultsStore().view).not.toBe("settings");

    // Once the probe resolves to a valid install, the button enables normally.
    resolveDetect(installed());
    await flushPromises();
    expect((button.element as HTMLButtonElement).disabled).toBe(false);
  });

  it("does nothing when the picker is cancelled", async () => {
    routeRecordMode({});
    mocks.open.mockResolvedValue(null);
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    await wrapper.get('[data-testid="import-document"]').trigger("click");
    await flushPromises();

    expect(mocks.invoke.mock.calls.some((c) => c[0] === "convert_document")).toBe(false);
  });

  it("logs and toasts an error when convert_document fails", async () => {
    routeRecordMode({ convert: () => Promise.reject(new Error("pandoc crashed")) });
    mocks.open.mockResolvedValue("/home/user/Report.docx");
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();

    await wrapper.get('[data-testid="import-document"]').trigger("click");
    await flushPromises();

    const notes = useNotificationsStore();
    expect(
      notes.items.some((i) => i.kind === "error" && i.message.includes("pandoc crashed")),
    ).toBe(true);
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("convert_document failed"),
    );
  });
});
