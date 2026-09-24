<script setup lang="ts">
/**
 * The guided walkthrough's coach (Task 56; F-46, F-49; SCREENS 10;
 * ONBOARDING.md; A23–A26): a card that explains one lesson and a ring around
 * the REAL control that lesson is about — never a copy of it, and never
 * pressing it. Trying the control is recorded (`markExplored`, a click
 * inside it) and never advances the lesson; only Back/Next do.
 *
 * - **Where.** The lesson's typed key is resolved through the registry and
 *   tracked (`useCoachTarget`); the card goes beside it without covering it,
 *   or docks below 1100 px (`position.ts`). Before a lesson shows, its safe
 *   preparation reveals the tab, drawer or selection its control needs
 *   (`prepare.ts`) — view state only.
 * - **Nothing reaches a render.** Ring and card are fixed-position siblings
 *   mounted by `EditorShell` OUTSIDE the preview section, drawn over the
 *   page, pointer-transparent (the ring) — never inside `PreviewSurface`,
 *   whose stage is what a render mirrors.
 * - **Keys.** Inside the card every key is the card's (editing shortcuts
 *   never fire from it); F6 moves focus to the control and back
 *   (`toggleFocus`, also what `EditorShell`'s F6 calls from anywhere else);
 *   Escape pauses. Escape elsewhere pauses too, but only once no menu is
 *   open (`dismiss`, `isGuideDismissKey`).
 * - **Suspension.** While a modal dialog is open (`DialogHost` →
 *   `editorOnboarding.suspended`) neither ring nor card renders; closing it
 *   shows the same lesson, and focus is the dialog's to restore.
 * - **Dimming** is optional (`preferences.dimming`) and off whenever motion
 *   is reduced; it is a shadow around the ring, so it never blocks a click.
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";

import { useCoachTarget } from "../../../composables/useCoachTarget";
import { GUIDE_CHAPTERS, GUIDE_STEPS, lessonCopy, STEP_TARGETS } from "../../../editor/guide/content";
import type { Size } from "../../../editor/guide/position";
import { placeCoach } from "../../../editor/guide/position";
import { prepareLesson } from "../../../editor/guide/prepare";
import { resolve } from "../../../editor/guide/targets";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import GuideCoachCard from "./GuideCoachCard.vue";

const guide = useEditorOnboardingStore();
const workspace = useEditorWorkspaceStore();
const editorProject = useEditorProjectStore();

const step = computed(() => GUIDE_STEPS[guide.stepIndex] ?? null);
const copy = computed(() => (step.value ? lessonCopy(step.value.id) : null));
const chapterTitle = computed(() => GUIDE_CHAPTERS.find((c) => c.id === step.value?.chapter)?.title ?? "");
const warning = computed(() => step.value?.warning === true);
const position = computed(() => `${guide.stepIndex + 1} / ${GUIDE_STEPS.length}`);
const isLast = computed(() => guide.stepIndex === GUIDE_STEPS.length - 1);
const progressPct = computed(() => ((guide.stepIndex + 1) / GUIDE_STEPS.length) * 100);

/** Open, not collapsed, lesson known — whether or not a dialog has it. */
const open = computed(() => guide.loaded && guide.progress.active && !guide.progress.collapsed && step.value !== null);
const showing = computed(() => open.value && !guide.suspended);
const collapsedShowing = computed(
  () => guide.loaded && guide.progress.active && guide.progress.collapsed && !guide.suspended && step.value !== null,
);

const target = useCoachTarget(() => (step.value ? STEP_TARGETS[step.value.id] : null), () => showing.value);
const targetState = computed(() => target.state());
const offScreen = computed(() => targetState.value === "missing" || targetState.value === "hidden");
const canFocusTarget = computed(() => target.found.value !== null);
const ringVisible = computed(() => showing.value && target.rect.value !== null);

// ---- placement ------------------------------------------------------------

