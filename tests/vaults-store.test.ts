import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

vi.mock("../src/logging", () => ({
  logWarning: vi.fn(),
}));

import { logWarning } from "../src/logging";
import { useVaultsStore } from "../src/stores/vaults";

const sampleVaults = [
  { id: "d4e5f6", name: "Personal", path: "C:\\vaults\\Personal", open: false },
  { id: "a1b2c3", name: "Work", path: "C:\\vaults\\Work", open: false },
];

describe("vaults store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("loads vaults via the list_vaults command", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(store.vaults).toEqual(sampleVaults);
    expect(store.loaded).toBe(true);
  });

  it("opening the panel triggers the first load", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    expect(store.panelOpen).toBe(false);
    await store.togglePanel();
    expect(store.panelOpen).toBe(true);
    expect(store.vaults).toEqual(sampleVaults);
  });

  it("reopening the panel always lands on the vault list", async () => {
    mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
    const store = useVaultsStore();
    store.openSettings();
    expect(store.view).toBe("settings");
    await store.togglePanel(); // open
    expect(store.view).toBe("list");
    store.openCaptureSettings("v1");
    expect(store.view).toBe("captureSettings");
    expect(store.captureSettingsVaultId).toBe("v1");
    store.showList();
    expect(store.view).toBe("list");
    expect(store.captureSettingsVaultId).toBeNull();
  });

  it("runAction passes the vault id and tracks busy state", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useVaultsStore();
    await store.runAction("open_daily_note", "a1b2c3");
    expect(calls).toEqual([{ cmd: "open_daily_note", args: { id: "a1b2c3" } }]);
    expect(store.busyVaultId).toBe(null);
    expect(store.busyCommand).toBe(null);
    expect(store.error).toBe(null);
  });

  it("closes the panel after a successful action", async () => {
    mockIPC(() => undefined);
    const store = useVaultsStore();
    store.panelOpen = true;
    await store.runAction("open_vault", "a1b2c3");
    expect(store.panelOpen).toBe(false);
  });

  it("keeps the panel open when an action fails", async () => {
    mockIPC(() => {
      throw "vault not found: nope";
    });
    const store = useVaultsStore();
    store.panelOpen = true;
    await store.runAction("open_vault", "nope");
    expect(store.panelOpen).toBe(true);
    expect(store.error).toContain("vault not found");
  });

  it("runAction surfaces command errors", async () => {
    mockIPC(() => {
      throw "vault not found: nope";
    });
    const store = useVaultsStore();
    await store.runAction("open_vault", "nope");
    expect(store.error).toContain("vault not found");
    expect(store.busyVaultId).toBe(null);
  });

  it("loadVaults surfaces failures instead of leaving the panel blank", async () => {
    mockIPC(() => {
      throw "ipc unavailable";
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(store.loaded).toBe(true);
    expect(store.vaults).toEqual([]);
    expect(store.error).toContain("ipc unavailable");
  });

  it("re-runs discovery on every panel open so an empty first load can recover", async () => {
    let discovered: typeof sampleVaults = [];
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return discovered;
    });
    const store = useVaultsStore();
    await store.togglePanel(); // Obsidian not set up yet
    expect(store.vaults).toEqual([]);
    await store.togglePanel(); // close

    discovered = sampleVaults; // user has now opened Obsidian
    await store.togglePanel(); // reopen
    expect(store.vaults).toEqual(sampleVaults);
  });

  it("keeps the previous vault list when a refresh fails transiently", async () => {
    let fail = false;
    mockIPC((cmd) => {
      if (fail) throw "ipc unavailable";
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(store.vaults).toEqual(sampleVaults);

    fail = true;
    await store.loadVaults();
    expect(store.vaults).toEqual(sampleVaults);
    expect(store.error).toContain("ipc unavailable");
  });

  it("a failing list_vaults logs a warning through the log bridge", async () => {
    mockIPC(() => {
      throw "ipc unavailable";
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("vault discovery failed"),
    );
  });

  it("a failing open_vault logs a warning through the log bridge", async () => {
    mockIPC(() => {
      throw "vault not found: nope";
    });
    const store = useVaultsStore();
    await store.runAction("open_vault", "nope");
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("open_vault failed"),
    );
  });

  it("openRecordings switches to the recordings view for a vault", () => {
    const store = useVaultsStore();
    store.openRecordings("a1b2c3");
    expect(store.view).toBe("recordings");
    expect(store.recordingsVaultId).toBe("a1b2c3");
  });

  it("showList clears the recordings vault id", () => {
    const store = useVaultsStore();
    store.openRecordings("a1b2c3");
    store.showList();
    expect(store.view).toBe("list");
    expect(store.recordingsVaultId).toBe(null);
  });
});
