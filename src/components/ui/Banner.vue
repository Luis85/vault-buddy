<!-- src/components/ui/Banner.vue -->
<script setup lang="ts">
// Inline alert strip (the panel error banner, warnings). `danger` keeps the
// established banner hues (red-200 on red/20); callers own live regions.
withDefaults(defineProps<{ tone?: "danger" | "warning" | "info" | "success" }>(), { tone: "danger" });
const TONE: Record<string, string> = {
  danger: "bg-red-500/20 text-red-200",
  warning: "bg-amber-500/20 text-amber-200",
  info: "bg-white/5 text-fg-muted",
  success: "bg-emerald-500/20 text-emerald-200",
};
</script>

<template>
  <p
    class="rounded-control px-2 py-1 text-xs"
    :class="TONE[tone]"
  >
    <slot />
  </p>
</template>
