<script setup lang="ts">
/**
 * A dialog footer's reason line (visual-parity Task 22; design D14): why
 * the footer's buttons cannot act right now, said on screen — not only in
 * a disabled button's `title`, which a keyboard user never hovers. Always
 * present and polite-live, so the reason is announced the moment it
 * appears and the line never jumps the footer's layout; empty when there
 * is nothing to say. Sits left of the buttons (`mr-auto`), Task 21's
 * Render and Checks footers' own pattern.
 */
defineProps<{ text: string | null }>();
</script>

<template>
  <span
    role="status"
    aria-live="polite"
    class="mr-auto text-[10px] text-fg-muted"
  >{{ text ?? "" }}</span>
</template>
