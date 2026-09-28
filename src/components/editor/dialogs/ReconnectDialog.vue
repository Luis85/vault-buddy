<script setup lang="ts">
/**
 * Reconnect missing originals (Task 40; F-03; A19; SCREENS 07: "Reconnect
 * shows expected source metadata, batch results and ambiguity; preserve
 * existing edits on failed import/relink").
 *
 * One row per original that was missing when the dialog opened: what it
 * was (name, size, length — never a path), and what the last reconnect
 * said about it. **Find all…** hands every still-missing original to Rust
 * in one call; Rust opens its own multi-file dialog and reconnects only
 * files that are unmistakably the original. Everything else is resolved
 * one row at a time:
 * - ambiguous (several files match equally) — never shown as reconnected;
 *   **Choose file…** picks the one, for that asset alone;
 * - not the original — the reason is shown, and **Replace…** picks a file
 *   to use INSTEAD, which Rust accepts only with that explicit
 *   confirmation (and only when it cannot break the edit);
 * - nothing matched — **Choose file…** again.
 * The edit itself is never touched: a reconnect changes where an asset's
 * bytes live, not its clips, cues or captions.
 *
 * Escape and the backdrop close it except while a reconnect is pending.
 * Every Rust message renders as text (mustache), never as markup.
 *
 * Visual-parity Task 22 (concept spec §9.10, "Reconnect original media",
 * `.source-row`): each original is a bordered row on the app background
 * (`ReconnectRow`), the footer is `DialogButton`s — Close · Find all… —
 * and why Find all… waits is said on screen (`FooterReason`, D14). A
 * reconnect's refusal is its own status line: `useMediaReconnect` never
 * routes one through `editorProject.lastError`, so the shell never toasts
 * it and there is nothing for this dialog to claim (ruling T7-1).
 */
import { computed, ref, watch } from "vue";

import { useMediaReconnect } from "../../../composables/useMediaReconnect";
import type { MissingMedia } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";
import FooterReason from "./FooterReason.vue";
import ReconnectRow from "./ReconnectRow.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const project = useEditorProjectStore();
const reconnect = useMediaReconnect();
const { outcomes, problems, unused, status, busy } = reconnect;

/** The originals this dialog is about — captured when it opens, so a row
 * that gets reconnected stays and says so instead of vanishing. */
const listed = ref<MissingMedia[]>([...project.missing]);

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    reconnect.reset();
    listed.value = [...project.missing];
  },
);

const stillMissing = computed(() => new Set(project.missing.map((m) => m.assetId)));
/** Still missing, and not one Rust already said it cannot reconnect here. */
const findAllIds = computed(() =>
  listed.value
    .map((m) => m.assetId)
    .filter((id) => stillMissing.value.has(id) && outcomes.value[id]?.kind !== "excluded"),
);

const BUSY = "Checking the chosen files…";
const statusText = computed(() => (busy.value ? BUSY : (status.value?.text ?? "")));
const statusRole = computed(() => (status.value?.alert ? "alert" : "status"));
const statusClass = computed(() => (status.value?.alert ? "text-danger-fg" : "text-fg-secondary"));
/** Why Close waits, or `null`. */
const closeReason = computed(() => (busy.value ? BUSY : null));
/** Why Find all… cannot act, or `null`. */
const findAllReason = computed(() => {
  if (busy.value) return BUSY;
  return findAllIds.value.length === 0 ? "Nothing is left to find." : null;
});

function close(): void {
  if (!busy.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Reconnect missing media"
    :closable="!busy"
    :close-reason="closeReason"
    @close="close"
  >
    <template #title>
      Reconnect missing media
    </template>
    <template #subtitle>
      Choose the original files. A file is reconnected only when it is clearly the
      original; anything less clear waits for your choice. Your edits are kept.
    </template>

    <div
      data-testid="reconnect-dialog"
      class="flex flex-col gap-3"
    >
      <ul
        class="flex flex-col gap-2"
        aria-label="Missing originals"
      >
        <ReconnectRow
          v-for="item in listed"
          :key="item.assetId"
          :item="item"
          :outcome="outcomes[item.assetId]"
          :busy="busy"
          @choose="reconnect.run([item.assetId], false)"
          @replace="reconnect.run([item.assetId], true)"
        />
      </ul>

      <ul
        v-if="problems.length > 0"
        class="flex flex-col gap-1 text-xs text-danger-fg"
        aria-label="Files that could not be read"
      >
        <li
          v-for="p in problems"
          :key="p.name"
        >
          “{{ p.name }}”: {{ p.error }}
        </li>
      </ul>

      <p
        v-if="unused.length > 0"
        data-testid="reconnect-unused"
        class="text-xs text-fg-secondary"
      >
        Not used — it matched none of the missing originals: {{ unused.map((n) => `“${n}”`).join(", ") }}
      </p>

      <p
        data-testid="reconnect-status"
        :role="statusRole"
        class="min-h-4 break-words text-xs"
        :class="statusClass"
      >
        {{ statusText }}
      </p>
    </div>

    <template #footer>
      <FooterReason
        data-testid="reconnect-find-all-reason"
        :text="findAllReason"
      />
      <DialogButton
        :reason="closeReason"
        @click="close"
      >
        Close
      </DialogButton>
      <DialogButton
        variant="primary"
        data-testid="reconnect-find-all"
        :reason="findAllReason"
        @click="reconnect.run(findAllIds, false)"
      >
        Find all…
      </DialogButton>
    </template>
  </DialogHost>
</template>
