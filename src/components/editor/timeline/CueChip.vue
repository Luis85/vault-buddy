<script setup lang="ts">
/**
 * One teaching cue on the timeline's Teaching layers row (visual-parity
 * Task 19; concept spec §6.4 `.fx-clip`, design D11, D14, D16): 19 px tall
 * at its lane's top, as wide as it plays (never under 9 px), its text or
 * kind — a zoom gold, reading "1.65× Focus". Positioned by its caller in
 * the lane's content space.
 *
 * **Select.** A click or Enter selects the cue and its clip and brings the
 * inspector in on Teaching properties (`selectCue`, the cue menu's own
 * "Edit annotation").
 *
 * **Move and trim** follow the clip drag (`ClipItem`): the body drags, the
 * 7 px edge grips trim; the preview follows the pointer and a release sends
 * ONE `updateEffect` (`useCueDrag`), Escape discards it; the dragged edge
 * snaps to what a clip edge snaps to and shows the timeline's dashed guide.
 * ←/→ nudge by a frame (Shift: a second), Delete removes the cue — and
 * stops there, so the shell's Delete never takes the cue's CLIP with it
 * (selecting a cue selects its clip too).
 *
 * **A cue on a locked track** is Rust's to refuse (`ensure_unlocked` on
 * every cue edit), so no drag starts: the chip says why in its `title`, and
 * a key that would move or delete it toasts the same reason — a refusal is
 * never silent (D14). It can still be selected and opened.
 *
 * Shift+F10 / the Menu key, or a right-click, asks `TimelineView` for the
 * cue menu (`menuSets.cueMenu`); Escape there gives focus back here.
 * `role="option"` inside the row's listbox, like a clip: the forced-colours
 * rule for `aria-selected` keeps the selection visible.
 */
import { computed, nextTick, ref } from "vue";

import { useCueDrag } from "../../../composables/useCueDrag";
import { selectCue } from "../../../composables/useEditorMenuContext";
import { usePointerPress } from "../../../composables/usePointerPress";
import { clipSpanOf } from "../../../editor/actionTargets";
import type { TeachingCue } from "../../../editor/cueLanes";
import { cueTop } from "../../../editor/cueLanes";
import { cueDragHint, useDragHintReport } from "../../../editor/dragHint";
import { EFFECT_NAMES } from "../../../editor/effectFields";
import { isContextMenuShortcut } from "../../../editor/shortcuts";
import { useSnapGuideReport } from "../../../editor/snapGuide";
import { msToX, snapTargets } from "../../../editor/timelineLayout";
import { cueOutputSpan } from "../../../editor/timeMap";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { useNotificationsStore } from "../../../stores/notifications";

const props = defineProps<{
  cue: TeachingCue;
  selected: boolean;
  zoom: number;
  /** Why the cue cannot change (its clip's track is locked), or `null`. */
  lockedReason: string | null;
}>();
const emit = defineEmits<{
  (e: "context-menu", payload: { effectId: string; clientX: number; clientY: number }): void;
}>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
const notifications = useNotificationsStore();

const drag = useCueDrag({
  effect: () => props.cue.effect,
  clip: () => props.cue.clip,
  zoom: () => props.zoom,
  snapEnabled: () => workspace.snap,
  snapTargets: () => snapTargets(editorProject.project, workspace.playheadMs),
  execute: (command) => editorProject.execute(command),
});

/** The dashed guide `TimelineView` draws while this cue's edge snaps, and
 * the footer's line for the drag. */
useSnapGuideReport(drag.snapGuideMs);
useDragHintReport(computed(() => cueDragHint(props.cue.effect.kind, drag.grip.value)));

// ---- geometry -------------------------------------------------------------------

/** The concept's floor: a cue is never narrower than this. */
const MIN_WIDTH_PX = 9;

/** The output span to draw: the live preview's, else the committed one. */
const span = computed<[number, number]>(() => {
  const p = drag.preview.value;
  const live = p && cueOutputSpan(clipSpanOf(props.cue.clip), p.startMs, p.endMs);
  return live ?? [props.cue.startMs, props.cue.endMs];
});
const leftPx = computed(() => msToX(span.value[0], props.zoom));
const widthPx = computed(() => Math.max(MIN_WIDTH_PX, msToX(span.value[1], props.zoom) - leftPx.value));
const style = computed(() => ({ left: `${leftPx.value}px`, width: `${widthPx.value}px`, top: `${cueTop(props.cue.lane)}px` }));

/** An edge grip's width; a chip narrower than two of them puts its grips
 * just outside its edges, so its body can still be pressed to move it
 * (`ClipItem`'s trim grips, the same rule). */
const GRIP_PX = 7;
/** The edge grips — none on a locked cue, which cannot be trimmed. */
const grips = computed(() => {
  if (props.lockedReason) return [];
  const narrow = widthPx.value < GRIP_PX * 2;
  return [
    { edge: "start" as const, place: narrow ? "right-full" : "left-0" },
    { edge: "end" as const, place: narrow ? "left-full" : "right-0" },
  ];
});

