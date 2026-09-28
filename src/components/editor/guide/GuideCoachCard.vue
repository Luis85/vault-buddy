<script setup lang="ts">
/**
 * What the coach card says (Task 56; visual-parity Task 23; concept spec
 * §9.2, SCREENS 10). Presentational: every action is an emit `GuideCoach`
 * answers, and the text is `lessonCopy` — this app's corrected copy, never
 * a raw step (GAP-203). Its parts render straight into the coach's own
 * `<section>` (a fragment), so the coach measures and caps them as one
 * card; only the copy scrolls when the card is squeezed.
 *
 * - **Top bar**: the book and the chapter's title — a button that opens the
 *   learning center's chapters — then Minimize and ✕ (pause; Help resumes).
 * - **Copy**: "GUIDED WALKTHROUGH" with the mono step counter, the lesson's
 *   title (focused when the coach opens), its body, the tip, and the task
 *   box in one of three voices: a plain prompt, an optional EDIT (gold —
 *   ONBOARDING.md: "Every optional action that changes the project says
 *   so"), or, once the control was tried, "Control explored" (teal). A gold
 *   line says when the control is not on screen; Focus control (F6) and
 *   the two-step Start over follow.
 * - **Nav**: Pause guide, Back (disabled, with its reason, on the first
 *   lesson), Next ›/Finish guide ✓; then the foot — where progress is kept
 *   (design D10: "Progress remembered on this PC" / "Session only") and
 *   "F6 to focus control" — and the 3px progress line.
 */
import { computed, ref } from "vue";

import type { LessonCopy } from "../../../editor/guide/content";
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";
import GuideStartOver from "./GuideStartOver.vue";

const props = defineProps<{
  stepId: string;
  title: string;
  /** The lesson's task is an optional edit. */
  warning: boolean;
  /** The lesson's control was tried (`markExplored`). */
  explored: boolean;
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
  (e: "contents"): void;
  (e: "collapse"): void;
  (e: "pause"): void;
  (e: "back"): void;
  (e: "next"): void;
  (e: "focus-target"): void;
  (e: "restart"): void;
}>();

type TaskVoice = "tried" | "edit" | "prompt";
const TASK_ICON: Record<TaskVoice, EditorIconName> = { tried: "check", edit: "edit", prompt: "cursor" };
const TASK_CLASS: Record<TaskVoice, string> = {
  tried: "border-[color-mix(in_srgb,var(--color-audio)_30%,transparent)] bg-audio-bg text-audio",
  edit: "border-[color-mix(in_srgb,var(--color-gold)_25%,transparent)] bg-gold-bg text-gold",
  prompt: "border-[color-mix(in_srgb,var(--color-accent)_16%,transparent)] bg-accent-bg text-accent-ink",
};
const voice = computed<TaskVoice>(() => (props.explored ? "tried" : props.warning ? "edit" : "prompt"));
const taskText = computed(() => {
  if (props.explored) return "Control explored. Continue whenever you are ready.";
  return props.copy.task ?? "Read this step, then continue when you are ready.";
});

const titleEl = ref<HTMLElement | null>(null);
defineExpose({ focusTitle: () => titleEl.value?.focus() });
</script>

