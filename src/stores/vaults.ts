import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { announce } from "../announce";
import { vaultOpenedMessage, dailyNoteOpenedMessage } from "../buddyMessages";
import type { Vault } from "../types";

export const useVaultsStore = defineStore("vaults", {
  state: () => ({
    vaults: [] as Vault[],
    loaded: false,
    // Which panel view is showing. Lives here (not in ActionPanel) because
    // the panel window is only hidden/shown, not destroyed — a failed update
    // install must be able to reopen it directly on settings, where the error
    // UI lives.
    view: "list" as
      | "list"
      | "settings"
      | "captureSettings"
      | "recordings"
      | "recordMode",
    // Which vault the captureSettings view edits.
    captureSettingsVaultId: null as string | null,
    // Which vault the recordings view lists.
    recordingsVaultId: null as string | null,
    // Which vault the recordMode view shows.
    recordModeVaultId: null as string | null,
    // A view to open ON THE NEXT panel-shown refresh, consumed once. The panel
    // defaults to the vault list on every open (`refresh`); a caller that must
    // reopen elsewhere (a failed update install → settings) sets this so the
    // open can't clobber it back to the list.
    pendingView: null as "list" | "settings" | "captureSettings" | null,
    pendingCaptureVaultId: null as string | null,
    // Bumped on every panel-shown refresh. The panel window is only
    // hidden/shown (never unmounted), so components watch this to reset
    // transient UI (open dialogs, filter text) that a close used to clear.
    shownNonce: 0,
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
    // The panel-shown handler: re-run discovery (one JSON read — a user who
    // saw the empty state, then opened Obsidian, must not stay stuck on the
    // cached result) and pick the view. Defaults to the vault list on every
    // open, unless a one-shot `requestView` asked for somewhere else.
    async refresh() {
      if (this.pendingView) {
        this.view = this.pendingView;
        this.captureSettingsVaultId = this.pendingCaptureVaultId;
        this.pendingView = null;
        this.pendingCaptureVaultId = null;
      } else {
        this.showList();
      }
      this.shownNonce++;
      await this.loadVaults();
    },
    // Ask the next panel open to land on `view` instead of the vault list.
    // Reflected immediately (a still-open panel updates now) and stored as
    // pending so the panel-shown `refresh` re-applies it rather than resetting
    // to the list.
    requestView(
      view: "list" | "settings" | "captureSettings",
      captureVaultId: string | null = null,
    ) {
      this.pendingView = view;
      this.pendingCaptureVaultId = captureVaultId;
      this.view = view;
      this.captureSettingsVaultId = captureVaultId;
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
        // the buddy acknowledges the launch (panel window is the single
        // announcer for opens); a failed open falls through to the catch and
        // stays silent — the inline error banner is the feedback there.
        const vault = this.vaults.find((v) => v.id === vaultId);
        announce(
          command === "open_daily_note"
            ? dailyNoteOpenedMessage()
            : vaultOpenedMessage(vault?.name ?? ""),
        );
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
      this.recordingsVaultId = null;
      this.recordModeVaultId = null;
    },
    openSettings() {
      this.view = "settings";
    },
    openCaptureSettings(vaultId: string) {
      this.view = "captureSettings";
      this.captureSettingsVaultId = vaultId;
    },
    openRecordings(vaultId: string) {
      this.view = "recordings";
      this.recordingsVaultId = vaultId;
    },
    openRecordMode(vaultId: string) {
      this.view = "recordMode";
      this.recordModeVaultId = vaultId;
    },
    /** Back to the current view's fixed parent (no history stack). */
    back() {
      if (this.view === "recordings" && this.recordingsVaultId) {
        this.openRecordMode(this.recordingsVaultId);
      } else {
        this.showList();
      }
    },
  },
});
