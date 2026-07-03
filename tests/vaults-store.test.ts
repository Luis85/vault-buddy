import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useVaultsStore } from "../src/stores/vaults";

const sampleVaults = [
  { id: "d4e5f6", name: "Personal", path: "C:\\vaults\\Personal" },
  { id: "a1b2c3", name: "Work", path: "C:\\vaults\\Work" },
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

  it("runAction passes the vault id and tracks busy state", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useVaultsStore();
    await store.runAction("open_daily_note", "a1b2c3");
    expect(calls).toEqual([{ cmd: "open_daily_note", args: { id: "a1b2c3" } }]);
    expect(store.busyVaultId).toBe(null);
    expect(store.error).toBe(null);
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
});
