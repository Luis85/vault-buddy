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
vi.mock("../src/logging", () => ({ logWarning: vi.fn() }));

import { logWarning } from "../src/logging";
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
    const download = vi.fn().mockResolvedValue(undefined);
    const install = vi.fn().mockResolvedValue(undefined);
    mocks.check.mockResolvedValue({ version: "0.2.0", download, install });
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();
    expect(download).toHaveBeenCalledTimes(1);
    expect(install).toHaveBeenCalledTimes(1);
    expect(mocks.relaunch).toHaveBeenCalledTimes(1);
    // the marker is only mis-stamped when install() throws after
    // prepare_update_install — a successful install must not re-arm
    expect(mocks.invoke).not.toHaveBeenCalledWith("rearm_crash_detection");
  });

  it("keeps the panel open while downloading, closes it before installing", async () => {
    // the download can be slow or fail — its spinner/error live in the panel
    // window, so the panel must not close until the process is about to exit
    const closedYet = () =>
      mocks.invoke.mock.calls.some((c) => c[0] === "close_panel");
    const closedDuring = { download: true, install: false };
    const download = vi.fn().mockImplementation(async () => {
      closedDuring.download = closedYet();
    });
    const install = vi.fn().mockImplementation(async () => {
      closedDuring.install = closedYet();
    });
    mocks.check.mockResolvedValue({ version: "0.2.0", download, install });
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();

    expect(closedDuring.download).toBe(false); // still open while downloading
    expect(closedDuring.install).toBe(true); // closed before installing
  });

  it("closes the panel before installing", async () => {
    // the install path exits the process; the panel window closes first.
    // The buddy window never shifts, so there is no home position to restore.
    const download = vi.fn().mockResolvedValue(undefined);
    const install = vi.fn().mockResolvedValue(undefined);
    mocks.check.mockResolvedValue({ version: "0.2.0", download, install });
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();

    expect(mocks.invoke).toHaveBeenCalledWith("close_panel");
    expect(mocks.invoke).toHaveBeenCalledWith("prepare_update_install");
    // close_panel must land before the install starts
    const closeIdx = mocks.invoke.mock.calls.findIndex(
      (c) => c[0] === "close_panel",
    );
    const closeOrder = mocks.invoke.mock.invocationCallOrder[closeIdx];
    const installOrder = install.mock.invocationCallOrder[0];
    expect(closeOrder).toBeLessThan(installOrder);
  });

  it("keeps the update retryable when the download fails", async () => {
    const download = vi.fn().mockRejectedValue("download broke");
    const install = vi.fn();
    mocks.check.mockResolvedValue({ version: "0.2.0", download, install });
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();
    expect(store.phase).toBe("error");
    expect(store.error).toContain("download broke");
    expect(store.available).not.toBeNull(); // retry stays possible
    // the download failed before the panel-close step — it was never closed,
    // so the error stays visible in the still-open panel
    expect(mocks.invoke).not.toHaveBeenCalledWith("close_panel");
    expect(install).not.toHaveBeenCalled();
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("reopens the panel on the settings view when the install fails", async () => {
    const vaults = useVaultsStore();
    const download = vi.fn().mockResolvedValue(undefined);
    const install = vi.fn().mockImplementation(async () => {
      // whatever the view state was when the process was about to exit,
      // the reopened panel must land on settings
      vaults.view = "list";
      throw "install broke";
    });
    mocks.check.mockResolvedValue({ version: "0.2.0", download, install });
    vaults.view = "settings"; // installs start from the settings view
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();
    expect(store.phase).toBe("error");
    expect(store.error).toContain("install broke");
    expect(store.available).not.toBeNull(); // retry stays possible
    // close_panel hid the panel window before the install threw — toggle_panel
    // re-shows it, on the settings view where the error/retry button live
    expect(mocks.invoke).toHaveBeenCalledWith("toggle_panel");
    expect(vaults.view).toBe("settings");
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });

  it("a failing install logs a warning through the log bridge", async () => {
    const download = vi.fn().mockResolvedValue(undefined);
    const install = vi.fn().mockRejectedValue("install broke");
    mocks.check.mockResolvedValue({ version: "0.2.0", download, install });
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("install failed"),
    );
  });

  it("re-arms crash detection when the install fails", async () => {
    // prepare_update_install already stamped the run marker "clean" and
    // latched crash detection off before install() ran — if install()
    // then throws, the app keeps running with detection permanently
    // disabled unless the frontend explicitly asks Rust to re-arm it.
    const download = vi.fn().mockResolvedValue(undefined);
    const install = vi.fn().mockRejectedValue("install broke");
    mocks.check.mockResolvedValue({ version: "0.2.0", download, install });
    const store = useUpdatesStore();
    await store.checkForUpdates();
    await store.installUpdate();
    expect(mocks.invoke).toHaveBeenCalledWith("rearm_crash_detection");
  });

  it("ignores install requests when no update is available", async () => {
    const store = useUpdatesStore();
    await store.installUpdate();
    expect(store.phase).toBe("idle");
    expect(mocks.relaunch).not.toHaveBeenCalled();
  });
});
