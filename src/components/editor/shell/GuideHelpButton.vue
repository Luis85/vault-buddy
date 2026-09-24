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
 * Three items: **Learning center** (chapters, lessons, quick answers,
 * progress and preferences — `LearningCenter`), **Resume walkthrough**
 * (the coach at the exact saved lesson; F1 and ? do the same from the
 * keyboard) and **Keyboard shortcuts** (the learning center, on its
 * shortcut table). There is deliberately no item for anything not built
 * yet. The menu closes on a choice, on Escape (focus back on Help) and on
 * a pointer press outside it — `SaveProjectMenu`'s behaviour.
 */
import { onBeforeUnmount, onMounted, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import AppButton from "../../ui/AppButton.vue";
import type { LearningTab } from "../guide/LearningCenter.vue";
import LearningCenter from "../guide/LearningCenter.vue";

type HelpItem = "center" | "resume" | "shortcuts";

const ITEMS: readonly { id: HelpItem; label: string; testid: string }[] = [
  { id: "center", label: "Learning center", testid: "editor-help-learning-center" },
  { id: "resume", label: "Resume walkthrough", testid: "editor-help-resume" },
  { id: "shortcuts", label: "Keyboard shortcuts", testid: "editor-help-shortcuts" },
];

const onboarding = useEditorOnboardingStore();
const helpTarget = useGuideTarget("header.help");

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const centerOpen = ref(false);
const centerTab = ref<LearningTab>("walkthrough");

function choose(item: HelpItem): void {
  open.value = false;
  if (item === "resume") {
    onboarding.start();
    return;
  }
  centerTab.value = item === "shortcuts" ? "shortcuts" : "walkthrough";
  centerOpen.value = true;
}

function onEscape(event: KeyboardEvent): void {
  if (!open.value) return;
  event.stopPropagation();
  open.value = false;
  root.value?.querySelector<HTMLElement>('[data-testid="editor-header-help"]')?.focus();
}

function onPointerDown(event: PointerEvent): void {
  if (open.value && !root.value?.contains(event.target as Node)) open.value = false;
}

onMounted(() => document.addEventListener("pointerdown", onPointerDown));
onBeforeUnmount(() => document.removeEventListener("pointerdown", onPointerDown));
</script>

<template>
  <div
    ref="root"
    class="relative"
    @keydown.esc="onEscape"
  >
    <AppButton
      :ref="helpTarget"
      variant="ghost"
      size="sm"
      data-testid="editor-header-help"
      aria-haspopup="menu"
      :aria-expanded="open"
      title="Help, the learning center and the guided walkthrough (F1 resumes it)"
      @click="open = !open"
    >
      Help
    </AppButton>
    <div
      v-if="open"
      role="menu"
      aria-label="Help"
      class="absolute right-0 top-full z-20 mt-1 flex min-w-48 flex-col gap-0.5 rounded-control border border-white/10 bg-slate-800 p-1 shadow-lg"
    >
      <button
        v-for="item in ITEMS"
        :key="item.id"
        type="button"
        role="menuitem"
        :data-testid="item.testid"
        class="cursor-pointer rounded px-2 py-1 text-left text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="choose(item.id)"
      >
        {{ item.label }}
      </button>
    </div>
  </div>
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
