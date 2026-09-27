<script setup lang="ts">
/**
 * One missing original in `ReconnectDialog` (Task 40): what it was — name,
 * size, length, never a path — what the last reconnect said about it, and
 * the one or two ways forward. Presentational: the dialog runs the
 * reconnect. A reconnected or replaced row — or one Rust left out of a
 * batch as not reconnectable here — offers nothing more; a mismatched one
 * also offers **Replace…**; everything else (untried, ambiguous,
 * unmatched, a failed copy) only **Choose file…**, for this asset alone.
 *
 * Visual-parity Task 22 (concept spec §9.10, `.source-row`): a bordered
 * row on the app background (padding 11, radius 8) — the name bold 12px,
 * its facts and the outcome 10px under it, the buttons at the right —
 * with `DialogButton`s that say why they wait while a reconnect runs.
 */
import { computed } from "vue";

import type { ReconnectOutcome } from "../../../composables/useMediaReconnect";
import { describeOutcome } from "../../../editor/reconnectText";
import type { MissingMedia } from "../../../editorTypes";
import { formatBytes } from "../../../utils/formatBytes";
import { formatDuration } from "../../../utils/formatDuration";
import DialogButton from "./DialogButton.vue";

const props = defineProps<{ item: MissingMedia; outcome: ReconnectOutcome | undefined; busy: boolean }>();
const emit = defineEmits<{ (e: "choose"): void; (e: "replace"): void }>();

/** A lightweight file from an older build may not know the size (GAP-182). */
const facts = computed(() => {
  const size = props.item.expectedSize > 0 ? formatBytes(props.item.expectedSize) : "size unknown";
  return `${size} · ${formatDuration(props.item.expectedDurationMs)}`;
});
const done = computed(() => props.outcome?.kind === "matched" || props.outcome?.kind === "replaced");
const replaceable = computed(() => props.outcome?.kind === "mismatched");
/** Rust left it out of a batch: no file choice would be accepted. */
const actionable = computed(() => !done.value && props.outcome?.kind !== "excluded");
const text = computed(() => describeOutcome(props.outcome));
const reason = computed(() => (props.busy ? "Checking the chosen files…" : null));
</script>

<template>
  <li
    :data-testid="`reconnect-row-${item.assetId}`"
    class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-line bg-app p-[11px]"
  >
    <span class="flex min-w-0 flex-col gap-1.5">
      <b class="text-xs font-semibold [overflow-wrap:anywhere] text-fg">{{ item.name }}</b>
      <span class="text-[10px] text-fg-muted">{{ facts }}</span>
      <span
        class="text-[10px]"
        :class="done ? 'text-audio' : 'text-fg-secondary'"
      >{{ text }}</span>
    </span>
    <span
      v-if="actionable"
      class="flex shrink-0 flex-wrap gap-2"
    >
      <DialogButton
        data-testid="reconnect-choose"
        :reason="reason"
        @click="emit('choose')"
      >
        Choose file…
      </DialogButton>
      <DialogButton
        v-if="replaceable"
        data-testid="reconnect-replace"
        :reason="reason"
        @click="emit('replace')"
      >
        Replace…
      </DialogButton>
    </span>
  </li>
</template>
