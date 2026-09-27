<script setup lang="ts">
/**
 * Nothing selected (visual-parity Task 13; concept spec §5 "None"): what to
 * do next rather than a panel of disabled controls (SCREENS §02), then the
 * project's facts — its clip count and length, its canvas — and **Review
 * readiness**, which opens the Checks dialog through the header button's
 * own door (`openChecks`).
 */
import { computed } from "vue";

import { openChecks } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import EditorIcon from "../icons/EditorIcon.vue";
import { formatMenuTime } from "../menus/menuModel";
import InspectorButton from "./InspectorButton.vue";
import InspectorSection from "./InspectorSection.vue";

const editorProject = useEditorProjectStore();

const clipsLine = computed(() => {
  const n = editorProject.project?.clips.length ?? 0;
  return `${n === 1 ? "1 clip" : `${n} clips`} · ${formatMenuTime(editorProject.durationMs)}`;
});
const canvasLine = computed(() => {
  const c = editorProject.project?.canvas;
  return c ? `${c.width} × ${c.height} · ${c.fps} fps` : "";
});
</script>

<template>
  <div
    data-testid="inspector-empty"
    class="flex flex-col items-center py-5 text-center text-[11px] leading-[1.7] text-fg-muted"
  >
    <EditorIcon
      name="cursor"
      :size="27"
      class="mb-2.5"
    />
    <b class="block text-[12px] text-fg">Select something to shape it.</b>
    <p class="mt-2">
      Choose a clip, annotation or track. Its controls will appear here.
    </p>
  </div>
  <InspectorSection title="Project">
    <p class="text-[10px] leading-[1.6] text-fg-muted">
      <span
        data-testid="inspector-project-clips"
        class="block"
      >{{ clipsLine }}</span>
      <span
        data-testid="inspector-project-canvas"
        class="block"
      >{{ canvasLine }}</span>
    </p>
    <InspectorButton
      icon="check"
      class="self-start"
      data-testid="inspector-review-readiness"
      @click="openChecks"
    >
      Review readiness
    </InspectorButton>
  </InspectorSection>
</template>
