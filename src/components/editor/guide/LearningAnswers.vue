<script setup lang="ts">
/**
 * The learning center's Quick answers (Task 57; F-47): a search over the
 * lessons as the coach tells them (`answers.ts` — case- and
 * diacritic-insensitive), each answer opening to its text and a **Show me
 * in the editor** jump to that lesson. The answers say where controls are
 * now; there is no second, hand-written answer list.
 */
import { computed, ref } from "vue";

import { searchAnswers } from "../../../editor/guide/answers";
import type { GuideStepId } from "../../../editor/guide/content";

const emit = defineEmits<{ (e: "jump", id: GuideStepId): void }>();

const query = ref("");
const answers = computed(() => searchAnswers(query.value));
</script>

<template>
  <section class="flex max-h-[50vh] flex-col gap-2 overflow-y-auto pr-1">
    <input
      v-model="query"
      type="search"
      data-testid="learning-search"
      aria-label="Search quick answers"
      placeholder="Search: audio, saving, camera…"
      autocomplete="off"
      class="w-full rounded-control border border-line bg-raised px-2 py-1 text-sm text-fg placeholder:text-fg-subtle focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    >
    <details
      v-for="a in answers"
      :key="a.stepId"
      data-testid="learning-answer"
      :data-step-id="a.stepId"
      class="rounded-control border border-line px-2 py-1"
    >
      <summary class="cursor-pointer text-sm text-fg">
        {{ a.question }}
      </summary>
      <p class="mt-1 text-xs text-fg-secondary">
        {{ a.answer }}
      </p>
      <p class="mt-1 text-xs text-fg-muted">
        {{ a.tip }}
      </p>
      <button
        type="button"
        :data-testid="`learning-answer-show-${a.stepId}`"
        class="mt-1 cursor-pointer text-xs text-accent-fg underline focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="emit('jump', a.stepId)"
      >
        Show me in the editor ({{ a.label }})
      </button>
    </details>
    <p
      v-if="answers.length === 0"
      data-testid="learning-no-answers"
      role="status"
      class="text-xs text-fg-muted"
    >
      No matching answers. Try “save”, “audio” or “camera”.
    </p>
  </section>
</template>
