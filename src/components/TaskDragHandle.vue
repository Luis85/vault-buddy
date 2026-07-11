<script setup lang="ts">
// The manual-sort grip: a focusable handle that forwards its raw pointer /
// key events up (the container's reorder composable owns the drag state
// machine). Its own component so TaskRow's template stays under the
// complexity threshold.
defineProps<{ title: string; busy: boolean }>();
defineEmits<{
  (e: "handlePointerDown", ev: PointerEvent): void;
  (e: "handleKeydown", ev: KeyboardEvent): void;
}>();
</script>

<template>
  <button
    type="button"
    data-testid="task-drag"
    :disabled="busy"
    :aria-label="`Reorder ${title} (arrow keys move it)`"
    title="Drag to reorder"
    class="shrink-0 cursor-grab touch-none rounded p-0.5 text-slate-500 hover:text-slate-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
    @pointerdown="$emit('handlePointerDown', $event)"
    @keydown="$emit('handleKeydown', $event)"
  >
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="currentColor"
      aria-hidden="true"
    >
      <circle
        cx="9"
        cy="5"
        r="1.6"
      /><circle
        cx="15"
        cy="5"
        r="1.6"
      />
      <circle
        cx="9"
        cy="12"
        r="1.6"
      /><circle
        cx="15"
        cy="12"
        r="1.6"
      />
      <circle
        cx="9"
        cy="19"
        r="1.6"
      /><circle
        cx="15"
        cy="19"
        r="1.6"
      />
    </svg>
  </button>
</template>
