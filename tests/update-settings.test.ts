import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("0.1.0"),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn() }));

import UpdateSettings from "../src/components/UpdateSettings.vue";
import { useUpdatesStore } from "../src/stores/updates";
import { useVaultsStore } from "../src/stores/vaults";

describe("UpdateSettings", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => vi.clearAllMocks());

  it("keeps the manual check-for-updates control", () => {
    const wrapper = mount(UpdateSettings);
    expect(wrapper.find('[data-testid="check-updates"]').exists()).toBe(true);
  });

  it("links an available update to the dedicated view instead of installing inline", async () => {
    const updates = useUpdatesStore();
    updates.phase = "available";
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    updates.available = { version: "0.2.0" } as any;
    const vaults = useVaultsStore();
    const wrapper = mount(UpdateSettings);
    // the inline install button moved to UpdateView — settings only links now
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(false);
    await wrapper.get('[data-testid="view-update"]').trigger("click");
    expect(vaults.view).toBe("update");
  });
});
