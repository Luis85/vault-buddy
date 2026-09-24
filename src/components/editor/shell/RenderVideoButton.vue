<script setup lang="ts">
/**
 * The header's **Render video** (Task 47; F-41; SCREENS-AND-INTERACTIONS.md
 * §02: the header owns "Save project and Render video") and the dialog it
 * opens. Split out of `EditorHeader` so the header's own template stays
 * under the complexity ratchet.
 *
 * Disabled — with its reason visible next to it, R20 — only when there is
 * nothing to render. The dialog's range default is the selected clips'
 * output span (the one in/out range this editor has), frozen the moment the
 * dialog opens. A render's errors stay inside the dialog and never reach
 * the header's save status (Task 46's carry).
 */
import { computed, ref } from "vue";

import { selectionRange } from "../../../editor/renderRanges";
import type { RenderRange } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import AppButton from "../../ui/AppButton.vue";
import RenderDialog from "../dialogs/RenderDialog.vue";

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const reason = computed<string | null>(() => {
  if (!editorProject.sessionId) return "No project is open.";
  if (editorProject.durationMs === 0) return "Place a clip on the timeline to render a video.";
  return null;
});
const title = computed(() => reason.value ?? undefined);

const open = ref(false);
const initialRange = ref<RenderRange | null>(null);

function onRender(): void {
  if (reason.value) return;
  initialRange.value = selectionRange(editorProject.project, workspace.selectionClipIds, editorProject.durationMs);
  open.value = true;
}
</script>

<template>
  <AppButton
    variant="primary"
    size="sm"
    data-testid="editor-header-render"
    :disabled="Boolean(reason)"
    :title="title"
    @click="onRender"
  >
    Render video
  </AppButton>
  <span
    v-if="reason"
    data-testid="editor-header-render-reason"
    class="text-micro text-fg-subtle"
  >{{ reason }}</span>
  <RenderDialog
    :open="open"
    :initial-range="initialRange"
    @close="open = false"
  />
</template>
