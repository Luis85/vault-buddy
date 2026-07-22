<!-- src/components/ui/Chip.vue -->
<script setup lang="ts">
// Pill for tags, counts, and filters. `interactive` renders a focusable
// button (the clickable tag chip); neutral/accent render a static span.
withDefaults(
  defineProps<{ variant?: "neutral" | "accent" | "interactive"; label?: string; title?: string }>(),
  { variant: "neutral", label: undefined, title: undefined },
);
defineEmits<{ (e: "click"): void }>();
</script>

<template>
  <button
    v-if="variant === 'interactive'"
    type="button"
    :aria-label="label"
    :title="title"
    class="shrink-0 cursor-pointer rounded-full bg-white/10 px-1.5 text-micro text-accent-fg transition-colors hover:bg-accent/30 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    @click="$emit('click')"
  >
    <slot />
  </button>
  <span
    v-else
    :title="title"
    class="shrink-0 rounded-full px-1.5 text-micro"
    :class="variant === 'accent' ? 'bg-accent/20 text-accent-fg' : 'bg-white/10 text-fg-muted'"
  >
    <slot />
  </span>
</template>
