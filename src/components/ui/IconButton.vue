<!-- src/components/ui/IconButton.vue -->
<script setup lang="ts">
// Icon-only button. Encapsulates the hover/focus/disabled treatment that was
// copy-pasted 59× across the panel (header actions, VaultList row actions,
// TaskRow edit/archive), resolving their drift (slate-300 vs 400 base, white
// vs slate-100 hover, opacity-40 vs 50) into ONE treatment. The caller passes
// the icon via the default slot and a required accessible `label`.
withDefaults(
  defineProps<{
    label: string;
    title?: string;
    size?: "sm" | "md";
    variant?: "ghost" | "danger";
    disabled?: boolean;
  }>(),
  { size: "md", variant: "ghost", disabled: false },
);
defineEmits<{ (e: "click", ev: MouseEvent): void }>();
</script>

<template>
  <button
    type="button"
    :aria-label="label"
    :title="title ?? label"
    :disabled="disabled"
    class="shrink-0 cursor-pointer rounded-control text-fg-muted transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
    :class="[
      size === 'sm' ? 'p-1' : 'p-1.5',
      variant === 'danger' ? 'hover:text-danger-fg' : 'hover:text-fg',
    ]"
    @click="$emit('click', $event)"
  >
    <slot />
  </button>
</template>
