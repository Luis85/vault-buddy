<!-- src/components/ui/Field.vue -->
<script setup lang="ts">
// Standard text input. Root IS the <input>, so type/placeholder/aria-label
// and native listeners (@keydown.escape) fall through by default.
defineProps<{ modelValue: string }>();
defineEmits<{ (e: "update:modelValue", v: string): void }>();
</script>

<template>
  <input
    :value="modelValue"
    class="w-full rounded-control border border-white/10 bg-white/5 px-2 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
    @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
  >
</template>
