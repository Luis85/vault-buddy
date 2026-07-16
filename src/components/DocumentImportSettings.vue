<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { usePandocStore } from "../stores/pandoc";
import type { PandocStatus } from "../types";
import { withDialogSuppressed } from "../utils/nativeDialog";

const status = ref<PandocStatus | null>(null);
const pathOverride = ref("");
// Set once the user has touched the override field. The input is enabled while
// the initial detect() is still in flight, so a user who types during a slow
// probe must not have their edit clobbered by the on-mount seed below.
const dirtied = ref(false);
const error = ref<string | null>(null);
// Single in-flight guard shared by recheck() and savePath() — mirrors
// McpSettings' one `saving` flag serializing save()/regenerate(): two
// concurrent detect/save calls could otherwise land out of order and leave
// a stale status showing.
const saving = ref(false);
const pandocStore = usePandocStore();

// Monotonic ticket so out-of-order detect responses can't regress the status:
// a slow initial probe must not overwrite the fresher result of a save/browse
// re-detect that resolved first (same idiom as Search's request ticket).
let detectTicket = 0;

async function detect() {
  const ticket = ++detectTicket;
  // Claim the store's probe token at the START (not at resolution): if the user
  // leaves settings and an intake ensureDetected runs a newer probe, this one's
  // token goes stale and markDetected below drops the write-through instead of
  // clobbering the fresher intake result (Codex P2). The local detectTicket
  // still orders this card's own out-of-order responses.
  const token = pandocStore.beginProbe();
  try {
    const s = await invoke<PandocStatus>("detect_pandoc");
    if (ticket === detectTicket) {
      status.value = s;
      // Keep the shared intake-menu cache fresh after a settings-side probe
      // (Recheck / path-override re-detect), so the record chooser sees the fix
      // — but only while this probe is still the newest across the store.
      pandocStore.markDetected(s, token);
    }
  } catch (e) {
    // Not running under Tauri (unit tests) or IPC failure — leave the card
    // empty, same degraded-but-continuing pattern as McpSettings/CaptureSettings.
    if (ticket === detectTicket) error.value = String(e);
    logWarning(`document import settings: detect_pandoc failed: ${String(e)}`);
  }
}

onMounted(async () => {
  await detect();
  // Seed the override field from the resolved status, not a second command —
  // but never over a value the user already typed while detect was in flight.
  if (!dirtied.value) {
    pathOverride.value = status.value?.configuredPath ?? "";
  }
});

async function recheck() {
  if (saving.value) return;
  saving.value = true;
  error.value = null;
  try {
    await detect();
  } finally {
    saving.value = false;
  }
}

async function savePath() {
  if (saving.value) return;
  saving.value = true;
  error.value = null;
  try {
    const trimmed = pathOverride.value.trim();
    await invoke("set_pandoc_path", { pandocPath: trimmed || null });
    // Re-detect so the new (or cleared) override resolves immediately —
    // the card must not keep showing the pre-save status.
    await detect();
  } catch (e) {
    error.value = String(e);
    logWarning(`document import settings: set_pandoc_path failed: ${String(e)}`);
  } finally {
    saving.value = false;
  }
}

async function browse() {
  if (saving.value) return;
  saving.value = true;
  error.value = null;
  try {
    const selected = await withDialogSuppressed(() =>
      open({
        multiple: false,
        filters: [{ name: "Pandoc", extensions: ["exe", ""] }],
      }),
    );
    if (typeof selected === "string") {
      // Browse assigns programmatically (no @input fires), so mark the field
      // dirty explicitly — otherwise a still-pending initial detect could
      // reseed pathOverride from the old configuredPath and savePath would
      // persist the stale value instead of the picked executable.
      dirtied.value = true;
      pathOverride.value = selected;
      // savePath() self-guards on `saving`; release it first so its own
      // set_pandoc_path + re-detect run, then finally restores the guard.
      saving.value = false;
      await savePath();
    }
  } catch (e) {
    // Not running under Tauri (unit tests) or a picker failure — same
    // warn-and-continue pattern as every other guarded action here.
    error.value = String(e);
    logWarning(`document import settings: browse failed: ${String(e)}`);
  } finally {
    saving.value = false;
  }
}

const INSTALL_URL = "https://pandoc.org/installing.html";

// Open the install page in the OS browser via Rust — a raw `target="_blank"`
// in a Tauri v2 webview either no-ops or replaces the app UI, so we intercept
// the click and route through the logged `open_external_url` command. The
// `href` stays for accessibility / right-click-copy; a plain-tap failure just
// warns (the URL is visible to copy).
async function openInstall() {
  try {
    await invoke("open_external_url", { url: INSTALL_URL });
  } catch (e) {
    logWarning(`document import settings: open_external_url failed: ${String(e)}`);
  }
}

const statusLabel = computed(() => {
  const s = status.value;
  // No status yet: distinguish "still probing" from "the probe failed" — the
  // latter must stay visible so the error + Recheck + path override below (the
  // recovery affordances) aren't hidden exactly when Pandoc detection breaks.
  if (!s) return error.value ? "Couldn't detect Pandoc" : "Checking…";
  if (!s.installed) return "Not installed";
  if (!s.sandboxSupported) {
    return `Installed (${s.version}) — too old for safe import (need 2.15+)`;
  }
  return `Installed (${s.version})`;
});
</script>

<template>
  <!-- Always rendered (no v-if on status): a failed detect_pandoc leaves
       status null, and the error line + Recheck + path override below are the
       exact recovery affordances — hiding the whole card would strand a user
       whose Pandoc probe broke. -->
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
      Document import — Pandoc
    </h2>
    <div class="flex flex-col gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between gap-2">
        <span
          data-testid="pandoc-status"
          class="text-sm text-slate-200"
        >{{ statusLabel }}</span>
        <button
          type="button"
          data-testid="pandoc-recheck"
          class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 hover:bg-white/10 disabled:cursor-default disabled:opacity-50"
          :disabled="saving"
          @click="recheck"
        >
          Recheck
        </button>
      </div>
      <a
        :href="INSTALL_URL"
        rel="noopener noreferrer"
        data-testid="pandoc-install-link"
        class="text-xs text-violet-300 hover:text-violet-200"
        @click.prevent="openInstall"
      >
        Install Pandoc
      </a>
      <div>
        <label
          for="pandoc-path"
          class="mb-1 block text-sm text-slate-200"
        >
          Path override
          <span class="block text-xs text-slate-500">Only needed if Pandoc isn't on PATH</span>
        </label>
        <div class="flex items-center gap-1.5">
          <input
            id="pandoc-path"
            v-model="pathOverride"
            data-testid="pandoc-path-input"
            type="text"
            placeholder="pandoc"
            class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none disabled:cursor-default disabled:opacity-50"
            :disabled="saving"
            @input="dirtied = true"
            @change="savePath"
          >
          <button
            type="button"
            data-testid="pandoc-browse"
            class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 hover:bg-white/10 disabled:cursor-default disabled:opacity-50"
            :disabled="saving"
            @click="browse"
          >
            Browse…
          </button>
        </div>
      </div>
      <p
        v-if="error"
        data-testid="pandoc-error"
        class="text-xs text-rose-400"
      >
        {{ error }}
      </p>
    </div>
  </section>
</template>
