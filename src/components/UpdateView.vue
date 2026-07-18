<script setup lang="ts">
import { computed, onMounted } from "vue";

import { useUpdatesStore } from "../stores/updates";

const updates = useUpdatesStore();

// The view is reached when an update is available (the announcement's
// landing spot, and the settings "View update →" link) or when an install
// just failed but kept `available` for retry — the same gate the settings
// card uses today.
const showUpdate = computed(
  () =>
    updates.phase === "available" ||
    updates.phase === "installing" ||
    (updates.phase === "error" && updates.available !== null),
);
// Named phase flags keep the template from repeating `updates.phase === …`.
const installing = computed(() => updates.phase === "installing");
const errored = computed(() => updates.phase === "error");

// Release notes come from the signed release feed; render as PLAIN text
// (never v-html, no markdown dependency) — honest and injection-proof.
const releaseNotes = computed(() => updates.available?.body?.trim() ?? "");

// "You're on v0.1.0 · released 2026-07-18" — assembled here so the template
// carries one conditional line instead of a nested date span.
const subhead = computed(() => {
  const current = updates.currentVersion;
  if (!current) return "";
  const date = updates.available?.date;
  return date ? `You're on v${current} · released ${date}` : `You're on v${current}`;
});

onMounted(() => {
  void updates.loadVersion();
});
</script>

<template>
  <div
    v-if="showUpdate && updates.available"
    class="flex flex-col gap-3"
  >
    <div>
      <p class="text-sm font-semibold text-slate-100">
        Version {{ updates.available.version }} is available
      </p>
      <p
        v-if="subhead"
        class="text-xs text-slate-400"
      >
        {{ subhead }}
      </p>
    </div>

    <section>
      <h2
        class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-400"
      >
        What's new
      </h2>
      <pre
        v-if="releaseNotes"
        data-testid="release-notes"
        class="max-h-48 overflow-y-auto whitespace-pre-wrap rounded-xl border border-white/10 bg-white/5 p-2 font-sans text-xs leading-relaxed text-slate-200"
      >{{ releaseNotes }}</pre>
      <p
        v-else
        class="rounded-xl border border-white/10 bg-white/5 p-2 text-xs text-slate-400"
      >
        No release notes provided.
      </p>
    </section>

    <button
      type="button"
      class="cursor-pointer rounded-lg border border-violet-400 bg-violet-500/20 px-3 py-1.5 text-sm text-slate-100 transition-colors hover:bg-violet-500/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
      :disabled="installing"
      data-testid="install-update"
      @click="updates.installUpdate()"
    >
      <span
        v-if="installing"
        class="flex items-center justify-center gap-1.5"
      >
        <span
          class="h-3 w-3 animate-spin rounded-full border-2 border-white/30 border-t-white"
          role="status"
          aria-label="Installing update…"
        />
        Installing…
      </span>
      <span v-else>Install &amp; restart</span>
    </button>

    <p
      v-if="errored"
      data-testid="update-error"
      class="text-xs text-red-300"
    >
      {{ updates.error }}
    </p>
  </div>

  <p
    v-else
    class="text-xs text-slate-400"
  >
    No update is available right now.
  </p>
</template>
