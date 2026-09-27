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
 *
 * Visual-parity Task 23 (concept spec §9.1): 350 wide, 24 from the right
 * and 76 from the top, the overline, the compass tile beside the two-line
 * heading, the body, the primary "Show me around ›" beside a "Not now"
 * link, and the "Always available from Help" footer.
 */
import { computed } from "vue";

import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import EditorIcon from "../icons/EditorIcon.vue";

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
    class="fixed top-[76px] right-6 z-40 w-[350px] max-w-[calc(100vw-28px)] rounded-[14px] border border-guide-edge bg-panel px-6 pt-[25px] pb-[18px] text-left text-fg shadow-[var(--editor-guide-shadow)]"
  >
    <button
      type="button"
      data-testid="guide-invitation-close"
      aria-label="Dismiss walkthrough invitation"
      class="absolute top-2 right-[9px] flex h-8 w-8 items-center justify-center border-transparent bg-transparent p-1.5 text-fg-secondary"
      @click="guide.dismissInvitation()"
    >
      <EditorIcon name="x" />
    </button>
    <p class="text-[10px] leading-[1.6] font-bold tracking-[1.7px] text-accent">
      NEW HERE? START HERE.
    </p>
    <div class="mt-[17px] mb-2.5 flex items-center gap-3.5">
      <span class="grid h-[50px] w-[46px] shrink-0 place-items-center rounded-[13px] bg-accent-bg text-accent">
        <EditorIcon
          name="compass"
          :size="27"
        />
      </span>
      <h2
        id="guide-invitation-title"
        class="text-[22px] leading-[1.18] font-[650] tracking-[-0.5px]"
      >
        A little guidance.<br>A clearer first edit.
      </h2>
    </div>
    <p class="mt-3.5 mb-[18px] text-[13px] leading-[1.65] text-fg-secondary">
      Get to know the editor, one useful step at a time. You can pause and pick up later.
    </p>
    <div class="mb-[15px] flex items-center gap-[18px]">
      <button
        type="button"
        data-testid="guide-invitation-start"
        class="flex min-h-[39px] items-center gap-1.5 rounded-[7px] border-transparent bg-primary px-3 text-xs font-semibold text-white enabled:hover:bg-primary-hover"
        @click="guide.start()"
      >
        Show me around
        <EditorIcon
          name="chevronRight"
          :size="15"
        />
      </button>
      <button
        type="button"
        data-testid="guide-invitation-dismiss"
        class="border-transparent bg-transparent px-0.5 py-[5px] text-xs text-accent-ink hover:underline"
        @click="guide.dismissInvitation()"
      >
        Not now
      </button>
    </div>
    <p class="border-t border-line pt-[13px] text-[10px] leading-[1.7] text-fg-muted">
      Always available from <b>Help</b> · No editing required
    </p>
  </section>
</template>
