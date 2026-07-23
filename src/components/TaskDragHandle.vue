<script setup lang="ts">
// The manual-sort grip: a focusable handle that forwards its raw pointer /
// key events up (the container's reorder composable owns the drag state
// machine). Its own component so TaskRow's template stays under the
// complexity threshold. `busy` renders as aria-disabled + an emit guard,
// NEVER the `disabled` attribute: a disabled control drops keyboard focus
// to <body> mid-write, so consecutive Arrow steps would each cost a re-Tab.
const props = defineProps<{ title: string; busy: boolean }>();
const emit = defineEmits<{
  (e: "handlePointerDown", ev: PointerEvent): void;
  (e: "handleKeydown", ev: KeyboardEvent): void;
}>();
function onPointerDown(ev: PointerEvent) {
  if (!props.busy) emit("handlePointerDown", ev);
}
function onKeydown(ev: KeyboardEvent) {
  if (!props.busy) emit("handleKeydown", ev);
}
</script>

<template>
  <button
    type="button"
    data-testid="task-drag"
    :aria-disabled="busy || undefined"
    :aria-label="`Reorder ${title} (arrow keys move it)`"
    title="Drag to reorder"
    class="shrink-0 touch-none rounded p-0.5 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    :class="busy ? 'cursor-default text-slate-600 opacity-40' : 'cursor-grab text-fg-subtle hover:text-slate-200'"
    @pointerdown="onPointerDown"
    @keydown="onKeydown"
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
