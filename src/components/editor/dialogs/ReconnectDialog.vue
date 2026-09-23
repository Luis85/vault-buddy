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
 */
import { computed, ref, watch } from "vue";

import { useMediaReconnect } from "../../../composables/useMediaReconnect";
import type { MissingMedia } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";
import ReconnectRow from "./ReconnectRow.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const project = useEditorProjectStore();
const reconnect = useMediaReconnect();
const { outcomes, problems, status, busy } = reconnect;

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
const findAllIds = computed(() => listed.value.map((m) => m.assetId).filter((id) => stillMissing.value.has(id)));

const statusText = computed(() => (busy.value ? "Checking the chosen files…" : (status.value?.text ?? "")));
const statusRole = computed(() => (status.value?.alert ? "alert" : "status"));
const statusClass = computed(() => (status.value?.alert ? "text-danger-fg" : "text-fg-secondary"));
const findAllDisabled = computed(() => busy.value || findAllIds.value.length === 0);

function close(): void {
  if (!busy.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Reconnect missing media"
    :closable="!busy"
    @close="close"
  >
    <div
      data-testid="reconnect-dialog"
      class="flex w-[32rem] max-w-full flex-col gap-3"
    >
      <header class="sticky -top-4 z-10 -mx-4 -mt-4 border-b border-line bg-panel px-4 pb-3 pt-4">
        <h2 class="text-sm font-semibold text-fg">
          Reconnect missing media
        </h2>
        <p class="text-xs text-fg-muted">
          Choose the original files. A file is reconnected only when it is clearly the
          original; anything less clear waits for your choice. Your edits are kept.
        </p>
      </header>

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
        data-testid="reconnect-status"
        :role="statusRole"
        class="min-h-4 break-words text-xs"
        :class="statusClass"
      >
        {{ statusText }}
      </p>

      <footer class="sticky -bottom-4 -mx-4 -mb-4 flex justify-end gap-2 border-t border-line bg-panel px-4 pb-4 pt-3">
        <AppButton
          variant="ghost"
          :disabled="busy"
          @click="close"
        >
          Close
        </AppButton>
        <AppButton
          data-testid="reconnect-find-all"
          :disabled="findAllDisabled"
          @click="reconnect.run(findAllIds, false)"
        >
          Find all…
        </AppButton>
      </footer>
    </div>
  </DialogHost>
</template>
