<script setup lang="ts">
/**
 * A finished publish (Task 48; F-43), split out of `PublishDialog` for its
 * template-complexity ratchet (visual-parity Task 21): the vault by NAME
 * (the receipt's own `vaultName`, design D6), the landed file NAMES — all
 * the dialog shows; the full paths only travel to `open_screen_capture` —
 * and a note that could not be written, as a warning beside a video that
 * WAS published.
 */
import type { PublishReceipt } from "../../../editorTypes";

defineProps<{ receipt: PublishReceipt }>();

/** A path's last component — the landed NAME. */
function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
</script>

<template>
  <div
    data-testid="publish-result"
    role="status"
    class="flex flex-col gap-1 text-xs text-fg-secondary"
  >
    <p data-testid="publish-vault-name">
      Published into {{ receipt.vaultName }}:
    </p>
    <p
      data-testid="publish-video-name"
      class="font-medium text-fg"
    >
      {{ fileName(receipt.videoPath) }}
    </p>
    <p
      v-if="receipt.notePath"
      data-testid="publish-note-name"
      class="font-medium text-fg"
    >
      {{ fileName(receipt.notePath) }}
    </p>
    <p
      v-if="receipt.warning"
      data-testid="publish-warning"
      class="text-danger-fg"
    >
      {{ receipt.warning }}
    </p>
  </div>
</template>
