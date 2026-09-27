<script setup lang="ts">
/**
 * One inspector section (visual-parity Task 13; concept spec §5
 * `.section`): an 11px/600 heading over its controls, 15px apart, with a
 * rule between sections (none above the first).
 */
defineProps<{ title: string }>();
</script>

<template>
  <section class="flex flex-col gap-2.5 border-t border-line py-[15px] first:border-t-0 first:pt-0">
    <h3 class="text-[11px] font-semibold text-fg">
      {{ title }}
    </h3>
    <slot />
  </section>
</template>
