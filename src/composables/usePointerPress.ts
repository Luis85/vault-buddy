/**
 * The press half of a timeline drag (visual-parity Task 19 fix round 1,
 * shared by `ClipItem` and `CueChip`): primary button only (a right-press
 * is the context menu's, never a drag), pointer capture so a fast drag past
 * the element's edge keeps tracking, focus on the element so Escape
 * mid-drag reaches its keydown handler — and the click that ENDS a real
 * drag swallowed, so releasing a dragged clip or cue does not also run its
 * click (a clip's would collapse the selection just dragged as a group).
 */
import type { Ref } from "vue";

/** Pointer travel (px) below which a press-and-release is still a click. */
const DRAG_SLOP_PX = 3;

export interface PointerPress {
  /** On pointerdown: whether this press may start a drag. */
  begin: (event: PointerEvent) => boolean;
  /** On pointerup: releases the capture and notes a real drag. */
  end: (event: PointerEvent) => void;
  /** On click: whether this click ended a drag and must do nothing. */
  swallowClick: () => boolean;
}

export function usePointerPress(root: Ref<HTMLElement | null>): PointerPress {
  let press: { x: number; y: number } | null = null;
  let suppressNextClick = false;

  function begin(event: PointerEvent): boolean {
    if (event.button !== 0) return false;
    (event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId);
    root.value?.focus({ preventScroll: true });
    press = { x: event.clientX, y: event.clientY };
    suppressNextClick = false;
    return true;
  }

  function end(event: PointerEvent): void {
    const el = event.currentTarget as HTMLElement;
    if (el.hasPointerCapture?.(event.pointerId)) el.releasePointerCapture?.(event.pointerId);
    if (press && Math.hypot(event.clientX - press.x, event.clientY - press.y) > DRAG_SLOP_PX) {
      suppressNextClick = true;
    }
    press = null;
  }

  function swallowClick(): boolean {
    const swallow = suppressNextClick;
    suppressNextClick = false;
    return swallow;
  }

  return { begin, end, swallowClick };
}
