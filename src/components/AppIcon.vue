<script setup lang="ts">
// Presentational-only: the standard 24×24 line-icon <svg> wrapper shared by
// every stroked icon in the panel (ActionPanel header/all-tasks buttons,
// VaultList row actions). It carries no behaviour — the caller supplies the
// inner <path>/<circle>/<rect> children via the default slot. Factored out to
// dissolve the repeated wrapper boilerplate the fallow clone detector counted.
withDefaults(defineProps<{ size?: number }>(), { size: 16 });
</script>

<template>
  <svg
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <slot />
  </svg>
</template>
