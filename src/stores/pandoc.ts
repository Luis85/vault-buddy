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
    // Monotonic id of the latest probe — whether run internally by
    // ensureDetected or by a caller that claimed one via beginProbe (the
    // settings card, which probes directly). A probe applies its result and
    // clears the `checking` gate only while it still holds this id, so a slow
    // probe that resolves after a newer one — an intake ensureDetected, a
    // settings-side re-detect — can't clobber the fresher result or drop the
    // gate early (Codex P2).
    probeSeq: 0,
  }),
  actions: {
    // Called on mount by the intake surfaces. Once Pandoc is known installed it
    // returns without probing (the "found → don't re-check" behavior); when the
    // status is unknown/not-installed it probes once and caches, so a freshly
    // installed Pandoc is still picked up on the next open. No concurrent-probe
    // dedup: the two consumers never mount at the same time.
    async ensureDetected(): Promise<void> {
      // Cache only a USABLE result: an "installed but too old (<2.15)" Pandoc
      // keeps re-probing so an update is picked up on the next open (like a
      // not-installed status), rather than staying stale until a settings Recheck.
      if (this.status?.installed && this.status.sandboxSupported) return;
      const seq = ++this.probeSeq;
      this.checking = true;
      try {
        const result = await invoke<PandocStatus>("detect_pandoc");
        // Only the latest probe applies — an older, slower one is stale and
        // must not overwrite a newer result (a concurrent probe or a
        // settings-side markDetected).
        if (seq === this.probeSeq) this.status = result;
      } catch (e) {
        // Degrade to "not installed" (null) — the fallback the components used
        // before this cache existed. Leave status untouched (an older probe
        // failing must not blank a newer result).
        logWarning(`pandoc store: detect_pandoc failed: ${String(e)}`);
      } finally {
        // Only the latest probe drops the gate; an older one finishing while a
        // newer probe is still pending must leave "checking" set.
        if (seq === this.probeSeq) this.checking = false;
      }
    },
    // Claim the latest-probe token for a probe the CALLER runs itself (the
    // settings card, which invokes detect_pandoc directly rather than through
    // ensureDetected). Claimed at probe START, paired with markDetected(status,
    // token) at the end — so a settings probe that resolves after a newer probe
    // holds a stale token and is dropped.
    beginProbe(): number {
      return ++this.probeSeq;
    },
    // Write-through from a caller-run probe (the settings card's own probe, so
    // a settings-side Recheck / path-override fix refreshes the cache the
    // intake menu reads). Applies — and clears the gate — only while `token` is
    // still the latest probe: a slow settings probe that resolved after a newer
    // intake ensureDetected must NOT become authoritative and flip the active
    // import UI back to the setup/too-old gate (Codex P2).
    markDetected(status: PandocStatus, token: number) {
      if (token !== this.probeSeq) return;
      this.status = status;
      this.checking = false;
    },
  },
});
