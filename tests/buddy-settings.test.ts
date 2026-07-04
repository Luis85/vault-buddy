import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";

const updaterMocks = vi.hoisted(() => ({
  getVersion: vi.fn(),
  check: vi.fn(),
  relaunch: vi.fn(),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: updaterMocks.getVersion,
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: updaterMocks.check }));
vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: updaterMocks.relaunch,
}));

import BuddySettings from "../src/components/BuddySettings.vue";
import { CHARACTERS } from "../src/characters";
import { useSettingsStore } from "../src/stores/settings";

const flush = () => new Promise((r) => setTimeout(r));

describe("BuddySettings", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    updaterMocks.getVersion.mockReset().mockResolvedValue("0.1.0");
    updaterMocks.check.mockReset();
  });

  it("shows every character with the current one selected", () => {
    const wrapper = mount(BuddySettings);
    const options = wrapper.findAll(".character-option");
    expect(options).toHaveLength(CHARACTERS.length);
    for (const c of CHARACTERS) expect(wrapper.text()).toContain(c.name);
    // classic is the persisted default
    expect(
      wrapper.find('[aria-label="Choose Classic"]').attributes("aria-checked"),
    ).toBe("true");
  });

  it("selecting a character updates and persists the store", async () => {
    const wrapper = mount(BuddySettings);
    await wrapper.find('[aria-label="Choose Knight"]').trigger("click");
    expect(useSettingsStore().character).toBe("knight");
    expect(localStorage.getItem("vault-buddy.character")).toBe("knight");
    expect(
      wrapper.find('[aria-label="Choose Knight"]').attributes("aria-checked"),
    ).toBe("true");
  });

  it("selects the buddy's home view direction", async () => {
    const wrapper = mount(BuddySettings);
    const options = wrapper.findAll(".facing-option");
    expect(options).toHaveLength(2);
    // right is the default
    expect(options[1].attributes("aria-checked")).toBe("true");

    await options[0].trigger("click");
    expect(useSettingsStore().facing).toBe("left");
    expect(localStorage.getItem("vault-buddy.facing")).toBe("left");
    expect(options[0].attributes("aria-checked")).toBe("true");
  });

  it("mirrors the dragging toggle", async () => {
    const wrapper = mount(BuddySettings);
    const toggle = wrapper.find("#dragging-toggle");
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    await toggle.setValue(false);
    expect(useSettingsStore().draggingEnabled).toBe(false);
  });

  it("mirrors the animations toggle", async () => {
    const wrapper = mount(BuddySettings);
    const toggle = wrapper.find("#animations-toggle");
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    await toggle.setValue(false);
    expect(useSettingsStore().animationsEnabled).toBe(false);
  });

  it("shows the Updates section with the current version", async () => {
    const wrapper = mount(BuddySettings);
    await flush();
    expect(wrapper.text()).toContain("Updates");
    expect(wrapper.text()).toContain("Version 0.1.0");
    expect(wrapper.find('[data-testid="check-updates"]').exists()).toBe(true);
  });

  it("checks for updates and reports up to date", async () => {
    updaterMocks.check.mockResolvedValue(null);
    const wrapper = mount(BuddySettings);
    await wrapper.find('[data-testid="check-updates"]').trigger("click");
    await flush();
    expect(wrapper.text()).toContain("You're up to date.");
  });

  it("offers install & restart when an update is available", async () => {
    updaterMocks.check.mockResolvedValue({
      version: "0.2.0",
      downloadAndInstall: vi.fn(),
    });
    const wrapper = mount(BuddySettings);
    await wrapper.find('[data-testid="check-updates"]').trigger("click");
    await flush();
    expect(wrapper.text()).toContain("Version 0.2.0 is available");
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(true);
  });

  it("keeps the install button visible for retry after a failure", async () => {
    // the store keeps `available` after a failed download/install exactly
    // so the user can retry — the button must not vanish behind the error
    updaterMocks.check.mockResolvedValue({
      version: "0.2.0",
      download: vi.fn().mockRejectedValue("download broke"),
      install: vi.fn(),
    });
    const wrapper = mount(BuddySettings);
    await wrapper.find('[data-testid="check-updates"]').trigger("click");
    await flush();
    await wrapper.find('[data-testid="install-update"]').trigger("click");
    await flush();
    expect(wrapper.text()).toContain("download broke");
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(true);
  });
});
