<script setup lang="ts">
import { computed } from "vue";

import { useExternalTool } from "../composables/useExternalTool";
import { usePandocStore } from "../stores/pandoc";
import type { PandocStatus } from "../types";
import Field from "./ui/Field.vue";

// The document-import toolchain card. The probe / Recheck / override / Browse
// machinery — the monotonic detect ticket, the shared in-flight guard, the
// probe token claimed at START so a stale settings probe can't clobber a
// newer intake result (Codex P2), the dirtied-field rule Browse depends on —
// all live in `useExternalTool`, which the ffmpeg card consumes too. It was
// extracted when that second card appeared, not before: one implementation of
// a thing is not a pattern.

const INSTALL_URL = "https://pandoc.org/installing.html";

const tool = useExternalTool<PandocStatus>({
  detectCommand: "detect_pandoc",
  setPathCommand: "set_pandoc_path",
  setPathArg: "pandocPath",
  filterName: "Pandoc",
  installUrl: INSTALL_URL,
  label: "document import settings",
  store: usePandocStore(),
});
const { status, pathOverride, error, saving } = tool;

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
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted">
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
          class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary hover:bg-white/10 disabled:cursor-default disabled:opacity-50"
          :disabled="saving"
          @click="tool.recheck"
        >
          Recheck
        </button>
      </div>
      <a
        :href="INSTALL_URL"
        rel="noopener noreferrer"
        data-testid="pandoc-install-link"
        class="text-xs text-violet-300 hover:text-accent-fg"
        @click.prevent="tool.openInstall"
      >
        Install Pandoc
      </a>
      <div>
        <label
          for="pandoc-path"
          class="mb-1 block text-sm text-slate-200"
        >
          Path override
          <span class="block text-xs text-fg-subtle">Only needed if Pandoc isn't on PATH</span>
        </label>
        <div class="flex items-center gap-1.5">
          <Field
            id="pandoc-path"
            v-model="pathOverride"
            data-testid="pandoc-path-input"
            type="text"
            placeholder="pandoc"
            class="disabled:cursor-default disabled:opacity-50"
            :disabled="saving"
            @input="tool.markDirty"
            @change="tool.savePath"
          />
          <button
            type="button"
            data-testid="pandoc-browse"
            class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary hover:bg-white/10 disabled:cursor-default disabled:opacity-50"
            :disabled="saving"
            @click="tool.browse"
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
