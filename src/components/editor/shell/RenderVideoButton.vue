<script setup lang="ts">
/**
 * The header's **Render video** (Task 47; F-41; SCREENS-AND-INTERACTIONS.md
 * §02: the header owns "Save project and Render video") and the dialog it
 * opens. Split out of `EditorHeader` so the header's own template stays
 * under the complexity ratchet.
 *
 * Visual-parity Task 8 (concept spec §2): the concept's `.primary` — the
 * video icon and white 600 on `--color-primary` (`HeaderButton`).
 *
 * Disabled — with its reason visible next to it, R20 — only when there is
 * nothing to render. The dialog's range default is the selected clips'
 * output span (the one in/out range this editor has), frozen the moment the
 * dialog opens. A render's errors stay inside the dialog and never reach
 * the header's save status (Task 46's carry). The Checks dialog's
 * "Continue to render" opens it too (`onReveal("render")`, Task 54).
 *
 * Hardening Task 15 (GAP-208): a render still running when the editor
 * webview reloaded has no Channel and no dialog; `editorJobs` adopts it on
 * the new session's reconcile, and this opens the dialog on it
 * (`resumeJobId`) so its progress is on screen until it ends.
 */
import { computed, ref, watch } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { selectionRange } from "../../../editor/renderRanges";
import { onReveal } from "../../../editor/revealBus";
import type { RenderRange } from "../../../editorTypes";
import { useEditorJobsStore } from "../../../stores/editorJobs";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import RenderDialog from "../dialogs/RenderDialog.vue";
import HeaderButton from "./HeaderButton.vue";

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
/** The guide's `header.render` (Task 55). */
const renderTarget = useGuideTarget("header.render");

const reason = computed<string | null>(() => {
  if (!editorProject.sessionId) return "No project is open.";
  if (editorProject.durationMs === 0) return "Place a clip on the timeline to render a video.";
  return null;
});
const title = computed(() => reason.value ?? undefined);

const open = ref(false);
const initialRange = ref<RenderRange | null>(null);
/** A running render the dialog follows instead of offering a new one. */
const resumeJobId = ref<string | null>(null);

function onRender(): void {
  if (reason.value) return;
  resumeJobId.value = null;
  initialRange.value = selectionRange(editorProject.project, workspace.selectionClipIds, editorProject.durationMs);
  open.value = true;
}

function onClose(): void {
  open.value = false;
  resumeJobId.value = null;
}

const jobs = useEditorJobsStore();
watch(
  () => jobs.adoptedRender,
  (jobId) => {
    if (!jobId || open.value) return;
    resumeJobId.value = jobId;
    initialRange.value = null;
    open.value = true;
  },
  { immediate: true },
);

/** Task 54: the Checks dialog's "Continue to render". */
onReveal("render", onRender);
</script>

<template>
  <HeaderButton
    :ref="renderTarget"
    icon="video"
    variant="primary"
    data-testid="editor-header-render"
    :disabled="Boolean(reason)"
    :title="title"
    @click="onRender"
  >
    Render video
  </HeaderButton>
  <span
    v-if="reason"
    data-testid="editor-header-render-reason"
    class="text-micro text-fg-subtle"
  >{{ reason }}</span>
  <RenderDialog
    :open="open"
    :initial-range="initialRange"
    :resume-job-id="resumeJobId"
    @close="onClose"
  />
</template>
