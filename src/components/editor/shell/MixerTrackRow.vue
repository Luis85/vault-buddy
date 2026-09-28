<script setup lang="ts">
/**
 * One track in the mixer (Task 27): its level, Mute and Solo — each a
 * render-affecting `setTrackFlags` — and whether it reaches the mix at all,
 * read from `mixRules.isTrackAudible` (the preview's own solo rule). A
 * locked track's controls send nothing and say why, matching Rust's
 * `set_track_flags` refusal — in its `title` and, since visual-parity
 * Task 22 (D14), in the row's status line too.
 *
 * Visual-parity Task 22 (concept spec §9.8, `.mixer-row`): a
 * `125px 1fr 52px` grid — the name, the M / S toggles (27×26, gold while
 * on) and the track's kind; the range; the mono dB readout.
 */
import { computed } from "vue";

import { lockedReason } from "../../../editor/actionMeta";
import { announceDisabled } from "../../../editor/disabledAnnouncer";
import { isTrackAudible } from "../../../editor/mixRules";
import type { Track } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import MixerSlider from "./MixerSlider.vue";

const props = defineProps<{ track: Track; tracks: readonly Track[] }>();

/** Mirrors Rust's track volume range `[0,2]`. */
const TRACK_MAX = 2;
const FLAGS = [
  { flag: "muted", short: "M", verb: "Mute" },
  { flag: "solo", short: "S", verb: "Solo" },
] as const;

const editorProject = useEditorProjectStore();

const reason = computed(() => (props.track.locked ? lockedReason(props.track.name) : undefined));
const status = computed(() => {
  if (reason.value) return reason.value;
  if (props.track.muted) return "Muted";
  return isTrackAudible(props.track, props.tracks) ? "" : "Silenced by solo";
});

function commitVolume(volume: number): Promise<boolean> {
  return editorProject.execute({ kind: "setTrackFlags", trackId: props.track.id, volume });
}
function toggle(flag: "muted" | "solo"): void {
  if (props.track.locked) return announceDisabled(reason.value);
  const patch = flag === "muted" ? { muted: !props.track.muted } : { solo: !props.track.solo };
  void editorProject.execute({ kind: "setTrackFlags", trackId: props.track.id, ...patch });
}
</script>

<template>
  <div
    :data-testid="`mixer-track-${track.id}`"
    class="grid grid-cols-[125px_1fr_52px] items-center gap-2.5 border-b border-line py-3"
    :title="reason"
  >
    <div class="min-w-0">
      <b class="block truncate text-[11px] font-semibold text-fg">{{ track.name }}</b>
      <span class="mt-1 flex items-center gap-1">
        <button
          v-for="f in FLAGS"
          :key="f.flag"
          type="button"
          :data-testid="`mixer-${f.flag}-${track.id}`"
          class="h-[26px] min-h-[26px] w-[27px] rounded-[7px] border p-0.5 text-[10px]"
          :class="track[f.flag] ? 'border-gold/40 bg-gold-bg text-gold' : 'border-line bg-panel text-fg-secondary'"
          :aria-label="`${f.verb} ${track.name}`"
          :aria-pressed="track[f.flag] ? 'true' : 'false'"
          :aria-disabled="track.locked"
          @click="toggle(f.flag)"
        >
          {{ f.short }}
        </button>
        <span class="text-[10px] text-fg-muted">{{ track.kind }}</span>
      </span>
      <span
        :data-testid="`mixer-status-${track.id}`"
        class="block text-[10px] leading-[1.4] text-fg-muted"
      >{{ status }}</span>
    </div>
    <MixerSlider
      :slider-label="`${track.name} volume`"
      :testid="`mixer-volume-${track.id}`"
      :readout-testid="`mixer-readout-${track.id}`"
      :value="track.volume"
      :max="TRACK_MAX"
      :disabled="track.locked"
      :commit="commitVolume"
    />
  </div>
</template>
