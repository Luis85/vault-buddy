import { invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";

import { logWarning } from "../logging";
import type { PandocStatus } from "../types";

// App-global Pandoc detection, cached for the panel session so the intake
// surfaces (RecordMode, ImportVaultPicker) don't re-spawn `pandoc --version`
// every time their view opens. Shared via the panel's single Pinia instance.
export const usePandocStore = defineStore("pandoc", {
  state: () => ({
    // Last resolved status; null before the first probe or after a failed one
    // (consumers treat null as "not installed").
    status: null as PandocStatus | null,
    // True only while a probe runs with no cached status yet — drives the
    // intake surfaces' "Checking Pandoc…" gate.
    checking: false,
  }),
  actions: {
    // Called on mount by the intake surfaces. Once Pandoc is known installed it
    // returns without probing (the "found → don't re-check" behavior); when the
    // status is unknown/not-installed it probes once and caches, so a freshly
    // installed Pandoc is still picked up on the next open. No concurrent-probe
    // dedup: the two consumers never mount at the same time.
    async ensureDetected(): Promise<void> {
      if (this.status?.installed) return;
      this.checking = true;
      try {
        this.status = await invoke<PandocStatus>("detect_pandoc");
      } catch (e) {
        // Degrade to "not installed" (null) — the fallback the components used
        // before this cache existed.
        logWarning(`pandoc store: detect_pandoc failed: ${String(e)}`);
      } finally {
        this.checking = false;
      }
    },
    // Write-through from the settings card's own probe, so a settings-side
    // Recheck / path-override fix refreshes the cache the intake menu reads.
    markDetected(status: PandocStatus) {
      this.status = status;
    },
  },
});
