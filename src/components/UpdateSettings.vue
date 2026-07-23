<script setup lang="ts">
import { computed, onMounted } from "vue";

import { useSettingsStore } from "../stores/settings";
import { useUpdatesStore } from "../stores/updates";
import { useVaultsStore } from "../stores/vaults";

const updates = useUpdatesStore();
const settings = useSettingsStore();
const vaults = useVaultsStore();

// a failed download/install keeps `available` for retry — the install
// button must stay visible alongside the error, not vanish behind it
const showInstall = computed(
  () =>
    updates.phase === "available" ||
    updates.phase === "installing" ||
    (updates.phase === "error" && updates.available !== null),
);

onMounted(() => {
  void updates.loadVersion();
});
</script>

<template>
  <section>
    <h2
      class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted"
    >
      Updates
    </h2>
    <div class="rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between gap-2">
        <span class="text-sm text-slate-200">
          Version {{ updates.currentVersion || "…" }}
        </span>
        <button
          type="button"
          class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
          :disabled="updates.phase === 'checking' || updates.phase === 'installing'"
          data-testid="check-updates"
          @click="updates.checkForUpdates()"
        >
          {{ updates.phase === "checking" ? "Checking…" : "Check for updates" }}
        </button>
      </div>
      <p
        v-if="updates.phase === 'upToDate'"
        class="mt-1.5 text-xs text-emerald-300"
      >
        You're up to date.
      </p>
      <div
        v-else-if="showInstall"
        class="mt-1.5 flex items-center justify-between gap-2"
      >
        <span class="text-xs text-fg-secondary">
          Version {{ updates.available?.version }} is available
        </span>
        <button
          type="button"
          class="cursor-pointer rounded-control border border-violet-400 bg-violet-500/20 px-2 py-0.5 text-xs text-fg transition-colors hover:bg-violet-500/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          data-testid="view-update"
          @click="vaults.openUpdate()"
        >
          View update →
        </button>
      </div>
      <p
        v-if="updates.phase === 'error'"
        class="mt-1.5 text-xs text-red-300"
      >
        {{ updates.error }}
      </p>
      <div class="mt-1.5 flex items-center justify-between border-t border-white/10 pt-1.5">
        <label
          for="update-on-start-toggle"
          class="text-sm text-slate-200"
        >
          Check on startup
          <span class="block text-xs text-fg-subtle">
            Asks before installing · silent when up to date
          </span>
        </label>
        <input
          id="update-on-start-toggle"
          data-testid="update-on-start-toggle"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="settings.checkUpdatesOnStart"
          @change="settings.toggleCheckUpdatesOnStart()"
        >
      </div>
    </div>
  </section>
</template>
