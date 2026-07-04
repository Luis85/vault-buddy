import { beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { isReactive } from "vue";

const mocks = vi.hoisted(() => ({
  getVersion: vi.fn(),
  check: vi.fn(),
  relaunch: vi.fn(),
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({ getVersion: mocks.getVersion }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: mocks.relaunch }));

import { useUpdatesStore } from "../src/stores/updates";
import { useVaultsStore } from "../src/stores/vaults";

describe("updates store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    mocks.getVersion.mockReset().mockResolvedValue("0.1.0");
    mocks.check.mockReset();
    mocks.relaunch.mockReset().mockResolvedValue(undefined);
    mocks.invoke.mockReset().mockResolvedValue(undefined);
  });

  it("loads the current app version", async () => {
    const store = useUpdatesStore();
    await store.loadVersion();
    expect(store.currentVersion).toBe("0.1.0");
  });

  it("reports up to date when the check returns nothing", async () => {
    mocks.check.mockResolvedValue(null);
    const store = useUpdatesStore();
    await store.checkForUpdates();
    expect(store.phase).toBe("upToDate");
    expect(store.available).toBeNull();
  });

  it("surfaces an available update", async () => {
    const update = { version: "0.2.0", downloadAndInstall: vi.fn() };
    mocks.check.mockResolvedValue(update);
    const store = useUpdatesStore();
    await store.checkForUpdates();
    expect(store.phase).toBe("available");
    expect(store.available?.version).toBe("0.2.0");
  });

  it("keeps the update object out of reactive state", async () => {
    // the real Update extends Resource, whose rid lives in a JS private
    // field — a Vue reactive proxy around it breaks downloadAndInstall()
    const update = { version: "0.2.0", downloadAndInstall: vi.fn() };
    mocks.check.mockResolvedValue(update);
    const store = useUpdatesStore();
    await store.checkForUpdates();
    expect(isReactive(store.available)).toBe(false);
  });

  it("surfaces check failures as an error state", async () => {
    mocks.check.mockRejectedValue("endpoint unreachable");
    const store = useUpdatesStore();
    await store.checkForUpdates();
    expect(store.phase).toBe("error");
    expect(store.error).toContain("endpoint unreachable");
  });

  it("downloads, installs and relaunches on install", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    mocks.check.mockResolvedValue({ version: "0.2.0", downloadAndInstall });
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();
    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
    expect(mocks.relaunch).toHaveBeenCalledTimes(1);
  });

  it("closes the panel and restores the home position before installing", async () => {
    // the install path exits the process without close/quit hooks — the
    // shifted window position must be restored before that happens
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    mocks.check.mockResolvedValue({ version: "0.2.0", downloadAndInstall });
    const vaults = useVaultsStore();
    vaults.panelOpen = true;
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();

    expect(vaults.panelOpen).toBe(false);
    expect(mocks.invoke).toHaveBeenCalledWith("prepare_update_install");
    // the restore must land before the download/install starts
    const restoreOrder = mocks.invoke.mock.invocationCallOrder[0];
    const installOrder = downloadAndInstall.mock.invocationCallOrder[0];
    expect(restoreOrder).toBeLessThan(installOrder);
  });

  it("keeps the update retryable when the install fails", async () => {
    const downloadAndInstall = vi.fn().mockRejectedValue("download broke");
    mocks.check.mockResolvedValue({ version: "0.2.0", downloadAndInstall });
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();
    expect(store.phase).toBe("error");
    expect(store.error).toContain("download broke");
    expect(store.available).not.toBeNull(); // retry stays possible
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("ignores install requests when no update is available", async () => {
    const store = useUpdatesStore();
    await store.installUpdate();
    expect(store.phase).toBe("idle");
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });
});
