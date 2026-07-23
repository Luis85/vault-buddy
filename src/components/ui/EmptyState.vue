<!-- src/components/ui/EmptyState.vue -->
<script setup lang="ts">
// Centered empty/degraded-state block: an optional icon, a title (the exact
// former `<p>` message), an optional hint, and an optional action slot.
// Replaces the lonely gray one-liners with a consistent, on-brand treatment.
defineProps<{ title: string; hint?: string }>();
</script>

<template>
  <div
    data-testid="empty-state"
    class="flex flex-col items-center gap-2 px-4 py-8 text-center"
  >
    <span
      v-if="$slots.icon"
      data-testid="empty-state-icon"
      class="text-fg-subtle"
      aria-hidden="true"
    >
      <slot name="icon" />
    </span>
    <p class="text-xs text-fg-muted">
      {{ title }}
    </p>
    <p
      v-if="hint"
      class="text-micro text-fg-subtle"
    >
      {{ hint }}
    </p>
    <slot name="action" />
  </div>
</template>
