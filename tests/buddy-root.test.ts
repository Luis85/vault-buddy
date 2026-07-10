import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises,mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import BuddyRoot from "../src/roots/BuddyRoot.vue";
import { useCaptureStore } from "../src/stores/capture";
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

  it("puts the buddy in its working animation while transcribing", async () => {
    const wrapper = mount(BuddyRoot);
    await flushPromises();
    const capture = useCaptureStore();
    // transcription is the buddy's "working" state — it should run/pulse, not
    // just show the dot. Driven from the capture store, like recording/paused.
    capture.transcriptions = {
      "/v/a.mp3": {
        mp3: "/v/a.mp3",
        vaultId: "v1",
        name: "a",
        phase: "preparing",
        progress: null,
        model: null,
        error: null,
        startedAtMs: Date.now(),
      },
    };
    await wrapper.vm.$nextTick();
    expect(wrapper.find("button.buddy").classes()).toContain("working");
  });

  it("reads the position-derived buddy facing from Rust on mount", async () => {
    mount(BuddyRoot);
    await flushPromises();
    // Facing is derived from the buddy's position by Rust; the buddy window
    // pulls the initial value on mount and then listens for `buddy-facing`
    // flips (it no longer pushes a stored facing setting to Rust).
    expect(calls).toContain("get_buddy_facing");
    expect(calls).not.toContain("set_buddy_facing");
  });
});
