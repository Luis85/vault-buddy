<script setup lang="ts">
/**
 * One missing original in `ReconnectDialog` (Task 40): what it was — name,
 * size, length, never a path — what the last reconnect said about it, and
 * the one or two ways forward. Presentational: the dialog runs the
 * reconnect. A reconnected or replaced row offers nothing more; a
 * mismatched one also offers **Replace…**; everything else (untried,
 * ambiguous, unmatched) only **Choose file…**, for this asset alone.
 */
import { computed } from "vue";

import type { ReconnectOutcome } from "../../../composables/useMediaReconnect";
import { describeOutcome } from "../../../editor/reconnectText";
import type { MissingMedia } from "../../../editorTypes";
import { formatBytes } from "../../../utils/formatBytes";
import { formatDuration } from "../../../utils/formatDuration";
import AppButton from "../../ui/AppButton.vue";

const props = defineProps<{ item: MissingMedia; outcome: ReconnectOutcome | undefined; busy: boolean }>();
const emit = defineEmits<{ (e: "choose"): void; (e: "replace"): void }>();

/** A lightweight file from an older build may not know the size (GAP-182). */
const facts = computed(() => {
  const size = props.item.expectedSize > 0 ? formatBytes(props.item.expectedSize) : "size unknown";
  return `${size} · ${formatDuration(props.item.expectedDurationMs)}`;
});
const done = computed(() => props.outcome?.kind === "matched" || props.outcome?.kind === "replaced");
const replaceable = computed(() => props.outcome?.kind === "mismatched");
const text = computed(() => describeOutcome(props.outcome));
</script>

<template>
  <li
    :data-testid="`reconnect-row-${item.assetId}`"
    class="flex flex-col gap-1 rounded-control border border-line p-2 text-xs"
  >
    <span class="font-medium text-fg">{{ item.name }}</span>
    <span class="text-fg-muted">{{ facts }}</span>
    <span :class="done ? 'text-success' : 'text-fg-secondary'">{{ text }}</span>
    <span
      v-if="!done"
      class="flex flex-wrap gap-2"
    >
      <AppButton
        variant="secondary"
        data-testid="reconnect-choose"
        :disabled="busy"
        @click="emit('choose')"
      >
        Choose file…
      </AppButton>
      <AppButton
        v-if="replaceable"
        variant="secondary"
        data-testid="reconnect-replace"
        :disabled="busy"
        @click="emit('replace')"
      >
        Replace…
      </AppButton>
    </span>
  </li>
</template>
