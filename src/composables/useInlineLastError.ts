/**
 * A surface that shows `editorProject.lastError` INLINE (ruling T7-1,
 * visual-parity Task 21). `useEditorFeedback` toasts every new `lastError`;
 * a dialog that also printed it beside its own control showed the same
 * refusal twice. While `active()` is true this claims every `lastError`
 * raised: the surface renders it (`error`) and the feedback watcher does
 * not toast it — one visible surface, never zero, because the claim holds
 * exactly as long as the surface that renders what it claimed.
 *
 * General on purpose: the Checks dialog's destination picker and the Save
 * a copy dialog use it, and so can any later dialog (the webcam dialog, the
 * close guard, recovery) without the feedback watcher knowing its name.
 *
 * The claim is a module-level count, not per error: `lastError` is set
 * inside the store BEFORE the awaiting caller resumes, and the feedback
 * watcher runs in between, so a claim made after the fact would lose that
 * race. An error raised while nothing claims is toasted as before; an error
 * already on screen when a surface activates is not re-shown by it.
 */
import { onScopeDispose, ref, shallowRef, watch } from "vue";

import type { EditorError } from "../editorTypes";
import { useEditorProjectStore } from "../stores/editorProject";

const claims = ref(0);

/** Whether a surface is on screen that renders `lastError` itself. */
export function lastErrorShownInline(): boolean {
  return claims.value > 0;
}

export function useInlineLastError(active: () => boolean) {
  const project = useEditorProjectStore();
  const error = shallowRef<EditorError | null>(null);
  let claimed = false;

  function setClaim(on: boolean): void {
    if (on === claimed) return;
    claimed = on;
    claims.value += on ? 1 : -1;
  }

  watch(
    active,
    (on) => {
      setClaim(on);
      error.value = null;
    },
    { immediate: true },
  );
  // Sync, so the error is on this surface in the same tick the store set
  // it — before the feedback watcher could ever look for a surface.
  watch(
    () => project.lastError,
    (next) => {
      if (claimed && next) error.value = next;
    },
    { flush: "sync" },
  );
  onScopeDispose(() => setClaim(false));

  /** Forget the shown error (a retry is in flight). */
  function clear(): void {
    error.value = null;
  }

  return { error, clear };
}
