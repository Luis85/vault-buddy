<script setup lang="ts">
/**
 * **Detach audio** for one clip in the Audio inspector (Task 27, F-24;
 * split out of `AudioSection` for the template-complexity ratchet). It
 * reads the SAME `actions.ts` verdict and command the context menu does,
 * so the two can never disagree. Whether the source has sound at all is in
 * `sources.json`, which only Rust reads — an enabled Detach on a silent
 * video comes back refused, through the store's `lastError`. A clip that
 * IS detached audio names the video it came from.
 */
import { computed } from "vue";

import { baseActionContext } from "../../../editor/actionContext";
import { commandFor, resolveActions } from "../../../editor/actions";
import type { Asset } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const props = defineProps<{ asset: Asset | undefined }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const linkedFrom = computed(() => {
  const root = props.asset?.linked_asset;
  return root ? (editorProject.project?.assets.find((a) => a.id === root)?.name ?? root) : null;
});

const actionContext = computed(() =>
  baseActionContext(editorProject.project, editorProject.snapshot, workspace.playheadMs, workspace.selectionClipIds),
);
const detach = computed(() => resolveActions(actionContext.value).detachAudio);
const title = computed(() => detach.value.reason ?? "Moves the sound to its own audio clip and mutes this one");
const buttonClass = computed(() => (detach.value.enabled ? "text-fg-secondary" : "cursor-default opacity-50"));

function onDetach(): void {
  const command = commandFor("detachAudio", actionContext.value);
  if (command) void editorProject.execute(command);
}
</script>

<template>
  <button
    type="button"
    data-testid="audio-section-detach"
    class="cursor-pointer rounded px-1.5 py-0.5 text-left transition-colors hover:bg-white/10"
    :class="buttonClass"
    :aria-disabled="!detach.enabled"
    :title="title"
    @click="onDetach"
  >
    {{ detach.label }}
  </button>
  <p
    v-if="linkedFrom"
    data-testid="audio-section-linked"
  >
    Detached from {{ linkedFrom }}.
  </p>
</template>
