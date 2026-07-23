<!-- src/components/ui/AppButton.vue -->
<script setup lang="ts">
// Text button with the shared variants. Primary = accent fill; secondary =
// bordered glass; ghost = text-only; danger = danger fill.
withDefaults(
  defineProps<{
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "sm" | "md";
    disabled?: boolean;
    type?: "button" | "submit";
  }>(),
  { variant: "primary", size: "md", disabled: false, type: "button" },
);
defineEmits<{ (e: "click", ev: MouseEvent): void }>();

const VARIANT: Record<string, string> = {
  primary: "bg-accent text-white hover:bg-accent-strong",
  secondary: "border border-white/10 bg-white/5 text-fg hover:bg-white/10",
  ghost: "text-fg-muted hover:bg-white/10 hover:text-fg",
  danger: "bg-danger text-white hover:opacity-90",
};
</script>

<template>
  <button
    :type="type"
    :disabled="disabled"
    class="cursor-pointer rounded-control font-medium transition active:scale-95 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
    :class="[size === 'sm' ? 'px-2 py-1 text-xs' : 'px-3 py-1.5 text-sm', VARIANT[variant]]"
    @click="$emit('click', $event)"
  >
    <slot />
  </button>
</template>
