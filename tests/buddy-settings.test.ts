import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

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

import { CHARACTERS } from "../src/characters";
import BuddySettings from "../src/components/BuddySettings.vue";
import { useSettingsStore } from "../src/stores/settings";
import { useVaultsStore } from "../src/stores/vaults";

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

  it("surfaces an available update with a link to the dedicated update view", async () => {
    updaterMocks.check.mockResolvedValue({
      version: "0.2.0",
      downloadAndInstall: vi.fn(),
    });
    const wrapper = mount(BuddySettings);
    await wrapper.find('[data-testid="check-updates"]').trigger("click");
    await flush();
    expect(wrapper.text()).toContain("Version 0.2.0 is available");
    // install moved to the dedicated UpdateView; settings only links now
    expect(wrapper.find('[data-testid="view-update"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(false);
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

  it("the View update link opens the dedicated update view", async () => {
    // install + retry live in UpdateView now (see update-view.test.ts); the
    // settings card only routes there.
    updaterMocks.check.mockResolvedValue({
      version: "0.2.0",
      downloadAndInstall: vi.fn(),
    });
    const wrapper = mount(BuddySettings);
    await wrapper.find('[data-testid="check-updates"]').trigger("click");
    await flush();
    await wrapper.find('[data-testid="view-update"]').trigger("click");
    expect(useVaultsStore().view).toBe("update");
  });
});

describe("BuddySettings tabs", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
  });

  it("groups settings into Buddy / System / Integrations tabs", () => {
    const wrapper = mount(BuddySettings);
    for (const id of ["buddy", "system", "integrations"]) {
      expect(wrapper.find(`[data-testid="tab-${id}"]`).exists()).toBe(true);
    }
    // Character grid lives under the (default) Buddy tab, visible on mount.
    expect(wrapper.get('[data-testid="panel-buddy"]').isVisible()).toBe(true);
  });

  it("puts the autostart control under the System tab and MCP under Integrations", () => {
    const wrapper = mount(BuddySettings);
    // Panels are eager-mounted (v-show); assert the autostart toggle's markup
    // lives inside the System panel, not just anywhere.
    expect(wrapper.find('[data-testid="autostart-toggle"]').exists()).toBe(true);
    expect(wrapper.get('[data-testid="panel-system"]').html()).toContain("autostart-toggle");
    // McpSettings + DocumentImportSettings render inside the Integrations panel.
    expect(wrapper.find('[data-testid="panel-integrations"]').exists()).toBe(true);
  });
});

