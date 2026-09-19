<script setup lang="ts">
import { computed } from "vue";

import { useNowTicker } from "../composables/useNowTicker";
import { useScreenCaptureStore } from "../stores/screenCapture";
import { formatDuration } from "../utils/formatDuration";
import Chip from "./ui/Chip.vue";
import StatusDot from "./ui/StatusDot.vue";

const store = useScreenCaptureStore();
const now = useNowTicker();

// The store owns the paused-time arithmetic (elapsedMs) so the bar and any
// later reader cannot disagree about what "elapsed" means; the bar only
// supplies the clock. The Rust clock excludes paused time by construction —
// a bar that counted the gap would contradict the file it describes.
const elapsed = computed(() => formatDuration(store.elapsedMs(now.value)));
const label = computed(() =>
  store.paused ? `Paused ${elapsed.value}` : `Recording ${elapsed.value}`,
);
// Pause and Stop both send a control message to the session; while a stop is
// in flight that session is already tearing down, so neither is offered.
const busy = computed(() => store.stopping);
</script>

<template>
  <!-- Dense controls deliberately keep RecordingBar's bespoke button
       treatment rather than AppButton: this bar sits beside it on the same
       view, and AGENTS.md records the compact/themed-button exception to the
       primitive rule precisely so siblings do not drift apart. StatusDot and
       Chip ARE clean drop-ins and are used. -->
  <div
    class="rounded-control px-2 py-1.5"
    :class="store.paused ? 'bg-amber-500/15' : 'bg-red-500/15'"
  >
    <div class="flex items-center gap-2">
      <StatusDot
        tone="recording"
        :pulse="!store.paused"
      />
      <span
        data-testid="screen-elapsed"
        class="flex-1 text-sm font-medium"
        :class="store.paused ? 'text-amber-100' : 'text-red-100'"
        role="status"
      >
        {{ label }}
      </span>
      <Chip
        v-if="store.dropped > 0"
        data-testid="screen-dropped"
        variant="neutral"
        title="Frames the encoder could not keep up with (advisory)"
      >
        {{ store.dropped }} dropped
      </Chip>
      <button
        type="button"
        data-testid="screen-pause"
        class="cursor-pointer rounded-control bg-white/10 px-2 py-1 text-xs font-semibold text-white hover:bg-white/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
        :aria-label="store.paused ? 'Resume screen capture' : 'Pause screen capture'"
        :disabled="busy"
        @click="store.paused ? store.resume() : store.pause()"
      >
        {{ store.paused ? "▶ Resume" : "⏸ Pause" }}
      </button>
      <button
        type="button"
        data-testid="screen-stop"
        class="cursor-pointer rounded-control bg-red-500/80 px-2 py-1 text-xs font-semibold text-white hover:bg-red-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
        aria-label="Stop screen capture"
        :disabled="busy"
        @click="store.stop()"
      >
        ⏹ Stop
      </button>
    </div>
    <p
      v-if="store.sourceTitle"
      data-testid="screen-source"
      class="mt-0.5 truncate text-xs text-fg-muted"
    >
      {{ store.sourceTitle }}
    </p>
    <!-- Spec 14: a vanished source or device warns and the capture finalizes
         cleanly. The store withholds the toast while a capture is live
         because this line exists; without it the warning has nowhere to go. -->
    <p
      v-if="store.warning"
      data-testid="screen-warning"
      class="mt-1 text-xs text-amber-200"
    >
      {{ store.warning }}
    </p>
  </div>
</template>