const kindName = computed(() => EFFECT_NAMES[props.cue.effect.kind]);
const title = computed(() => props.lockedReason ?? `${kindName.value}: ${props.cue.label}`);
const chipClass = computed(() => [
  props.cue.effect.kind === "zoom" ? "border-gold bg-gold-bg text-gold" : "border-cue-edge bg-accent-bg text-accent-ink",
  props.selected ? "z-[4] outline-2 outline-offset-0 outline-accent" : "z-[2]",
  props.lockedReason ? "cursor-default" : "cursor-grab",
]);

// ---- pointer --------------------------------------------------------------------

const root = ref<HTMLElement | null>(null);
/** Primary button, pointer capture, focus, the drag-ending click
 * swallowed — `ClipItem`'s own press plumbing. */
const press = usePointerPress(root);

function select(): void {
  if (!press.swallowClick()) selectCue(workspace, props.cue.effect);
}

/** A press on an unlocked cue starts the grip's drag; a locked cue only
 * takes focus (its title says why nothing moves). */
function onPointerDown(event: PointerEvent, grip: "move" | "start" | "end"): void {
  if (press.begin(event) && !props.lockedReason) drag.begin(grip, event.clientX);
}
function onPointerMove(event: PointerEvent): void {
  drag.update(event.clientX);
}
async function onPointerUp(event: PointerEvent): Promise<void> {
  press.end(event);
  await drag.end();
}

function onContextMenu(event: MouseEvent): void {
  emit("context-menu", { effectId: props.cue.effect.id, clientX: event.clientX, clientY: event.clientY });
}

// ---- keyboard -------------------------------------------------------------------

const SECOND_MS = 1_000;
/** One frame at the project's own rate (the ruler's own step). */
const frameMs = computed(() => Math.round(1_000 / (editorProject.project?.canvas.fps || 30)));

/** Says why a key did nothing to this cue (D14). */
function refuse(reason: string): void {
  notifications.info(reason);
}

async function nudge(event: KeyboardEvent): Promise<void> {
  event.preventDefault();
  if (props.lockedReason) return refuse(props.lockedReason);
  const forward = event.key === "ArrowRight";
  const step = event.shiftKey ? SECOND_MS : frameMs.value;
  const moved = await drag.nudge(forward ? step : -step);
  if (!moved) return refuse(`The cue is at the ${forward ? "end" : "start"} of its clip.`);
  // Through this chip's own element: a document query by test id could
  // find another element wearing it first (the preview's cue overlay did).
  await nextTick();
  root.value?.focus();
}

function remove(event: KeyboardEvent): void {
  event.preventDefault();
  event.stopPropagation();
  if (props.lockedReason) return refuse(props.lockedReason);
  void editorProject.execute({ kind: "removeEffect", effectId: props.cue.effect.id });
}

function openMenuFromKeyboard(event: KeyboardEvent): void {
  event.preventDefault();
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  emit("context-menu", { effectId: props.cue.effect.id, clientX: rect.left, clientY: rect.bottom });
}

/** Each key this chip answers, by `event.key`. */
const KEYS: Record<string, (event: KeyboardEvent) => void> = {
  Enter: (event) => {
    event.preventDefault();
    select();
  },
  " ": (event) => {
    event.preventDefault();
    select();
  },
  Escape: (event) => {
    // Claimed only when it cancelled a drag: the shell's guide dismissal
    // reads `defaultPrevented` (`ClipItem`'s review finding F-M2).
    if (drag.cancel()) event.preventDefault();
  },
  ArrowLeft: (event) => void nudge(event),
  ArrowRight: (event) => void nudge(event),
  Delete: remove,
  Backspace: remove,
};

function onKeydown(event: KeyboardEvent): void {
  if (isContextMenuShortcut(event)) return openMenuFromKeyboard(event);
  KEYS[event.key]?.(event);
}
</script>

<template>
  <div
    ref="root"
    :data-testid="`timeline-cue-${cue.effect.id}`"
    role="option"
    tabindex="0"
    :aria-selected="selected"
    :aria-label="`${kindName}: ${cue.label}`"
    :title="title"
    class="absolute h-[19px] touch-none rounded-[4px] border py-[2px] text-[9px] leading-[13px] whitespace-nowrap select-none focus-visible:z-[5] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
    :class="chipClass"
    :style="style"
    @click="select"
    @contextmenu.prevent="onContextMenu"
    @keydown="onKeydown"
    @pointerdown="onPointerDown($event, 'move')"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
  >
    <!-- The padding is the label's, not the chip's: a chip's box cannot be
         narrower than its own padding, so a 9px cue would draw 14px. -->
    <span class="pointer-events-none block truncate px-1.5">{{ cue.label }}</span>
    <span
      v-for="g in grips"
      :key="g.edge"
      :data-testid="`timeline-cue-${cue.effect.id}-trim-${g.edge}`"
      aria-hidden="true"
      class="absolute top-0 bottom-0 w-[7px] cursor-ew-resize"
      :class="g.place"
      @pointerdown.stop="onPointerDown($event, g.edge)"
      @pointermove.stop="onPointerMove"
      @pointerup.stop="onPointerUp"
    />
  </div>
</template>
