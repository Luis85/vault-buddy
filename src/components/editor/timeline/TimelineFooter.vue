<script setup lang="ts">
/**
 * The timeline footer (visual-parity Task 20; concept spec §7,
 * `.timeline-footer`): 27 px under the lanes, a `line` border on top,
 * 10px muted text, 13px side padding.
 *
 * - **Left**: the link icon and the live edit hint (`#editHint`) — what
 *   the drag in hand will do, else "Callouts follow their clip."
 *   (`dragHint.ts`). `TimelineView` passes the line the dragged clip or
 *   cue reported; the concept's hidden key hints are not drawn.
 * - **Right**: "Audio mixer" with the audio-track count, which opens the
 *   mixer (`MixerPopover`, whose trigger it is — ruling T12-1 moved it here
 *   from the transport row, with the guide's "audio" target).
 *
 * The footer sits outside the scrolling lanes, so it never scrolls
 * sideways and needs no pinned label cell.
 */
import { computed } from "vue";

import { DEFAULT_EDIT_HINT } from "../../../editor/dragHint";
import { readPreviewPeak } from "../../../editor/previewPeak";
import EditorIcon from "../icons/EditorIcon.vue";
import MixerPopover from "../shell/MixerPopover.vue";

const props = withDefaults(defineProps<{ hint?: string | null }>(), { hint: null });

const line = computed(() => props.hint ?? DEFAULT_EDIT_HINT);
</script>

<template>
  <div
    data-testid="timeline-footer"
    class="flex h-[27px] min-h-[27px] shrink-0 items-center justify-between gap-2.5 border-t border-line px-[13px] text-[10px] text-fg-muted"
  >
    <div class="flex min-w-0 items-center gap-2">
      <EditorIcon
        data-testid="timeline-footer-hint-icon"
        name="link"
      />
      <span
        data-testid="timeline-footer-hint"
        class="truncate"
      >{{ line }}</span>
    </div>
    <MixerPopover :read-peak="readPreviewPeak" />
  </div>
</template>
