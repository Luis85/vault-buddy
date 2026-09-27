<script setup lang="ts">
/**
 * The timeline's Captions row (visual-parity Task 19; concept spec §6.4
 * `.caption-track-row`, design D11): shown only while the edit has captions,
 * right under the ruler — 35 px, a pinned gold label "Captions N" (the same
 * sticky label column as every track, D12), and each caption as a 24 px
 * gold cue at the output time it plays (`captionRules.captionRows`, the
 * Captions tab's own list).
 *
 * A click (or Enter) opens the Captions tab ON that cue
 * (`checkReveal.openCaptionCue`, what a caption finding's "Open Captions"
 * does): the tab, the caption selected and the playhead on it. Captions are
 * edited in the tab, so the row is one tab stop: ←/→ (Home/End) move focus
 * between its cues (`useRovingTablist`).
 *
 * `rows` are the captions to draw (`TimelineView` keeps those within a
 * screen of the viewport, as it does clips); `count` is how many the edit
 * has.
 */
import { computed, ref, watch } from "vue";

import { useRovingTablist } from "../../../composables/useRovingTablist";
import type { CaptionRow } from "../../../editor/captionRules";
import { openCaptionCue } from "../../../editor/checkReveal";
import { msToX } from "../../../editor/timelineLayout";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{
  rows: CaptionRow[];
  count: number;
  zoom: number;
  widthPx: number;
  labelWidth: number;
}>();

const workspace = useEditorWorkspaceStore();

/** The concept's floor: a caption is never narrower than this. */
const MIN_WIDTH_PX = 8;

const selectedId = computed(() => (workspace.selected?.type === "caption" ? workspace.selected.id : null));

/** The one cue that takes Tab (the selected one when it is drawn). */
const focusIndex = ref(0);
watch(
  [selectedId, () => props.rows],
  ([id, rows]) => {
    const i = rows.findIndex((r) => r.cue.id === id);
    if (i !== -1) focusIndex.value = i;
    else if (focusIndex.value >= rows.length) focusIndex.value = Math.max(0, rows.length - 1);
  },
  { immediate: true },
);
const roving = useRovingTablist(
  () => props.rows.length,
  () => focusIndex.value,
  (i) => (focusIndex.value = i),
);

function cueStyle(row: CaptionRow) {
  const left = msToX(row.startMs, props.zoom);
  return { left: `${left}px`, width: `${Math.max(MIN_WIDTH_PX, msToX(row.endMs, props.zoom) - left)}px` };
}
</script>

<template>
  <div
    v-if="count > 0"
    data-testid="captions-row"
    class="flex w-max min-w-full border-b border-line bg-panel"
    :style="{ height: '35px' }"
  >
    <div
      data-testid="captions-row-label"
      class="sticky left-0 z-[15] flex shrink-0 items-center gap-2 border-r border-line bg-panel px-3 py-[7px] text-[10px] text-gold"
      :style="{ width: `${labelWidth}px` }"
    >
      <EditorIcon
        name="captions"
        :size="16"
      />Captions <span class="ml-auto text-fg-muted">{{ count }}</span>
    </div>

    <div
      data-testid="captions-row-lane"
      role="listbox"
      aria-label="Captions"
      aria-orientation="horizontal"
      class="relative shrink-0"
      :style="{ width: `${widthPx}px` }"
      @keydown="roving.onKeydown"
    >
      <button
        v-for="(row, i) in rows"
        :key="row.cue.id"
        :ref="(el) => roving.setTabRef(i, el as Element | null)"
        type="button"
        role="option"
        :data-testid="`caption-cue-${row.cue.id}`"
        :tabindex="i === focusIndex ? 0 : -1"
        :aria-selected="row.cue.id === selectedId"
        :aria-label="`Edit caption ${row.index}: ${row.cue.text}`"
        :title="row.cue.text"
        class="absolute top-[5px] block h-[24px] min-h-[24px] truncate rounded-[4px] border border-caption-edge bg-gold-bg px-[7px] py-[3px] text-left text-[9px] leading-[16px] whitespace-nowrap text-gold focus-visible:z-[5] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        :class="row.cue.id === selectedId ? 'z-[4] outline-2 outline-offset-0 outline-accent' : 'z-[2]'"
        :style="cueStyle(row)"
        @click="openCaptionCue(row.cue.id)"
      >
        {{ row.cue.text }}
      </button>
    </div>
  </div>
</template>
