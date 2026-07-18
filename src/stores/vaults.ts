import { invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";

import { announce } from "../announce";
import { dailyNoteOpenedMessage, vaultOpenedMessage } from "../buddyMessages";
import { logWarning } from "../logging";
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
      | "recordMode"
      | "transcriptions"
      | "tasks"
      | "search"
      | "importPicker"
      | "documentImport"
      | "update",
    // Which vault the captureSettings view edits.
    captureSettingsVaultId: null as string | null,
    // Which vault the recordings view lists.
    recordingsVaultId: null as string | null,
    // Which vault the recordMode view shows.
    recordModeVaultId: null as string | null,
    // Which vault the tasks view lists.
    tasksVaultId: null as string | null,
    // The dropped document's path, armed by a Rust-owned buddy drop
    // (`take_pending_import`, consumed in `refresh`) and read by
    // ImportVaultPicker to drive `convert_document`.
    // The FIFO queue of dropped-document paths awaiting a vault pick; the head
    // (index 0) is the document the picker is currently showing (GAP-55).
    pendingImports: [] as string[],
    // Monotonic token for the current import-queue "session", bumped whenever
    // the queue is cleared (showList). A conversion captures it when its pick
    // starts and passes it back on completion; a mismatch means the user backed
    // out — possibly re-dropping the SAME path, which by-value matching would
    // wrongly consume — so the stale completion must touch neither the queue nor
    // navigation (Codex P2).
    importEpoch: 0,
    // A view to open ON THE NEXT panel-shown refresh, consumed once. The panel
    // defaults to the vault list on every open (`refresh`); a caller that must
    // reopen elsewhere (a failed update install → settings) sets this so the
    // open can't clobber it back to the list.
    pendingView: null as "list" | "settings" | "captureSettings" | "update" | null,
    pendingCaptureVaultId: null as string | null,
    // Bumped on every panel-shown refresh. The panel window is only
    // hidden/shown (never unmounted), so components watch this to reset
    // transient UI (open dialogs, filter text) that a close used to clear.
    shownNonce: 0,
    busyVaultId: null as string | null,
    busyCommand: null as "open_vault" | "open_daily_note" | null,
    error: null as string | null,
    // Open-task count per vault id (status new; done/archived excluded), for
    // the vault-row Tasks badge. Refreshed on every panel open.
    taskCounts: {} as Record<string, number>,
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
      // Consume the Rust-owned pending buddy-drop import FIRST: it always
      // wins over the pendingView/list default, because the drop is the
      // reason this exact refresh is happening (see the module comment on
      // `pendingImports` and document_commands::begin_document_import).
      const dropped =
        (await invoke<string[]>("take_pending_import").catch(
          () => [] as string[],
        )) ?? [];
      // Drain the buddy-menu "Import document…" request in the same pass —
      // even when a drop wins below — so it can never fire stale on a later
      // reopen. Same Rust-owned-stash reasoning as the drop queue.
      const addRequested = await invoke<boolean>(
        "take_add_document_request",
      ).catch(() => false);
      if (dropped.length) {
        this.enqueueImports(dropped);
        // A drop supersedes any armed one-shot view (e.g. the startup update
        // check's "settings"): clear it so a LATER panel-shown refresh doesn't
        // consume it stale and navigate away after the import returns to list.
        this.pendingView = null;
        this.pendingCaptureVaultId = null;
      } else if (addRequested) {
        // The picker with an EMPTY queue is its vault-first mode: pick the
        // vault, then the file (ImportVaultPicker opens the OS picker on
        // pick). The menu click is why this refresh is happening, so it
        // supersedes an armed one-shot view exactly like a drop does.
        this.view = "importPicker";
        this.pendingView = null;
        this.pendingCaptureVaultId = null;
      } else if (this.view === "importPicker" && this.pendingImports.length) {
        // An un-picked import queue survives a spurious empty-drain refresh:
        // near-simultaneous drops each fire panel-shown → refresh, and a later
        // refresh can drain empty; leaving the picker as-is keeps the queue the
        // first refresh built (and keeps an un-picked single drop, too).
      } else if (this.pendingView) {
        this.view = this.pendingView;
        this.captureSettingsVaultId = this.pendingCaptureVaultId;
        this.pendingView = null;
        this.pendingCaptureVaultId = null;
      } else {
        this.showList();
      }
      this.shownNonce++;
      await this.loadVaults();
      await this.loadTaskCounts();
    },
    async loadTaskCounts() {
      // Best-effort, in parallel; a failed/absent count is treated as 0. Replace
      // the map wholesale so a removed vault's stale count can't linger.
      const entries = await Promise.all(
        this.vaults.map(async (v) => {
          try {
            return [
              v.id,
              await invoke<number>("count_open_tasks", { id: v.id }),
            ] as const;
          } catch (e) {
            // Degrade the badge to 0, but never swallow the error silently — a
            // broken counter must be distinguishable from a vault with no open
            // tasks (Diagnostics invariant: caught errors go through logging).
            logWarning(`count_open_tasks failed for vault ${v.id}: ${String(e)}`);
            return [v.id, 0] as const;
          }
        }),
      );
      this.taskCounts = Object.fromEntries(entries);
    },
    /** Refresh ONE vault's open-task badge after a mutation (GAP-32 / Codex
     * PR #46): panel-shown is too late for a badge the user is looking at.
     * On failure keep the previous count — zeroing a badge because one
     * mid-session refresh failed would misreport a vault that has tasks. */
    async refreshTaskCount(id: string) {
      try {
        const count = await invoke<number>("count_open_tasks", { id });
        this.taskCounts = { ...this.taskCounts, [id]: count };
      } catch (e) {
        logWarning(`count_open_tasks refresh failed for vault ${id}: ${String(e)}`);
      }
    },
    // Ask the next panel open to land on `view` instead of the vault list.
    // Reflected immediately (a still-open panel updates now) and stored as
    // pending so the panel-shown `refresh` re-applies it rather than resetting
    // to the list.
    requestView(
      view: "list" | "settings" | "captureSettings" | "update",
      captureVaultId: string | null = null,
    ) {
      this.pendingView = view;
      this.pendingCaptureVaultId = captureVaultId;
      this.view = view;
      this.captureSettingsVaultId = captureVaultId;
    },
    // The gentle variant: arm the NEXT open only, without flipping the live
    // view — the startup update check must not yank an already-open panel to
    // settings mid-task (requestView's immediate flip exists for the
    // failed-install reopen, where the panel is known hidden).
    requestViewOnNextOpen(view: "list" | "settings" | "captureSettings" | "update") {
      this.pendingView = view;
      this.pendingCaptureVaultId = null;
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
      this.tasksVaultId = null;
      this.pendingImports = [];
      // Invalidate any in-flight conversion's claim on the queue (see importEpoch).
      this.importEpoch++;
    },
    openSettings() {
      this.view = "settings";
    },
    openUpdate() {
      this.view = "update";
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
    openTranscriptions() {
      this.view = "transcriptions";
    },
    openTasks(vaultId: string) {
      this.view = "tasks";
      this.tasksVaultId = vaultId;
    },
    /** The cross-vault "All tasks" view — tasks view with no vault selected. */
    openAllTasks() {
      this.view = "tasks";
      this.tasksVaultId = null;
    },
    // Cross-vault, so no per-vault id to remember (unlike tasks/recordings).
    // back() needs no case: search falls through to the final else → showList.
    openSearch() {
      this.view = "search";
    },
    // The focused Pandoc setup screen the Import gates route to when Pandoc is
    // missing/too old — a dedicated view rather than the buried settings card.
    // back() needs no case: it falls through to the final else → showList.
    openDocumentImport() {
      this.view = "documentImport";
    },
    // Append drained buddy-drop paths to the FIFO queue and show the picker.
    // back() needs no case: it falls through to the final else → showList,
    // which also clears the queue.
    enqueueImports(paths: string[]) {
      this.view = "importPicker";
      this.pendingImports.push(...paths);
    },
    // Called by the picker when a conversion completes. `epoch` is the token
    // captured when the pick STARTED. Drop the just-converted head (the picker
    // always converts pendingImports[0], and a mid-conversion drop appends to
    // the tail) and, once drained, leave the picker for the list — but only
    // while the epoch still matches. A stale completion after a back-out
    // (even one that re-dropped the same path) no longer owns the queue, so it
    // must neither consume an entry nor yank navigation (Codex P2). Removing by
    // head rather than by value is what makes the same-path re-drop safe.
    dequeueImport(epoch: number) {
      if (epoch !== this.importEpoch) return;
      this.pendingImports.shift();
      if (this.pendingImports.length === 0 && this.view === "importPicker") {
        this.showList();
      }
    },
    // Completion for a VAULT-FIRST conversion (buddy-menu flow): the source
    // came from the OS dialog, not the queue, so there is nothing to consume
    // — dequeueImport's shift here would silently eat a document dropped onto
    // the buddy while the conversion ran (Codex PR #63). Same epoch guard and
    // return-to-list semantics; a mid-conversion drop keeps the picker open
    // with that new head offered next.
    settleAddImport(epoch: number) {
      if (epoch !== this.importEpoch) return;
      if (this.pendingImports.length === 0 && this.view === "importPicker") {
        this.showList();
      }
    },
    /** Back to the current view's fixed parent (no history stack). */
    back() {
      if (this.view === "recordings" && this.recordingsVaultId) {
        this.openRecordMode(this.recordingsVaultId);
      } else if (this.view === "transcriptions") {
        return this.showList();
      } else if (this.view === "tasks") {
        // Leaving the tasks view: a full reload (not just the one row a
        // single mutation would refresh) covers bulk edits and the
        // aggregate view's null vaultId, where there's no single vault to
        // target with refreshTaskCount.
        void this.loadTaskCounts();
        return this.showList();
      } else {
        this.showList();
      }
    },
  },
});
