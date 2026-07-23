<script setup lang="ts">
// Presentational S/M/L segmented control for the panel's preset size. The
// container (BuddySettings) owns reading/persisting the value; this
// component only renders the current selection and reports a pick.
defineProps<{ modelValue: "compact" | "comfortable" | "large" }>();
defineEmits<{ (e: "update:modelValue", v: "compact" | "comfortable" | "large"): void }>();

const OPTIONS = [
  { value: "compact", label: "Compact" },
  { value: "comfortable", label: "Comfortable" },
  { value: "large", label: "Large" },
] as const;
</script>

<template>
  <div
    class="flex gap-0.5"
    role="radiogroup"
    aria-label="Panel size"
  >
    <button
      v-for="o in OPTIONS"
      :key="o.value"
      type="button"
      role="radio"
      :data-testid="`panel-size-${o.value}`"
      :aria-pressed="modelValue === o.value"
      :aria-checked="modelValue === o.value"
      class="cursor-pointer rounded-control border px-2 py-1 text-xs transition focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="
        modelValue === o.value
          ? 'border-focus bg-accent/20 text-fg'
          : 'border-white/10 bg-white/5 text-fg-muted hover:bg-white/10'
      "
      @click="$emit('update:modelValue', o.value)"
    >
      {{ o.label }}
    </button>
  </div>
</template>
