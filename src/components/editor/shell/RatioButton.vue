<script setup lang="ts">
/**
 * The preview header's ratio button (visual-parity Task 11; concept spec
 * §4.1 `.ratio-button`): the canvas's ratio in mono ("16:9") and a 12px
 * chevron, opening **Frame your tutorial** (`FrameDialog`), which replaced
 * Task 32's native `<select>` (`RatioSelect`). Disabled, with the
 * registry's reason, while no project is open.
 *
 * A `canvasReview` finding (Task 54) asks to reveal "ratio": focus lands
 * here, after the tick, so the closing Checks dialog's own focus restore
 * does not land on top of it.
 */
import { computed, nextTick, ref } from "vue";

import { ratioLabel } from "../../../editor/previewHeader";
import { onReveal } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import FrameDialog from "../dialogs/FrameDialog.vue";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{ disabledReason: string | null }>();

const editorProject = useEditorProjectStore();
const label = computed(() => ratioLabel(editorProject.project?.canvas));
const button = ref<HTMLButtonElement | null>(null);
const open = ref(false);

onReveal("ratio", () => {
  void nextTick(() => button.value?.focus());
});

function onClick(): void {
  if (!props.disabledReason) open.value = true;
}
</script>

<template>
  <button
    ref="button"
    type="button"
    data-testid="preview-ratio"
    aria-haspopup="dialog"
    :disabled="Boolean(disabledReason)"
    :title="disabledReason ?? 'Frame your tutorial: choose the video format'"
    class="vb-mono inline-flex min-h-[30px] min-w-[55px] shrink-0 items-center gap-1 border border-transparent bg-transparent px-[7px] py-1 text-[10px] text-fg-muted"
    @click="onClick"
  >
    {{ label }}
    <EditorIcon
      name="chevronDown"
      :size="12"
    />
  </button>
  <FrameDialog
    :open="open"
    @close="open = false"
  />
</template>
