import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises,mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import PanelRoot from "../src/roots/PanelRoot.vue";
import { useSettingsStore } from "../src/stores/settings";
import { useVaultsStore } from "../src/stores/vaults";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

// Capture every Tauri event listener the roots install, keyed by event name,
// so a test can fire "panel-shown" the way Rust's toggle_panel does.
const listeners: Record<string, Array<(e: { payload: unknown }) => void>> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: { payload: unknown }) => void) => {
    (listeners[event] ??= []).push(cb);
    return Promise.resolve(() => {});
  },
}));

function firePanelShown() {
  (listeners["panel-shown"] ?? []).forEach((cb) => cb({ payload: undefined }));
}

const calls: string[] = [];

describe("PanelRoot", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    calls.length = 0;
    for (const key of Object.keys(listeners)) delete listeners[key];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_vaults") return [];
    });
  });
  afterEach(() => clearMocks());

  it("initializes the capture store on mount so the panel reflects recording", async () => {
    mount(PanelRoot);
    await flushPromises();
    // capture.init() resyncs via capture_status; without it the panel's own
    // capture store never sees capture:* events (dead level meter, stuck save).
    expect(calls).toContain("capture_status");
  });

  it("runs discovery each time the panel is shown, not on mount", async () => {
    mount(PanelRoot);
    await flushPromises();
    // hidden at startup: no discovery until the panel is actually shown
    expect(calls).not.toContain("list_vaults");
    firePanelShown();
    await flushPromises();
    expect(calls).toContain("list_vaults");

    calls.length = 0;
    firePanelShown();
    await flushPromises();
    expect(calls).toContain("list_vaults"); // re-runs on every open
  });

  it("closes the panel on Escape", async () => {
    mount(PanelRoot);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await Promise.resolve();
    expect(calls).toContain("close_panel");
  });

  it("ignores a composing Escape (IME candidate-cancel), never closing the panel", async () => {
    mount(PanelRoot);
    // GAP-31 follow-up: the filter's own composing guard made a candidate-
    // cancel Escape bubble to this window handler instead of being swallowed
    // there — closing the WHOLE panel is worse than the filter-clearing the
    // original bug caused. isComposing must stop it at this chokepoint.
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", isComposing: true, bubbles: true }),
    );
    await Promise.resolve();
    expect(calls).not.toContain("close_panel");
  });

  it("closes the panel when the transparent gutter is clicked", async () => {
    const wrapper = mount(PanelRoot);
    await flushPromises();
    // clicking the gutter itself (target === currentTarget), not the card
    await wrapper.find("div.h-screen").trigger("click");
    expect(calls).toContain("close_panel");
  });

  it("defaults to the vault list when the panel is shown", async () => {
    mount(PanelRoot);
    await flushPromises();
    const store = useVaultsStore();
    store.openSettings();
    firePanelShown();
    await flushPromises();
    expect(store.view).toBe("list");
  });

  it("honors a requested view on open instead of resetting to the list", async () => {
    mount(PanelRoot);
    await flushPromises();
    const store = useVaultsStore();
    // a failed update install requests settings before the panel reopens
    store.requestView("settings");
    firePanelShown();
    await flushPromises();
    expect(store.view).toBe("settings");
  });

  it("re-syncs settings from a cross-window storage event", async () => {
    mount(PanelRoot);
    await flushPromises();
    localStorage.setItem("vault-buddy.animations", "off");
    window.dispatchEvent(new Event("storage"));
    expect(useSettingsStore().animationsEnabled).toBe(false);
  });
});
