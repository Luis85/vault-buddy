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
 * **The concept's hub (visual-parity Task 23; concept spec §9.3, SCREENS
 * 11)**: 870 wide, scrolling as a whole (`DialogHost`'s `flush` body): the
 * "Help & learning center" bar with its ✕, the hero (`LearningHero`: the
 * start/resume action and the progress ring), the underlined tabs, the
 * section, and "Progress & preferences" as the footer's disclosure
 * (`LearningPreferences`: Save progress file… and Load progress file…, the
 * latter Task 57's "Restore progress file…" under the concept's name).
 *
 * **The tabs are a real tablist** (GAP-207 (5), deferred from Task 57's
 * review): each `role="tab"` names the `role="tabpanel"` it controls via
 * `aria-controls`, the panel names the active tab back via
 * `aria-labelledby` (the `InspectorPanel.vue`/`LibraryPanel.vue`
 * precedent), and ArrowLeft/Right/Home/End move focus AND selection
 * through the shared `useRovingTablist` composable — the same one those
 * two panels use, rather than a third hand-rolled copy (`cloneGroups 0`).
 */
import { ref, watch } from "vue";

import { useRovingTablist } from "../../../composables/useRovingTablist";
import type { GuideStepId } from "../../../editor/guide/content";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import EditorIcon from "../icons/EditorIcon.vue";
import DialogHost from "../shell/DialogHost.vue";
import LearningAnswers from "./LearningAnswers.vue";
import LearningHero from "./LearningHero.vue";
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
    flush
    @close="emit('close')"
  >
    <div
      data-testid="learning-center"
      class="flex flex-col"
    >
      <header class="flex items-center justify-between border-b border-line px-[21px] py-[13px] text-xs text-fg-secondary">
        <span class="flex items-center gap-[9px]">
          <EditorIcon
            name="book"
            class="text-accent"
          />
          Help &amp; learning center
        </span>
        <button
          type="button"
          data-testid="learning-close"
          aria-label="Close Help"
          class="flex h-8 w-8 items-center justify-center border-transparent bg-transparent p-1.5 text-fg-secondary"
          @click="emit('close')"
        >
          <EditorIcon name="x" />
        </button>
      </header>

      <LearningHero @resume="resume" />

      <div
        role="tablist"
        aria-label="Help sections"
        data-testid="learning-tablist"
        class="flex gap-[23px] border-b border-line px-[34px]"
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
          class="relative rounded-none border-transparent bg-transparent pt-[15px] pb-3 text-xs font-[550] hover:bg-transparent"
          :class="active === t.id ? 'text-accent-ink after:absolute after:right-0 after:-bottom-px after:left-0 after:h-0.5 after:bg-accent' : 'text-fg-muted hover:text-fg'"
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
        class="px-[34px] py-6"
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
