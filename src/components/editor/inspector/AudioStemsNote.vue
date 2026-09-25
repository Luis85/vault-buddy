<script setup lang="ts">
/**
 * The Audio inspector's note on per-input STEMS (Task 53, F-05). Split out of
 * `AudioSection` for the template-complexity ratchet, like its siblings.
 *
 * A staged capture recorded WITHOUT stems has every audio input mixed into
 * its one track, and no editor command can split a mix after the fact — so
 * selecting it says so and names the setting that records the inputs
 * separately, instead of leaving the user to look for a per-input control
 * that cannot exist here. Once the capture HAS stems (migration placed them
 * on their own tracks, `stem-<n>` assets) there is nothing to explain.
 */
import { computed } from "vue";

import { isStemAsset } from "../../../editor/stems";
import type { Asset } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";

const props = defineProps<{ asset: Asset | undefined }>();

const editorProject = useEditorProjectStore();

const absent = computed(
  () =>
    props.asset?.builtin === "screen" &&
    !(editorProject.project?.assets ?? []).some(isStemAsset),
);
</script>

<template>
  <p
    v-if="absent"
    data-testid="audio-section-stems-absent"
    class="text-fg-subtle"
  >
    Every audio input of this recording is mixed into this one track. To get
    each input on its own track, turn on "Keep each audio input as a separate
    track" in the vault's Screen settings before your next recording.
  </p>
</template>
