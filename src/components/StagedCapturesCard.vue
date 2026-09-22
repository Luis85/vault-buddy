<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { ClearStagedResult, StagingUsage } from "../types";
import { formatBytes } from "../utils/formatBytes";
import AppButton from "./ui/AppButton.vue";
import Banner from "./ui/Banner.vue";

// Staged screen captures on this machine (Buddy settings > System, beside
// DiagnosticsSettings) — spec §10's disk-pressure paragraph, the second half
// of docs/Gaps.md GAP-115. Until this landed nothing anywhere reported the
// staging directory's size and nothing cleared it in bulk; a user who
// recorded a dozen 1080p captures and saved none accumulated gigabytes under
// %LOCALAPPDATA% with only a one-at-a-time Discard to get them back.
//
// System, not Integrations: this is app-owned storage on the user's disk,
// the same concern as logs and crash records, where Integrations is about
// external tools (Pandoc, ffmpeg, the MCP endpoint). It is app-GLOBAL and
// never per-vault, which is also why it is not a card in the per-vault
// Screen tab: a staged capture records which vault it is for, but they all
// live in one directory outside every vault.
//
// Clear is destructive and irreversible, so it sits behind a two-step
// in-component confirm — spec §10: nothing is ever deleted silently. A
// native dialog would be wrong here for the reason ExportBar records:
// DIALOG_ACTIVE is a process-wide bool with two drivers already (GAP-128).

const usage = ref<StagingUsage | null>(null);
const confirming = ref(false);
const busy = ref(false);
const error = ref<string | null>(null);
const outcome = ref<string | null>(null);

const hasCaptures = computed(() => (usage.value?.captures ?? 0) > 0);

const summary = computed(() => {
  const u = usage.value;
  if (!u) return "Checking…";
  if (u.captures === 0) return "No staged captures";
  const noun = u.captures === 1 ? "capture" : "captures";
  return `${u.captures} ${noun} · ${formatBytes(u.bytes)}`;
});

async function refresh() {
  try {
    usage.value = await invoke<StagingUsage>("staging_usage");
  } catch (e) {
    // The command itself degrades to zero rather than rejecting, so a
    // rejection here means IPC is unavailable; keep the last good reading
    // rather than blanking a number the user is reading.
    error.value = String(e);
    logWarning(`staging_usage failed: ${String(e)}`);
  }
}

/** Say what actually happened, including the parts that are not success.
 * A capture left behind because an export is writing it, or refused because
 * its leaf is a symlink, must never be reported as cleared. */
function describe(r: ClearStagedResult): string {
  const parts = [`Cleared ${r.cleared} · freed ${formatBytes(r.bytesFreed)}`];
  if (r.skipped > 0) parts.push(`${r.skipped} left alone (being saved)`);
  if (r.skippedPinned > 0) parts.push(`${r.skippedPinned} kept (used by a tutorial project)`);
  if (r.failed > 0) parts.push(`${r.failed} could not be removed`);
  return parts.join(" · ");
}

async function clearAll() {
  busy.value = true;
  error.value = null;
  outcome.value = null;
  try {
    const result = await invoke<ClearStagedResult>("clear_staged_captures");
    outcome.value = describe(result);
  } catch (e) {
    error.value = String(e);
    logWarning(`clear_staged_captures failed: ${String(e)}`);
  } finally {
    busy.value = false;
    confirming.value = false;
    await refresh();
  }
}

onMounted(refresh);
defineExpose({ refresh });
</script>

<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted">
      Staged screen captures
    </h2>
    <div class="flex flex-col gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
      <p class="text-xs text-fg-muted">
        Recordings waiting to be edited and saved. They are kept outside your
        vaults until you save them.
      </p>
      <p
        class="text-sm text-fg"
        data-testid="staging-summary"
      >
        {{ summary }}
      </p>

      <Banner
        v-if="error"
        tone="danger"
      >
        {{ error }}
      </Banner>
      <Banner
        v-else-if="outcome"
        tone="info"
      >
        {{ outcome }}
      </Banner>

      <div
        v-if="!confirming"
        class="flex justify-end"
      >
        <AppButton
          variant="secondary"
          size="sm"
          :disabled="!hasCaptures || busy"
          data-testid="staging-clear"
          @click="confirming = true"
        >
          Clear staged captures
        </AppButton>
      </div>
      <div
        v-else
        class="flex flex-col gap-1.5"
      >
        <Banner tone="warning">
          This permanently deletes every staged capture and cannot be undone.
          A capture being saved right now is left alone.
        </Banner>
        <div class="flex justify-end gap-1.5">
          <AppButton
            variant="ghost"
            size="sm"
            :disabled="busy"
            @click="confirming = false"
          >
            Cancel
          </AppButton>
          <AppButton
            variant="danger"
            size="sm"
            :disabled="busy"
            data-testid="staging-clear-confirm"
            @click="clearAll"
          >
            {{ busy ? "Clearing…" : "Delete them all" }}
          </AppButton>
        </div>
      </div>
    </div>
  </section>
</template>