const CARD_WIDTH = 340;
/** Before the card has rendered once (and in a layout-less test DOM). */
const FALLBACK_HEIGHT = 300;
const card = ref<HTMLElement | null>(null);
const cardHeight = ref(FALLBACK_HEIGHT);

/** The card's natural height: its parts' full content, even while its
 * body is scrolled inside a capped card. */
function measureCard(): void {
  const el = card.value;
  if (!el) return;
  const natural = [...el.children].reduce((h, c) => h + c.scrollHeight, 0);
  if (natural > 0) cardHeight.value = natural + 2;
}

const placement = computed(() => {
  const size: Size = { width: CARD_WIDTH, height: cardHeight.value };
  return placeCoach(target.rect.value, size, target.viewport.value);
});
const px = (n: number) => `${Math.round(n)}px`;
const cardStyle = computed(() => ({
  left: px(placement.value.x),
  top: px(placement.value.y),
  width: px(placement.value.width),
  maxHeight: px(placement.value.maxHeight),
}));
const ringStyle = computed(() => {
  const r = target.rect.value;
  return r ? { left: px(r.x - 4), top: px(r.y - 4), width: px(r.width + 8), height: px(r.height + 8) } : {};
});

function reducedMotion(): boolean {
  const motion = guide.progress.preferences.motion;
  if (motion !== "system") return motion === "reduced";
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}
/** The ring's label sits above the control, or inside the ring's lower
 * edge when the control is at the top of the window. */
const labelClass = computed(() => ((target.rect.value?.y ?? 0) < 28 ? "-bottom-6" : "-top-6"));
const dimmed = computed(() => guide.progress.preferences.dimming && !reducedMotion());
const ringClass = computed(() => ({ "guide-ring-dim": dimmed.value, "guide-ring-animated": !reducedMotion() }));

// ---- a lesson arriving -----------------------------------------------------

/** Prepares the lesson, then brings its control into view and measures. */
async function arrive(): Promise<void> {
  if (!step.value) return;
  prepareLesson(STEP_TARGETS[step.value.id], workspace, editorProject.project);
  await nextTick();
  target.measure();
  target.found.value?.element.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  target.measure();
  measureCard();
}
watch(
  () => (showing.value ? guide.progress.currentStepId : null),
  (id) => {
    if (id !== null) void arrive();
  },
  { immediate: true },
);

// ---- focus -----------------------------------------------------------------

const cardContent = ref<InstanceType<typeof GuideCoachCard> | null>(null);
const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

function focusCard(): void {
  cardContent.value?.focusTitle();
}

function focusTarget(): void {
  const el = target.found.value?.element;
  if (!(el instanceof HTMLElement)) return;
  const inner = el.matches(FOCUSABLE) ? el : el.querySelector<HTMLElement>(FOCUSABLE);
  if (inner) return inner.focus();
  if (!el.hasAttribute("tabindex")) el.tabIndex = -1;
  el.focus();
}

/** F6: card → control, anywhere else → card. False when not showing. */
function toggleFocus(): boolean {
  if (!showing.value) return false;
  if (card.value?.contains(document.activeElement)) focusTarget();
  else focusCard();
  return true;
}

/** Help gets focus once the card goes away under it. */
function focusHelp(): void {
  (resolve("header.help")?.element as HTMLElement | undefined)?.focus();
}

// Opening the coach (Show me around, Help, F1, Resume) moves focus into it;
// the saved `active` arriving with the load does not — that is not the user
// asking. After the render ("post"), so the card is there to focus.
watch(
  () => [guide.progress.active && !guide.progress.collapsed, guide.loaded] as const,
  ([on], [, wasLoaded]) => {
    if (on && wasLoaded) focusCard();
  },
  { flush: "post" },
);

// ---- actions -----------------------------------------------------------------

