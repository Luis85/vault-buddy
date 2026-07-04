<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";

// Reveal the folder holding vault-buddy.log + crash.log so the user can
// attach logs after a crash. Guarded: unit tests run without a Tauri runtime.
function openLogs() {
  void invoke("open_logs_folder").catch(() => {
    // not running under Tauri (unit tests) — nothing to open
  });
}
</script>

<template>
  <section>
    <h2
      class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400"
    >
      Diagnostics
    </h2>
    <div class="rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between gap-2">
        <span class="text-sm text-slate-200">Logs</span>
        <button
          type="button"
          class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          data-testid="open-logs"
          @click="openLogs"
        >
          Open logs folder
        </button>
      </div>
      <p class="mt-1.5 text-xs text-slate-500">
        Share these if the buddy crashes.
      </p>
    </div>
  </section>
</template>
