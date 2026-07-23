<!-- src/components/ui/Field.vue -->
<script setup lang="ts">
// Standard text input. Root IS the <input>, so type/placeholder/aria-label
// and native listeners (@keydown.escape) fall through by default.
defineProps<{ modelValue: string }>();
// Only `update:modelValue` is declared on purpose. A consumer's @input/@change
// stay native DOM listeners on the root <input> and merge with the handler
// below — do NOT add them to `emits`, or Vue consumes them as component events
// and the fall-through (e.g. RecordingSettings / DocumentImportSettings
// dirtied/save wiring) silently breaks.
defineEmits<{ (e: "update:modelValue", v: string): void }>();
</script>

<template>
  <input
    :value="modelValue"
    class="w-full rounded-control border border-white/10 bg-white/5 px-2 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
    @input="$emit('update:modelValue', ($event.target as HTMLInputElement).value)"
  >
</template>
