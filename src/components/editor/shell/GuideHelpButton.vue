<script setup lang="ts">
/**
 * The header's **Help** menu (Tasks 55–57; SCREENS-AND-INTERACTIONS.md
 * §02: the header owns "Help, Checks, Save project and Render video";
 * ONBOARDING.md: "Help → Resume walkthrough returns to the same stable
 * lesson") — the guide's `header.help` target — and, when guide progress
 * cannot be stored, "Session only" beside it (ONBOARDING.md § State and
 * persistence). Split out of `EditorHeader` so the header's own template
 * stays under the complexity ratchet.
 *
 * Four items: **Learning center** (chapters, lessons, quick answers,
 * progress and preferences — `LearningCenter`), **Resume walkthrough**
 * (the coach at the exact saved lesson; F1 and ? do the same from the
 * keyboard), **Keyboard shortcuts** (the learning center, on its
 * shortcut table) and, since Task 58, **Export diagnostics** (Rust's own
 * save dialog writes counts, capabilities and error codes — never project
 * content — to a new file; a toast says where it landed). There is
 * deliberately no item for anything not built yet. Since visual-parity
 * Task 23 (ruling T8-1) the menu is the one `MenuPanel` (`helpMenu.ts`
 * holds the items), so it opens, moves, closes and gives focus back
 * exactly like the Project and View menus.
 *
 * Visual-parity Task 8 (concept spec §2 `.guide-help-button`): the book
 * icon in the accent colour, and a 5px gold dot after the label while a
 * walkthrough is paused part way, so Help says there is a lesson to come
 * back to (F1 resumes it).
 *
 * Visual-parity Task 11: the preview header's View menu asks for the
 * shortcut table ("Keyboard shortcuts & help…") through the reveal bus —
 * the learning center stays this component's.
 */
import { computed, ref } from "vue";

import { useDiagnosticsExport } from "../../../composables/useDiagnosticsExport";
import { useGuideTarget } from "../../../composables/useGuideTarget";
import { HELP_MENU_HEADING, HELP_MENU_SUBTITLE, helpMenuItems } from "../../../editor/helpMenu";
import { onReveal } from "../../../editor/revealBus";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import type { LearningTab } from "../guide/LearningCenter.vue";
import LearningCenter from "../guide/LearningCenter.vue";
import MenuPanel from "../menus/MenuPanel.vue";
import HeaderButton from "./HeaderButton.vue";

const onboarding = useEditorOnboardingStore();
/** A walkthrough stopped part way: a lesson to resume, the coach not up. */
const paused = computed(() => onboarding.progress.currentStepId !== null && !onboarding.progress.active);
const helpTarget = useGuideTarget("header.help");

const trigger = ref<HTMLButtonElement | null>(null);
function setTrigger(el: unknown): void {
  helpTarget(el as HTMLButtonElement | null);
  const root: unknown = (el as { $el?: unknown } | null)?.$el ?? el;
  trigger.value = root instanceof HTMLButtonElement ? root : null;
}
const open = ref(false);
const centerOpen = ref(false);
const centerTab = ref<LearningTab>("walkthrough");

const { exportDiagnostics } = useDiagnosticsExport();

/** The learning center, on `tab`. Focus goes to Help first: the center's
 * `DialogHost` gives focus back to whatever had it when it opened, and the
 * menu item (or the coach's chapter title) that asked is gone by then. */
function openCenter(tab: LearningTab): void {
  trigger.value?.focus();
  centerTab.value = tab;
  centerOpen.value = true;
}

const items = computed(() =>
  helpMenuItems({
    openLearningCenter: () => openCenter("walkthrough"),
    resumeWalkthrough: () => onboarding.start(),
    openShortcuts: () => openCenter("shortcuts"),
    exportDiagnostics: () => void exportDiagnostics(),
  }),
);

/** The View menu's "Keyboard shortcuts & help…" (visual-parity Task 11)
 * and the coach's chapter title (Task 23): this button owns the center. */
onReveal("shortcuts", () => openCenter("shortcuts"));
onReveal("learningCenter", () => openCenter("walkthrough"));

/** The open menu closes itself on a press outside it; the trigger is
 * outside it, so without this a press there would close and reopen it. */
function onTriggerPointerDown(event: PointerEvent): void {
  if (open.value) event.stopPropagation();
}
</script>

<template>
  <HeaderButton
    :ref="setTrigger"
    icon="book"
    icon-class="text-accent"
    data-testid="editor-header-help"
    aria-haspopup="menu"
    :aria-expanded="open"
    title="Help, the learning center and the guided walkthrough (F1 resumes it)"
    @pointerdown="onTriggerPointerDown"
    @click="open = !open"
  >
    Help
    <span
      v-if="paused"
      data-testid="editor-header-help-resume-dot"
      aria-hidden="true"
      class="h-[5px] w-[5px] rounded-full bg-gold"
    />
  </HeaderButton>
  <MenuPanel
    v-if="open && trigger"
    testid="editor-help-menu"
    :heading="HELP_MENU_HEADING"
    :subtitle="HELP_MENU_SUBTITLE"
    :items="items"
    :anchor="trigger"
    @close="open = false"
  />
  <span
    v-if="onboarding.sessionOnly"
    data-testid="editor-header-guide-session-only"
    title="Guide progress cannot be stored on this device right now. It lasts until the editor closes; Help → Learning center can save it to a file."
    class="text-micro text-fg-subtle"
  >Session only</span>
  <LearningCenter
    :open="centerOpen"
    :tab="centerTab"
    @close="centerOpen = false"
  />
</template>
