<script setup lang="ts">
/**
 * The editor's status bar (visual-parity Task 4; concept spec §7): the
 * frame's last grid row, 25px (23 in a window 760px tall or less, which
 * the shell's row sizes), 9px muted text on `--bg` under a `line` border,
 * three slots. The concept's copy in two of them is browser-only
 * ("Recovery cached in this browser", "Browser reference · downloads stay
 * local"); design D10 keeps the slots and gives them native words:
 *
 * - left: where the work lives — "Local only. No media is uploaded."
 * - centre: the session's recovery state, read from `editorProject.dirty`
 *   (an unsaved session is journaled for recovery, Task 37) — or "Save
 *   failed" whenever the last save was refused (`editorProject.saveError`,
 *   ruling T4-1: a refusal is never invisible), its role-worded reason in
 *   the tooltip. A click saves the project through the header's own path
 *   (`useProjectSave`).
 * - right: how many products this project has rendered; a click shows
 *   them through `revealWorkspaceProducts`, the helper the header's
 *   Project menu uses too.
 */
import { computed, watch } from "vue";

import { useProjectSave } from "../../../composables/useProjectSave";
import { withoutRedactionHandles } from "../../../editor/errorCopy";
import { revealWorkspaceProducts } from "../../../editor/revealProducts";
import { useEditorProductsStore } from "../../../stores/editorProducts";
import { useEditorProjectStore } from "../../../stores/editorProject";
import EditorIcon from "../icons/EditorIcon.vue";

const editorProject = useEditorProjectStore();
const products = useEditorProductsStore();
const { disabledReason, save } = useProjectSave();

/** The ledger is read once per session here, since the library's products
 * view (which also reads it) is not always mounted. */
watch(
  () => editorProject.sessionId,
  (id) => {
    if (id) void products.refresh();
  },
  { immediate: true },
);

const recovery = computed(() => {
  if (editorProject.saveError) return "Save failed";
  return editorProject.dirty ? "Unsaved changes are journaled for recovery" : "All changes saved";
});
/** Why the slot cannot act, else why the last save failed, else what a
 * click does. */
const recoveryTitle = computed(() => {
  const failure = editorProject.saveError;
  return disabledReason.value ?? (failure ? withoutRedactionHandles(failure.message) : "Save project");
});

const productsLabel = computed(() => {
  const n = products.current.length;
  return `${n} rendered ${n === 1 ? "video" : "videos"} · Workspace & rendered products`;
});
</script>

<template>
  <footer
    data-testid="editor-statusbar"
    class="flex min-w-0 items-center justify-between gap-2.5 overflow-hidden border-t border-line bg-app px-[13px] text-[9px] text-fg-muted"
  >
    <span
      data-testid="editor-statusbar-local"
      class="flex min-w-0 shrink-0 items-center gap-1.5"
    >
      <EditorIcon
        name="shield"
        :size="12"
      />
      <span>Local only. No media is uploaded.</span>
    </span>
    <button
      type="button"
      data-testid="editor-statusbar-recovery"
      class="status-slot"
      :disabled="Boolean(disabledReason)"
      :title="recoveryTitle"
      @click="save"
    >
      {{ recovery }}
    </button>
    <button
      type="button"
      data-testid="editor-statusbar-products"
      class="status-slot"
      title="Show your rendered products in the library"
      @click="revealWorkspaceProducts"
    >
      {{ productsLabel }}
    </button>
  </footer>
</template>

<style scoped>
/* A slot reads as status text, not a bordered button: the base control
   height (32px) would not fit a 23–25px row. */
.status-slot {
  min-height: 0;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  border-radius: 4px;
  padding: 1px 4px;
}
</style>
