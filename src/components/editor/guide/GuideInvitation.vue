<script setup lang="ts">
/**
 * The first-use invitation (Task 56; F-46; SCREENS 01: "A nonmodal
 * invitation offers Show me around / Not now over an immediately usable
 * workspace. It must not steal focus, block editing, request devices …").
 *
 * A region in the corner, not a dialog: no `aria-modal`, no backdrop, no
 * focus taken on mount — the person can keep editing and answer it
 * whenever. It shows once the saved progress has been read and neither
 * answer has been given (`invitationDismissed`); **Show me around** starts
 * the walkthrough, **Not now** (or ✕) dismisses it for good. Help in the
 * header stays the way back either way. Hidden while a modal dialog has the
 * screen, like the coach.
 */
import { computed } from "vue";

import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import AppButton from "../../ui/AppButton.vue";
import IconButton from "../../ui/IconButton.vue";

const guide = useEditorOnboardingStore();
const visible = computed(
  () => guide.loaded && !guide.progress.invitationDismissed && !guide.progress.active && !guide.suspended,
);
</script>

<template>
  <section
    v-if="visible"
    role="region"
    aria-labelledby="guide-invitation-title"
    data-testid="guide-invitation"
    class="fixed right-4 bottom-4 z-40 flex w-[340px] max-w-[calc(100vw-2rem)] flex-col gap-2 rounded-control border border-focus bg-panel p-4 text-fg shadow-xl"
  >
    <div class="flex items-start gap-2">
      <p class="grow text-micro font-semibold uppercase tracking-wide text-accent-fg">
        New here? Start here.
      </p>
      <IconButton
        label="Dismiss the invitation"
        data-testid="guide-invitation-close"
        @click="guide.dismissInvitation()"
      >
        &#10005;
      </IconButton>
    </div>
    <h2
      id="guide-invitation-title"
      class="text-base font-semibold"
    >
      A little guidance. A clearer first edit.
    </h2>
    <p class="text-sm text-fg-secondary">
      Get to know the editor, one useful step at a time. You can pause and pick up later.
    </p>
    <div class="flex items-center gap-2">
      <AppButton
        data-testid="guide-invitation-start"
        @click="guide.start()"
      >
        Show me around
      </AppButton>
      <AppButton
        variant="ghost"
        data-testid="guide-invitation-dismiss"
        @click="guide.dismissInvitation()"
      >
        Not now
      </AppButton>
    </div>
    <p class="text-micro text-fg-subtle">
      Always available from Help. No editing required.
    </p>
  </section>
</template>
