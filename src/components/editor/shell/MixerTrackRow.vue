<script setup lang="ts">
/**
 * One track in the mixer (Task 27): its level, Mute and Solo — each a
 * render-affecting `setTrackFlags` — and whether it reaches the mix at all,
 * read from `mixRules.isTrackAudible` (the preview's own solo rule). A
 * locked track's controls send nothing and say why, matching Rust's
 * `set_track_flags` refusal.
 */
import { computed } from "vue";

import { lockedReason } from "../../../editor/actionMeta";
import { isTrackAudible } from "../../../editor/mixRules";
import type { Track } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import MixerSlider from "./MixerSlider.vue";

const props = defineProps<{ track: Track; tracks: readonly Track[] }>();

/** Mirrors Rust's track volume range `[0,2]`. */
const TRACK_MAX = 2;
const FLAGS = [
  { flag: "muted", label: "Mute" },
  { flag: "solo", label: "Solo" },
] as const;

const editorProject = useEditorProjectStore();

const reason = computed(() => (props.track.locked ? lockedReason(props.track.name) : undefined));
const status = computed(() => {
  if (props.track.muted) return "Muted";
  return isTrackAudible(props.track, props.tracks) ? "" : "Silenced by solo";
});

function commitVolume(volume: number): Promise<boolean> {
  return editorProject.execute({ kind: "setTrackFlags", trackId: props.track.id, volume });
}
function toggle(flag: "muted" | "solo"): void {
  if (props.track.locked) return;
  const patch = flag === "muted" ? { muted: !props.track.muted } : { solo: !props.track.solo };
  void editorProject.execute({ kind: "setTrackFlags", trackId: props.track.id, ...patch });
}
</script>

<template>
  <div
    :data-testid="`mixer-track-${track.id}`"
    class="flex flex-col gap-0.5"
    :title="reason"
  >
    <MixerSlider
      :label="track.name"
      :slider-label="`${track.name} volume`"
      :testid="`mixer-volume-${track.id}`"
      :value="track.volume"
      :max="TRACK_MAX"
      :disabled="track.locked"
      :commit="commitVolume"
    />
    <span class="flex items-center gap-1">
      <button
        v-for="f in FLAGS"
        :key="f.flag"
        type="button"
        :data-testid="`mixer-${f.flag}-${track.id}`"
        class="rounded px-1 hover:bg-white/10"
        :class="track[f.flag] ? 'bg-raised text-fg' : ''"
        :aria-pressed="track[f.flag] ? 'true' : 'false'"
        :aria-disabled="track.locked"
        @click="toggle(f.flag)"
      >
        {{ f.label }}
      </button>
      <span
        :data-testid="`mixer-status-${track.id}`"
        class="text-fg-subtle"
      >{{ status }}</span>
    </span>
  </div>
</template>
