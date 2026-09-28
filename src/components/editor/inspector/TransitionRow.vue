<script setup lang="ts">
/**
 * One transition the selected clip carries, in the Fades tab's "Between two
 * clips" (Task 30; F-19; visual-parity Task 15, concept spec §5 "Fades"):
 * the `.callout` naming it — "Cross dissolve" or "Equal-power crossfade",
 * its overlap, "selected track only" — then Blend · 0.5s / 1s and an
 * Overlap (s) field (`setTransitionDuration`), and Remove crossfade
 * (`removeTransition`). Its own component so each row owns its own draft
 * (`useInspectorDraft` runs once per row's setup, keyed on the transition
 * id by the caller) — a clip can gain or lose a transition while the tab
 * stays mounted.
 *
 * The overlap's bound mirrors Rust's `transitions::duration_bound` through
 * `transitionRules.transitionBoundMs`, from the live clips: a preview of the
 * refusal, never the authority — `set_transition_duration` re-checks on
 * every commit.
 */
import { computed } from "vue";

import { secondsField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { blendTooLong, transitionBoundMs } from "../../../editor/transitionRules";
import type { Transition } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import BlendPresets from "./BlendPresets.vue";
import InspectorButton from "./InspectorButton.vue";
import InspectorNumberInput from "./InspectorNumberInput.vue";

const props = defineProps<{ transition: Transition; side: "in" | "out"; lockReason: string | null }>();

const editorProject = useEditorProjectStore();

const from = computed(() => editorProject.clipById(props.transition.from));
const to = computed(() => editorProject.clipById(props.transition.to));
/** The pair's bound from the live clips, so a trim while the tab is open
 * moves it. */
const boundMs = computed(() => (from.value && to.value ? transitionBoundMs(from.value, to.value) : 0));

const live = computed(() => editorProject.project?.transitions.find((t) => t.id === props.transition.id));
const durationMs = computed(() => live.value?.duration_ms ?? props.transition.duration_ms);
const title = computed(() => (props.transition.kind === "dissolve" ? "Cross dissolve" : "Equal-power crossfade"));
const partner = computed(() => {
  const clip = props.side === "in" ? from.value : to.value;
  const name = clip?.name ?? (props.side === "in" ? props.transition.from : props.transition.to);
  return props.side === "in" ? `From ${name}` : `Into ${name}`;
});

function setDuration(ms: number): Promise<boolean> {
  // The overlap already in place is not an edit.
  if (ms === durationMs.value) return Promise.resolve(true);
  return editorProject.execute({ kind: "setTransitionDuration", transitionId: props.transition.id, durationMs: ms });
}

const duration = useInspectorDraft(
  secondsField({ value: () => durationMs.value, label: "Overlap", minMs: 1, maxMs: () => boundMs.value }),
  setDuration,
);

function onRemove(): void {
  void editorProject.execute({ kind: "removeTransition", transitionId: props.transition.id });
}
</script>

<template>
  <div
    data-testid="transition-row"
    class="flex flex-col gap-2.5"
  >
    <p
      class="rounded-[7px] border border-accent/22 bg-accent-bg p-[13px] text-[11px] leading-[1.7] text-fg-secondary"
    >
      <b class="text-accent-ink">{{ title }}</b><br>
      {{ (durationMs / 1000).toFixed(2) }}s overlap · selected track only<br>
      <small class="text-[10px]">{{ partner }}</small>
    </p>
    <BlendPresets
      testid="transition-row-blend"
      :reason-for="(ms) => lockReason ?? blendTooLong(boundMs, ms)"
      :current-ms="durationMs"
      @blend="setDuration"
    />
    <InspectorNumberInput
      :field="duration"
      label="Overlap (s)"
      testid="transition-row-duration"
      :disabled="lockReason !== null"
      :title="lockReason ?? undefined"
      :step="0.05"
      :min="0"
    />
    <InspectorButton
      class="self-start"
      icon="x"
      data-testid="transition-row-remove"
      :reason="lockReason"
      @click="onRemove"
    >
      Remove crossfade
    </InspectorButton>
  </div>
</template>
