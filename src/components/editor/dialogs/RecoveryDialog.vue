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
 */
import { computed } from "vue";

import { useEditorRecovery } from "../../../composables/useEditorRecovery";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";

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

    <div
      v-if="offer"
      data-testid="recovery-dialog"
      class="flex flex-col gap-3"
    >
      <p class="text-sm text-fg-secondary">
        “{{ offer.title }}” (last saved {{ savedAt }}) has changes that were never
        saved. Resume them, or discard them and continue from the saved project.
      </p>
      <div
        v-if="failure"
        role="alert"
        class="flex flex-col gap-1 text-xs text-danger-fg"
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
      <AppButton
        variant="danger"
        :disabled="busy"
        @click="recovery.discard"
      >
        Discard
      </AppButton>
      <AppButton
        v-if="resumeFailed"
        :disabled="busy"
        @click="recovery.openSaved"
      >
        Open saved project
      </AppButton>
      <AppButton
        v-else
        :disabled="busy"
        @click="recovery.resume"
      >
        Resume
      </AppButton>
    </template>
  </DialogHost>
</template>
