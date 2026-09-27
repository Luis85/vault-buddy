<script setup lang="ts">
/**
 * A teaching cue's "Timing within this clip" (visual-parity Task 15;
 * concept spec §5 "Teaching cue"): Starts at / Ends at in seconds from the
 * clip's own start, as the cue plays on the timeline (OUTPUT time at the
 * clip's speed), and how long it is on screen. A cue's times are SOURCE
 * time (Task 34), so a commit goes back through `timeMap.sourceAtClamped`;
 * a span Rust refuses (start not before end) reverts the field. The bound
 * is the clip's output length, read once — the caller keys the cue.
 */
import { computed } from "vue";

import { secondsField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { clipSpanOf } from "../../../editor/actionTargets";
import { clipOutputEnd, cueOutputSpan, sourceAtClamped } from "../../../editor/timeMap";
import type { Clip, Effect } from "../../../editorTypes";
import InspectorNumberInput from "./InspectorNumberInput.vue";

const props = defineProps<{
  effect: Effect;
  clip: Clip;
  disabled: boolean;
  send: (update: { startMs?: number; endMs?: number }) => Promise<boolean>;
}>();

/** The cue's span in ms from the clip's start, as it plays. */
const span = computed<[number, number]>(() => {
  const c = clipSpanOf(props.clip);
  const [start, end] = cueOutputSpan(c, props.effect.start_ms, props.effect.end_ms) ?? [c.start_ms, c.start_ms];
  return [start - c.start_ms, end - c.start_ms];
});
const lengthMs = clipOutputEnd(clipSpanOf(props.clip)) - props.clip.start_ms;

/** A time `ms` into the clip, as the source instant it shows. */
function source(ms: number): number {
  const c = clipSpanOf(props.clip);
  return sourceAtClamped(c, c.start_ms + ms);
}

const start = useInspectorDraft(
  secondsField({ value: () => span.value[0], label: "Starts at", maxMs: lengthMs }),
  (ms) => props.send({ startMs: source(ms) }),
);
const end = useInspectorDraft(
  secondsField({ value: () => span.value[1], label: "Ends at", maxMs: lengthMs }),
  (ms) => props.send({ endMs: source(ms) }),
);
</script>

<template>
  <div class="grid grid-cols-2 gap-[9px]">
    <InspectorNumberInput
      :field="start"
      label="Starts at (s)"
      testid="effect-field-start"
      :disabled="disabled"
      :step="0.1"
      :min="0"
    />
    <InspectorNumberInput
      :field="end"
      label="Ends at (s)"
      testid="effect-field-end"
      :disabled="disabled"
      :step="0.1"
      :min="0"
    />
  </div>
  <p
    data-testid="effect-section-timing-help"
    class="text-[10px] leading-[1.6] text-fg-muted"
  >
    {{ ((span[1] - span[0]) / 1000).toFixed(1) }}s on screen. Follows this clip when moved.
  </p>
</template>
