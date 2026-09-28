<script setup lang="ts">
import { computed } from "vue";

import type { CaptureSourceInfo } from "../types";
import SelectMenu from "./SelectMenu.vue";
import EmptyState from "./ui/EmptyState.vue";

/**
 * The Window tab: a DROPDOWN, where the Screen tab is a list of cards.
 *
 * The asymmetry is the point rather than an inconsistency. A machine has two
 * or three monitors, and each card's second line — its resolution, and which
 * one is primary — is what makes them tellable apart, worth the vertical
 * space. Open windows are unbounded: the reported session had a dozen, which
 * filled the panel, pushed Start below the fold, and made choosing a
 * scroll-and-hunt. One row collapses that however many are open.
 *
 * Extracted rather than inlined in `ScreenSourcePicker`, for the reason
 * `ScreenRegionPicker` was: adding a third branch to that file's tab template
 * tripped the complexity ratchet, and this repo pays that back by splitting
 * rather than by moving the baseline.
 */
const props = defineProps<{
  windows: readonly CaptureSourceInfo[];
  selectedId: string | null;
}>();
const emit = defineEmits<{ (e: "update:selectedId", value: string | null): void }>();

/**
 * The process name rides in the LABEL rather than being dropped, because it
 * is often the only thing distinguishing two entries — the reported session
 * had three windows all ending "- Visual Studio Code".
 *
 * The leading placeholder is a real option, not a disabled hint: the trigger
 * would otherwise render blank whenever nothing on THIS tab is selected (a
 * screen or a region can be), and choosing it clears the selection, which a
 * user may legitimately want.
 */
const options = computed(() => [
  { value: "", label: "Choose a window…" },
  ...props.windows.map((w) => ({
    value: w.id,
    label: w.detail ? `${w.title} — ${w.detail}` : w.title,
  })),
]);

// Back to `null`, never `""`: Start is gated on `selectedId !== null`, so an
// empty string would arm it with no source.
function onPicked(value: string | number) {
  emit("update:selectedId", value === "" ? null : String(value));
}
</script>

<template>
  <EmptyState
    v-if="windows.length === 0"
    title="No capture sources available."
    hint="Open a window or connect a display, then reopen this screen."
  />
  <SelectMenu
    v-else
    data-testid="source-window-select"
    aria-label="Window to capture"
    wide
    :model-value="selectedId ?? ''"
    :options="options"
    @update:model-value="onPicked"
  />
</template>
