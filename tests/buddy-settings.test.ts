import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

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
vi.mock("../src/logging", () => ({
  logWarning: vi.fn(),
  logBreadcrumb: vi.fn(),
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

  it("previews a character's motion on hover and stops on leave", async () => {
    const wrapper = mount(BuddySettings);
    const knight = wrapper.get('[aria-label="Choose Knight"]');
    await knight.trigger("pointerenter");
    // BuddyAvatar renders the run loop via the .running class on its sheet
    expect(knight.find(".sheet").classes()).toContain("running");
    await knight.trigger("pointerleave");
    expect(knight.find(".sheet").classes()).not.toContain("running");
  });

  it("does not preview while animations are off", async () => {
    useSettingsStore().toggleAnimations(); // off
    const wrapper = mount(BuddySettings);
    const knight = wrapper.get('[aria-label="Choose Knight"]');
    await knight.trigger("pointerenter");
    expect(knight.find(".sheet").classes()).not.toContain("running");
  });

  it("marks the selected character with a badge", async () => {
    const wrapper = mount(BuddySettings);
    expect(
      wrapper
        .get('[aria-label="Choose Classic"]')
        .find('[data-testid="selected-badge"]')
        .exists(),
    ).toBe(true);
    expect(
      wrapper
        .get('[aria-label="Choose Knight"]')
        .find('[data-testid="selected-badge"]')
        .exists(),
    ).toBe(false);
    await wrapper.get('[aria-label="Choose Knight"]').trigger("click");
    expect(
      wrapper
        .get('[aria-label="Choose Knight"]')
        .find('[data-testid="selected-badge"]')
        .exists(),
    ).toBe(true);
  });

  it("no longer shows a manual view-direction control (facing is derived from position)", () => {
    const wrapper = mount(BuddySettings);
    expect(wrapper.findAll(".facing-option")).toHaveLength(0);
    expect(wrapper.text()).not.toContain("View direction");
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

  it("mirrors the buddy-messages toggle", async () => {
    const wrapper = mount(BuddySettings);
    const toggle = wrapper.find("#messages-toggle");
    expect((toggle.element as HTMLInputElement).checked).toBe(true);
    await toggle.setValue(false);
    expect(useSettingsStore().buddyMessagesEnabled).toBe(false);
  });

  it("loads the OS autostart state into the System card", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_autostart") return true;
    });
    const wrapper = mount(BuddySettings);
    await flush();
    expect(wrapper.text()).toContain("System");
    const toggle = wrapper.get<HTMLInputElement>('[data-testid="autostart-toggle"]');
    expect(toggle.element.checked).toBe(true);
    expect(toggle.element.disabled).toBe(false);
    clearMocks();
  });

  it("disables the autostart toggle and shows the error when the read fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_autostart") throw new Error("registry unavailable");
    });
    const wrapper = mount(BuddySettings);
    await flush();
    const toggle = wrapper.get<HTMLInputElement>('[data-testid="autostart-toggle"]');
    expect(toggle.element.disabled).toBe(true);
    expect(wrapper.get('[data-testid="autostart-error"]').text()).toContain(
      "registry unavailable",
    );
    clearMocks();
  });

  it("toggling autostart invokes set_autostart", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_autostart") return false;
    });
    const wrapper = mount(BuddySettings);
    await flush();
    await wrapper.get('[data-testid="autostart-toggle"]').setValue(true);
    await flush();
    expect(calls.find((c) => c.cmd === "set_autostart")?.args).toEqual({
      enabled: true,
    });
    clearMocks();
  });

  it("reverts the autostart toggle and shows the error when the write fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_autostart") return false;
      if (cmd === "set_autostart") throw new Error("access denied");
    });
    const wrapper = mount(BuddySettings);
    await flush();
    const toggle = wrapper.get<HTMLInputElement>('[data-testid="autostart-toggle"]');
    await wrapper.get('[data-testid="autostart-toggle"]').setValue(true);
    await flush();
    expect(toggle.element.checked).toBe(false); // reverted
    expect(wrapper.get('[data-testid="autostart-error"]').text()).toContain(
      "access denied",
    );
    clearMocks();
  });

  it("mirrors and persists the check-on-startup toggle in the Updates card", async () => {
    const wrapper = mount(BuddySettings);
    await flush();
    const toggle = wrapper.get<HTMLInputElement>(
      '[data-testid="update-on-start-toggle"]',
    );
    expect(toggle.element.checked).toBe(true); // on by default
    await toggle.setValue(false);
    expect(useSettingsStore().checkUpdatesOnStart).toBe(false);
    expect(localStorage.getItem("vault-buddy.checkUpdatesOnStart")).toBe("off");
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

  it("groups the toggles under a Behavior card with a message-duration select", async () => {
    const wrapper = mount(BuddySettings, { attachTo: document.body });
    expect(wrapper.text()).toContain("Behavior");
    expect(wrapper.find('[data-testid="message-duration-select"]').exists()).toBe(true);
    // the three toggles keep their ids inside the card
    for (const id of ["#animations-toggle", "#dragging-toggle", "#messages-toggle"]) {
      expect(wrapper.find(id).exists()).toBe(true);
    }
    wrapper.unmount();
    document.body.innerHTML = "";
  });

  it("picking a message duration persists it to the store", async () => {
    const wrapper = mount(BuddySettings, { attachTo: document.body });
    await wrapper.get('[data-testid="message-duration-select"]').trigger("click");
    (document.body.querySelector(
      '[data-testid="message-duration-select-option-long"]',
    ) as HTMLElement).click();
    await flush();
    expect(useSettingsStore().messageDuration).toBe("long");
    expect(localStorage.getItem("vault-buddy.messageDuration")).toBe("long");
    wrapper.unmount();
    document.body.innerHTML = "";
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
