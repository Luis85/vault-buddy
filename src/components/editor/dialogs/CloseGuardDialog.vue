<script setup lang="ts">
/**
 * The editor window's close guard (Task 37 Part A; F-44) — see
 * `useEditorCloseGuard` for when it opens and what each choice does.
 * `EditorRoot` calls `request()` on every `editor:closeRequested`.
 *
 * Deliberately nothing on unmount: a render keeps running whatever happens
 * to this component, and is cancelled only by "Cancel the render".
 *
 * Visual-parity Task 22 (concept spec §9.9–9.10): the session dialogs'
 * lead row (`SessionLead`) names what the close would leave behind, and the
 * footer is `DialogButton`s — the destructive choice in the danger ink, the
 * safe one primary — with the reason they wait on screen while a choice
 * runs (`FooterReason`, D14). The render's cancel stays its own explicit
 * button.
 */
import { computed } from "vue";

import { useEditorCloseGuard } from "../../../composables/useEditorCloseGuard";
import type { EditorIconName } from "../icons/conceptIcons";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";
import FooterReason from "./FooterReason.vue";
import SessionLead from "./SessionLead.vue";

const props = defineProps<{
  /** Reopen `projectId` after a refused Discard changes (`EditorRoot`'s
   * own); `true` when the editor has a session again. */
  reattach: (projectId: string) => Promise<boolean>;
}>();

const guard = useEditorCloseGuard((projectId) => props.reattach(projectId));
const { mode, busy, error } = guard;

const COPY: Record<"dirty" | "render" | "take", { icon: EditorIconName; title: string; text: string }> = {
  render: {
    icon: "video",
    title: "A render is running",
    text: "A render is running — keep it running in the background, or cancel it.",
  },
  take: {
    icon: "webcam",
    title: "Unsaved webcam take",
    text: "You have an unsaved webcam take. Closing now loses it — go back to finish it, or discard it and close.",
  },
  dirty: {
    icon: "save",
    title: "Unsaved changes",
    text: "This project has changes that are not saved yet. Keeping them for later leaves them ready for when you come back, even after a restart.",
  },
};
const copy = computed(() => COPY[mode.value ?? "dirty"]);
/** Why the footer's buttons (and the ✕) wait, or `null`. */
const reason = computed(() => (busy.value ? "Finishing your choice…" : null));

defineExpose({ request: guard.request });
</script>

<template>
  <DialogHost
    :open="mode !== null"
    label="Close the editor"
    :closable="!busy"
    :close-reason="reason"
    @close="guard.dismiss"
  >
    <template #title>
      Before the editor closes
    </template>

    <div
      data-testid="close-guard"
      class="flex flex-col gap-4"
    >
      <SessionLead
        data-testid="close-guard-lead"
        :icon="copy.icon"
        :title="copy.title"
      >
        {{ copy.text }}
      </SessionLead>
      <p
        v-if="error"
        role="alert"
        class="text-xs text-danger-fg"
      >
        {{ error }}
      </p>
    </div>

    <template #footer>
      <FooterReason
        data-testid="close-guard-reason"
        :text="reason"
      />
      <DialogButton
        :reason="reason"
        @click="guard.dismiss"
      >
        Cancel
      </DialogButton>
      <template v-if="mode === 'render'">
        <DialogButton
          variant="danger"
          :reason="reason"
          @click="guard.cancelRender"
        >
          Cancel the render
        </DialogButton>
        <DialogButton
          variant="primary"
          :reason="reason"
          @click="guard.keep"
        >
          Keep it running
        </DialogButton>
      </template>
      <DialogButton
        v-else-if="mode === 'take'"
        variant="danger"
        :reason="reason"
        @click="guard.discardTakes"
      >
        Discard the take
      </DialogButton>
      <template v-else>
        <DialogButton
          variant="danger"
          :reason="reason"
          @click="guard.discard"
        >
          Discard changes
        </DialogButton>
        <DialogButton
          :reason="reason"
          @click="guard.keep"
        >
          Keep for later
        </DialogButton>
        <DialogButton
          variant="primary"
          icon="save"
          :reason="reason"
          @click="guard.save"
        >
          Save project
        </DialogButton>
      </template>
    </template>
  </DialogHost>
</template>
