import { invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";

import { logWarning } from "../logging";
import type { FfmpegStatus } from "../types";

// App-global ffmpeg detection, cached for the panel session so the Record
// Screen pre-flight doesn't re-spawn `ffmpeg -version` every time the picker
// opens. The `usePandocStore` analogue, and deliberately the same shape — but
// what it GATES is different, and that difference is the point of GAP-144:
// Pandoc blocks a document import outright, while ffmpeg is needed only by the
// final Save into a vault. Recording and editing work perfectly without it, so
// every consumer of this store must WARN, never block.
export const useFfmpegStore = defineStore("ffmpeg", {
  state: () => ({
    // Last resolved status; null before the first probe or after a failed one
    // (consumers treat null as "not installed" — a warning we cannot justify
    // suppressing is better than a silent late failure at Save).
    status: null as FfmpegStatus | null,
    // True only while a probe runs with no cached status yet — lets a consumer
    // hold its notice back until the answer is real, instead of flashing
    // "ffmpeg is missing" at every open.
    checking: false,
    // Monotonic id of the latest probe, whether run here by ensureDetected or
    // by a caller that claimed one via beginProbe (the settings card, which
    // probes directly). A probe applies its result and clears the gate only
    // while it still holds this id, so a slow probe resolving after a newer one
    // can't clobber the fresher result or drop the gate early.
    probeSeq: 0,
  }),
  actions: {
    // Called on mount by the Record Screen picker. Once ffmpeg is known
    // installed it returns without probing; when the status is unknown or
    // not-installed it probes once and caches, so an ffmpeg installed in
    // response to the notice is picked up on the next open with no restart.
    async ensureDetected(): Promise<void> {
      // Cache rule, and it DIVERGES from the Pandoc store on purpose: Pandoc
      // keeps re-probing an "installed but too old" result because such a
      // Pandoc cannot import at all. An ffmpeg with no H.264 encoder is not
      // the same thing — it still remuxes an untouched capture, which is a
      // real, complete save path — so `installed` alone is a cache hit and the
      // missing-encoder case is reported by the settings card rather than
      // re-spawning a subprocess on every open forever.
      if (this.status?.installed) return;
      const seq = ++this.probeSeq;
      this.checking = true;
      try {
        const result = await invoke<FfmpegStatus>("detect_ffmpeg");
        // Only the latest probe applies — an older, slower one is stale and
        // must not overwrite a newer result (a concurrent probe, or a
        // settings-side markDetected).
        if (seq === this.probeSeq) this.status = result;
      } catch (e) {
        // Degrade to "not installed" (leave status null): the pre-flight then
        // warns, which is the safe direction — it blocks nothing and costs the
        // user only a line of text if the probe was merely broken.
        logWarning(`ffmpeg store: detect_ffmpeg failed: ${String(e)}`);
      } finally {
        // Only the latest probe drops the gate; an older one finishing while a
        // newer probe is still pending must leave "checking" set.
        if (seq === this.probeSeq) this.checking = false;
      }
    },
    // Claim the latest-probe token for a probe the CALLER runs itself (the
    // settings card, which invokes detect_ffmpeg directly). Claimed at probe
    // START, paired with markDetected(status, token) at the end — so a settings
    // probe that resolves after a newer one holds a stale token and is dropped.
    beginProbe(): number {
      return ++this.probeSeq;
    },
    // Write-through from a caller-run probe, so a settings-side Recheck or
    // path-override fix refreshes the cache the Record Screen pre-flight reads
    // — but only while `token` is still the latest probe.
    markDetected(status: FfmpegStatus, token: number) {
      if (token !== this.probeSeq) return;
      this.status = status;
      this.checking = false;
    },
  },
});
