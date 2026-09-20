<script setup lang="ts">
/**
 * The segment strip (spec 8.2). Presentational: it renders what it is given
 * and emits what was clicked. Every width and the playhead are fractions of
 * OUTPUT time, never source time — positioning by source would misplace both
 * the moment anything is cut, which is every moment this surface is useful.
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import type { TimelineDto } from "../../types";
import { dropIndex, outputDurationMs, segmentWidths } from "../../utils/timelineGeometry";

const props = defineProps<{
  timeline: TimelineDto;
  selected: number | null;
  playheadMs: number;
}>();

/**
 * `reorder`'s second payload is an insertion SLOT in `[0, n]`, not a
 * destination index in `[0, n)`. The two are different concepts and the
 * parameter is named for the one it carries: `n` ("dropped past the last
 * block") is a real answer here, and it is exactly the value
 * `useEditorTimeline.reorder` REFUSES. A container converts before calling
 * it — `slot > from ? slot - 1 : slot` — and a container that forgets gets a
 * drag that silently does nothing.
 */
const emit = defineEmits<{ select: [index: number]; reorder: [from: number, slot: number] }>();

const widths = computed(() => segmentWidths(props.timeline));
const playheadPct = computed(() => {
  const total = outputDurationMs(props.timeline);
  if (total === 0) return 0;
  return Math.min(100, Math.max(0, (props.playheadMs / total) * 100));
});

/** Drag-to-reorder (spec 8.2), pointer-based rather than HTML5 drag-and-drop
 * because Tauri intercepts the latter — the same reason the task list's own
 * reorder is pointer-based (AGENTS.md, the tasks domain). */
const dragFrom = ref<number | null>(null);
const strip = ref<HTMLElement | null>(null);

function onDown(index: number) {
  dragFrom.value = index;
}

function onUp(e: PointerEvent) {
  const from = dragFrom.value;
  // Cleared FIRST, so the window-level copy of this handler (below) is inert
  // for a release that the strip's own handler already answered.
  dragFrom.value = null;
  if (from === null || strip.value === null) return;
  const rect = strip.value.getBoundingClientRect();
  if (rect.width === 0) return;
  const fraction = (e.clientX - rect.left) / rect.width;
  // Released off the strip: a cancelled drag, not a drop at the nearest end.
  // Guessing an end would commit a reorder — an undo entry and a sidecar
  // write — for a gesture the user abandoned by dragging away.
  if (fraction < 0 || fraction > 1) return;
  const slot = dropIndex(widths.value, fraction);
  // A block released anywhere over its OWN place is a select, not a
  // zero-distance reorder. Nothing here tracks movement, so this is
  // positional rather than distance-based; a press that never moved is the
  // special case of it. `reorder` itself also no-ops, but emitting would
  // push an undo entry for a click that changed nothing.
  if (slot === from || slot === from + 1) return;
  emit("reorder", from, slot);
}

/**
 * The same handler again, on the window.
 *
 * `@pointerup` on the strip answers the ordinary release, over the strip.
 * It cannot see a release ANYWHERE ELSE — drag a block off the window and
 * let go, and `dragFrom` would stay set; the next pointerup over the
 * strip (a release with no pointerdown on a block) would then reorder a
 * block the user stopped dragging a minute ago. `pointercancel`
 * does not cover it: nothing cancelled, the pointer simply went somewhere
 * else. `onUp` clears `dragFrom` before doing anything, so the two paths
 * can both fire for one release without emitting twice.
 */
onMounted(() => window.addEventListener("pointerup", onUp));
onBeforeUnmount(() => window.removeEventListener("pointerup", onUp));
</script>

<template>
  <!-- NO padding on the strip and NO gap between the blocks, and both are
       load-bearing rather than taste. `segmentWidths` returns percentages of
       the WHOLE strip and the playhead is positioned `left: X%` of the same
       box, while `onUp` turns a pointer position into a fraction of that box
       before handing it to `dropIndex`. Padding inset the blocks from the box
       all three measure against, so the playhead never lined up with the
       boundaries it marks (4 px out at either end), and a gap shrank every
       block below its nominal width, so the rendered geometry stopped being
       the geometry `dropIndex` computes against — drift that grows with every
       split (the phase review's m-4). The blocks already carry their own
       border, which is what separates them now. -->
  <div
    ref="strip"
    data-testid="timeline-strip"
    class="relative h-16 w-full overflow-hidden rounded-control bg-white/5"
    @pointerup="onUp"
    @pointercancel="dragFrom = null"
  >
    <!-- Reachable only from a capture with no duration or a hand-edited
         sidecar: `deleteSegment` refuses to remove the last segment, so the
         user cannot empty a timeline from this surface. The copy used to say
         "undo a delete or revert the edit", which named a delete that cannot
         have happened and a Revert verb this window does not offer at all
         (the phase review's m-1). -->
    <p
      v-if="widths.length === 0"
      data-testid="strip-empty"
      class="flex h-full items-center justify-center text-xs text-fg-muted"
    >
      This capture has no footage to show.
    </p>
    <div
      v-else
      class="flex h-full"
    >
      <button
        v-for="(w, i) in widths"
        :key="i"
        type="button"
        :data-testid="`segment-${i}`"
        :aria-pressed="selected === i"
        :aria-label="`Segment ${i + 1} of ${widths.length}`"
        :style="{ width: `${w}%` }"
        class="h-full cursor-pointer rounded border transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="
          selected === i
            ? 'border-violet-400 bg-accent/25'
            : 'border-white/10 bg-white/10 hover:bg-white/15'
        "
        @click="emit('select', i)"
        @pointerdown="onDown(i)"
      />
    </div>
    <div
      v-if="widths.length > 0"
      data-testid="playhead"
      class="pointer-events-none absolute top-0 h-full w-0.5 bg-accent"
      :style="{ left: `${playheadPct}%` }"
    />
  </div>
</template>
