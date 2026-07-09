<script setup lang="ts">
import { computed } from "vue";
import { highlightParts } from "../utils/highlight";

const props = defineProps<{ text: string; query: string }>();
// Prop-gated: Vue skips this component's render while the parent re-renders
// on every keystroke, so the index-based split runs only when the result
// set (or the query it answered) actually changes.
const parts = computed(() => highlightParts(props.text, props.query));
</script>

<template>
  <template v-for="(part, i) in parts" :key="i">
    <mark v-if="part.match" class="rounded bg-violet-500/40 text-inherit">{{
      part.text
    }}</mark>
    <template v-else>{{ part.text }}</template>
  </template>
</template>
