<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { PandocStatus } from "../types";

const status = ref<PandocStatus | null>(null);
const pathOverride = ref("");
const error = ref<string | null>(null);
// Single in-flight guard shared by recheck() and savePath() — mirrors
// McpSettings' one `saving` flag serializing save()/regenerate(): two
// concurrent detect/save calls could otherwise land out of order and leave
// a stale status showing.
const saving = ref(false);

async function detect() {
  try {
    status.value = await invoke<PandocStatus>("detect_pandoc");
  } catch (e) {
    // Not running under Tauri (unit tests) or IPC failure — leave the card
    // empty, same degraded-but-continuing pattern as McpSettings/CaptureSettings.
    error.value = String(e);
    logWarning(`document import settings: detect_pandoc failed: ${String(e)}`);
  }
}

onMounted(async () => {
  await detect();
  // Seed the override field from the resolved status, not a second command.
  pathOverride.value = status.value?.configuredPath ?? "";
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
    const selected = await open({
      multiple: false,
      filters: [{ name: "Pandoc", extensions: ["exe", ""] }],
    });
    if (typeof selected === "string") {
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
  if (!s) return "";
  if (!s.installed) return "Not installed";
  if (!s.sandboxSupported) {
    return `Installed (${s.version}) — too old for safe import (need 2.15+)`;
  }
  return `Installed (${s.version})`;
});
</script>

<template>
  <section v-if="status">
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
