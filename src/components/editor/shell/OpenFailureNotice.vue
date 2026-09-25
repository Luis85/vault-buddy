<script setup lang="ts">
/**
 * The editor window's own words when an open was refused (Task 59), and —
 * final whole-branch review I3 — the way out for a project too damaged to
 * open: a confirmed Discard through `editor_discard_project`.
 *
 * `projectId` is set by `EditorRoot` only for a PROJECT open that Rust
 * refused as `invalidProject`: its files no longer parse, so no retry can
 * open it, and without this it could never be discarded (the session
 * discard needs an open) — nor could the capture it pinned. A staged
 * capture's failure, or one a retry could fix, offers nothing here.
 *
 * Presentational apart from the one port call; the refusal is said in the
 * role wording Rust gives it, without its redaction handle
 * (`toEditorError`).
 */
import { ref, watch } from "vue";

import { toEditorError, useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";

const props = defineProps<{ message: string; projectId: string | null }>();

const project = useEditorProjectStore();
const confirming = ref(false);
const busy = ref(false);
const discarded = ref(false);
const error = ref<string | null>(null);

watch(
  () => props.projectId,
  () => {
    confirming.value = false;
    discarded.value = false;
    error.value = null;
  },
);

async function discard(): Promise<void> {
  const id = props.projectId;
  if (id === null || busy.value) return;
  busy.value = true;
  error.value = null;
  try {
    await project.port.discardProject(id);
    discarded.value = true;
  } catch (e) {
    error.value = `The project could not be discarded. ${toEditorError(e).message}`;
  } finally {
    busy.value = false;
    confirming.value = false;
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <p
      data-testid="editor-open-failed"
      role="alert"
      class="rounded-control border border-line bg-panel px-3 py-2 text-sm text-danger-fg"
    >
      This could not be opened. {{ props.message }}
    </p>
    <p
      v-if="discarded"
      data-testid="broken-project-discarded"
      role="status"
      class="px-1 text-sm text-fg-secondary"
    >
      The project was discarded. A recording it used is back in your staged captures.
    </p>
    <template v-else-if="props.projectId !== null">
      <p class="px-1 text-sm text-fg-secondary">
        This project's files are damaged, so it cannot be opened again. You can discard it:
        its edits and renders are deleted from this computer; published videos and the
        recording itself stay.
      </p>
      <p
        v-if="error"
        data-testid="broken-project-error"
        role="alert"
        class="px-1 text-xs text-danger-fg"
      >
        {{ error }}
      </p>
      <div class="flex flex-wrap gap-2">
        <AppButton
          v-if="!confirming"
          data-testid="broken-project-discard"
          variant="secondary"
          :disabled="busy"
          @click="confirming = true"
        >
          Discard this project…
        </AppButton>
        <template v-else>
          <AppButton
            data-testid="broken-project-confirm"
            variant="danger"
            :disabled="busy"
            @click="discard"
          >
            Discard for good
          </AppButton>
          <AppButton
            data-testid="broken-project-keep"
            variant="ghost"
            :disabled="busy"
            @click="confirming = false"
          >
            Keep it
          </AppButton>
        </template>
      </div>
    </template>
  </div>
</template>
