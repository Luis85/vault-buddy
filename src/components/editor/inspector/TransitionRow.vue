<script setup lang="ts">
/**
 * One transition the selected clip carries, in the Fades inspector (Task
 * 30; F-19; SCREENS-AND-INTERACTIONS.md §04: "Fades are different from
 * crossfades; show numeric duration/curve and warn about track shortening
 * where applicable"). Its own component so each row owns its own numeric
 * draft (`useInspectorDraft` runs once per row's setup, keyed on the
 * transition id by the caller) — a clip can gain or lose a transition while
 * the Fades tab stays mounted.
 *
 * The duration's upper bound mirrors Rust's `transitions::duration_bound`
 * through `transitionRules.transitionBoundMs`, read once at setup (the
 * `FadesSection` precedent): a preview of the refusal, never the authority —
 * `set_transition_duration` re-checks on every commit.
 */
import { computed } from "vue";

import { numberField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { transitionBoundMs } from "../../../editor/transitionRules";
import type { Transition } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";

const props = defineProps<{ transition: Transition; side: "in" | "out" }>();

const editorProject = useEditorProjectStore();

const from = editorProject.clipById(props.transition.from);
const to = editorProject.clipById(props.transition.to);
const boundMs = from && to ? transitionBoundMs(from, to) : 0;

const title = computed(() => (props.transition.kind === "dissolve" ? "Cross dissolve" : "Equal-power crossfade"));
const partner = computed(() => {
  const clip = props.side === "in" ? from : to;
  const name = clip?.name ?? (props.side === "in" ? props.transition.from : props.transition.to);
  return props.side === "in" ? `From ${name}` : `Into ${name}`;
});

const duration = useInspectorDraft(
  numberField({
    value: () => editorProject.project?.transitions.find((t) => t.id === props.transition.id)?.duration_ms ?? 0,
    label: "Transition",
    min: 1,
    max: boundMs,
    rangeLabel: `1 and ${boundMs} ms`,
    integer: true,
  }),
  (durationMs) =>
    editorProject.execute({ kind: "setTransitionDuration", transitionId: props.transition.id, durationMs }),
);

function onRemove(): void {
  void editorProject.execute({ kind: "removeTransition", transitionId: props.transition.id });
}
</script>

<template>
  <div
    data-testid="transition-row"
    class="flex flex-col gap-0.5 rounded border border-line px-1.5 py-1"
  >
    <span class="text-fg-secondary">{{ title }} · {{ partner }}</span>
    <label class="flex flex-col gap-0.5">
      <span class="text-fg-subtle">Duration (ms)</span>
      <input
        data-testid="transition-row-duration"
        type="text"
        class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
        :value="duration.draft.value"
        @input="duration.draft.value = ($event.target as HTMLInputElement).value"
        @keydown.enter="duration.submit()"
        @keydown.escape="duration.revert()"
        @blur="duration.submit()"
      >
      <span
        v-if="duration.error.value"
        data-testid="transition-row-duration-error"
        class="text-danger"
      >{{ duration.error.value }}</span>
    </label>
    <p class="text-fg-subtle">
      The overlap shortens only this track.
    </p>
    <button
      type="button"
      data-testid="transition-row-remove"
      class="cursor-pointer self-start rounded px-1.5 py-0.5 text-fg-secondary transition-colors hover:bg-white/10"
      @click="onRemove"
    >
      Remove transition
    </button>
  </div>
</template>
