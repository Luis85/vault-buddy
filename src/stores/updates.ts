import { defineStore } from "pinia";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

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
          this.available = update;
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
        await this.available.downloadAndInstall();
        // Tauri's signature check has already verified the payload; hand
        // over to the new version.
        await relaunch();
      } catch (e) {
        // keep `available` so the user can retry the install
        this.error = String(e);
        this.phase = "error";
      }
    },
  },
});
