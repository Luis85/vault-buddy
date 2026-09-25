<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";

import type { CaptureSourceInfo, RegionSelection } from "../types";
import { regionDetail, regionTitle } from "../utils/regionLabel";
import AppButton from "./ui/AppButton.vue";
import EmptyState from "./ui/EmptyState.vue";

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
  /** The monitor the held region was cut from — NOT `targetId`, which only
   * says which row is highlighted right now. See the owner's own ref. */
  monitorId: string | null;
  region: RegionSelection | null;
  selectedId: string | null;
  selecting: boolean;
  starting: boolean;
}>();
defineEmits<{
  (e: "update:targetId", value: string): void;
  (e: "update:selectedId", value: string): void;
  (e: "select"): void;
}>();

/** Must read the same as the Rust side's own title for the resolved source —
 * see `src/utils/regionLabel.ts`. Keyed on the region's OWN monitor, never on
 * `targetId`: clicking another target row without selecting would otherwise
 * re-label the armed region after a monitor the capture bar and the staging
 * sidecar (which render the Rust string) will never name. The fallback never
 * renders in practice (the button that produces a region is disabled until a
 * target is picked), but a monitor unplugged between the pick and the render
 * must not print "Region on undefined". */
const rowTitle = computed(() =>
  regionTitle(
    props.screens.find((s) => s.id === props.monitorId)?.title ?? "this screen",
  ),
);

const buttonLabel = computed(() => {
  if (props.selecting) return "Selecting…";
  return props.region ? "Reselect region…" : "Select region…";
});

const selectButton = ref<{ $el: HTMLElement } | null>(null);

// Clicking the button disables it, which drops focus to <body>; nothing put it
// back when it re-enabled after a cancel (the GAP-27 family this codebase
// otherwise honors). Only reclaim focus we actually dropped — if anything else
// took it while the overlay was up, stealing it back would be worse.
watch(
  () => props.selecting,
  async (now, before) => {
    if (now || !before) return;
    await nextTick();
    const active = document.activeElement;
    if (active === null || active === document.body) selectButton.value?.$el?.focus();
  },
);
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- The list tabs say so when they have nothing; without this the tab
         reads "Pick a screen…" above an empty list and a disabled button,
         which reads as "not yet" rather than "there is nothing here". -->
    <EmptyState
      v-if="screens.length === 0"
      title="No capture sources available."
      hint="Connect a display, then reopen this screen."
    />
    <template v-else>
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
        ref="selectButton"
        data-testid="region-select"
        variant="secondary"
        :disabled="targetId === null || selecting || starting"
        @click="$emit('select')"
      >
        {{ buttonLabel }}
      </AppButton>
    </template>
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
