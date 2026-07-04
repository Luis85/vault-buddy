import { markRaw } from "vue";
import { defineStore } from "pinia";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useVaultsStore } from "./vaults";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "upToDate"
  | "available"
  | "installing"
  | "error";

export const useUpdatesStore = defineStore("updates", {
  state: () => ({
    currentVersion: "",
    phase: "idle" as UpdatePhase,
    available: null as Update | null,
    error: null as string | null,
  }),
  actions: {
    async loadVersion() {
      try {
        this.currentVersion = await getVersion();
      } catch {
        // not running under Tauri (unit tests)
      }
    },
    async checkForUpdates() {
      this.phase = "checking";
      this.error = null;
      try {
        const update = await check();
        if (update) {
          // Update extends Resource, whose rid lives in a JS private field;
          // a reactive proxy around it would make downloadAndInstall() throw
          this.available = markRaw(update);
          this.phase = "available";
        } else {
          this.available = null;
          this.phase = "upToDate";
        }
      } catch (e) {
        this.error = String(e);
        this.phase = "error";
      }
    },
    async installUpdate() {
      if (!this.available) return;
      this.phase = "installing";
      this.error = null;
      try {
        // Download with the panel still open: the spinner and any
        // download/signature error render inside it, so it must not vanish
        // while the slow part runs.
        await this.available.download();
      } catch (e) {
        // keep `available` so the user can retry the install
        this.error = String(e);
        this.phase = "error";
        return;
      }
      const vaults = useVaultsStore();
      try {
        // The install path exits the process without the normal close/quit
        // hooks, and the window-state plugin persists the position on exit.
        // Close the panel (its transition restores the window's unshifted
        // home position) and run the Rust-side restore as a deterministic
        // backstop, so installing with the panel open at a screen edge
        // can't persist the shifted point.
        vaults.panelOpen = false;
        await new Promise((resolve) => setTimeout(resolve, 150));
        await invoke("prepare_update_install").catch(() => {});
        await this.available.install();
        // Tauri's signature check has already verified the payload; hand
        // over to the new version.
        await relaunch();
      } catch (e) {
        // reopen the panel so the error is visible; keep `available` so
        // the user can retry the install
        vaults.panelOpen = true;
        this.error = String(e);
        this.phase = "error";
      }
    },
  },
});
