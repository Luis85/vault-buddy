import { onScopeDispose, type Ref, ref } from "vue";

// Pointer + keyboard reordering over the rendered task rows. Pointer-based
// by necessity: Tauri's drag-drop interception (the buddy's document-drop
// feature) breaks HTML5 drag-and-drop inside the webviews. The composable
// owns only the interaction state machine — the caller supplies the
// section's row elements (visual order) and commits the final slot.

export interface DragState {
  sectionKey: string;
  fromIndex: number;
  toIndex: number;
  // The section the pointer is currently over — the origin when a drag starts,
  // another section's key when the pointer moves onto it. A release over a
  // DIFFERENT section is a cross-section move rather than a within-section
  // reorder (Task 11: drag a task onto another list).
  overSectionKey: string | null;
}

export function useTaskReorder(opts: {
  enabled: () => boolean;
  rowsFor: (sectionKey: string) => HTMLElement[];
  // Which section (by key) sits under the pointer, or null when over none.
  sectionAt: (clientX: number, clientY: number) => string | null;
  commit: (
    sectionKey: string,
    fromIndex: number,
    toIndex: number,
    overSectionKey: string | null,
  ) => void | Promise<void>;
}): {
  dragState: Ref<DragState | null>;
  onHandlePointerDown: (e: PointerEvent, sectionKey: string, index: number) => void;
  onHandleKeydown: (e: KeyboardEvent, sectionKey: string, index: number) => void;
} {
  const dragState = ref<DragState | null>(null);

  // The active drag's cancel hook. Its pointer listeners live on `window`, so
  // a view unmounted mid-drag (a panel view switch) would leave them armed and
  // the eventual pointerup would commit a reorder computed against the dead
  // view's rows — tear the drag down with the owning scope instead.
  let cancelActiveDrag: (() => void) | null = null;
  onScopeDispose(() => cancelActiveDrag?.());

  // The slot the dragged row would occupy in the FINAL order: how many OTHER
  // rows' midpoints sit above the pointer.
  function slotFor(sectionKey: string, fromIndex: number, y: number): number {
    let pos = 0;
    opts.rowsFor(sectionKey).forEach((el, i) => {
      if (i === fromIndex) return;
      const r = el.getBoundingClientRect();
      if (y > r.top + r.height / 2) pos += 1;
    });
    return pos;
  }

  function onHandlePointerDown(e: PointerEvent, sectionKey: string, index: number) {
    if (!opts.enabled() || dragState.value !== null) return;
    // Mouse drags start from the primary button only; touch/pen report 0.
    if (e.pointerType === "mouse" && e.button !== 0) return;
    e.preventDefault();
    (e.target as HTMLElement | null)?.setPointerCapture?.(e.pointerId);
    dragState.value = { sectionKey, fromIndex: index, toIndex: index, overSectionKey: sectionKey };

    const onMove = (ev: PointerEvent) => {
      if (!dragState.value) return;
      // Keep the last known section when the pointer is momentarily between
      // sections (over none) so a brief gap doesn't drop the target.
      const over = opts.sectionAt(ev.clientX, ev.clientY);
      dragState.value = {
        ...dragState.value,
        toIndex: slotFor(sectionKey, index, ev.clientY),
        overSectionKey: over ?? dragState.value.overSectionKey,
      };
    };
    const finish = (commitIt: boolean) => {
      cancelActiveDrag = null;
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("keydown", onKey, true);
      const st = dragState.value;
      dragState.value = null;
      // Commit a within-section reorder (slot changed) OR a cross-section move
      // (released over a different section, where the slot may be unchanged).
      if (commitIt && st && (st.toIndex !== st.fromIndex || st.overSectionKey !== st.sectionKey)) {
        void opts.commit(st.sectionKey, st.fromIndex, st.toIndex, st.overSectionKey);
      }
    };
    const onUp = () => finish(true);
    const onCancel = () => finish(false);
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") {
        // The drag consumes its own Escape — it must not bubble to the
        // window handler that closes the whole panel (GAP-27 class).
        ev.stopPropagation();
        finish(false);
      }
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("keydown", onKey, true);
    cancelActiveDrag = () => finish(false);
  }

  // The accessible complement: the focused handle moves its row one slot per
  // Arrow press, through the exact same commit (and rank math).
  function onHandleKeydown(e: KeyboardEvent, sectionKey: string, index: number) {
    if (!opts.enabled()) return;
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    e.stopPropagation();
    const to = e.key === "ArrowUp" ? index - 1 : index + 1;
    if (to < 0 || to >= opts.rowsFor(sectionKey).length) return;
    // Keyboard only reorders within the section — the over-section is the origin.
    void opts.commit(sectionKey, index, to, sectionKey);
  }

  return { dragState, onHandlePointerDown, onHandleKeydown };
}
