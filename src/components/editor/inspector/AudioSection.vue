<script setup lang="ts">
/**
 * The Inspector's Audio category (Task 27; F-05, F-24, F-25; visual-parity
 * Task 15, concept spec §5 "Audio"). **Clip audio**: the clip's own
 * render-affecting Volume (a 0–200 % range over the LINEAR `Clip.volume`,
 * `[0,2]`; a drag commits once, on release), Mute this clip (over the whole
 * selection, `setClipMix` takes `clipIds`), Detach source audio
 * (`AudioDetachControl`, the registry's own verdict) and the stems note
 * (`AudioStemsNote`). **Mix**: Open audio mixer, the same reveal View →
 * Audio mixer… makes.
 *
 * Volume needs exactly one clip and says so rather than acting on
 * `clipIds[0]` (R20). A locked track disables the tab; the inspector's
 * frame says why and the fieldset carries the reason. Nothing here is
 * monitoring: the preview's mute lives in the transport and the mixer, and
 * never reaches a command.
 */
import { computed } from "vue";

import { useSelectedClips } from "../../../composables/useSelectedClips";
import { requestReveal } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import AudioDetachControl from "./AudioDetachControl.vue";
import AudioStemsNote from "./AudioStemsNote.vue";
import InspectorButton from "./InspectorButton.vue";
import InspectorRange from "./InspectorRange.vue";
import InspectorSection from "./InspectorSection.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();

const { clips, lockReason } = useSelectedClips(() => props.clipIds);
const clip = computed(() => (props.clipIds.length === 1 ? clips.value[0] : undefined));

const asset = computed(() => editorProject.project?.assets.find((a) => a.id === clip.value?.asset_id));
const isStill = computed(() => clip.value !== undefined && asset.value?.media_type === "image");

const allMuted = computed(() => clips.value.length > 0 && clips.value.every((c) => c.muted));

function mix(change: { volume?: number; muted?: boolean }): Promise<boolean> {
  if (lockReason.value || clips.value.length === 0) return Promise.resolve(false);
  return editorProject.execute({ kind: "setClipMix", clipIds: clips.value.map((c) => c.id), ...change });
}
/** A refused mute puts the box back where the projection says it is. */
async function onMute(event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  if (!(await mix({ muted: box.checked }))) box.checked = allMuted.value;
}
</script>

<template>
  <!-- The mixer is not an edit, so Mix stays outside the locked fieldset. -->
  <div class="flex min-w-0 flex-col">
    <fieldset
      data-testid="audio-section"
      :disabled="lockReason !== null"
      :title="lockReason ?? undefined"
      class="flex min-w-0 flex-col"
    >
      <InspectorSection title="Clip audio">
        <p
          v-if="isStill"
          data-testid="audio-section-still"
        >
          A still image has no sound.
        </p>
        <template v-else>
          <InspectorRange
            v-if="clip"
            label="Volume"
            testid="audio-section-volume"
            :value="Math.round(clip.volume * 100)"
            :min="0"
            :max="200"
            :step="1"
            suffix="%"
            :commit="(v) => mix({ volume: v / 100 })"
          />
          <p
            v-else
            data-testid="audio-section-multi"
          >
            Select a single clip to set its volume or detach its audio.
          </p>
          <label class="flex items-center gap-2 text-[11px] text-fg">
            <input
              data-testid="audio-section-mute"
              type="checkbox"
              class="shrink-0"
              :checked="allMuted"
              @change="onMute"
            >
            Mute this clip
          </label>
          <p class="text-[10px] leading-[1.6] text-fg-muted">
            Track and master levels are applied after the clip level.
          </p>
          <AudioDetachControl
            v-if="clip"
            :asset="asset"
          />
          <AudioStemsNote :asset="asset" />
        </template>
      </InspectorSection>
    </fieldset>
    <InspectorSection title="Mix">
      <InspectorButton
        class="self-start"
        icon="sliders"
        data-testid="audio-section-mixer"
        @click="requestReveal('mixer')"
      >
        Open audio mixer
      </InspectorButton>
      <p class="text-[10px] leading-[1.6] text-fg-muted">
        Monitoring mute does not mute exports. Track mute and solo do affect exports.
      </p>
    </InspectorSection>
  </div>
</template>
