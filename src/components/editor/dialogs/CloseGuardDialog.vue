<script setup lang="ts">
/**
 * The editor window's close guard (Task 37 Part A; F-44) — see
 * `useEditorCloseGuard` for when it opens and what each choice does.
 * `EditorRoot` calls `request()` on every `editor:closeRequested`.
 *
 * Deliberately nothing on unmount: a render keeps running whatever happens
 * to this component, and is cancelled only by "Cancel the render".
 */
import { useEditorCloseGuard } from "../../../composables/useEditorCloseGuard";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";

const guard = useEditorCloseGuard();
const { mode, busy, error } = guard;

defineExpose({ request: guard.request });
</script>

<template>
  <DialogHost
    :open="mode !== null"
    label="Close the editor"
    :closable="!busy"
    @close="guard.dismiss"
  >
    <div
      data-testid="close-guard"
      class="flex w-96 max-w-full flex-col gap-3"
    >
      <template v-if="mode === 'render'">
        <h2 class="text-sm font-semibold text-fg">
          A render is running
        </h2>
        <p class="text-sm text-fg-secondary">
          A render is running — keep it running in the background, or cancel it.
        </p>
        <div class="flex flex-wrap justify-end gap-2">
          <AppButton
            variant="ghost"
            :disabled="busy"
            @click="guard.dismiss"
          >
            Cancel
          </AppButton>
          <AppButton
            variant="danger"
            :disabled="busy"
            @click="guard.cancelRender"
          >
            Cancel the render
          </AppButton>
          <AppButton
            :disabled="busy"
            @click="guard.keep"
          >
            Keep it running
          </AppButton>
        </div>
      </template>
      <template v-else>
        <h2 class="text-sm font-semibold text-fg">
          Unsaved changes
        </h2>
        <p class="text-sm text-fg-secondary">
          This project has changes that are not saved yet. Keeping them for later
          leaves them ready for when you come back, even after a restart.
        </p>
        <div class="flex flex-wrap justify-end gap-2">
          <AppButton
            variant="ghost"
            :disabled="busy"
            @click="guard.dismiss"
          >
            Cancel
          </AppButton>
          <AppButton
            variant="danger"
            :disabled="busy"
            @click="guard.discard"
          >
            Discard changes
          </AppButton>
          <AppButton
            variant="secondary"
            :disabled="busy"
            @click="guard.keep"
          >
            Keep for later
          </AppButton>
          <AppButton
            :disabled="busy"
            @click="guard.save"
          >
            Save project
          </AppButton>
        </div>
      </template>
      <p
        v-if="error"
        role="alert"
        class="text-xs text-danger-fg"
      >
        {{ error }}
      </p>
    </div>
  </DialogHost>
</template>
