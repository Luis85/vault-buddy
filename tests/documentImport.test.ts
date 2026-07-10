import { flushPromises, mount } from "@vue/test-utils";
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
import { logWarning } from "../src/logging";

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
    expect(link.attributes("target")).toBe("_blank");
    expect(wrapper.find('[data-testid="pandoc-browse"]').exists()).toBe(true);
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

  it("logs and stays empty when detect_pandoc fails on mount", async () => {
    mocks.invoke.mockImplementation((cmd: string) => {
      if (cmd === "detect_pandoc") return Promise.reject(new Error("io error"));
      return Promise.resolve(undefined);
    });
    const wrapper = mount(DocumentImportSettings);
    await flushPromises();
    // status stayed null → the whole card (v-if="status") never renders.
    expect(wrapper.find('[data-testid="pandoc-status"]').exists()).toBe(false);
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("detect_pandoc failed"),
    );
  });
});
