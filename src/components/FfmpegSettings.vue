<script setup lang="ts">
import { computed } from "vue";

import { useExternalTool } from "../composables/useExternalTool";
import { useFfmpegStore } from "../stores/ffmpeg";
import type { FfmpegStatus } from "../types";
import Field from "./ui/Field.vue";

// The editor render's toolchain card (the retired phase-5 export's before
// Task 59) — a render's own refusal names ffmpeg and where to set it up, so
// it lives in Buddy settings →
// Integrations beside the Pandoc one: two user-installed, never-bundled
// external tools, one place to set either up. Before it existed the refusal
// pointed at a screen that did not exist (docs/Gaps.md GAP-144).
//
// The probe / Recheck / override / Browse machinery is the shared
// `useExternalTool` composable, which the Pandoc card consumes too — the same
// move `ffmpeg.rs` makes against `external_tool.rs` one layer down. Only the
// status wording below is this tool's own, and it has to be: ffmpeg's
// capability axis (can this build encode H.264?) has no Pandoc counterpart.

const INSTALL_URL = "https://ffmpeg.org/download.html";

const tool = useExternalTool<FfmpegStatus>({
  detectCommand: "detect_ffmpeg",
  setPathCommand: "set_ffmpeg_path",
  setPathArg: "ffmpegPath",
  filterName: "ffmpeg",
  installUrl: INSTALL_URL,
  label: "ffmpeg settings",
  store: useFfmpegStore(),
});
const { status, pathOverride, error, saving } = tool;

const statusLabel = computed(() => {
  const s = status.value;
  // No status yet: distinguish "still probing" from "the probe failed" — the
  // latter must stay visible so the recovery affordances aren't hidden exactly
  // when detection breaks.
  if (!s) return error.value ? "Couldn't detect ffmpeg" : "Checking…";
  if (!s.installed) return "Not installed";
  const version = s.version ?? "unknown version";
  // A build with no H.264 encoder is NOT simply "installed": it can remux an
  // untouched capture but cannot save an EDITED one, and calling it installed
  // would send the user back into the same late failure this card exists to
  // prevent.
  if (!s.h264Encoder) {
    return `Installed (${version}) — no H.264 encoder, so edited captures can't be saved`;
  }
  return `Installed (${version})`;
});
</script>

<template>
  <!-- Always rendered (no v-if on status): a failed detect_ffmpeg leaves
       status null, and the error line + Recheck + path override below are the
       recovery affordances. -->
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted">
      Screen capture — ffmpeg
    </h2>
    <div class="flex flex-col gap-2 rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-start justify-between gap-2">
        <span
          data-testid="ffmpeg-status"
          class="min-w-0 break-words text-sm text-slate-200"
        >{{ statusLabel }}</span>
        <button
          type="button"
          data-testid="ffmpeg-recheck"
          class="shrink-0 cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary hover:bg-white/10 disabled:cursor-default disabled:opacity-50"
          :disabled="saving"
          @click="tool.recheck"
        >
          Recheck
        </button>
      </div>
      <!-- Says what is and is not affected, so a user who reads this card
           after the Record Screen notice gets the same two halves. -->
      <p class="text-xs text-fg-subtle">
        Only saving a screen capture into a vault needs ffmpeg — recording and
        editing work without it.
      </p>
      <a
        :href="INSTALL_URL"
        rel="noopener noreferrer"
        data-testid="ffmpeg-install-link"
        class="text-xs text-violet-300 hover:text-accent-fg"
        @click.prevent="tool.openInstall"
      >
        Install ffmpeg
      </a>
      <div>
        <label
          for="ffmpeg-path"
          class="mb-1 block text-sm text-slate-200"
        >
          Path override
          <span class="block text-xs text-fg-subtle">Only needed if ffmpeg isn't on PATH</span>
        </label>
        <div class="flex items-center gap-1.5">
          <Field
            id="ffmpeg-path"
            v-model="pathOverride"
            data-testid="ffmpeg-path-input"
            type="text"
            placeholder="ffmpeg"
            class="disabled:cursor-default disabled:opacity-50"
            :disabled="saving"
            @input="tool.markDirty"
            @change="tool.savePath"
          />
          <button
            type="button"
            data-testid="ffmpeg-browse"
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
        data-testid="ffmpeg-error"
        class="text-xs text-rose-400"
      >
        {{ error }}
      </p>
    </div>
  </section>
</template>
