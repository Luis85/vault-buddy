<script setup lang="ts">
/**
 * The editor's export surface (spec §8.3, §10): Save into a vault, Discard,
 * the progress readout, and the Open that follows a successful save.
 *
 * PRESENTATIONAL — no `invoke`, no store, no event listener. Every decision
 * here is a function of the props, and `EditorRoot` owns the commands and
 * the four `screen:export*` events. That split is what keeps `EditorRoot`
 * under the frontend LOC cap, and it follows how `ScreenRegionPicker` and
 * `ScreenAudioPicker` were extracted from `ScreenSourcePicker`.
 *
 * `failed` deliberately renders the SAME verbs as `idle`. An export failure
 * is recoverable — the staged capture and its timeline are untouched, so the
 * remedy is to fix the cause and press Save again — and hiding Save the way
 * `done` does would strand the recording with Discard as its only exit.
 */
import { computed, ref, watch } from "vue";

import AppButton from "../ui/AppButton.vue";
import Banner from "../ui/Banner.vue";

const props = defineProps<{
  phase: "idle" | "exporting" | "done" | "failed";
  /** 0..1, never 0..100 — `screen:exportProgress` carries the fraction. */
  fraction: number;
  /** The failure message, the success line, or nothing at all. A CANCEL is
   * not a failure (spec §14), so it arrives here as `null`. */
  message: string | null;
  /** False when the timeline holds no footage. A UI hint only — the export
   * refuses the same case server-side. */
  canSave: boolean;
  /** A discard is in flight. */
  busy: boolean;
}>();
defineEmits<{ save: []; discard: []; cancel: []; open: [] }>();

/** ONE number behind both the announced value and the drawn width.
 *
 * Clamped rather than trusted: the fraction crosses IPC, and a bar that
 * draws `width: 4000%` from a mis-scaled payload is a layout bug on top of
 * a data bug. Rounded once, here, so the percent a screen reader announces
 * and the percent the user sees can never be one apart. */
const percent = computed(() => Math.round(Math.min(1, Math.max(0, props.fraction)) * 100));

/** A success line and a failure share one slot, and they must not share a
 * colour: "Saved" in danger red reads as an error at a glance. */
const messageTone = computed(() => (props.phase === "failed" ? "danger" : "success"));

/** Discard destroys the ONLY copy of a recording (spec §10: "Discard is
 * confirm-gated, being irreversible. Nothing is ever deleted silently").
 *
 * A two-step in-component confirm, the `TaskSectionMenu` delete precedent,
 * rather than a native dialog: a native dialog steals OS focus, and
 * `DIALOG_ACTIVE` is a process-wide bool that already has two drivers
 * (docs/Gaps.md GAP-128). */
const armed = ref(false);
/** An armed button that cannot be disarmed is a trap — the next stray click
 * deletes the recording — so the confirm always comes with a way out, and it
 * also drops whenever the bar changes state underneath it. */
watch(
  () => props.phase,
  () => {
    armed.value = false;
  },
);
</script>

<template>
  <div class="flex shrink-0 flex-col gap-2">
    <Banner
      v-if="message"
      data-testid="export-message"
      :tone="messageTone"
    >
      {{ message }}
    </Banner>
    <!-- ffmpeg reports progress as a percentage of the OUTPUT it has
         written, so this really does advance; the indeterminate treatment
         `ImportProgress` uses for Pandoc would understate what is known. -->
    <div
      v-if="phase === 'exporting'"
      data-testid="export-progress"
      role="progressbar"
      aria-label="Export progress"
      aria-valuemin="0"
      aria-valuemax="100"
      :aria-valuenow="percent"
      class="h-1.5 w-full overflow-hidden rounded-control bg-white/10"
    >
      <div
        data-testid="export-progress-fill"
        class="h-full bg-accent transition-[width]"
        :style="{ width: `${percent}%` }"
      />
    </div>
    <div class="flex items-center gap-2">
      <template v-if="phase === 'exporting'">
        <span class="flex-1 truncate text-micro text-fg-subtle">
          Saving into your vault… {{ percent }}%
        </span>
        <AppButton
          data-testid="export-cancel"
          variant="secondary"
          @click="$emit('cancel')"
        >
          Cancel
        </AppButton>
      </template>
      <!-- The capture is out of staging now, so Save would export a base
           whose sidecar no longer exists and Discard would delete nothing. -->
      <template v-else-if="phase === 'done'">
        <AppButton
          data-testid="export-open"
          variant="secondary"
          @click="$emit('open')"
        >
          Open in Obsidian
        </AppButton>
      </template>
      <template v-else>
        <AppButton
          data-testid="export-save"
          :disabled="!canSave || busy"
          @click="$emit('save')"
        >
          Save to vault
        </AppButton>
        <AppButton
          data-testid="export-discard"
          variant="danger"
          :disabled="busy"
          @click="armed ? $emit('discard') : (armed = true)"
        >
          {{ armed ? "Delete this recording" : "Discard" }}
        </AppButton>
        <AppButton
          v-if="armed"
          data-testid="export-discard-keep"
          variant="ghost"
          @click="armed = false"
        >
          Keep it
        </AppButton>
      </template>
    </div>
  </div>
</template>
