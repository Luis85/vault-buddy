import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import PanelRoot from "../src/roots/PanelRoot.vue";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

const focusHandlers: Array<(e: { payload: boolean }) => void> = [];
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onFocusChanged: (cb: (e: { payload: boolean }) => void) => {
      focusHandlers.push(cb);
      return Promise.resolve(() => {});
    },
  }),
}));

const calls: string[] = [];

describe("PanelRoot", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    calls.length = 0;
    focusHandlers.length = 0;
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_vaults") return [];
    });
  });
  afterEach(() => clearMocks());

  it("refreshes vaults on mount", async () => {
    mount(PanelRoot);
    await Promise.resolve();
    expect(calls).toContain("list_vaults");
  });

  it("closes the panel on Escape", async () => {
    mount(PanelRoot);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await Promise.resolve();
    expect(calls).toContain("close_panel");
  });

  it("re-runs discovery each time the panel window regains focus", async () => {
    mount(PanelRoot);
    await flushPromises();
    calls.length = 0; // drop the mount refresh
    focusHandlers.forEach((cb) => cb({ payload: true }));
    await flushPromises();
    expect(calls).toContain("list_vaults");
  });

  it("does not refresh when the panel window loses focus", async () => {
    mount(PanelRoot);
    await flushPromises();
    calls.length = 0;
    focusHandlers.forEach((cb) => cb({ payload: false }));
    await flushPromises();
    expect(calls).not.toContain("list_vaults");
  });
});
