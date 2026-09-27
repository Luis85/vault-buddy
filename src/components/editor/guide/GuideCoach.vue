<script setup lang="ts">
/**
 * The guided walkthrough's coach (Task 56; F-46, F-49; SCREENS 10;
 * ONBOARDING.md; A23–A26): a card that explains one lesson and a ring around
 * the REAL control that lesson is about — never a copy of it, and never
 * pressing it. Trying the control is recorded (`markExplored`, a click
 * inside it) and never advances the lesson; only Back/Next do.
 *
 * - **Where.** The lesson's typed key is resolved through the registry and
 *   tracked (`useCoachTarget`); the card goes right, left, below or above
 *   it — the first that fits, preferring one that leaves an open menu
 *   uncovered — or shrinks into the larger free band and scrolls its copy
 *   (`position.ts`, concept §9.2). Before a lesson shows, its safe
 *   preparation reveals the tab, drawer or selection its control needs
 *   (`prepare.ts`) — view state only.
 * - **Nothing reaches a render.** Ring, label and card are fixed-position siblings
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
 *   is reduced; it is a shadow around the ring, so it never blocks a click
 *   (`GuideHighlight`).
 * - **The chapter title** in the card's top bar pauses the walkthrough and
 *   opens the learning center's chapters (the concept's "contents"), which
 *   `GuideHelpButton` owns — so focus goes to Help first, and returns there
 *   when the center closes.
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";

import { useCoachTarget } from "../../../composables/useCoachTarget";
import { GUIDE_CHAPTERS, GUIDE_STEPS, lessonCopy, STEP_TARGETS } from "../../../editor/guide/content";
import { placeCoach } from "../../../editor/guide/position";
import { prepareLesson } from "../../../editor/guide/prepare";
import { resolve } from "../../../editor/guide/targets";
import { requestReveal } from "../../../editor/revealBus";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import GuideCoachCard from "./GuideCoachCard.vue";
import GuideHighlight from "./GuideHighlight.vue";
import GuideMini from "./GuideMini.vue";

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

/** Before the card has rendered once (and in a layout-less test DOM). */
const FALLBACK_HEIGHT = 300;
const card = ref<HTMLElement | null>(null);
const cardHeight = ref(FALLBACK_HEIGHT);

/** The card's natural height: its parts' full content, even while its
 * copy is scrolled inside a capped card. */
function measureCard(): void {
  const el = card.value;
  if (!el) return;
  const natural = [...el.children].reduce((h, c) => h + c.scrollHeight, 0);
  if (natural > 0) cardHeight.value = natural + 2;
}

const placement = computed(() =>
  placeCoach(target.rect.value, cardHeight.value, target.viewport.value, target.menu.value),
);
const px = (n: number) => `${Math.round(n)}px`;
const cardStyle = computed(() => ({
  left: px(placement.value.x),
  top: px(placement.value.y),
  width: px(placement.value.width),
  maxHeight: px(placement.value.maxHeight),
}));

function reducedMotion(): boolean {
  const motion = guide.progress.preferences.motion;
  if (motion !== "system") return motion === "reduced";
  return window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
}
const dimmed = computed(() => guide.progress.preferences.dimming && !reducedMotion());

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

/** The chapter title: pause, then the learning center's chapters, with
 * focus on Help so it comes back there when the center closes. */
function onContents(): void {
  guide.pause();
  focusHelp();
  requestReveal("learningCenter");
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
  <GuideHighlight
    v-if="ringVisible && target.rect.value && copy"
    :rect="target.rect.value"
    :label="copy.label"
    :viewport="target.viewport.value"
    :menu-open="target.menu.value !== null"
    :dimmed="dimmed"
    :animated="!reducedMotion()"
  />

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
    class="fixed z-40 flex flex-col overflow-hidden rounded-[13px] border border-guide-edge bg-panel text-fg shadow-[var(--editor-guide-shadow)]"
    :style="cardStyle"
    @keydown="onCardKeydown"
  >
    <GuideCoachCard
      ref="cardContent"
      :step-id="step.id"
      :title="step.title"
      :warning="warning"
      :explored="guide.progress.explored.includes(step.id)"
      :copy="copy"
      :chapter-title="chapterTitle"
      :position="position"
      :progress-pct="progressPct"
      :can-go-back="guide.stepIndex > 0"
      :is-last="isLast"
      :off-screen="offScreen"
      :can-focus-target="canFocusTarget"
      :session-only="guide.sessionOnly"
      @contents="onContents"
      @collapse="onCollapse"
      @pause="onPause"
      @back="guide.back()"
      @next="onNext"
      @focus-target="focusTarget"
      @restart="onRestart"
    />
  </section>

  <GuideMini
    v-if="collapsedShowing && step"
    :position="`${guide.stepIndex + 1}/${GUIDE_STEPS.length}`"
    :title="step.title"
    @expand="guide.start()"
    @dismiss="onPause"
  />
</template>
