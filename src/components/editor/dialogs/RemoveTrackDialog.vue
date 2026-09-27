<script setup lang="ts">
/**
 * "Remove this track?" (visual-parity Task 13; concept spec §9.10's
 * `confirmDialog`, the words of the concept's `deleteTrack`): opened by
 * `trackRemovalRequest` (`trackRemoval.ts`) — the track menu's and the
 * inspector's "Remove track…" both land here. Continue sends one
 * `deleteTrack`; a refusal is said by the shell's feedback toast
 * (visual-parity Task 7). A track that disappears while the question is open
 * (an undo, a removal elsewhere) closes it, and so does a session change:
 * there is nothing left to ask about.
 */
import { computed, watch } from "vue";

import { clipsOnTrack, removalMessage, trackRemovalRequest } from "../../../editor/trackRemoval";
import { useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";

const editorProject = useEditorProjectStore();

/** The track asked about — only in the session that asked. */
const track = computed(() => {
  const request = trackRemovalRequest.value;
  if (!request || request.sessionId !== editorProject.sessionId) return null;
  return editorProject.project?.tracks.find((t) => t.id === request.trackId) ?? null;
});
const message = computed(() =>
  track.value ? removalMessage(track.value.name, clipsOnTrack(editorProject.project, track.value.id)) : "",
);

watch(
  track,
  (t) => {
    if (!t) trackRemovalRequest.value = null;
  },
  { immediate: true },
);

function cancel(): void {
  trackRemovalRequest.value = null;
}

function confirm(): void {
  const trackId = track.value?.id;
  trackRemovalRequest.value = null;
  if (trackId) void editorProject.execute({ kind: "deleteTrack", trackId });
}
</script>

<template>
  <DialogHost
    :open="track !== null"
    label="Remove this track?"
    close-testid="remove-track-close"
    @close="cancel"
  >
    <template #title>
      Remove this track?
    </template>

    <p
      data-testid="remove-track-dialog"
      class="text-[12px] leading-relaxed text-fg-secondary"
    >
      {{ message }}
    </p>

    <template #footer>
      <AppButton
        variant="ghost"
        data-testid="remove-track-cancel"
        @click="cancel"
      >
        Cancel
      </AppButton>
      <AppButton
        data-testid="remove-track-confirm"
        @click="confirm"
      >
        Continue
      </AppButton>
    </template>
  </DialogHost>
</template>
