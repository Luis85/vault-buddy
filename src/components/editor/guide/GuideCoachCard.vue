<script setup lang="ts">
/**
 * What the coach card says (Task 56; SCREENS 10): chapter, "n / 22", the
 * lesson's title, body, tip and optional task (an optional EDIT is marked
 * as one — ONBOARDING.md: "Every optional action that changes the project
 * says so"), a note when the lesson's control is not on screen, and the
 * Collapse / Close / Pause / Back / Next controls. Presentational: every
 * action is an emit that `GuideCoach` answers, and the text is
 * `lessonCopy` — this app's corrected copy, never a raw step (GAP-203).
 *
 * Its three parts render straight into the coach's own `<section>` (a
 * fragment), so the coach measures and caps them as one card.
 */
import { ref } from "vue";

import type { LessonCopy } from "../../../editor/guide/content";
import AppButton from "../../ui/AppButton.vue";
import IconButton from "../../ui/IconButton.vue";
import GuideStartOver from "./GuideStartOver.vue";

defineProps<{
  stepId: string;
  title: string;
  /** The lesson's task is an optional edit. */
  warning: boolean;
  copy: LessonCopy;
  chapterTitle: string;
  position: string;
  progressPct: number;
  canGoBack: boolean;
  isLast: boolean;
  /** The lesson's control is registered but not on screen, or missing. */
  offScreen: boolean;
  canFocusTarget: boolean;
  sessionOnly: boolean;
}>();
const emit = defineEmits<{
  (e: "collapse"): void;
  (e: "pause"): void;
  (e: "back"): void;
  (e: "next"): void;
  (e: "focus-target"): void;
  (e: "restart"): void;
}>();

const titleEl = ref<HTMLElement | null>(null);
defineExpose({ focusTitle: () => titleEl.value?.focus() });
</script>

<template>
  <header class="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2">
    <span class="grow truncate text-xs font-medium text-fg-secondary">{{ chapterTitle }}</span>
    <IconButton
      label="Collapse guide"
      data-testid="guide-collapse"
      @click="emit('collapse')"
    >
      &#8212;
    </IconButton>
    <IconButton
      label="Close guide"
      data-testid="guide-close"
      @click="emit('pause')"
    >
      &#10005;
    </IconButton>
  </header>

  <div class="flex min-h-0 grow flex-col gap-2 overflow-y-auto px-3 py-2 text-sm">
    <p class="flex items-center justify-between text-micro uppercase tracking-wide text-fg-subtle">
      <span>Guided walkthrough</span>
      <span data-testid="guide-coach-count">{{ position }}</span>
    </p>
    <h2
      ref="titleEl"
      tabindex="-1"
      data-testid="guide-coach-title"
      class="text-base font-semibold text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    >
      {{ title }}
    </h2>
    <p data-testid="guide-coach-body">
      {{ copy.body }}
    </p>
    <p
      data-testid="guide-coach-tip"
      class="text-xs text-fg-muted"
    >
      {{ copy.tip }}
    </p>
    <p
      v-if="copy.task"
      data-testid="guide-coach-task"
      class="rounded-control border px-2 py-1 text-xs"
      :class="warning ? 'border-gold bg-gold-bg text-gold' : 'border-line text-fg-secondary'"
    >
      {{ copy.task }}
    </p>
    <p
      v-if="offScreen"
      data-testid="guide-coach-missing"
      class="text-xs text-fg-subtle"
    >
      This control is not on screen right now. Back and Next still work.
    </p>
    <div class="flex flex-wrap items-center gap-2 text-xs">
      <button
        v-if="canFocusTarget"
        type="button"
        data-testid="guide-focus-target"
        class="cursor-pointer rounded px-1 text-fg-secondary underline hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="emit('focus-target')"
      >
        Focus control (F6)
      </button>
      <GuideStartOver
        :key="stepId"
        @restart="emit('restart')"
      />
    </div>
  </div>

  <footer class="flex shrink-0 flex-col gap-1 border-t border-line px-3 py-2">
    <div class="flex items-center gap-2">
      <AppButton
        size="sm"
        variant="ghost"
        data-testid="guide-pause"
        @click="emit('pause')"
      >
        Pause guide
      </AppButton>
      <span class="grow" />
      <AppButton
        size="sm"
        variant="secondary"
        data-testid="guide-back"
        :disabled="!canGoBack"
        @click="emit('back')"
      >
        Back
      </AppButton>
      <AppButton
        size="sm"
        data-testid="guide-next"
        @click="emit('next')"
      >
        {{ isLast ? "Finish" : "Next" }}
      </AppButton>
    </div>
    <p
      v-if="sessionOnly"
      data-testid="guide-coach-session-only"
      class="text-micro text-fg-subtle"
    >
      Session only: progress lasts until the editor closes.
    </p>
    <div
      class="h-1 overflow-hidden rounded bg-raised"
      aria-hidden="true"
    >
      <div
        class="h-full bg-accent"
        :style="{ width: `${progressPct}%` }"
      />
    </div>
  </footer>
</template>
