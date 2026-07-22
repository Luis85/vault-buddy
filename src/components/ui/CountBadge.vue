<!-- src/components/ui/CountBadge.vue -->
<script setup lang="ts">
// The corner count badge (VaultList tasks button, ActionPanel all-tasks).
// Fixed geometry keeps a 14px circle legible, so it keeps its literal 9px
// text rather than the micro caption token. The caller owns positioning.
const props = withDefaults(defineProps<{ count: number; max?: number }>(), { max: 99 });
const label = () => (props.count > props.max ? `${props.max}+` : String(props.count));
</script>

<template>
  <span
    v-if="count > 0"
    class="min-w-[14px] rounded-full bg-accent px-1 text-center text-[9px] font-semibold leading-[14px] text-white"
  >{{ label() }}</span>
</template>
