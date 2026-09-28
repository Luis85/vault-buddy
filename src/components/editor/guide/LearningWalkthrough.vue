<script setup lang="ts">
/**
 * The learning center's Walkthrough section (Task 57; F-47; visual-parity
 * Task 23, concept spec §9.3): "Pick up a skill.", then the seven chapters
 * as cards in two columns (the last spanning both). A card's main button
 * jumps to the chapter's first lesson; "See n steps" opens its lessons,
 * each a jump — numbered, or ✓ once read, the one the walkthrough is paused
 * on marked "Resume here". A chapter whose lessons were all read turns
 * teal with a check. **Start over** is the coach's own two-step confirm
 * (`GuideStartOver`): it resets guide state only (ONBOARDING.md). The
 * safety note closes the section: the guide never edits.
 */
import { computed } from "vue";

import type { GuideStepId } from "../../../editor/guide/content";
import { GUIDE_CHAPTERS, GUIDE_STEPS } from "../../../editor/guide/content";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";
import GuideStartOver from "./GuideStartOver.vue";

const emit = defineEmits<{
  (e: "jump", id: GuideStepId): void;
  (e: "restart"): void;
}>();

const onboarding = useEditorOnboardingStore();

const TILE = {
  done: "border-[color-mix(in_srgb,var(--color-audio)_15%,transparent)] bg-audio-bg text-audio",
  open: "border-[color-mix(in_srgb,var(--color-accent)_15%,transparent)] bg-accent-bg text-accent",
};
const ROW = {
  current: "bg-accent-bg text-accent-ink",
  other: "bg-transparent text-fg-secondary hover:bg-hover-subtle",
};

const chapters = computed(() => {
  const p = onboarding.progress;
  const paused = !p.active && !p.completed;
  return GUIDE_CHAPTERS.map((chapter, i) => {
    const lessons = GUIDE_STEPS.filter((s) => s.chapter === chapter.id).map((s) => {
      const read = p.reviewed.includes(s.id);
      const current = s.id === p.currentStepId;
      return {
        id: s.id,
        title: s.title,
        mark: read ? "✓" : String(GUIDE_STEPS.indexOf(s) + 1).padStart(2, "0"),
        read,
        ariaCurrent: current ? ("step" as const) : undefined,
        rowClass: current ? ROW.current : ROW.other,
        resumeHere: paused && current,
      };
    });
    const read = lessons.filter((l) => l.read).length;
    const done = read === lessons.length;
    return {
      ...chapter,
      icon: (done ? "check" : chapter.icon) as EditorIconName,
      tileClass: done ? TILE.done : TILE.open,
      cardClass: i === GUIDE_CHAPTERS.length - 1 ? "sm:col-span-2" : "",
      label: `Open chapter ${i + 1}: ${chapter.title}`,
      lessons,
      read,
    };
  });
});
</script>

<template>
  <section>
    <div class="mb-[17px] flex items-center justify-between gap-4">
      <div>
        <h3 class="mb-[5px] text-[15px] font-semibold tracking-[-0.2px] text-fg">
          Pick up a skill.
        </h3>
        <p class="text-xs text-fg-muted">
          Start at the beginning or jump straight to what you need.
        </p>
      </div>
      <GuideStartOver @restart="emit('restart')" />
    </div>
    <div class="grid grid-cols-1 gap-[11px] sm:grid-cols-2">
      <article
        v-for="chapter in chapters"
        :key="chapter.id"
        :data-testid="`learning-card-${chapter.id}`"
        class="min-w-0 self-start overflow-hidden rounded-[10px] border border-line bg-app"
        :class="chapter.cardClass"
      >
        <button
          type="button"
          :data-testid="`learning-chapter-${chapter.id}`"
          :aria-label="chapter.label"
          class="flex min-h-16 w-full items-center gap-[11px] rounded-none border-transparent bg-transparent px-3 pt-[13px] pb-[9px] text-left hover:bg-hover-subtle"
          @click="emit('jump', chapter.lessons[0].id)"
        >
          <span
            class="grid h-[37px] w-[34px] shrink-0 place-items-center rounded-[9px] border"
            :class="chapter.tileClass"
          >
            <EditorIcon
              :name="chapter.icon"
              :size="19"
            />
          </span>
          <span class="min-w-0 flex-1">
            <b class="block text-xs leading-[1.35] font-semibold text-fg">{{ chapter.title }}</b>
            <small class="mt-[5px] block text-[10px] leading-[1.5] text-fg-muted">{{ chapter.subtitle }}</small>
          </span>
          <span class="shrink-0 text-[10px] text-fg-muted">{{ chapter.read }}/{{ chapter.lessons.length }}</span>
          <EditorIcon
            name="chevronRight"
            :size="13"
            class="shrink-0 text-fg-muted"
          />
        </button>
        <details class="group">
          <summary class="min-h-[25px] cursor-pointer list-none pr-3 pb-2.5 pl-[58px] text-[10px] text-fg-muted">
            See {{ chapter.lessons.length }} steps
            <span
              aria-hidden="true"
              class="text-xs group-open:hidden"
            >+</span>
            <span
              aria-hidden="true"
              class="hidden text-xs group-open:inline"
            >−</span>
          </summary>
          <div class="border-t border-line p-[5px]">
            <button
              v-for="lesson in chapter.lessons"
              :key="lesson.id"
              type="button"
              :data-testid="`learning-lesson-${lesson.id}`"
              :aria-current="lesson.ariaCurrent"
              class="flex min-h-[35px] w-full items-center gap-[9px] border-transparent p-[7px] text-left text-[11px]"
              :class="lesson.rowClass"
              @click="emit('jump', lesson.id)"
            >
              <span
                class="vb-mono w-5 shrink-0 text-accent"
                aria-hidden="true"
              >{{ lesson.mark }}</span>
              <span
                v-if="lesson.read"
                class="sr-only"
              >Read:</span>
              <span class="min-w-0 flex-1">{{ lesson.title }}</span>
              <small
                v-if="lesson.resumeHere"
                class="ml-auto shrink-0 text-[9px] text-accent"
              >Resume here</small>
            </button>
          </div>
        </details>
      </article>
    </div>
    <div class="mt-5 flex items-start gap-2.5 rounded-[9px] border border-line p-[15px] text-[11px] text-fg-muted">
      <EditorIcon
        name="lock"
        class="mt-0.5 shrink-0 text-accent"
      />
      <p class="leading-[1.7]">
        <b class="text-fg-secondary">Your edit stays yours.</b> The guide does not automatically cut, record or render. You
        can simply read and choose Next. Controls you use still edit the current project.
      </p>
    </div>
  </section>
</template>