describe("BuddySettings panel size", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
  });

  it("loads the current panel size and marks it selected, inside the Buddy tab", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_panel_config") return { size: "large" };
    });
    const wrapper = mount(BuddySettings);
    await flush();
    expect(
      wrapper.get('[data-testid="panel-size-large"]').attributes("aria-checked"),
    ).toBe("true");
    // Lives in the Buddy tab (TabGroup's first/default tab) so it survives
    // the re-show below, which remounts the panel back to its first tab.
    expect(wrapper.get('[data-testid="panel-buddy"]').html()).toContain(
      "panel-size-large",
    );
    clearMocks();
  });

  it("defaults to comfortable and saves nothing when the read fails", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_panel_config") throw new Error("config unavailable");
    });
    const wrapper = mount(BuddySettings);
    await flush();
    expect(
      wrapper.get('[data-testid="panel-size-comfortable"]').attributes("aria-checked"),
    ).toBe("true");
    // The mount-time read failing must never itself trigger a save/re-show.
    expect(calls.some((c) => c.cmd === "set_panel_size")).toBe(false);
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(false);
    expect(calls.some((c) => c.cmd === "open_panel")).toBe(false);
    clearMocks();
  });

  it("does not save or re-show the panel from the initial programmatic load", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_panel_config") return { size: "large" };
    });
    mount(BuddySettings);
    await flush();
    // The full settings tree fires other cards' own onMounted reads
    // (mcp/pandoc/transcription/autostart) too; only assert on the
    // panel-size-relevant commands so this stays about the guard being
    // tested, not an inventory of every sibling card's IPC traffic.
    expect(calls.some((c) => c.cmd === "get_panel_config")).toBe(true);
    expect(calls.some((c) => c.cmd === "set_panel_size")).toBe(false);
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(false);
    expect(calls.some((c) => c.cmd === "open_panel")).toBe(false);
    clearMocks();
  });

  it("picking a size persists it via set_panel_size and re-shows the panel on Settings", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_panel_config") return { size: "comfortable" };
    });
    const wrapper = mount(BuddySettings);
    await flush();
    await wrapper.get('[data-testid="panel-size-large"]').trigger("click");
    await flush();
    expect(calls.find((c) => c.cmd === "set_panel_size")?.args).toEqual({
      size: "large",
    });
    // set_panel_size never touches a visible window; the re-show is what
    // makes the new preset take effect (position_panel's hidden-only guard).
    // The full settings tree interleaves other cards' own IPC traffic, so
    // assert relative order among the panel-size commands rather than an
    // exact/exclusive call list.
    const idxSet = calls.findIndex((c) => c.cmd === "set_panel_size");
    const idxClose = calls.findIndex((c) => c.cmd === "close_panel");
    const idxOpen = calls.findIndex((c) => c.cmd === "open_panel");
    expect(idxSet).toBeGreaterThanOrEqual(0);
    expect(idxClose).toBeGreaterThan(idxSet);
    expect(idxOpen).toBeGreaterThan(idxClose);
    expect(useVaultsStore().view).toBe("settings");
    expect(
      wrapper.get('[data-testid="panel-size-large"]').attributes("aria-checked"),
    ).toBe("true");
    clearMocks();
  });

  it("reverts the selection and shows an error when the save fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_panel_config") return { size: "comfortable" };
      if (cmd === "set_panel_size") throw new Error("disk full");
    });
    const wrapper = mount(BuddySettings);
    await flush();
    await wrapper.get('[data-testid="panel-size-large"]').trigger("click");
    await flush();
    expect(
      wrapper.get('[data-testid="panel-size-comfortable"]').attributes("aria-checked"),
    ).toBe("true");
    expect(
      wrapper.get('[data-testid="panel-size-large"]').attributes("aria-checked"),
    ).toBe("false");
    expect(wrapper.get('[data-testid="panel-size-error"]').text()).toContain(
      "disk full",
    );
    clearMocks();
  });

  it("does not re-show the panel when the save fails", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_panel_config") return { size: "comfortable" };
      if (cmd === "set_panel_size") throw new Error("disk full");
    });
    const wrapper = mount(BuddySettings);
    await flush();
    await wrapper.get('[data-testid="panel-size-large"]').trigger("click");
    await flush();
    expect(calls.some((c) => c.cmd === "close_panel")).toBe(false);
    expect(calls.some((c) => c.cmd === "open_panel")).toBe(false);
    clearMocks();
  });

  it("keeps the new size when the save succeeds but the re-show then fails", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_panel_config") return { size: "comfortable" };
      // The write lands; only the subsequent re-show faults.
      if (cmd === "close_panel") throw new Error("window gone");
    });
    const wrapper = mount(BuddySettings);
    await flush();
    await wrapper.get('[data-testid="panel-size-large"]').trigger("click");
    await flush();
    // A re-show fault must NOT revert a size that is already persisted, nor
    // surface an error — the preset applies on the next open regardless.
    expect(calls.find((c) => c.cmd === "set_panel_size")?.args).toEqual({
      size: "large",
    });
    expect(
      wrapper.get('[data-testid="panel-size-large"]').attributes("aria-checked"),
    ).toBe("true");
    expect(wrapper.find('[data-testid="panel-size-error"]').exists()).toBe(false);
    clearMocks();
  });

  it("ignores a second pick while the first save+re-show is in flight", async () => {
    let releaseSave: () => void = () => {};
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_panel_config") return { size: "comfortable" };
      if (cmd === "set_panel_size") {
        return new Promise<void>((resolve) => {
          releaseSave = resolve;
        });
      }
    });
    const wrapper = mount(BuddySettings);
    await flush();
    // First pick hangs on set_panel_size → the control is now busy/disabled.
    await wrapper.get('[data-testid="panel-size-large"]').trigger("click");
    await flush();
    expect(
      wrapper.get('[data-testid="panel-size-large"]').attributes("disabled"),
    ).toBeDefined();
    // A second pick in that window is ignored — no two writes can race the
    // ConfigWriteLock in an unguaranteed order.
    await wrapper.get('[data-testid="panel-size-compact"]').trigger("click");
    await flush();
    expect(calls.filter((c) => c.cmd === "set_panel_size").length).toBe(1);
    releaseSave();
    await flush();
    clearMocks();
  });

  it("ignores picks until the initial read resolves, so a late read can't clobber a selection", async () => {
    let resolveRead: (v: { size: string }) => void = () => {};
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_panel_config") {
        return new Promise((resolve) => {
          resolveRead = resolve;
        });
      }
    });
    const wrapper = mount(BuddySettings);
    await flush();
    // Mount read still pending → the control is busy; a pick is ignored, so
    // there is no optimistic selection for the late read to overwrite.
    expect(
      wrapper.get('[data-testid="panel-size-large"]').attributes("disabled"),
    ).toBeDefined();
    await wrapper.get('[data-testid="panel-size-large"]').trigger("click");
    await flush();
    expect(calls.some((c) => c.cmd === "set_panel_size")).toBe(false);
    // The read lands last and wins.
    resolveRead({ size: "compact" });
    await flush();
    expect(
      wrapper.get('[data-testid="panel-size-compact"]').attributes("aria-checked"),
    ).toBe("true");
    clearMocks();
  });
});
