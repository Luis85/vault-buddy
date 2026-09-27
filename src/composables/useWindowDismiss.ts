/**
 * Wires a window-level `pointerdown` and `keydown` listener for the
 * lifetime of the calling component, removing both on unmount — the
 * shared mount/unmount plumbing behind every "an outside click or Escape
 * closes it" popover in the tutorial editor — `MixerPopover.vue`'s audio
 * mixer (`TrackHeader.vue`'s ⋮ track menu used it too, until visual-parity
 * Task 17 moved the track menu onto the shared `MenuPanel`).
 * Extracted once `check:quality`'s clone-group gate caught the two as a
 * byte-identical 8-line block (the `useRovingTablist.ts` precedent for
 * the same gate catching a different pair of components).
 *
 * Deliberately just the wiring, not the open/closed decision or the
 * outside-vs-inside test: each caller's own `onWindowPointerDown`/
 * `onWindowKeydown` still decides whether IT is open and whether the
 * event's target is its own — this composable only ensures the two
 * listeners are added once, on mount, and removed once, on unmount,
 * however many callers wire one up.
 */
import { onBeforeUnmount, onMounted } from "vue";

export function useWindowDismiss(
  onPointerDown: (event: PointerEvent) => void,
  onKeydown: (event: KeyboardEvent) => void,
): void {
  onMounted(() => {
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeydown);
  });
  onBeforeUnmount(() => {
    window.removeEventListener("pointerdown", onPointerDown);
    window.removeEventListener("keydown", onKeydown);
  });
}
