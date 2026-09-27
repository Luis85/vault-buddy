<script lang="ts">
/** Which section the learning center opens on. */
export type LearningTab = "walkthrough" | "answers" | "shortcuts";
</script>

<script setup lang="ts">
/**
 * The learning center (Task 57; F-47; ONBOARDING.md: "chapter navigation,
 * individual lessons, searchable quick answers, shortcuts,
 * progress/preferences and restart"; SCREENS-AND-INTERACTIONS.md §11).
 * Opened from Help → Learning center or Help → Keyboard shortcuts.
 *
 * A `DialogHost` modal, so while it is open the coach is suspended (a
 * modal "suspends the coach; closing it resumes the same lesson"). Every
 * control here changes GUIDE state only — which lesson, what was read, two
 * presentation preferences — through `editorOnboarding`; nothing sends an
 * edit, a save, a device request or a render. A lesson or chapter jump,
 * Resume and Start over close the center so the coach can show.
 *
 * Progress is "lessons read" out of 22, and "finished" means every lesson
 * was read (`hasFinished`, from `reviewed`) — never the `completed` flag a
 * revisit clears (docs/Gaps.md GAP-205 (6)).
 *
 * **The tabs are a real tablist** (GAP-207 (5), deferred from Task 57's
 * review): each `role="tab"` names the `role="tabpanel"` it controls via
 * `aria-controls`, the panel names the active tab back via
 * `aria-labelledby` (the `InspectorPanel.vue`/`LibraryPanel.vue`
 * precedent), and ArrowLeft/Right/Home/End move focus AND selection
 * through the shared `useRovingTablist` composable — the same one those
 * two panels use, rather than a third hand-rolled copy (`cloneGroups 0`).
 */
import { computed, ref, watch } from "vue";

import { useRovingTablist } from "../../../composables/useRovingTablist";
import type { GuideStepId } from "../../../editor/guide/content";
import { GUIDE_STEPS } from "../../../editor/guide/content";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";
import LearningAnswers from "./LearningAnswers.vue";
import LearningPreferences from "./LearningPreferences.vue";
import LearningShortcuts from "./LearningShortcuts.vue";
import LearningWalkthrough from "./LearningWalkthrough.vue";

const props = defineProps<{ open: boolean; tab: LearningTab }>();
const emit = defineEmits<{ (e: "close"): void }>();

const TABS: readonly { id: LearningTab; label: string }[] = [
  { id: "walkthrough", label: "Walkthrough" },
  { id: "answers", label: "Quick answers" },
  { id: "shortcuts", label: "Shortcuts" },
];

const onboarding = useEditorOnboardingStore();
const active = ref<LearningTab>(props.tab);
watch(
  () => props.open,
  (open) => {
    if (open) active.value = props.tab;
  },
);

// ---- roving tabindex over the tablist (the InspectorPanel/LibraryPanel
// precedent, via the shared composable) ------------------------------------

function tabId(id: LearningTab): string {
  return `learning-tab-${id}`;
}
function panelId(id: LearningTab): string {
  return `learning-panel-${id}`;
}

const { setTabRef, onKeydown: onTablistKeydown } = useRovingTablist(
  () => TABS.length,
  () => TABS.findIndex((t) => t.id === active.value),
  (i) => {
    active.value = TABS[i].id;
  },
);

const total = GUIDE_STEPS.length;
/** Says what `start()` will do: a finished guide is revisited from lesson
 * 1, so it is "again", never a resume. */
const resumeLabel = computed(() => {
  const p = onboarding.progress;
  if (p.completed) return "Start walkthrough again";
  return p.currentStepId === null ? "Start walkthrough" : "Resume walkthrough";
});

function jump(id: GuideStepId): void {
  onboarding.jumpTo(id);
  emit("close");
}
function resume(): void {
  onboarding.start();
  emit("close");
}
function restart(): void {
  onboarding.restart();
  emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Help and learning center"
    :width="870"
    close-testid="learning-close"
    @close="emit('close')"
  >
    <template #title>
      Help and learning center
    </template>

    <div
      data-testid="learning-center"
      class="flex flex-col gap-3"
    >
      <div class="flex flex-wrap items-center gap-3">
        <p
          data-testid="learning-progress"
          class="min-w-0 flex-1 text-xs text-fg-muted"
        >
          {{ onboarding.reviewedCount }} / {{ total }} lessons read
          <span
            v-if="onboarding.hasFinished"
            data-testid="learning-finished"
            class="ml-1 text-success"
          >· You have read every lesson</span>
        </p>
        <AppButton
          size="sm"
          data-testid="learning-resume"
          @click="resume"
        >
          {{ resumeLabel }}
        </AppButton>
      </div>

      <div
        role="tablist"
        aria-label="Help sections"
        data-testid="learning-tablist"
        class="flex gap-1 border-b border-line"
        @keydown="onTablistKeydown"
      >
        <button
          v-for="(t, i) in TABS"
          :id="tabId(t.id)"
          :key="t.id"
          :ref="(el) => setTabRef(i, el as Element | null)"
          type="button"
          role="tab"
          :aria-selected="active === t.id"
          :aria-controls="panelId(t.id)"
          :tabindex="active === t.id ? 0 : -1"
          :data-testid="`learning-tab-${t.id}`"
          class="cursor-pointer border-b-2 px-2 py-1 text-xs focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          :class="active === t.id ? 'border-violet-400 text-fg' : 'border-transparent text-fg-muted hover:text-fg'"
          @click="active = t.id"
        >
          {{ t.label }}
        </button>
      </div>

      <div
        :id="panelId(active)"
        role="tabpanel"
        :aria-labelledby="tabId(active)"
        data-testid="learning-panel"
      >
        <LearningWalkthrough
          v-if="active === 'walkthrough'"
          @jump="jump"
          @restart="restart"
        />
        <LearningAnswers
          v-else-if="active === 'answers'"
          @jump="jump"
        />
        <LearningShortcuts v-else />
      </div>

      <LearningPreferences />
    </div>
  </DialogHost>
</template>
