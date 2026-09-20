<script setup lang="ts">
import { computed } from "vue";

import type { CaptureSourceInfo, RegionSelection } from "../types";
import { regionDetail, regionTitle } from "../utils/regionLabel";
import AppButton from "./ui/AppButton.vue";

// Spec 7.2's third tab, split out of ScreenSourcePicker's own template: that
// template already carried the two enumerated row lists, and folding a third,
// differently-shaped panel into it pushed it over the complexity gate. This
// half is purely presentational — the owner keeps the IPC, the selection and
// the error — the same split ScreenAudioPicker already uses.
//
// The tab lists MONITORS as targets rather than offering one bare
// "Select region…" button, because a region lives on exactly one monitor and
// the overlay covers exactly one: a single overlay spanning the virtual
// desktop cannot be described by one scale factor on a mixed-DPI desktop
// (core::screen_geometry's module doc says so explicitly).
const props = defineProps<{
  screens: CaptureSourceInfo[];
  targetId: string | null;
  region: RegionSelection | null;
  selectedId: string | null;
  selecting: boolean;
}>();
defineEmits<{
  (e: "update:targetId", value: string): void;
  (e: "update:selectedId", value: string): void;
  (e: "select"): void;
}>();

/** Must read the same as the Rust side's own title for the resolved source —
 * see `src/utils/regionLabel.ts`. The fallback never renders in practice (the
 * button that produces a region is disabled until a target is picked), but a
 * monitor unplugged between the pick and the render must not print
 * "Region on undefined". */
const rowTitle = computed(() =>
  regionTitle(
    props.screens.find((s) => s.id === props.targetId)?.title ?? "this screen",
  ),
);

const buttonLabel = computed(() => {
  if (props.selecting) return "Selecting…";
  return props.region ? "Reselect region…" : "Select region…";
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <p class="text-xs text-fg-muted">
      Pick a screen, then drag out the area you want to record.
    </p>
    <ul class="flex flex-col gap-1">
      <li
        v-for="s in screens"
        :key="s.id"
      >
        <button
          type="button"
          :data-testid="`region-target-${s.id}`"
          :aria-pressed="targetId === s.id"
          class="w-full cursor-pointer rounded-control border bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          :class="targetId === s.id ? 'border-violet-400' : 'border-white/10'"
          @click="$emit('update:targetId', s.id)"
        >
          <span class="block truncate text-sm font-medium text-fg">{{ s.title }}</span>
          <span class="block truncate text-xs text-fg-muted">{{ s.detail }}</span>
        </button>
      </li>
    </ul>
    <AppButton
      data-testid="region-select"
      variant="secondary"
      :disabled="targetId === null || selecting"
      @click="$emit('select')"
    >
      {{ buttonLabel }}
    </AppButton>
    <button
      v-if="region"
      type="button"
      :data-testid="`source-${region.sourceId}`"
      :aria-pressed="selectedId === region.sourceId"
      class="w-full cursor-pointer rounded-control border bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="selectedId === region.sourceId ? 'border-violet-400' : 'border-white/10'"
      @click="$emit('update:selectedId', region.sourceId)"
    >
      <span class="block truncate text-sm font-medium text-fg">{{ rowTitle }}</span>
      <span class="block truncate text-xs text-fg-muted">{{ regionDetail(region) }}</span>
    </button>
  </div>
</template>
