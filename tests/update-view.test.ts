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

import UpdateView from "../src/components/UpdateView.vue";
import { useUpdatesStore } from "../src/stores/updates";

function primeAvailable(overrides: Record<string, unknown> = {}) {
  const store = useUpdatesStore();
  store.phase = "available";
  store.currentVersion = "0.1.0";
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  store.available = {
    version: "0.2.0",
    currentVersion: "0.1.0",
    date: "2026-07-18",
    body: "- Faster startup\n- Bug fixes",
    ...overrides,
  } as any;
  return store;
}

describe("UpdateView", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => vi.clearAllMocks());

  it("shows the available version and its release notes", () => {
    primeAvailable();
    const wrapper = mount(UpdateView);
    expect(wrapper.text()).toContain("0.2.0");
    expect(wrapper.get('[data-testid="release-notes"]').text()).toContain(
      "Faster startup",
    );
  });

  it("falls back gracefully when a release has no notes", () => {
    primeAvailable({ body: "" });
    const wrapper = mount(UpdateView);
    expect(wrapper.find('[data-testid="release-notes"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("No release notes provided.");
  });

  it("installs via the store on the Install button", async () => {
    const store = primeAvailable();
    const spy = vi.spyOn(store, "installUpdate").mockResolvedValue();
    const wrapper = mount(UpdateView);
    await wrapper.get('[data-testid="install-update"]').trigger("click");
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("shows a spinner and disables the button while installing", () => {
    const store = primeAvailable();
    store.phase = "installing";
    const wrapper = mount(UpdateView);
    const btn = wrapper.get('[data-testid="install-update"]');
    expect(btn.text()).toContain("Installing…");
    expect(btn.attributes("disabled")).toBeDefined();
  });

  it("surfaces an install error while keeping the retry button", () => {
    const store = primeAvailable();
    store.phase = "error";
    store.error = "signature mismatch";
    const wrapper = mount(UpdateView);
    expect(wrapper.get('[data-testid="update-error"]').text()).toContain(
      "signature mismatch",
    );
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(true);
  });

  it("shows a friendly empty state when no update is available", () => {
    const store = useUpdatesStore();
    store.phase = "idle";
    store.available = null;
    const wrapper = mount(UpdateView);
    expect(wrapper.text()).toContain("No update is available right now.");
    expect(wrapper.find('[data-testid="install-update"]').exists()).toBe(false);
  });
});
