<script setup lang="ts">
/**
 * The Inspector's Audio category (Task 27; F-05, F-24, F-25). Fills
 * `InspectorPanel`'s `#audio` slot (`EditorRoot.vue`) with a clip's own
 * render-affecting audio: its volume, its mute, and **Detach audio**.
 *
 * **Volume is stored LINEAR and read out in dB** (`AudioVolumeField`, one
 * shared `useInspectorDraft`: a slider drag only moves the draft, the
 * release sends exactly one `setClipMix`).
 *
 * **Mute** acts on the whole selection (`setClipMix` takes `clipIds`), so a
 * multi-selection can be muted at once; volume and Detach need exactly one
 * clip and say so rather than acting on `clipIds[0]` (R20).
 *
 * **Detach audio** (`AudioDetachControl`) reads the SAME `actions.ts`
 * verdict the context menu does, so the two can never disagree.
 *
 * Nothing here is monitoring: the preview's mute and volume live in the
 * transport and the mixer, and never reach a command.
 */
import { computed } from "vue";

import { lockedReason } from "../../../editor/actionMeta";
import { useEditorProjectStore } from "../../../stores/editorProject";
import AudioDetachControl from "./AudioDetachControl.vue";
import AudioVolumeField from "./AudioVolumeField.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();

const clips = computed(() =>
  props.clipIds.map((id) => editorProject.clipById(id)).filter((c) => c !== undefined),
);
const clip = computed(() => (props.clipIds.length === 1 ? clips.value[0] : undefined));

const asset = computed(() => editorProject.project?.assets.find((a) => a.id === clip.value?.asset_id));
const isStill = computed(() => clip.value !== undefined && asset.value?.media_type === "image");

/** The first locked track among the selection's, as Rust words it. */
const lockReason = computed(() => {
  const tracks = editorProject.project?.tracks ?? [];
  const locked = clips.value.map((c) => tracks.find((t) => t.id === c.track_id)).find((t) => t?.locked);
  return locked ? lockedReason(locked.name) : null;
});

const allMuted = computed(() => clips.value.length > 0 && clips.value.every((c) => c.muted));
const muteLabel = computed(() => (allMuted.value ? "Unmute clip audio" : "Mute clip audio"));
function onToggleMute(): void {
  if (lockReason.value || clips.value.length === 0) return;
  void editorProject.execute({ kind: "setClipMix", clipIds: clips.value.map((c) => c.id), muted: !allMuted.value });
}
</script>

<template>
  <div
    data-testid="audio-section"
    class="flex flex-col gap-2"
  >
    <p
      v-if="isStill"
      data-testid="audio-section-still"
    >
      A still image has no sound.
    </p>
    <template v-else>
      <AudioVolumeField
        v-if="clip"
        :clip-id="clip.id"
        :lock-reason="lockReason"
      />
      <p
        v-else
        data-testid="audio-section-multi"
      >
        Select a single clip to set its volume or detach its audio.
      </p>

      <button
        type="button"
        data-testid="audio-section-mute"
        class="cursor-pointer rounded px-1.5 py-0.5 text-left transition-colors hover:bg-white/10"
        :aria-pressed="allMuted ? 'true' : 'false'"
        :aria-disabled="lockReason !== null"
        :title="lockReason ?? 'Mutes the clip in the rendered video'"
        @click="onToggleMute"
      >
        {{ muteLabel }}
      </button>

      <AudioDetachControl
        v-if="clip"
        :asset="asset"
      />
    </template>
  </div>
</template>