function onNext(): void {
  const finishing = isLast.value;
  guide.next();
  if (finishing) focusHelp();
}
function onPause(): void {
  guide.pause();
  focusHelp();
}
function onCollapse(): void {
  guide.collapse();
  void nextTick(() => document.querySelector<HTMLElement>('[data-testid="guide-resume"]')?.focus());
}

function onRestart(): void {
  guide.restart();
  void nextTick(focusCard);
}

/** Escape outside the card (no menu open): pause, leaving focus where the
 * user is working. */
function dismiss(): boolean {
  if (!showing.value) return false;
  guide.pause();
  return true;
}

function onCardKeydown(event: KeyboardEvent): void {
  if (event.key === "F6") {
    event.preventDefault();
    toggleFocus();
  } else if (event.key === "Escape") {
    event.preventDefault();
    onPause();
  }
  // The card owns the keyboard while focused: no editing shortcut fires.
  event.stopPropagation();
}

// Trying the highlighted control is recorded — and only recorded.
function onDocumentClick(event: MouseEvent): void {
  const el = target.found.value?.element;
  const id = step.value?.id;
  if (showing.value && el && id && event.target instanceof Node && el.contains(event.target)) guide.markExplored(id);
}
watch(
  showing,
  (on) => {
    if (on) document.addEventListener("click", onDocumentClick, true);
    else document.removeEventListener("click", onDocumentClick, true);
  },
  { immediate: true },
);
onBeforeUnmount(() => document.removeEventListener("click", onDocumentClick, true));

defineExpose({ toggleFocus, dismiss });
</script>

<template>
  <div
    v-if="ringVisible"
    data-guide-layer="ring"
    data-testid="guide-ring"
    aria-hidden="true"
    class="guide-ring pointer-events-none fixed z-[39] rounded-control"
    :class="ringClass"
    :style="ringStyle"
  >
    <span
      class="absolute left-0 whitespace-nowrap rounded-control bg-accent px-1.5 py-0.5 text-micro text-white"
      :class="labelClass"
    >{{ copy?.label }}</span>
  </div>

  <section
    v-if="showing && step && copy"
    ref="card"
    data-guide-layer="card"
    data-testid="guide-coach"
    role="region"
    aria-label="Guided walkthrough"
    :data-step-id="step.id"
    :data-target-state="targetState"
    :data-placement="placement.mode"
    class="fixed z-40 flex flex-col overflow-hidden rounded-control border border-focus bg-panel text-fg shadow-xl"
    :style="cardStyle"
    @keydown="onCardKeydown"
  >
    <GuideCoachCard
      ref="cardContent"
      :step-id="step.id"
      :title="step.title"
      :warning="warning"
      :copy="copy"
      :chapter-title="chapterTitle"
      :position="position"
      :progress-pct="progressPct"
      :can-go-back="guide.stepIndex > 0"
      :is-last="isLast"
      :off-screen="offScreen"
      :can-focus-target="canFocusTarget"
      :session-only="guide.sessionOnly"
      @collapse="onCollapse"
      @pause="onPause"
      @back="guide.back()"
      @next="onNext"
      @focus-target="focusTarget"
      @restart="onRestart"
    />
  </section>

  <button
    v-if="collapsedShowing"
    type="button"
    data-guide-layer="resume"
    data-testid="guide-resume"
    class="fixed right-4 bottom-4 z-40 cursor-pointer rounded-control border border-focus bg-panel px-3 py-1.5 text-xs text-fg shadow-lg hover:bg-raised focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    @click="guide.start()"
  >
    Resume guide · {{ position }}
  </button>
</template>

<style scoped>
.guide-ring {
  outline: 2px solid var(--color-focus);
  outline-offset: 0;
}
.guide-ring-dim {
  box-shadow: 0 0 0 9999px rgb(0 0 0 / 0.4);
}
.guide-ring-animated {
  transition: left 150ms ease, top 150ms ease, width 150ms ease, height 150ms ease;
}
@media (forced-colors: active) {
  .guide-ring {
    outline-color: Highlight;
  }
}
</style>
