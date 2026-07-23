<script setup lang="ts">
// Presentational S/M/L segmented control for the panel's preset size. The
// container (BuddySettings) owns reading/persisting the value; this
// component only renders the current selection and reports a pick. `disabled`
// is held true by the container while the mount-time read or a save+re-show is
// in flight, so a pick can't race either (the autostart-toggle busy pattern).
defineProps<{
  modelValue: "compact" | "comfortable" | "large";
  disabled?: boolean;
}>();
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
      :aria-checked="modelValue === o.value"
      :disabled="disabled"
      class="cursor-pointer rounded-control border px-2 py-1 text-xs transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-50"
      :class="
        modelValue === o.value
          ? 'border-violet-400 bg-accent/20 text-fg'
          : 'border-white/10 bg-white/5 text-fg-muted hover:bg-white/10'
      "
      @click="$emit('update:modelValue', o.value)"
    >
      {{ o.label }}
    </button>
  </div>
</template>
