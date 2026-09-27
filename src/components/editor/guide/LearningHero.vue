<script setup lang="ts">
/**
 * The learning center's hero (visual-parity Task 23; concept spec §9.3,
 * SCREENS 11): the overline, the heading and the one action that starts,
 * resumes or revisits the walkthrough — with a line saying where it picks
 * up — and, on the right, the progress ring: the share of the 22 lessons
 * READ (`reviewed`), never the `completed` flag a revisit clears
 * (docs/Gaps.md GAP-205 (6)). "Walkthrough reviewed" is every lesson read
 * (`hasFinished`).
 */
import { computed } from "vue";

import { GUIDE_CHAPTERS, GUIDE_STEPS } from "../../../editor/guide/content";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import EditorIcon from "../icons/EditorIcon.vue";

const emit = defineEmits<{ (e: "resume"): void }>();

const onboarding = useEditorOnboardingStore();
const total = GUIDE_STEPS.length;

const percent = computed(() => Math.round((onboarding.reviewedCount / total) * 100));
/** What `start()` will do: a completed guide is revisited from lesson 1. */
const action = computed(() => {
  const p = onboarding.progress;
  if (p.completed) return { icon: "bookmark" as const, label: "Revisit walkthrough" };
  if (p.currentStepId === null) return { icon: "play" as const, label: "Start walkthrough" };
  return { icon: "bookmark" as const, label: "Resume walkthrough" };
});
const detail = computed(() => {
  const p = onboarding.progress;
  if (p.completed) return `${onboarding.reviewedCount} of ${total} steps reviewed. Repeat any chapter below.`;
  const at = GUIDE_STEPS.findIndex((s) => s.id === p.currentStepId);
  if (at >= 0) return `Continue at step ${at + 1} · ${GUIDE_STEPS[at].title}`;
  return `${GUIDE_CHAPTERS.length} chapters · ${total} short steps · no setup needed`;
});
</script>

<template>
  <div
    class="flex items-center justify-between gap-7 border-b border-line px-[34px] pt-[31px] pb-[25px]"
    style="background: linear-gradient(108deg, color-mix(in srgb, var(--color-accent-bg) 53%, var(--color-panel)), var(--color-panel))"
  >
    <div class="min-w-0 flex-1">
      <p
        v-if="onboarding.hasFinished"
        data-testid="learning-finished"
        class="text-[10px] leading-[1.6] font-bold tracking-[1.7px] text-accent"
      >
        WALKTHROUGH REVIEWED
      </p>
      <p
        v-else
        class="text-[10px] leading-[1.6] font-bold tracking-[1.7px] text-accent"
      >
        YOUR EDITOR, EXPLAINED
      </p>
      <h2 class="my-3 max-w-[425px] text-[29px] leading-[1.13] font-[650] tracking-[-0.8px] text-fg">
        {{ onboarding.hasFinished ? "A reference whenever you need it." : "From recording to a clear tutorial." }}
      </h2>
      <p class="mb-5 text-[13px] leading-[1.65] text-fg-secondary">
        One explained control at a time. Skip anything. Come back anytime.
      </p>
      <button
        type="button"
        data-testid="learning-resume"
        class="flex min-h-10 items-center gap-2 rounded-[7px] border-transparent bg-primary pr-4 pl-[15px] text-xs font-semibold text-white enabled:hover:bg-primary-hover"
        @click="emit('resume')"
      >
        <EditorIcon
          :name="action.icon"
          :size="15"
        />
        {{ action.label }}
      </button>
      <p
        data-testid="learning-resume-detail"
        class="mt-[11px] text-[11px] text-fg-muted"
      >
        {{ detail }}
      </p>
    </div>
    <div
      data-testid="learning-progress"
      role="img"
      :aria-label="`${onboarding.reviewedCount} of ${total} steps reviewed`"
      class="mr-3 grid h-32 w-32 shrink-0 place-items-center rounded-full p-1"
      :style="{ background: `conic-gradient(var(--color-accent) ${percent}%, var(--color-line) ${percent}%)` }"
    >
      <div class="flex h-full w-full flex-col items-center justify-center gap-[3px] rounded-full bg-panel">
        <b class="text-[31px] leading-none font-semibold tracking-[-1.5px] text-fg">{{ percent }}%</b>
        <span class="text-[10px] text-fg-muted">{{ onboarding.reviewedCount }} / {{ total }} reviewed</span>
      </div>
    </div>
  </div>
</template>