<template>
  <header class="flex shrink-0 items-center justify-between gap-1 border-b border-line py-2 pr-[9px] pl-3.5">
    <button
      type="button"
      data-testid="guide-coach-contents"
      title="Open all walkthrough chapters"
      class="flex min-h-0 min-w-0 items-center gap-1.5 border-transparent bg-transparent p-[3px] text-left text-[11px] leading-[1.35] text-fg-muted"
      @click="emit('contents')"
    >
      <EditorIcon
        name="book"
        :size="15"
        class="shrink-0 text-accent"
      />
      <span class="truncate">{{ chapterTitle }}</span>
    </button>
    <div class="flex shrink-0 gap-px">
      <button
        type="button"
        data-testid="guide-collapse"
        aria-label="Minimize guide"
        class="flex h-7 min-h-7 w-[27px] items-center justify-center border-transparent bg-transparent p-1.5 text-fg-secondary"
        @click="emit('collapse')"
      >
        <EditorIcon name="minus" />
      </button>
      <button
        type="button"
        data-testid="guide-close"
        aria-label="Dismiss guide and keep progress"
        class="flex h-7 min-h-7 w-[27px] items-center justify-center border-transparent bg-transparent p-1.5 text-fg-secondary"
        @click="emit('pause')"
      >
        <EditorIcon name="x" />
      </button>
    </div>
  </header>

  <div class="min-h-0 grow overflow-y-auto px-[21px] pt-[17px] pb-[7px]">
    <p class="flex items-center justify-between gap-3 text-[9px] font-semibold tracking-[1.15px] text-fg-muted">
      <span>GUIDED WALKTHROUGH</span>
      <span
        data-testid="guide-coach-count"
        class="vb-mono text-[11px] font-normal tracking-normal text-accent"
      >{{ position }}</span>
    </p>
    <h2
      ref="titleEl"
      tabindex="-1"
      data-testid="guide-coach-title"
      class="mt-3 mb-[11px] rounded-[2px] text-[23px] leading-[1.18] font-[650] tracking-[-0.55px] text-fg focus:outline-none focus-visible:outline-2 focus-visible:outline-offset-[5px] focus-visible:outline-ring"
    >
      {{ title }}
    </h2>
    <p
      data-testid="guide-coach-body"
      class="mb-[13px] text-[13px] leading-[1.7] text-fg-secondary"
    >
      {{ copy.body }}
    </p>
    <div class="flex gap-[9px] border-t border-line pt-[13px] pb-[11px]">
      <EditorIcon
        name="info"
        :size="14"
        class="mt-px shrink-0 text-fg-muted"
      />
      <p
        data-testid="guide-coach-tip"
        class="text-[11px] leading-[1.65] text-fg-muted"
      >
        {{ copy.tip }}
      </p>
    </div>
    <div
      data-testid="guide-coach-task"
      :data-voice="voice"
      class="flex gap-[9px] rounded-[8px] border px-3 py-[11px]"
      :class="TASK_CLASS[voice]"
    >
      <EditorIcon
        :name="TASK_ICON[voice]"
        :size="14"
        class="mt-0.5 shrink-0"
      />
      <p class="text-[11px] leading-[1.6]">
        {{ taskText }}
      </p>
    </div>
    <p
      v-if="offScreen"
      data-testid="guide-coach-missing"
      role="status"
      class="mt-2.5 rounded-[6px] bg-gold-bg p-[9px] text-[11px] text-gold"
    >
      This control is not on screen right now. Back and Next still work.
    </p>
    <div class="mt-2 flex flex-wrap items-center justify-between gap-3 text-[11px]">
      <button
        v-if="canFocusTarget"
        type="button"
        data-testid="guide-focus-target"
        class="border-transparent bg-transparent px-0.5 py-[5px] text-left text-accent-ink hover:underline"
        @click="emit('focus-target')"
      >
        {{ offScreen ? "Show control" : "Focus control" }}
      </button>
      <GuideStartOver
        :key="stepId"
        @restart="emit('restart')"
      />
    </div>
  </div>

  <footer class="shrink-0">
    <div class="flex items-center justify-between gap-2 border-t border-line px-4 pt-2.5 pb-[13px]">
      <button
        type="button"
        data-testid="guide-pause"
        class="border-transparent bg-transparent px-0.5 text-[11px] text-fg-muted hover:underline"
        @click="emit('pause')"
      >
        Pause guide
      </button>
      <div class="flex gap-[7px]">
        <button
          type="button"
          data-testid="guide-back"
          :disabled="!canGoBack"
          :title="canGoBack ? undefined : 'This is the first step.'"
          class="min-h-[35px] rounded-[7px] border border-line bg-panel px-[13px] py-[7px] text-xs text-fg"
          @click="emit('back')"
        >
          Back
        </button>
        <button
          type="button"
          data-testid="guide-next"
          class="flex min-h-[35px] items-center gap-1.5 rounded-[7px] border-transparent bg-primary px-[13px] py-[7px] text-xs font-semibold text-white enabled:hover:bg-primary-hover"
          @click="emit('next')"
        >
          {{ isLast ? "Finish guide" : "Next" }}
          <EditorIcon
            :name="isLast ? 'check' : 'chevronRight'"
            :size="14"
          />
        </button>
      </div>
    </div>
    <p class="flex justify-between gap-2 px-[18px] pb-[11px] text-[9px] leading-[1.5] text-fg-muted">
      <span data-testid="guide-coach-storage">{{ sessionOnly ? "Session only" : "Progress remembered on this PC" }}</span>
      <span class="shrink-0">F6 to focus control</span>
    </p>
    <div
      class="h-[3px] bg-line"
      aria-hidden="true"
    >
      <div
        data-testid="guide-coach-progress"
        class="vb-guide-progress h-full bg-accent"
        :style="{ width: `${progressPct}%` }"
      />
    </div>
  </footer>
</template>
