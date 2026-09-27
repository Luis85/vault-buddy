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
import type { TeachingCue } from "../../../editor/cueLanes";
import { cueTop } from "../../../editor/cueLanes";
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

/** The dashed guide `TimelineView` draws while this cue's edge snaps. */
useSnapGuideReport(drag.snapGuideMs);

// ---- geometry -------------------------------------------------------------------

/** The concept's floor: a cue is never narrower than this. */
const MIN_WIDTH_PX = 9;

/** The output span to draw: the live preview's, else the committed one. */
const span = computed<[number, number]>(() => {
  const p = drag.preview.value;
  const c = props.cue.clip;
  const live = p && cueOutputSpan({ start_ms: c.start_ms, in_ms: c.in_ms, out_ms: c.out_ms, speed: c.speed ?? 1 }, p.startMs, p.endMs);
  return live ?? [props.cue.startMs, props.cue.endMs];
});
const style = computed(() => {
  const left = msToX(span.value[0], props.zoom);
  const width = Math.max(MIN_WIDTH_PX, msToX(span.value[1], props.zoom) - left);
  return { left: `${left}px`, width: `${width}px`, top: `${cueTop(props.cue.lane)}px` };
});

/** The edge grips — none on a locked cue, which cannot be trimmed. */
const grips = computed(() => (props.lockedReason ? [] : (["start", "end"] as const)));

const kindName = computed(() => EFFECT_NAMES[props.cue.effect.kind]);
const title = computed(() => props.lockedReason ?? `${kindName.value}: ${props.cue.label}`);
const chipClass = computed(() => [
  props.cue.effect.kind === "zoom" ? "border-gold bg-gold-bg text-gold" : "border-cue-edge bg-accent-bg text-accent-ink",
  props.selected ? "z-[4] outline-2 outline-offset-0 outline-accent" : "z-[2]",
  props.lockedReason ? "cursor-default" : "cursor-grab",
]);

// ---- pointer --------------------------------------------------------------------

const root = ref<HTMLElement | null>(null);
/** Pointer travel (px) below which a press-and-release is still a click. */
const DRAG_SLOP_PX = 3;
let pressX: number | null = null;
let suppressNextClick = false;

function select(): void {
  if (suppressNextClick) {
    suppressNextClick = false;
    return;
  }
  selectCue(workspace, props.cue.effect);
}

/** Primary button on an unlocked cue: capture the pointer, take focus (so
 * Escape reaches `onKeydown` mid-drag) and start the grip's drag. */
function onPointerDown(event: PointerEvent, grip: "move" | "start" | "end"): void {
  if (event.button !== 0) return;
  root.value?.focus({ preventScroll: true });
  if (props.lockedReason) return;
  (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
  pressX = event.clientX;
  suppressNextClick = false;
  drag.begin(grip, event.clientX);
}
function onPointerMove(event: PointerEvent): void {
  drag.update(event.clientX);
}
async function onPointerUp(event: PointerEvent): Promise<void> {
  const el = event.currentTarget as HTMLElement;
  if (el.hasPointerCapture?.(event.pointerId)) el.releasePointerCapture?.(event.pointerId);
  if (pressX !== null && Math.abs(event.clientX - pressX) > DRAG_SLOP_PX) suppressNextClick = true;
  pressX = null;
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
  await nextTick();
  document.querySelector<HTMLElement>(`[data-testid="cue-${props.cue.effect.id}"]`)?.focus();
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
    :data-testid="`cue-${cue.effect.id}`"
    role="option"
    tabindex="0"
    :aria-selected="selected"
    :aria-label="`${kindName}: ${cue.label}`"
    :title="title"
    class="absolute h-[19px] touch-none truncate rounded-[4px] border px-1.5 py-[2px] text-[9px] leading-[13px] whitespace-nowrap select-none focus-visible:z-[5] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
    :class="chipClass"
    :style="style"
    @click="select"
    @contextmenu.prevent="onContextMenu"
    @keydown="onKeydown"
    @pointerdown="onPointerDown($event, 'move')"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
  >
    <span
      v-for="edge in grips"
      :key="edge"
      :data-testid="`cue-${cue.effect.id}-grip-${edge}`"
      aria-hidden="true"
      class="absolute top-0 bottom-0 w-[7px] cursor-ew-resize"
      :class="edge === 'start' ? 'left-0' : 'right-0'"
      @pointerdown.stop="onPointerDown($event, edge)"
      @pointermove.stop="onPointerMove"
      @pointerup.stop="onPointerUp"
    />{{ cue.label }}
  </div>
</template>
