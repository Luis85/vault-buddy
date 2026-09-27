<script setup lang="ts">
/**
 * The Save a copy dialog's notes and its one status line (Task 39;
 * SCREENS 08: "Package preparation reports pending/success/failure/cancel
 * … Native copy changes only after a matching durable receipt"). Split out
 * of `SaveProjectDialog` for its template-complexity ratchet.
 *
 * - A portable file carries the originals — also the parts trimmed or
 *   covered — so it says so, in the gold warning; a lightweight one lists
 *   every original as missing elsewhere.
 * - The status is derived from the export's own state, never a timer, so
 *   "Saved to" can only follow a receipt. `refusal` is a refused rename
 *   (the dialog's inline `lastError`), which stops the copy before it
 *   starts.
 */
import { computed } from "vue";

import type { ExportState } from "../../../composables/useProjectPackage";
import type { PackageFormat } from "../../../editorTypes";

const props = defineProps<{ format: PackageFormat; state: ExportState; refusal: string | null }>();

function statusOf(s: ExportState): { text: string; alert: boolean } {
  switch (s.phase) {
    case "pending":
      return { text: "Preparing the project file…", alert: false };
    case "success":
      return { text: `Saved to ${s.fileName}`, alert: false };
    case "failure":
      return { text: `The project file was not saved. ${s.message}`, alert: true };
    case "cancelled":
      return { text: "Nothing was saved — the file dialog was closed.", alert: false };
    default:
      return { text: "", alert: false };
  }
}

const status = computed(() =>
  props.refusal ? { text: `The copy was not saved. ${props.refusal}`, alert: true } : statusOf(props.state),
);
</script>

<template>
  <p
    v-if="format === 'portable'"
    data-testid="save-project-originals-warning"
    class="rounded-[7px] border border-gold/30 bg-gold-bg p-3 text-[11px] leading-[1.6] text-gold"
  >
    A portable file includes your original recordings and imported media — also the parts you trimmed or covered. Share a
    rendered video instead when the people you send it to must not receive the originals. Media that is no longer
    available is left out and listed for reconnection when the file is opened.
  </p>
  <p
    v-else
    data-testid="save-project-reconnect-note"
    class="text-[11px] leading-[1.6] text-fg-secondary"
  >
    Opening this file on another computer lists every original as missing: keep your media, and reconnect it there.
  </p>
  <p
    data-testid="save-project-status"
    :role="status.alert ? 'alert' : 'status'"
    class="min-h-4 text-xs break-words"
    :class="status.alert ? 'text-danger-fg' : 'text-fg-secondary'"
  >
    {{ status.text }}
  </p>
</template>
