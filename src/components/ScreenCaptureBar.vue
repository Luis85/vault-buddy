<script setup lang="ts">
import { computed } from "vue";

import { useNowTicker } from "../composables/useNowTicker";
import { useScreenCaptureStore } from "../stores/screenCapture";
import { formatDuration } from "../utils/formatDuration";
import Chip from "./ui/Chip.vue";

const store = useScreenCaptureStore();
const now = useNowTicker();

// The store owns the paused-time arithmetic (elapsedMs) so the bar and any
// later reader cannot disagree about what "elapsed" means; the bar only
// supplies the clock. The Rust clock excludes paused time by construction —
// a bar that counted the gap would contradict the file it describes.
const elapsed = computed(() => formatDuration(store.elapsedMs(now.value)));
// The saving arm is not cosmetic: `status` stays `capturing` until
// `screen:stopped` lands and the ticker keeps ticking, so without it the bar
// counts on through the whole finalize window — bounded at 30 s by
// `STOP_TIMEOUT` (screen_commands.rs), which answers `stillSaving` on expiry
// — and claims seconds the file does not contain, exactly what the
// paused-time arithmetic above exists to prevent. `RecordingBar` answers
// "Saving…" here for the same reason. Coverage matches the sibling's: a
// tray-driven stop leaves `stopping` false in both domains, since each store
// sets its saving state only from its own `stop()`.
const label = computed(() => {
  if (store.stopping) return "Saving…";
  return store.paused ? `Paused ${elapsed.value}` : `Recording ${elapsed.value}`;
});
// Pause and Stop both send a control message to the session; while a stop is
// in flight that session is already tearing down, so neither is offered.
const busy = computed(() => store.stopping);
// The dot's paused/recording tone, resolved here rather than inline: the
// template is a complexity-gated surface (the quality ratchet counts every
// branch in it) and this keeps the bar's markup at the branch count it had
// while the tone still matches RecordingBar's dot exactly.
const dotTone = computed(() =>
  store.paused ? "bg-amber-400" : "animate-pulse bg-recording",
);
</script>

<template>
  <!-- Dense controls deliberately keep RecordingBar's bespoke button
       treatment rather than AppButton: this bar sits beside it on the same
       view, and AGENTS.md records the compact/themed-button exception to the
       primitive rule precisely so siblings do not drift apart. The status dot
       is bespoke for the SAME reason and is not an oversight: `StatusDot` is
       h-1.5 w-1.5 and has no amber tone, so it would render a smaller dot
       that stays red while paused, next to an audio bar whose dot is larger
       and turns amber — visible drift between two bars on one view. Widening
       the shared primitive for one call site would instead resize every
       existing dot (VaultList, TaskRow). `Chip` IS a clean drop-in and is
       used. -->
  <div
    class="rounded-control px-2 py-1.5"
    :class="store.paused ? 'bg-amber-500/15' : 'bg-red-500/15'"
  >
    <div class="flex items-center gap-2">
      <span
        data-testid="screen-dot"
        class="h-2.5 w-2.5 shrink-0 rounded-full"
        :class="dotTone"
        aria-hidden="true"
      />
      <!-- The ticking value sits INSIDE this live region, unlike
           ImportProgress (whose region carries a static message and whose
           tick is pure garnish, so AGENTS.md records the opposite call
           there). Deliberate here: the state word the region exists to
           announce — Recording / Paused / Saving… — is part of the same
           label, and this bar renders beside RecordingBar, which announces
           exactly this way. Two live regions behaving differently on one
           view is worse than the per-second chatter; if that chatter is
           fixed, both bars move together. -->
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
         because this line exists; without it a warning raised MID-capture has
         nowhere to go (a terminal one still rides Rust's stop toast). -->
    <p
      v-if="store.warning"
      data-testid="screen-warning"
      class="mt-1 text-xs text-amber-200"
    >
      {{ store.warning }}
    </p>
  </div>
</template>
