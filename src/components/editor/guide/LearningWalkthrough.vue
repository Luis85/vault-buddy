<script setup lang="ts">
/**
 * The learning center's Walkthrough section (Task 57; F-47): the seven
 * chapters with their lessons, each one a jump — a chapter to its first
 * lesson, a lesson to itself — and, per lesson, whether it was read.
 * **Start over** is the coach's own two-step confirm (`GuideStartOver`):
 * it resets guide state only (ONBOARDING.md).
 */
import { computed } from "vue";

import type { GuideStepId } from "../../../editor/guide/content";
import { GUIDE_CHAPTERS, GUIDE_STEPS } from "../../../editor/guide/content";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import GuideStartOver from "./GuideStartOver.vue";

const emit = defineEmits<{
  (e: "jump", id: GuideStepId): void;
  (e: "restart"): void;
}>();

const onboarding = useEditorOnboardingStore();

const chapters = computed(() =>
  GUIDE_CHAPTERS.map((chapter) => {
    const lessons = GUIDE_STEPS.filter((s) => s.chapter === chapter.id).map((s) => ({
      id: s.id,
      title: s.title,
      read: onboarding.progress.reviewed.includes(s.id),
    }));
    return { ...chapter, lessons, read: lessons.filter((l) => l.read).length };
  }),
);
</script>

<template>
  <section class="flex max-h-[50vh] flex-col gap-3 overflow-y-auto pr-1">
    <div class="flex items-center justify-between gap-2 text-xs">
      <p class="text-fg-muted">
        Start at the beginning or jump straight to what you need. The guide never edits for you.
      </p>
      <GuideStartOver @restart="emit('restart')" />
    </div>
    <div
      v-for="chapter in chapters"
      :key="chapter.id"
      class="flex flex-col gap-1 rounded-control border border-line p-2"
    >
      <button
        type="button"
        :data-testid="`learning-chapter-${chapter.id}`"
        class="flex cursor-pointer items-baseline justify-between gap-2 rounded px-1 text-left hover:bg-white/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="emit('jump', chapter.lessons[0].id)"
      >
        <span class="flex flex-col">
          <span class="text-sm font-medium text-fg">{{ chapter.title }}</span>
          <span class="text-micro text-fg-muted">{{ chapter.subtitle }}</span>
        </span>
        <span class="text-micro text-fg-subtle">{{ chapter.read }} / {{ chapter.lessons.length }}</span>
      </button>
      <ul class="flex flex-col">
        <li
          v-for="lesson in chapter.lessons"
          :key="lesson.id"
        >
          <button
            type="button"
            :data-testid="`learning-lesson-${lesson.id}`"
            class="flex w-full cursor-pointer items-center gap-2 rounded px-1 py-0.5 text-left text-xs text-fg-secondary hover:bg-white/5 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
            @click="emit('jump', lesson.id)"
          >
            <span
              class="w-3 text-success"
              :aria-label="lesson.read ? 'Read' : undefined"
            >{{ lesson.read ? "✓" : "" }}</span>
            {{ lesson.title }}
          </button>
        </li>
      </ul>
    </div>
  </section>
</template>
