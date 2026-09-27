<script setup lang="ts">
/**
 * The header's brand (visual-parity Task 8; design D6; concept spec §2
 * `.brand`): the buddy's face drawn in CSS — a 29×31 accent tile holding
 * two eyes (`··`) with two ears on top — and the wordmark "vault-buddy",
 * which the concept drops at or below 1350px wide (§1.4). No image file:
 * the mark is text and borders, so it follows the theme's tokens.
 */
import { computed } from "vue";

import { HEADER_COMPACT_MAX_WIDTH } from "../../../editor/panelLayout";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const workspace = useEditorWorkspaceStore();
const showWordmark = computed(() => workspace.viewportWidth > HEADER_COMPACT_MAX_WIDTH);
</script>

<template>
  <span
    data-testid="editor-header-brand"
    title="Vault Buddy · Tutorial Editor"
    class="mr-0.5 flex shrink-0 items-center gap-2 text-sm font-[650] tracking-[-0.5px] text-fg"
  >
    <span
      data-testid="editor-header-brand-mark"
      aria-hidden="true"
      class="brand-mark"
    >··</span>
    <span
      v-show="showWordmark"
      data-testid="editor-header-wordmark"
    >vault-buddy</span>
  </span>
</template>

<style scoped>
/* Concept §2: `.brand-mark` and its `:before`/`:after` ears. */
.brand-mark {
  position: relative;
  display: grid;
  place-items: center;
  width: 29px;
  height: 31px;
  font-size: 16px;
  letter-spacing: 3px;
  color: var(--color-accent);
  background: var(--color-accent-bg);
  border: 1px solid var(--color-accent);
  border-radius: 9px 9px 12px 12px;
}
.brand-mark::before,
.brand-mark::after {
  content: "";
  position: absolute;
  top: -5px;
  width: 6px;
  height: 7px;
  border: 1px solid var(--color-accent);
  border-bottom: 0;
  border-radius: 3px 3px 0 0;
  background: var(--color-accent-bg);
}
.brand-mark::before {
  left: 4px;
}
.brand-mark::after {
  right: 4px;
}
</style>
