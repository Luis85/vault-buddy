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
      try {
        this.vaults = await invoke<Vault[]>("list_vaults");
        this.error = null;
      } catch (e) {
        // Keep whatever list we had; a transient failure shouldn't blank
        // a panel that was working a moment ago.
        this.error = String(e);
      } finally {
        this.loaded = true;
      }
    },
    async togglePanel() {
      this.panelOpen = !this.panelOpen;
      // Refresh on every open: discovery is one JSON read, and a user who
      // saw the empty state, then opened Obsidian, must not stay stuck on
      // the cached result until the app restarts.
      if (this.panelOpen) {
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
