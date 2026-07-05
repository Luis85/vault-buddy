import { markRaw } from "vue";
import { defineStore } from "pinia";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { logWarning } from "../logging";
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
        logWarning(`update check failed: ${String(e)}`);
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
        logWarning(`update download failed: ${String(e)}`);
        return;
      }
      const vaults = useVaultsStore();
      try {
        // Close the panel window before handing off to the installer, which
        // exits the process. `prepare_update_install` also closes it; this
        // gets the UI out of the way first. The buddy window never shifts,
        // so there is no home position to restore anymore.
        await invoke("close_panel").catch(() => {});
        await invoke("prepare_update_install").catch(() => {});
        await this.available.install();
        // Tauri's signature check has already verified the payload; hand
        // over to the new version.
        await relaunch();
      } catch (e) {
        // The install threw, so the process is still alive: reopen the panel
        // on the settings view so the error and retry button are visible.
        // `close_panel`/`prepare_update_install` hid the panel window, so
        // `toggle_panel` reliably re-shows it. `available` is kept for retry.
        vaults.openSettings();
        await invoke("toggle_panel").catch(() => {});
        this.error = String(e);
        this.phase = "error";
        logWarning(`update install failed: ${String(e)}`);
        // prepare_update_install already stamped the run marker "clean" and
        // latched crash detection off, expecting the process to exit
        // moments later. It didn't — install() threw — so the session
        // keeps running with detection permanently disabled unless we tell
        // Rust to re-arm it. Fire-and-forget: this must never block or
        // fail the retry path.
        void invoke("rearm_crash_detection").catch(() => {});
      }
    },
  },
});
