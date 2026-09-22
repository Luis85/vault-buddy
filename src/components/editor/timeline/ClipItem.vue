<script setup lang="ts">
/**
 * One clip's visual (Task 20; F-04, F-14, F-26;
 * SCREENS-AND-INTERACTIONS.md §03: "Clips show media type, selected state,
 * trims and gold fade controls"). Presentational and absolutely positioned
 * by its caller (`TrackLane.vue` supplies `leftPx`/`widthPx` in CONTENT
 * space, `timelineLayout.ts`'s own convention) — this component never reads
 * zoom or scroll itself.
 *
 * Selection is a direct store write (`editorWorkspace.select`, the
 * `PreviewToolbar`/`InspectorPanel` precedent of components calling a store
 * directly rather than emitting purely upward) — the only thing this
 * component bubbles UP is "open a context menu here", because the ONE
 * `ContextMenu` instance lives at `TimelineView.vue`, several components
 * away. Right-click and Shift+F10/Menu (SCREENS-AND-INTERACTIONS.md §03:
 * "The same actions are reachable from … Shift+F10") funnel into the SAME
 * `context-menu` emit shape so `TimelineView` has one handler, not two.
 *
 * Trim handles are VISUAL ONLY this task (drag/trim lands in Task 21) — they
 * carry no pointer handlers, so dragging one does nothing yet; rendering
 * them now is what lets Task 21 add behavior without a markup change.
 *
 * `role="option"`, not `role="button"` (fix round 1, finding 3): `aria-
 * selected` is only a valid ARIA attribute on a handful of roles (option,
 * row, tab, gridcell, …) and `button` is not one of them —
 * `SearchHitRow.vue`'s own `role="option"` + `:aria-selected` is this
 * repo's existing precedent for exactly this "one of several selectable
 * items" shape. Because this element is a `<div>`, not a native `<button>`,
 * Enter/Space activation has to be wired by hand
 * (`TranscriptionSummary.vue`'s `@keydown.enter`/`@keydown.space.prevent`
 * precedent, folded into the same `onKeydown` this component already uses
 * for Shift+F10/Menu) — without it a keyboard user who tabs to a clip can
 * open its context menu but has no way to select it.
 */
import { isContextMenuShortcut } from "../../../editor/shortcuts";
import { clipOutputEnd } from "../../../editor/timeMap";
import type { Clip, ClipSpan } from "../../../editorTypes";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";

const props = defineProps<{
  clip: Clip;
  assetKind: "video" | "audio";
  selected: boolean;
  leftPx: number;
  widthPx: number;
}>();
const emit = defineEmits<{
  (e: "context-menu", payload: { clip: Clip; clientX: number; clientY: number }): void;
}>();

const workspace = useEditorWorkspaceStore();

function spanOf(c: Clip): ClipSpan {
  return { start_ms: c.start_ms, in_ms: c.in_ms, out_ms: c.out_ms, speed: c.speed ?? 1 };
}

function endMs(c: Clip): number {
  return clipOutputEnd(spanOf(c));
}

function label(c: Clip): string {
  return `Clip ${c.name}, ${formatDuration(c.start_ms)}–${formatDuration(endMs(c))}`;
}

function onSelect() {
  workspace.select([props.clip.id]);
}

function onContextMenu(event: MouseEvent) {
  emit("context-menu", { clip: props.clip, clientX: event.clientX, clientY: event.clientY });
}

function onKeydown(event: KeyboardEvent) {
  if (isContextMenuShortcut(event)) {
    event.preventDefault();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    emit("context-menu", { clip: props.clip, clientX: rect.left, clientY: rect.bottom });
    return;
  }
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    onSelect();
  }
}
</script>

<template>
  <div
    :data-testid="`clip-${clip.id}`"
    role="option"
    tabindex="0"
    :aria-selected="selected"
    :aria-label="label(clip)"
    class="absolute top-1 bottom-1 flex items-center overflow-hidden rounded border px-1 text-micro cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    :class="[
      assetKind === 'audio' ? 'bg-audio-bg border-audio text-audio' : 'bg-video-bg border-video text-video',
      selected ? 'ring-2 ring-accent' : '',
    ]"
    :style="{ left: `${leftPx}px`, width: `${Math.max(widthPx, 2)}px` }"
    @click="onSelect"
    @contextmenu.prevent="onContextMenu"
    @keydown="onKeydown"
  >
    <div
      v-if="clip.fade_in_ms > 0"
      :data-testid="`clip-${clip.id}-fade-in`"
      class="pointer-events-none absolute inset-y-0 left-0 w-3 bg-gold/40"
      style="clip-path: polygon(0 100%, 100% 100%, 0 0)"
    />
    <div
      v-if="clip.fade_out_ms > 0"
      :data-testid="`clip-${clip.id}-fade-out`"
      class="pointer-events-none absolute inset-y-0 right-0 w-3 bg-gold/40"
      style="clip-path: polygon(0 100%, 100% 100%, 100% 0)"
    />
    <!-- F-26: a reserved lane slot for a future waveform, not the waveform
         itself -- peaks rendering needs a Rust-side job this task does not
         add (the brief's own "(lane slot)" scoping). -->
    <div
      v-if="assetKind === 'audio'"
      :data-testid="`clip-${clip.id}-waveform-slot`"
      class="pointer-events-none absolute inset-x-1 bottom-0.5 h-3 rounded bg-audio-bg/60"
    />

    <span class="pointer-events-none relative z-10 truncate">{{ clip.name }}</span>

    <span
      :data-testid="`clip-${clip.id}-trim-start`"
      class="absolute inset-y-0 left-0 w-1 cursor-ew-resize bg-white/10"
    />
    <span
      :data-testid="`clip-${clip.id}-trim-end`"
      class="absolute inset-y-0 right-0 w-1 cursor-ew-resize bg-white/10"
    />
  </div>
</template>
