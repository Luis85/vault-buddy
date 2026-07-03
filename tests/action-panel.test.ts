import { beforeEach, afterEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import ActionPanel from "../src/components/ActionPanel.vue";
import { useVaultsStore } from "../src/stores/vaults";

const sampleVaults = [
  { id: "d4e5f6", name: "Personal", path: "C:\\vaults\\Personal" },
  { id: "a1b2c3", name: "Work", path: "C:\\vaults\\Work" },
];

describe("ActionPanel", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("lists each vault with both actions", () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("Personal");
    expect(wrapper.text()).toContain("Work");
    const buttons = wrapper.findAll("button");
    expect(buttons).toHaveLength(4); // 2 vaults × 2 actions
  });

  it("dispatches open_daily_note with the vault id", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    const dailyButtons = wrapper
      .findAll("button")
      .filter((b) => b.text().includes("daily note"));
    await dailyButtons[0].trigger("click");
    expect(calls).toEqual([{ cmd: "open_daily_note", args: { id: "d4e5f6" } }]);
  });

  it("shows the friendly empty state when no vaults were found", () => {
    const store = useVaultsStore();
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("Obsidian not found");
  });

  it("shows the error banner when an action failed", () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.error = "failed to launch obsidian://open";
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("failed to launch");
  });
});
