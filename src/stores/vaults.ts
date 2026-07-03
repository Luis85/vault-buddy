import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import type { Vault } from "../types";

export const useVaultsStore = defineStore("vaults", {
  state: () => ({
    vaults: [] as Vault[],
    loaded: false,
    panelOpen: false,
    busyVaultId: null as string | null,
    error: null as string | null,
  }),
  actions: {
    async loadVaults() {
      this.vaults = await invoke<Vault[]>("list_vaults");
      this.loaded = true;
    },
    async togglePanel() {
      this.panelOpen = !this.panelOpen;
      if (this.panelOpen && !this.loaded) {
        await this.loadVaults();
      }
    },
    async runAction(
      command: "open_vault" | "open_daily_note",
      vaultId: string,
    ) {
      this.busyVaultId = vaultId;
      this.error = null;
      try {
        await invoke(command, { id: vaultId });
      } catch (e) {
        this.error = String(e);
      } finally {
        this.busyVaultId = null;
      }
    },
  },
});
