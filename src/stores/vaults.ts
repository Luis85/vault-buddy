import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import type { Vault } from "../types";

export const useVaultsStore = defineStore("vaults", {
  state: () => ({
    vaults: [] as Vault[],
    loaded: false,
    // Which panel view is showing. Lives here (not in ActionPanel) because
    // the panel is destroyed while closed — a failed update install must be
    // able to reopen it directly on settings, where the error UI lives.
    view: "list" as "list" | "settings" | "captureSettings",
    // Which vault the captureSettings view edits.
    captureSettingsVaultId: null as string | null,
    busyVaultId: null as string | null,
    busyCommand: null as "open_vault" | "open_daily_note" | null,
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
        logWarning(`vault discovery failed: ${String(e)}`);
      } finally {
        this.loaded = true;
      }
    },
    async refresh() {
      this.showList();
      await this.loadVaults();
    },
    async runAction(
      command: "open_vault" | "open_daily_note",
      vaultId: string,
    ) {
      this.busyVaultId = vaultId;
      this.busyCommand = command;
      this.error = null;
      try {
        await invoke(command, { id: vaultId });
        void invoke("close_panel").catch(() => {});
      } catch (e) {
        this.error = String(e);
        logWarning(`${command} failed for vault ${vaultId}: ${String(e)}`);
      } finally {
        this.busyVaultId = null;
        this.busyCommand = null;
      }
    },
    showList() {
      this.view = "list";
      this.captureSettingsVaultId = null;
    },
    openSettings() {
      this.view = "settings";
    },
    openCaptureSettings(vaultId: string) {
      this.view = "captureSettings";
      this.captureSettingsVaultId = vaultId;
    },
  },
});
