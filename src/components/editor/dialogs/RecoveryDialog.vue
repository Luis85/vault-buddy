<script setup lang="ts">
/**
 * Unsaved changes left behind by an earlier run (Task 37 Part A; F-44;
 * A27) — see `useEditorRecovery` for when it opens and what each choice
 * does. `EditorRoot` calls `check()` after every newly opened session and
 * re-hydrates the workspace on `session-changed`.
 *
 * Not closable by Escape or the backdrop: until the user chooses, any new
 * edit would journal over the changes this dialog is offering back.
 * Every Rust message is rendered as text (mustache), never as markup.
 *
 * Visual-parity Task 22 (concept spec §9.9): the session dialogs' lead row
 * (`SessionLead`), `DialogButton`s — Discard in the danger ink, Resume (or
 * Open saved project) primary — and, while a choice runs, the reason both
 * wait on screen (`FooterReason`, D14). A refusal is the dialog's own
 * (`useEditorRecovery` tracks each choice, ruling T7-1), never also a
 * toast.
 */
import { computed } from "vue";

import { useEditorRecovery } from "../../../composables/useEditorRecovery";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";
import FooterReason from "./FooterReason.vue";
import SessionLead from "./SessionLead.vue";

const props = defineProps<{
  /** Reopen `projectId` after a refused Discard (`EditorRoot`'s own);
   * `true` when the editor has a session again. */
  reattach: (projectId: string) => Promise<boolean>;
}>();
const emit = defineEmits<{ (e: "session-changed"): void }>();

const recovery = useEditorRecovery(
  () => emit("session-changed"),
  (projectId) => props.reattach(projectId),
);
const { offer, failure, resumeFailed, busy } = recovery;

/** `updatedAt` comes from a hand-editable file: an unparseable value is
 * shown as written rather than as "Invalid Date". */
const savedAt = computed(() => {
  const raw = offer.value?.updatedAt ?? "";
  const when = new Date(raw);
  return Number.isNaN(when.getTime()) ? raw : when.toLocaleString();
});

/** Why the footer's buttons wait, or `null`. */
const reason = computed(() => (busy.value ? "Opening the project…" : null));

defineExpose({ check: recovery.check });
</script>

<template>
  <DialogHost
    :open="offer !== null"
    label="Unsaved changes from last time"
    :width="660"
    :closable="false"
    close-reason="Choose Resume or Discard first."
  >
    <template #title>
      Unsaved changes from last time
    </template>
    <template #subtitle>
      Your editable source. Nothing is lost until you choose.
    </template>

    <div
      v-if="offer"
      data-testid="recovery-dialog"
      class="flex flex-col gap-4"
    >
      <SessionLead
        data-testid="recovery-lead"
        icon="shield"
        title="Your unsaved changes are waiting"
      >
        “{{ offer.title }}” (last saved {{ savedAt }}) has changes that were never
        saved. Resume them, or discard them and continue from the saved project.
      </SessionLead>
      <div
        v-if="failure"
        role="alert"
        class="flex flex-col gap-1 rounded-[7px] border border-line bg-app p-3 text-xs text-danger-fg"
      >
        <template v-if="resumeFailed">
          <p>The unsaved changes could not be opened. Your saved project was not changed.</p>
          <p>The unsaved changes could not be read. Their file is kept in the project folder.</p>
        </template>
        <p class="break-words text-fg-muted">
          {{ failure }}
        </p>
      </div>
    </div>

    <template
      v-if="offer"
      #footer
    >
      <FooterReason
        data-testid="recovery-reason"
        :text="reason"
      />
      <DialogButton
        variant="danger"
        :reason="reason"
        @click="recovery.discard"
      >
        Discard
      </DialogButton>
      <DialogButton
        v-if="resumeFailed"
        variant="primary"
        :reason="reason"
        @click="recovery.openSaved"
      >
        Open saved project
      </DialogButton>
      <DialogButton
        v-else
        variant="primary"
        :reason="reason"
        @click="recovery.resume"
      >
        Resume
      </DialogButton>
    </template>
  </DialogHost>
</template>
