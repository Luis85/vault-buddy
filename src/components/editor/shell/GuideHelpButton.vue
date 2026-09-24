<script setup lang="ts">
/**
 * The header's **Help** (Task 55; SCREENS-AND-INTERACTIONS.md §02: the
 * header owns "Help, Checks, Save project and Render video") — the guide's
 * `header.help` target — and, when guide progress cannot be stored,
 * "Session only" beside it (ONBOARDING.md § State and persistence), so a
 * restart does not silently forget where the user was. Split out of
 * `EditorHeader` so the header's own template stays under the complexity
 * ratchet (the `ChecksButton`/`RenderVideoButton` precedent). What Help
 * opens is the learning center (Task 56).
 */
import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import AppButton from "../../ui/AppButton.vue";

const onboarding = useEditorOnboardingStore();
const helpTarget = useGuideTarget("header.help");
</script>

<template>
  <AppButton
    :ref="helpTarget"
    variant="ghost"
    size="sm"
    data-testid="editor-header-help"
  >
    Help
  </AppButton>
  <span
    v-if="onboarding.sessionOnly"
    data-testid="editor-header-guide-session-only"
    title="Guide progress cannot be stored on this device right now. It lasts until the editor closes."
    class="text-micro text-fg-subtle"
  >Session only</span>
</template>
