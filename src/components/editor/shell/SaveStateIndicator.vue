<script setup lang="ts">
/**
 * The header's save state (visual-parity Task 8; design D10; concept spec
 * §2 `#saveStatus`): a 5px dot and 10px muted words. The concept's words
 * are browser-only; D10's native ones are "Unsaved changes" (gold dot),
 * "Saving…", "Saved" (teal dot) and "Save failed" (danger dot), derived
 * every render from the store's own fields — never a timer, so "Saved"
 * can only follow a receipt (Task 16's rule). "Save failed" reads the
 * save-only `saveError`: a refused edit or open also sets the shared
 * `lastError`, and is not a failed save.
 *
 * Hidden at or below 1350px wide (§1.4) — the status bar's centre slot says
 * the same there — except a failed save, which stays visible at every
 * width (ruling T4-1) and carries its role-worded reason as the tooltip
 * (audit finding 7; the toast says it once).
 *
 * The concept makes these words a button opening its session dialog; no
 * native surface answers that yet (Task 22), so they stay words (D14: no
 * control without an effect). The status bar's slot saves on a click.
 */
import { computed } from "vue";

import { HEADER_COMPACT_MAX_WIDTH } from "../../../editor/panelLayout";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const state = computed<{ label: string; dot: string }>(() => {
  if (editorProject.saving) return { label: "Saving…", dot: "bg-gold" };
  if (editorProject.saveError) return { label: "Save failed", dot: "bg-danger-fg" };
  return editorProject.dirty ? { label: "Unsaved changes", dot: "bg-gold" } : { label: "Saved", dot: "bg-audio" };
});

const visible = computed(
  () => workspace.viewportWidth > HEADER_COMPACT_MAX_WIDTH || editorProject.saveError !== null,
);
</script>

<template>
  <span
    v-show="visible"
    data-testid="editor-header-save-state"
    class="flex shrink-0 items-center text-[10px] text-fg-muted"
    :title="editorProject.saveError?.message"
  >
    <span
      data-testid="editor-header-save-dot"
      aria-hidden="true"
      class="mr-[5px] h-[5px] w-[5px] shrink-0 rounded-full"
      :class="state.dot"
    />{{ state.label }}
  </span>
</template>
