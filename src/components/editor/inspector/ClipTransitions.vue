<script setup lang="ts">
/**
 * The Fades tab's "Between two clips" (Task 30; F-19; visual-parity Task
 * 15, concept spec §5 "Fades"). One `TransitionRow` per transition the clip
 * carries — at most one IN and one OUT, `transitionRules.transitionsOf` —
 * and, while the clip does not yet blend into the next one, Blend · 0.5s /
 * Blend · 1s: the `transition` action the clip menu offers, resolved
 * against this clip, so its reasons (a locked track, no adjacent next clip,
 * a taken side) can never disagree with the menu's; a blend longer than the
 * pair allows says so too.
 */
import { computed } from "vue";

import { baseActionContext } from "../../../editor/actionContext";
import { resolveActions } from "../../../editor/actions";
import { addTransitionCommand, blendRefusal, transitionsOf } from "../../../editor/transitionRules";
import type { Transition } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import BlendPresets from "./BlendPresets.vue";
import InspectorSection from "./InspectorSection.vue";
import TransitionRow from "./TransitionRow.vue";

const props = defineProps<{ clipId: string; lockReason: string | null }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const verdict = computed(
  () =>
    resolveActions(
      baseActionContext(editorProject.project, editorProject.snapshot, workspace.playheadMs, workspace.selectionClipIds, {
        kind: "clip",
        id: props.clipId,
        timeMs: null,
      }),
    ).transition,
);

const sides = computed(() =>
  editorProject.project ? transitionsOf(editorProject.project, props.clipId) : { incoming: null, outgoing: null },
);
const rows = computed(() => {
  const out: { transition: Transition; side: "in" | "out" }[] = [];
  if (sides.value.incoming) out.push({ transition: sides.value.incoming, side: "in" });
  if (sides.value.outgoing) out.push({ transition: sides.value.outgoing, side: "out" });
  return out;
});

function reasonFor(ms: number): string | null {
  const project = editorProject.project;
  const clip = editorProject.clipById(props.clipId);
  if (!verdict.value.enabled || !project || !clip) return verdict.value.reason ?? "Not available.";
  return blendRefusal(project, clip, ms);
}

function onBlend(ms: number): void {
  const project = editorProject.project;
  const clip = editorProject.clipById(props.clipId);
  if (project && clip) void editorProject.execute(addTransitionCommand(project, clip, ms));
}
</script>

<template>
  <InspectorSection title="Between two clips">
    <div
      data-testid="fades-section-transitions"
      class="flex flex-col gap-2.5"
    >
      <TransitionRow
        v-for="row in rows"
        :key="row.transition.id"
        :transition="row.transition"
        :side="row.side"
        :lock-reason="lockReason"
      />
      <template v-if="!sides.outgoing">
        <p class="text-[10px] leading-[1.6] text-fg-muted">
          Blend this clip into the next adjacent clip on the same track. The overlap shortens only this track.
        </p>
        <BlendPresets
          testid="fades-section-blend"
          :reason-for="reasonFor"
          @blend="onBlend"
        />
      </template>
    </div>
  </InspectorSection>
</template>
