<script setup lang="ts">
/**
 * The learning center's Quick answers (Task 57; F-47): a search over the
 * lessons as the coach tells them (`answers.ts` — case- and
 * diacritic-insensitive), each answer opening to its text and a **Show me
 * in the editor** jump to that lesson. The answers say where controls are
 * now; there is no second, hand-written answer list. Visual-parity Task 23
 * (concept §9.3): the search field with its magnifier, then the answers
 * as ruled rows.
 */
import { computed, ref } from "vue";

import { searchAnswers } from "../../../editor/guide/answers";
import type { GuideStepId } from "../../../editor/guide/content";
import EditorIcon from "../icons/EditorIcon.vue";

const emit = defineEmits<{ (e: "jump", id: GuideStepId): void }>();

const query = ref("");
const answers = computed(() => searchAnswers(query.value));
</script>

<template>
  <section>
    <label class="mb-4 flex items-center gap-[11px]">
      <EditorIcon
        name="search"
        class="shrink-0 text-fg-muted"
      />
      <input
        v-model="query"
        type="search"
        data-testid="learning-search"
        aria-label="Search quick answers"
        placeholder="Search audio, saving, camera…"
        autocomplete="off"
        class="min-h-[39px] w-full text-[13px] placeholder:text-fg-subtle"
      >
    </label>
    <details
      v-for="a in answers"
      :key="a.stepId"
      data-testid="learning-answer"
      :data-step-id="a.stepId"
      class="border-b border-line py-[13px]"
    >
      <summary class="cursor-pointer py-[3px] text-[13px] text-fg">
        {{ a.question }}
      </summary>
      <p class="mt-3 mb-[5px] text-xs leading-[1.75] text-fg-secondary">
        {{ a.answer }}
      </p>
      <p class="text-[11px] leading-[1.65] text-fg-muted">
        {{ a.tip }}
      </p>
      <button
        type="button"
        :data-testid="`learning-answer-show-${a.stepId}`"
        :aria-label="`Show me in the editor: ${a.label}`"
        class="mt-1 flex items-center gap-1 border-transparent bg-transparent px-0.5 text-xs text-accent-ink hover:underline"
        @click="emit('jump', a.stepId)"
      >
        Show me in the editor
        <EditorIcon
          name="chevronRight"
          :size="13"
        />
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
