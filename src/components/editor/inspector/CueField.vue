<script setup lang="ts">
/**
 * One property of a teaching cue in its inspector (visual-parity Task 15;
 * concept spec §5 "Teaching cue"), drawn as its spec says
 * (`effectFields.ts`): a slider for size, line weight, dim and
 * magnification (one commit on release), a checkbox for the readable
 * background, and a typed field for the rest — through a
 * `useInspectorDraft` buffer, so Enter/blur commits ONE `updateEffect`,
 * Escape reverts and an out-of-range entry stays visible with its
 * correction (R20). The caller keys the cue, so the spec never changes
 * under this instance.
 */
import type { InspectorField } from "../../../composables/useInspectorDraft";
import {
  numberField,
  percentField,
  secondsField,
  textField,
  useInspectorDraft,
} from "../../../composables/useInspectorDraft";
import type { EffectFieldSpec } from "../../../editor/effectFields";
import { MAX_EFFECT_TEXT_CHARS } from "../../../editor/effectFields";
import type { Effect } from "../../../editorTypes";
import InspectorNumberInput from "./InspectorNumberInput.vue";
import InspectorRange from "./InspectorRange.vue";

const props = defineProps<{
  spec: EffectFieldSpec;
  effect: Effect;
  disabled: boolean;
  send: (patch: Record<string, unknown>) => Promise<boolean>;
}>();

function stored(): number {
  const v = props.effect[props.spec.key as keyof Effect];
  return typeof v === "number" ? v : 0;
}

function fieldOf(spec: EffectFieldSpec): InspectorField<number> | InspectorField<string> | null {
  const { label } = spec;
  if (spec.kind === "percent") return percentField({ value: stored, label, min: () => spec.min, max: () => spec.max });
  if (spec.kind === "seconds") return secondsField({ value: stored, label, maxMs: spec.maxMs });
  if (spec.kind === "number") {
    return numberField({ ...spec, value: stored, rangeLabel: `${spec.min} and ${spec.max}` });
  }
  if (spec.kind === "text") {
    return textField({ value: () => props.effect.text ?? "", label, maxLength: MAX_EFFECT_TEXT_CHARS });
  }
  return null;
}

const field = fieldOf(props.spec) as InspectorField<unknown> | null;
const draft = field ? useInspectorDraft(field, (value) => props.send({ [props.spec.key]: value })) : null;
/** A slider's spec, shown `scale` times its stored value. */
const range = props.spec.kind === "range" ? props.spec : null;
function commitRange(shown: number): Promise<boolean> {
  return range ? props.send({ [range.key]: shown / range.scale }) : Promise.resolve(false);
}
const testid = `effect-field-${props.spec.key}`;

function onToggle(event: Event): void {
  void props.send({ [props.spec.key]: (event.target as HTMLInputElement).checked });
}
</script>

<template>
  <InspectorNumberInput
    v-if="draft"
    :field="draft"
    :label="spec.label"
    :testid="testid"
    :inputmode="spec.kind === 'text' ? 'text' : 'decimal'"
    :disabled="disabled"
  />
  <InspectorRange
    v-else-if="range"
    :label="range.label"
    :testid="testid"
    :value="Math.round(stored() * range.scale * 100) / 100"
    :min="range.min * range.scale"
    :max="range.max * range.scale"
    :step="range.step"
    :suffix="range.suffix"
    :commit="commitRange"
  />
  <label
    v-else-if="spec.kind === 'toggle'"
    class="flex items-center gap-2 text-[11px] text-fg"
  >
    <input
      :data-testid="testid"
      type="checkbox"
      class="shrink-0"
      :checked="effect.background ?? false"
      :disabled="disabled"
      @change="onToggle"
    >
    {{ spec.label }}
  </label>
</template>
