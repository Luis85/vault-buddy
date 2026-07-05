import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import BuddyRoot from "../src/roots/BuddyRoot.vue";
import { useSettingsStore } from "../src/stores/settings";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));

const calls: string[] = [];

describe("BuddyRoot", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    calls.length = 0;
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "start_buddy_drag") return true;
    });
  });
  afterEach(() => clearMocks());

  it("toggles the panel when the buddy is clicked", async () => {
    const wrapper = mount(BuddyRoot);
    await wrapper.find("button.buddy").trigger("click");
    expect(calls).toContain("toggle_panel");
  });

  it("closes the panel when a drag starts", async () => {
    const wrapper = mount(BuddyRoot);
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { buttons: 1, screenX: 90, screenY: 90 });
    await Promise.resolve();
    expect(calls).toContain("start_buddy_drag");
    expect(calls).toContain("close_panel");
  });

  it("re-syncs settings from localStorage on a cross-window storage event", async () => {
    mount(BuddyRoot);
    await Promise.resolve();
    localStorage.setItem("vault-buddy.animations", "off");
    window.dispatchEvent(new Event("storage"));
    expect(useSettingsStore().animationsEnabled).toBe(false);
  });

  it("pushes the buddy facing to Rust so the greeting bubble opens on the right side", async () => {
    mount(BuddyRoot);
    await Promise.resolve();
    // Rust needs the facing to place the bubble; the buddy window is the
    // single owner that pushes it.
    expect(calls).toContain("set_buddy_facing");
  });
});
