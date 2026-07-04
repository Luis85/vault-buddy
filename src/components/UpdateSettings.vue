<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useUpdatesStore } from "../stores/updates";

const updates = useUpdatesStore();

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
      class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400"
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
          class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
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
        <span class="text-xs text-slate-300">
          Version {{ updates.available?.version }} is available
        </span>
        <button
          type="button"
          class="cursor-pointer rounded-lg border border-violet-400 bg-violet-500/20 px-2 py-0.5 text-xs text-slate-100 transition-colors hover:bg-violet-500/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
          :disabled="updates.phase === 'installing'"
          data-testid="install-update"
          @click="updates.installUpdate()"
        >
          <span
            v-if="updates.phase === 'installing'"
            class="flex items-center gap-1.5"
          >
            <span
              class="h-3 w-3 animate-spin rounded-full border-2 border-white/30 border-t-white"
              role="status"
              aria-label="Installing update…"
            ></span>
            Installing…
          </span>
          <span v-else>Install &amp; restart</span>
        </button>
      </div>
      <p
        v-if="updates.phase === 'error'"
        class="mt-1.5 text-xs text-red-300"
      >
        {{ updates.error }}
      </p>
    </div>
  </section>
</template>
