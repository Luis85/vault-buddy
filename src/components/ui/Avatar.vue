<!-- src/components/ui/Avatar.vue -->
<script setup lang="ts">
// The letter avatar shared by VaultList (md) and TaskRow's vault marker (sm).
withDefaults(defineProps<{ name: string; size?: "sm" | "md" }>(), { size: "md" });
</script>

<template>
  <span
    class="flex shrink-0 items-center justify-center bg-accent-strong/80 font-bold text-white"
    :class="size === 'sm' ? 'h-4 w-4 rounded text-[9px]' : 'h-7 w-7 rounded-lg text-xs'"
    aria-hidden="true"
  >{{ name.charAt(0).toUpperCase() }}</span>
</template>
