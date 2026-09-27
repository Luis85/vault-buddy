<script setup lang="ts">
/**
 * The minimized coach (Task 56; visual-parity Task 23; concept spec §9.2
 * `.guide-mini`): a small bar in the bottom-right corner — the book, "n/22 ·
 * the lesson's title" and an up chevron, which brings the card back at the
 * same lesson, then a ✕ that pauses the walkthrough (Help resumes it).
 * Presentational: `GuideCoach` answers both emits.
 */
import EditorIcon from "../icons/EditorIcon.vue";

defineProps<{ position: string; title: string }>();
const emit = defineEmits<{ (e: "expand"): void; (e: "dismiss"): void }>();
</script>

<template>
  <div
    data-guide-layer="resume"
    data-testid="guide-mini"
    class="fixed right-5 bottom-[26px] z-40 flex max-w-[calc(100vw-28px)] rounded-[10px] border border-guide-edge bg-panel p-[3px] text-fg shadow-[var(--editor-guide-shadow)]"
  >
    <button
      type="button"
      data-testid="guide-resume"
      aria-label="Expand walkthrough"
      class="flex min-w-0 items-center gap-[9px] rounded-[7px] border-transparent bg-transparent px-2.5 py-2 text-[11px] text-fg"
      @click="emit('expand')"
    >
      <EditorIcon
        name="book"
        :size="15"
        class="shrink-0 text-accent"
      />
      <span class="max-w-[255px] truncate">{{ position }} · {{ title }}</span>
      <EditorIcon
        name="up"
        :size="15"
        class="shrink-0 text-accent"
      />
    </button>
    <button
      type="button"
      data-testid="guide-mini-close"
      aria-label="Dismiss minimized guide"
      class="flex items-center rounded-none rounded-r-[7px] border-0 border-l border-line bg-transparent px-2 text-fg-muted"
      @click="emit('dismiss')"
    >
      <EditorIcon
        name="x"
        :size="15"
      />
    </button>
  </div>
</template>
