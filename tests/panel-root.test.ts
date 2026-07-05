import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import PanelRoot from "../src/roots/PanelRoot.vue";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

const calls: string[] = [];

describe("PanelRoot", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    calls.length = 0;
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
});
