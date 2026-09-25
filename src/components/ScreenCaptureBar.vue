<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed } from "vue";

import { useNowTicker } from "../composables/useNowTicker";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useScreenCaptureStore } from "../stores/screenCapture";
import { formatDuration } from "../utils/formatDuration";
import Chip from "./ui/Chip.vue";

const store = useScreenCaptureStore();
const notifications = useNotificationsStore();
/** The finished capture this bar is offering, or `null` while one is
 * running.
 *
 * Gated on `status` as well as on `lastStaged`, because the two are not
 * redundant: `start()` clears the staged file only AFTER the status has gone
 * live, and a stale one surviving any path into `capturing` would offer Edit
 * on a file while another capture is being written. Conversely `lastStaged`
 * alone is what makes this a FINISHED capture rather than a bar with nothing
 * to point at — `applyStopped` resets to idle and then parks it, in that
 * order, so idle-with-nothing-staged is the state nobody should see this bar
 * in at all (`ActionPanel` does not render it there). */
const staged = computed(() => (store.status === "idle" ? store.lastStaged : null));

// Declared AFTER `staged` on purpose: `watch` evaluates its source getter
// once at creation, so reading `staged` from above it is a temporal-dead-zone
// ReferenceError, not a lazy closure.
//
// Gated on there being a clock to drive at all. This bar stays mounted for
// the rest of the process once a capture has finished (the staged row below),
// and the panel window is hidden rather than unmounted, so an ungated ticker
// would run at 1 Hz forever to update a value the finished row renders
// nowhere.
const now = useNowTicker(() => staged.value === null);

/** The quick way into the editor for the capture that just finished
 * (`StagedCaptureList` in the Record Screen picker reaches every staged
 * capture). Since Task 59 the editor's Render + Publish is the only way a
 * capture reaches a vault, which is what the button's label says. */
async function openEditor() {
  const capture = staged.value;
  if (capture === null) return;
  try {
    await invoke("open_capture_editor", { base: capture.base });
  } catch (e) {
    // A refusal here is `is_safe_base` rejecting the name or the editor
    // window being missing — both of which leave the click doing nothing
    // visible, which AGENTS.md's diagnostics invariant does not allow.
    logWarning(`open_capture_editor failed: ${String(e)}`);
    notifications.error(String(e));
  }
}

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
// The wash follows the same three states the row does. A finished capture
// keeping the recording red would say a capture is running, which is the
// falsehood the whole `staged` branch exists to retire.
const tone = computed(() => {
  if (staged.value) return "bg-white/5";
  return store.paused ? "bg-amber-500/15" : "bg-red-500/15";
});
// `applyStopped` calls `reset()`, which nulls the store's `sourceTitle`, so
// the finished row would identify the capture by its base name alone — even
// though the staged payload carries the very title the live row rendered a
// second earlier. The staged capture's own copy wins where there is one.
const sourceTitle = computed(() => staged.value?.sourceTitle ?? store.sourceTitle);
// Spec 14: a vanished source or device warns and the capture finalizes
// cleanly. The store withholds the TOAST while a capture is live precisely
// because this bar shows the warning inline — but since phase 4 the bar
// outlives the capture, so an ungated line rendered the same warning twice:
// a toast AND a sticky line nothing clears until the next capture starts.
// Resolved here rather than as `store.warning && !staged` in the markup for
// the reason `dotTone` and `tone` above are: this template is a
// complexity-gated surface and the ratchet counts every branch in it.
const warning = computed(() => (staged.value ? null : store.warning));
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
    :class="tone"
  >
    <!-- The finished state, which is NOT the live one wearing a different
         label: a staged capture has no session left, so Pause and Stop would
         address nothing. It replaces the row rather than joining it. -->
    <div
      v-if="staged"
      data-testid="screen-ready"
      class="flex items-center gap-2"
    >
      <span class="flex-1 truncate text-sm text-fg-secondary">
        Recorded {{ staged.base }}
      </span>
      <button
        type="button"
        data-testid="screen-edit"
        class="cursor-pointer rounded-control bg-white/10 px-2 py-1 text-xs font-semibold text-white hover:bg-white/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        aria-label="Edit to render and publish"
        title="Edit to render and publish"
        @click="openEditor"
      >
        Edit
      </button>
    </div>
    <div
      v-else
      class="flex items-center gap-2"
    >
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
      v-if="sourceTitle"
      data-testid="screen-source"
      class="mt-0.5 truncate text-xs text-fg-muted"
    >
      {{ sourceTitle }}
    </p>
    <!-- Spec 14: a vanished source or device warns and the capture finalizes
         cleanly. The store withholds the toast while a capture is live
         because this line exists; without it a warning raised MID-capture has
         nowhere to go (a terminal one still rides Rust's stop toast).
         `warning` is the LIVE-only view of it — see the computed. -->
    <p
      v-if="warning"
      data-testid="screen-warning"
      class="mt-1 text-xs text-amber-200"
    >
      {{ warning }}
    </p>
  </div>
</template>
