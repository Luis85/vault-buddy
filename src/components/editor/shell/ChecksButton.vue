<script setup lang="ts">
/**
 * The header's **Checks** (Task 54; F-45; SCREENS-AND-INTERACTIONS.md §02:
 * the header owns "Help, Checks, Save project and Render video") with its
 * badge — the blockers and warnings Rust found, never a score — and the
 * Checks dialog it opens. Split out of `EditorHeader` so the header's own
 * template stays under the complexity ratchet (the `RenderVideoButton`
 * precedent).
 *
 * The badge is always on screen, so this is what keeps `editorChecks`
 * current: it re-reads whenever the session, the project's revision or the
 * number of open webcam takes changes. The dialog's open flag is
 * `checkReveal.checksDialogOpen`, shared with the canvas-ratio toast's
 * "Open Checks".
 */
import { computed, watch } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { checksDialogOpen, openChecks } from "../../../editor/revealBus";
import { openWebcamTakes } from "../../../editor/webcamTakes";
import { useEditorChecksStore } from "../../../stores/editorChecks";
import { useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";
import CountBadge from "../../ui/CountBadge.vue";
import ChecksDialog from "../dialogs/ChecksDialog.vue";

const editorProject = useEditorProjectStore();
const checks = useEditorChecksStore();
/** The guide's `header.checks` (Task 55). */
const checksTarget = useGuideTarget("header.checks");

watch(
  () => {
    const session = editorProject.sessionId;
    return [session, editorProject.snapshot?.revision, session ? openWebcamTakes(session).length : 0];
  },
  () => void checks.refresh(),
  { immediate: true },
);

const label = computed(() =>
  checks.toReview > 0 ? `Checks, ${checks.toReview} to review` : "Checks",
);
</script>

<template>
  <AppButton
    :ref="checksTarget"
    variant="ghost"
    size="sm"
    data-testid="editor-header-checks"
    :aria-label="label"
    :disabled="!editorProject.sessionId"
    :title="editorProject.sessionId ? undefined : 'No project is open.'"
    @click="openChecks"
  >
    <span class="flex items-center gap-1">
      Checks
      <CountBadge
        data-testid="editor-header-checks-count"
        :count="checks.toReview"
      />
    </span>
  </AppButton>
  <ChecksDialog
    :open="checksDialogOpen"
    @close="checksDialogOpen = false"
  />
</template>
