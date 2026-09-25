<script setup lang="ts">
/**
 * The Inspector's Speed category (Task 31; F-16; SCREENS-AND-INTERACTIONS.md
 * §04). Presets (0.5×, 1×, 1.5×, 2×), a numeric speed, and whether pitch is
 * preserved — each one `setSpeed`, which Rust decides (`commands::layout`):
 * the output duration follows the speed, cues keep their source times, and
 * a slowdown that would run into the next clip is refused with the reason
 * (nothing moves to make room). The line under the controls says how long
 * the clip plays for, so the effect of a change is visible before and
 * after.
 *
 * `setSpeed` takes one clip, so a multi-selection gets a note rather than
 * a silent edit of `clipIds[0]` (R20), the Fades/Audio precedent.
 */
import { computed } from "vue";

import { numberField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { useSelectedClips } from "../../../composables/useSelectedClips";
import { clipOutputDuration } from "../../../editor/timeMap";
import { useEditorProjectStore } from "../../../stores/editorProject";
import InspectorNumberInput from "./InspectorNumberInput.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();

/** Mirrors `core::editor::limits::{SPEED_MIN, SPEED_MAX}` (read from the
 * Rust source, `useInspectorDraft.ts`'s rule). */
const SPEED_MIN = 0.25;
const SPEED_MAX = 4;
const PRESETS = [0.5, 1, 1.5, 2] as const;

const clip = computed(() => (props.clipIds.length === 1 ? editorProject.clipById(props.clipIds[0]) : undefined));
const speed = computed(() => clip.value?.speed ?? 1);
/** Unset reads as on: the reference keeps pitch unless told otherwise. */
const preservePitch = computed(() => clip.value?.preserve_pitch ?? true);

const { lockReason } = useSelectedClips(() => props.clipIds);

const playsForSeconds = computed(() => {
  const c = clip.value;
  return c ? (clipOutputDuration(c.in_ms, c.out_ms, speed.value) / 1000).toFixed(2) : "0";
});

function send(value: number, pitch: boolean): Promise<boolean> {
  const c = clip.value;
  if (!c || lockReason.value) return Promise.resolve(false);
  return editorProject.execute({ kind: "setSpeed", clipId: c.id, speed: value, preservePitch: pitch });
}

const speedField = useInspectorDraft(
  numberField({
    value: () => speed.value,
    label: "Speed",
    min: SPEED_MIN,
    max: SPEED_MAX,
    rangeLabel: `${SPEED_MIN}× and ${SPEED_MAX}×`,
  }),
  (v) => send(v, preservePitch.value),
);

function onPreset(value: number): void {
  if (value !== speed.value) void send(value, preservePitch.value);
}

function onPitch(event: Event): void {
  void send(speed.value, (event.target as HTMLInputElement).checked);
}
</script>

<template>
  <div
    v-if="clip"
    data-testid="speed-section"
    class="flex flex-col gap-2"
  >
    <p
      v-if="lockReason"
      data-testid="speed-section-locked"
    >
      {{ lockReason }} — unlock it to change this clip's speed.
    </p>
    <div
      class="flex flex-wrap gap-1"
      role="group"
      aria-label="Speed presets"
    >
      <button
        v-for="p in PRESETS"
        :key="p"
        type="button"
        :data-testid="`speed-preset-${p}`"
        :aria-pressed="p === speed"
        :aria-label="`${p}× speed`"
        :disabled="lockReason !== null"
        :title="lockReason ?? `Play at ${p}×`"
        class="cursor-pointer rounded border border-line px-1.5 py-0.5 hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-50"
        :class="p === speed ? 'bg-accent/20 text-accent-fg' : 'text-fg'"
        @click="onPreset(p)"
      >
        {{ p }}×
      </button>
    </div>
    <InspectorNumberInput
      :field="speedField"
      label="Speed (×)"
      testid="speed-section-speed"
      :disabled="lockReason !== null"
    />
    <label class="flex items-center gap-1.5">
      <input
        data-testid="speed-section-pitch"
        type="checkbox"
        :checked="preservePitch"
        :disabled="lockReason !== null"
        @change="onPitch"
      >
      <span>Preserve pitch</span>
    </label>
    <p data-testid="speed-section-duration">
      Plays for {{ playsForSeconds }} s. Cues stay on the same moments of the source.
    </p>
  </div>
  <p
    v-else
    data-testid="speed-section-multi"
  >
    Select a single clip to change its speed.
  </p>
</template>
