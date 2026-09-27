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
 * deliberately no item for anything not built yet. The menu closes on a
 * choice, on Escape (focus back on Help) and on a pointer press outside it
 * — the behaviour Task 39's Save menu had.
 *
 * Visual-parity Task 8 (concept spec §2 `.guide-help-button`): the book
 * icon in the accent colour, and a 5px gold dot after the label while a
 * walkthrough is paused part way, so Help says there is a lesson to come
 * back to (F1 resumes it).
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import { toEditorError, useEditorProjectStore } from "../../../stores/editorProject";
import { useNotificationsStore } from "../../../stores/notifications";
import type { LearningTab } from "../guide/LearningCenter.vue";
import LearningCenter from "../guide/LearningCenter.vue";
import HeaderButton from "./HeaderButton.vue";

type HelpItem = "center" | "resume" | "shortcuts" | "diagnostics";

const ITEMS: readonly { id: HelpItem; label: string; testid: string }[] = [
  { id: "center", label: "Learning center", testid: "editor-help-learning-center" },
  { id: "resume", label: "Resume walkthrough", testid: "editor-help-resume" },
  { id: "shortcuts", label: "Keyboard shortcuts", testid: "editor-help-shortcuts" },
  { id: "diagnostics", label: "Export diagnostics", testid: "editor-help-diagnostics" },
];

const onboarding = useEditorOnboardingStore();
/** A walkthrough stopped part way: a lesson to resume, the coach not up. */
const paused = computed(() => onboarding.progress.currentStepId !== null && !onboarding.progress.active);
const helpTarget = useGuideTarget("header.help");

const open = ref(false);
const root = ref<HTMLElement | null>(null);
const centerOpen = ref(false);
const centerTab = ref<LearningTab>("walkthrough");

const editorProject = useEditorProjectStore();
const notifications = useNotificationsStore();

/** A dismissed dialog says nothing; a refusal says why. */
async function exportDiagnostics(): Promise<void> {
  try {
    const name = await editorProject.port.exportDiagnostics();
    if (name) {
      notifications.success(`Saved diagnostics to ${name}. It holds counts and error codes, never project content.`);
    }
  } catch (e) {
    notifications.error(`The diagnostics could not be saved. ${toEditorError(e).message}`);
  }
}

function choose(item: HelpItem): void {
  open.value = false;
  if (item === "resume") {
    onboarding.start();
    return;
  }
  if (item === "diagnostics") {
    void exportDiagnostics();
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
    <HeaderButton
      :ref="helpTarget"
      icon="book"
      icon-class="text-accent"
      data-testid="editor-header-help"
      aria-haspopup="menu"
      :aria-expanded="open"
      title="Help, the learning center and the guided walkthrough (F1 resumes it)"
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
    <div
      v-if="open"
      role="menu"
      aria-label="Help"
      class="absolute right-0 top-full z-20 mt-1 flex min-w-48 flex-col gap-0.5 rounded-control border border-line bg-panel p-1 shadow-lg"
    >
      <button
        v-for="item in ITEMS"
        :key="item.id"
        type="button"
        role="menuitem"
        :data-testid="item.testid"
        class="cursor-pointer rounded px-2 py-1 text-left text-xs text-fg-secondary hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
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
