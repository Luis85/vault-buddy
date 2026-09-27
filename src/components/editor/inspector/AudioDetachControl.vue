<script setup lang="ts">
/**
 * **Detach source audio** for one clip in the Audio inspector (Task 27,
 * F-24; visual-parity Task 15, concept spec §5 "Audio"). Offered for a
 * video clip only, as in the concept: a sound clip has no picture to detach
 * from. It reads the SAME `actions.ts` verdict and command the context menu
 * does, so the two can never disagree, and a refusal it can see (a sound
 * already detached, a locked track) is its tooltip. Whether a source has
 * sound at all is in `sources.json`, which only Rust reads — a Detach on a
 * silent video comes back refused, as a toast. A clip that IS detached
 * audio names the video it came from.
 */
import { computed } from "vue";

import { baseActionContext } from "../../../editor/actionContext";
import { commandFor, resolveActions } from "../../../editor/actions";
import type { Asset } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import InspectorButton from "./InspectorButton.vue";

const props = defineProps<{ asset: Asset | undefined }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const isVideo = computed(() => props.asset?.kind === "video");
const linkedFrom = computed(() => {
  const root = props.asset?.linked_asset;
  return root ? (editorProject.project?.assets.find((a) => a.id === root)?.name ?? root) : null;
});

const actionContext = computed(() =>
  baseActionContext(editorProject.project, editorProject.snapshot, workspace.playheadMs, workspace.selectionClipIds),
);
const detach = computed(() => resolveActions(actionContext.value).detachAudio);

function onDetach(): void {
  const command = commandFor("detachAudio", actionContext.value);
  if (command) void editorProject.execute(command);
}
</script>

<template>
  <InspectorButton
    v-if="isVideo"
    class="self-start"
    icon="music"
    data-testid="audio-section-detach"
    :reason="detach.reason"
    @click="onDetach"
  >
    Detach source audio
  </InspectorButton>
  <p
    v-if="linkedFrom"
    data-testid="audio-section-linked"
  >
    Detached from {{ linkedFrom }}.
  </p>
</template>
