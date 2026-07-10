import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent } from "vue";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  announce: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("../src/logging", () => ({ logWarning: vi.fn() }));
vi.mock("../src/announce", () => ({ announce: mocks.announce }));

import {
  STARTUP_CHECK_DELAY_MS,
  useStartupUpdateCheck,
} from "../src/composables/useStartupUpdateCheck";
import { useVaultsStore } from "../src/stores/vaults";

const Host = defineComponent({
  setup: () => (useStartupUpdateCheck(), {}),
  render: () => null,
});

describe("useStartupUpdateCheck", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    vi.useFakeTimers();
    mocks.check.mockReset();
    mocks.announce.mockReset();
  });
  afterEach(() => vi.useRealTimers());

  it("asks via bubble + next-open settings when an update is found", async () => {
    mocks.check.mockResolvedValue({ version: "0.9.0" });
    mount(Host);
    expect(mocks.check).not.toHaveBeenCalled(); // waits out the settle delay
    await vi.advanceTimersByTimeAsync(STARTUP_CHECK_DELAY_MS);
    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(mocks.announce).toHaveBeenCalledWith(
      expect.stringContaining("0.9.0"),
    );
    // the ask lands on the settings view at the NEXT panel open
    expect(useVaultsStore().pendingView).toBe("settings");
  });

  it("stays silent when the app is current", async () => {
    mocks.check.mockResolvedValue(null);
    mount(Host);
    await vi.advanceTimersByTimeAsync(STARTUP_CHECK_DELAY_MS);
    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(mocks.announce).not.toHaveBeenCalled();
    expect(useVaultsStore().pendingView).toBeNull();
  });

  it("never checks when the setting is off", async () => {
    localStorage.setItem("vault-buddy.checkUpdatesOnStart", "off");
    mount(Host);
    await vi.advanceTimersByTimeAsync(STARTUP_CHECK_DELAY_MS * 2);
    expect(mocks.check).not.toHaveBeenCalled();
  });

  it("a pre-delay unmount cancels the check", async () => {
    mocks.check.mockResolvedValue({ version: "0.9.0" });
    const wrapper = mount(Host);
    wrapper.unmount();
    await vi.advanceTimersByTimeAsync(STARTUP_CHECK_DELAY_MS * 2);
    expect(mocks.check).not.toHaveBeenCalled();
  });
});
