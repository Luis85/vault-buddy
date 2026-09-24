<script setup lang="ts">
/**
 * The audio mixer (Task 27; F-05, F-25; onboarding step 14's
 * `[data-action="mixer"]` target) — per-track volume/mute/solo, the master
 * gain, a monitoring-only mute, and the preview's sample-peak meter, in a
 * popover opened from the transport row, beside the preview speaker.
 *
 * **Two kinds of control, deliberately worded apart** (USER-GUIDE.md: "The
 * speaker beside Play mutes your monitoring only; clip and track mute
 * affect the video output"): every track control and the master gain is a
 * RENDER-AFFECTING edit (`setTrackFlags` / `setMasterGain`, one command
 * each), while "Mute preview (does not affect the video)" is the
 * workspace's `monitor_muted` — the SAME flag the transport's speaker
 * toggles — and never reaches `editorProject.execute`.
 *
 * **Solo** reads `mixRules.isTrackAudible`, the one copy of Rust's
 * documented rule (`commands::tracks`) the preview's monitoring also uses,
 * so "Silenced by solo" here is exactly the set the preview silences.
 *
 * **Sliders commit once** (`MixerSlider`): dragging only moves the
 * readout; the release sends one command, and a refused value snaps back.
 *
 * **The meter is a PEAK** (`MixerPeakMeter`): the preview controller's
 * sample peak, polled only while the popover is open.
 *
 * Split into `MixerTrackRow`/`MixerSlider`/`MixerPeakMeter` so no one
 * template carries every branch (the fallow template-complexity ratchet).
 */
import { computed, ref } from "vue";

import { onReveal } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import MixerPeakMeter from "./MixerPeakMeter.vue";
import MixerSlider from "./MixerSlider.vue";
import MixerTrackRow from "./MixerTrackRow.vue";

defineProps<{
  /** The preview's current sample peak (linear), or `null` when nothing
   * is measured. Absent: no meter at all. */
  readPeak?: () => number | null;
}>();

/** Mirrors Rust's master gain range `[0,1]`. */
const MASTER_MAX = 1;

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const open = ref(false);
const triggerRef = ref<HTMLButtonElement | null>(null);
const tracks = computed(() => editorProject.project?.tracks ?? []);
const masterGain = computed(() => editorProject.project?.master_gain ?? 1);

function commitMaster(gain: number): Promise<boolean> {
  return editorProject.execute({ kind: "setMasterGain", gain });
}

function close(): void {
  open.value = false;
  triggerRef.value?.focus();
}

/** Task 54: a muted-or-clipping finding's "Open the mixer". */
onReveal("mixer", () => {
  open.value = true;
});
function onKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  event.stopPropagation();
  close();
}
</script>

<template>
  <span class="relative">
    <button
      ref="triggerRef"
      type="button"
      data-action="mixer"
      data-testid="mixer-toggle"
      class="rounded-control border border-line px-2 py-0.5 hover:bg-panel focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :aria-expanded="open ? 'true' : 'false'"
      aria-controls="editor-mixer"
      @click="open = !open"
    >
      Audio mixer
    </button>
    <div
      v-if="open"
      id="editor-mixer"
      role="dialog"
      aria-label="Audio mixer"
      data-testid="mixer-popover"
      class="absolute bottom-full right-0 z-30 mb-1 flex w-72 flex-col gap-2 rounded-control border border-line bg-panel p-2 text-micro text-fg-muted shadow-lg"
      @keydown="onKeydown"
    >
      <p class="text-fg-subtle">
        Track and master levels change the rendered video.
      </p>
      <MixerTrackRow
        v-for="track in tracks"
        :key="track.id"
        :track="track"
        :tracks="tracks"
      />
      <div class="border-t border-line pt-2">
        <MixerSlider
          label="Master"
          slider-label="Master gain"
          testid="mixer-master"
          :value="masterGain"
          :max="MASTER_MAX"
          :commit="commitMaster"
        />
      </div>
      <label class="flex items-center gap-1 border-t border-line pt-2">
        <input
          data-testid="mixer-monitor-mute"
          type="checkbox"
          class="accent-violet-500"
          :checked="workspace.monitorMuted"
          @change="workspace.toggleMonitorMute()"
        >
        Mute preview (does not affect the video)
      </label>
      <MixerPeakMeter
        v-if="readPeak"
        :read-peak="readPeak"
      />
      <button
        type="button"
        data-testid="mixer-close"
        class="self-end rounded px-1.5 py-0.5 hover:bg-white/10"
        @click="close"
      >
        Close
      </button>
    </div>
  </span>
</template>
