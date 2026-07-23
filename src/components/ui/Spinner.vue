<!-- src/components/ui/Spinner.vue -->
<script setup lang="ts">
// The shared spinning ring, dedup'ing the hand-rolled `animate-spin` copies.
withDefaults(defineProps<{ size?: "sm" | "md"; label?: string }>(), {
  size: "sm",
  label: "Loading…",
});
</script>

<template>
  <span
    data-testid="spinner"
    role="status"
    :aria-label="label"
    class="inline-block shrink-0 animate-spin rounded-full border-2 border-white/30 border-t-white"
    :class="size === 'md' ? 'h-5 w-5' : 'h-4 w-4'"
  />
</template>
