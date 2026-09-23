<script setup lang="ts">
/**
 * The Fades inspector's transitions block for one clip (Task 30; F-19;
 * SCREENS-AND-INTERACTIONS.md §04: "Fades are different from crossfades;
 * show numeric duration/curve and warn about track shortening where
 * applicable"). One `TransitionRow` per transition the clip carries — at
 * most one IN and one OUT, `transitionRules.transitionsOf` — each with its
 * own duration draft and Remove, and "Add transition": the SAME
 * `transition` action the clip context menu offers (`actions.ts`), resolved
 * against this clip as the pointer target, so its enabled state and reason
 * can never disagree with the menu's.
 */
import { computed } from "vue";

import { baseActionContext } from "../../../editor/actionContext";
import { commandFor, resolveActions } from "../../../editor/actions";
import { transitionsOf } from "../../../editor/transitionRules";
import type { Transition } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import TransitionRow from "./TransitionRow.vue";

const props = defineProps<{ clipId: string }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const actionContext = computed(() =>
  baseActionContext(
    editorProject.project,
    editorProject.snapshot,
    workspace.playheadMs,
    workspace.selectionClipIds,
    { kind: "clip", id: props.clipId, timeMs: null },
  ),
);
const addTransition = computed(() => resolveActions(actionContext.value).transition);

const rows = computed(() => {
  const project = editorProject.project;
  if (!project) return [];
  const { incoming, outgoing } = transitionsOf(project, props.clipId);
  const out: { transition: Transition; side: "in" | "out" }[] = [];
  if (incoming) out.push({ transition: incoming, side: "in" });
  if (outgoing) out.push({ transition: outgoing, side: "out" });
  return out;
});

function onAddTransition(): void {
  const command = commandFor("transition", actionContext.value);
  if (command) void editorProject.execute(command);
}
</script>

<template>
  <div
    data-testid="fades-section-transitions"
    class="flex flex-col gap-1 border-t border-line pt-2"
  >
    <span class="text-fg-subtle">Transitions</span>
    <TransitionRow
      v-for="row in rows"
      :key="row.transition.id"
      :transition="row.transition"
      :side="row.side"
    />
    <button
      type="button"
      data-testid="fades-section-add-transition"
      class="cursor-pointer self-start rounded px-1.5 py-0.5 text-left transition-colors hover:bg-white/10"
      :class="addTransition.enabled ? 'text-fg-secondary' : 'cursor-default opacity-50'"
      :aria-disabled="!addTransition.enabled"
      :title="addTransition.reason ?? 'Blends this clip into the next one; the overlap shortens only this track'"
      @click="onAddTransition"
    >
      {{ addTransition.label }}
    </button>
  </div>
</template>
