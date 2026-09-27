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

const props = defineProps<{
  /** Reopen `projectId` after a refused Discard changes (`EditorRoot`'s
   * own); `true` when the editor has a session again. */
  reattach: (projectId: string) => Promise<boolean>;
}>();

const guard = useEditorCloseGuard((projectId) => props.reattach(projectId));
const { mode, busy, error } = guard;

defineExpose({ request: guard.request });
</script>

<template>
  <DialogHost
    :open="mode !== null"
    label="Close the editor"
    :closable="!busy"
    :close-reason="busy ? 'Finish choosing first.' : null"
    @close="guard.dismiss"
  >
    <template #title>
      <template v-if="mode === 'render'">
        A render is running
      </template>
      <template v-else-if="mode === 'take'">
        Unsaved webcam take
      </template>
      <template v-else>
        Unsaved changes
      </template>
    </template>

    <div
      data-testid="close-guard"
      class="flex w-96 max-w-full flex-col gap-3"
    >
      <p
        v-if="mode === 'render'"
        class="text-sm text-fg-secondary"
      >
        A render is running — keep it running in the background, or cancel it.
      </p>
      <p
        v-else-if="mode === 'take'"
        class="text-sm text-fg-secondary"
      >
        You have an unsaved webcam take. Closing now loses it — go back to finish
        it, or discard it and close.
      </p>
      <p
        v-else
        class="text-sm text-fg-secondary"
      >
        This project has changes that are not saved yet. Keeping them for later
        leaves them ready for when you come back, even after a restart.
      </p>
      <p
        v-if="error"
        role="alert"
        class="text-xs text-danger-fg"
      >
        {{ error }}
      </p>
    </div>

    <template #footer>
      <template v-if="mode === 'render'">
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
      </template>
      <template v-else-if="mode === 'take'">
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
          @click="guard.discardTakes"
        >
          Discard the take
        </AppButton>
      </template>
      <template v-else>
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
      </template>
    </template>
  </DialogHost>
</template>
