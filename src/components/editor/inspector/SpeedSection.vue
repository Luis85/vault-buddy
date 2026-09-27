<script setup lang="ts">
/**
 * The Inspector's Speed category (Task 31; F-16; visual-parity Task 15,
 * concept spec §5 "Speed"). "Keep the useful pace": a readout of the speed
 * with how long the clip plays on the timeline and how much source it
 * uses, the Clip speed select, "When duration changes" and "Preserve voice
 * pitch where supported". Every change is `setSpeed`, which Rust decides
 * (`commands::layout`): the output duration follows the speed, cues and
 * captions keep their source times, and a slowdown that would run into the
 * next clip is refused with the reason. "Move following clips on this
 * track" makes that room first (`speedRipple.ts`), and a refusal after the
 * move puts the clips back.
 *
 * `setSpeed` takes one clip, so a multi-selection gets a note rather than
 * a silent edit of `clipIds[0]` (R20). A locked track disables the tab; the
 * inspector's frame says why and the fieldset carries the reason.
 */
import { computed } from "vue";

import { useSelectedClips } from "../../../composables/useSelectedClips";
import { runInOrderOrUndo, speedCommands, speedRipple } from "../../../editor/speedRipple";
import { clipOutputDuration } from "../../../editor/timeMap";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { formatMenuTime } from "../menus/menuModel";
import InspectorSection from "./InspectorSection.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
const { lockReason } = useSelectedClips(() => props.clipIds);

/** The concept's speeds; a speed set elsewhere is offered as itself. */
const SPEEDS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];

const clip = computed(() => (props.clipIds.length === 1 ? editorProject.clipById(props.clipIds[0]) : undefined));
const speed = computed(() => clip.value?.speed ?? 1);
/** Unset reads as on: the reference keeps pitch unless told otherwise. */
const preservePitch = computed(() => clip.value?.preserve_pitch ?? true);
const options = computed(() => (SPEEDS.includes(speed.value) ? SPEEDS : [...SPEEDS, speed.value].sort((a, b) => a - b)));

const timeline = computed(() => (clip.value ? clipOutputDuration(clip.value.in_ms, clip.value.out_ms, speed.value) : 0));
const source = computed(() => (clip.value ? clip.value.out_ms - clip.value.in_ms : 0));

function send(value: number, pitch: boolean): Promise<boolean> {
  const c = clip.value;
  const project = editorProject.project;
  if (!c || !project || lockReason.value) return Promise.resolve(false);
  const commands = speedCommands(project, c, value, pitch, speedRipple.value);
  return runInOrderOrUndo((command) => editorProject.execute(command), commands);
}

/** A refused change puts the control back where the projection says. */
async function onSpeed(event: Event): Promise<void> {
  const select = event.target as HTMLSelectElement;
  if (!(await send(Number(select.value), preservePitch.value))) select.value = String(speed.value);
}
async function onPitch(event: Event): Promise<void> {
  const box = event.target as HTMLInputElement;
  if (!(await send(speed.value, box.checked))) box.checked = preservePitch.value;
}
function onRipple(event: Event): void {
  speedRipple.value = (event.target as HTMLSelectElement).value === "leave" ? "leave" : "ripple";
}
</script>

<template>
  <fieldset
    v-if="clip"
    data-testid="speed-section"
    :disabled="lockReason !== null"
    :title="lockReason ?? undefined"
    class="flex min-w-0 flex-col"
  >
    <InspectorSection title="Keep the useful pace">
      <div
        data-testid="speed-section-readout"
        class="mb-2.5 flex items-center gap-[18px] rounded-[10px] bg-accent-bg px-[13px] py-[18px] text-accent-ink"
      >
        <b
          data-testid="speed-section-readout-speed"
          class="text-[35px] leading-none font-[550]"
        >{{ speed }}<small class="ml-[3px] text-[17px]">×</small></b>
        <span class="text-[11px] leading-[1.8]">
          {{ formatMenuTime(timeline) }} on the timeline<br>
          <small class="opacity-80">{{ formatMenuTime(source) }} source duration</small>
        </span>
      </div>
      <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
        Clip speed
        <select
          data-testid="speed-section-speed"
          class="text-[11px] text-fg"
          :value="String(speed)"
          @change="onSpeed"
        >
          <option
            v-for="v in options"
            :key="v"
            :value="String(v)"
          >
            {{ v === 1 ? "1× · original" : `${v}×` }}
          </option>
        </select>
      </label>
      <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
        When duration changes
        <select
          data-testid="speed-section-ripple"
          class="text-[11px] text-fg"
          :value="speedRipple"
          @change="onRipple"
        >
          <option value="ripple">Move following clips on this track</option>
          <option value="leave">Keep following clips in place</option>
        </select>
      </label>
      <label class="flex items-center gap-2 text-[11px] text-fg">
        <input
          data-testid="speed-section-pitch"
          type="checkbox"
          class="shrink-0"
          :checked="preservePitch"
          @change="onPitch"
        >
        Preserve voice pitch where supported
      </label>
      <p
        data-testid="speed-section-help"
        class="text-[10px] leading-[1.6] text-fg-muted"
      >
        Changes the edited clip, not its original. Captions and teaching cues follow the footage. Grouped clips move
        with their group. Monitor playback speed is separate.
      </p>
    </InspectorSection>
  </fieldset>
  <p
    v-else
    data-testid="speed-section-multi"
  >
    Select a single clip to change its speed.
  </p>
</template>
