/**
 * A surface that shows its OWN request's refusal inline (ruling T7-1,
 * visual-parity Task 21). `useEditorFeedback` toasts every new
 * `lastError`; a dialog that also printed it beside its own control showed
 * the same refusal twice. `track(request)` runs the surface's request and,
 * for exactly as long as it is in flight, claims every `lastError` raised:
 * the surface renders it (`error`) and the feedback watcher does not toast
 * it — one visible surface, never zero.
 *
 * Only the surface's own request is claimed (Task 21 fix round 1): an
 * error from anything else — a background job's refresh, an edit made
 * elsewhere — raised while the dialog sits open but idle still toasts, and
 * a closed dialog that stays mounted claims nothing. A captured error stays
 * on the surface until its next request, or its next activation (`active`
 * turning true, when given).
 *
 * General on purpose: the Checks dialog's destination picker and the Save
 * a copy dialog use it, and so can any later dialog without the feedback
 * watcher knowing its name.
 *
 * The claim is a module-level count, held from before the request is sent
 * until a tick after it settles: the store sets `lastError` BEFORE the
 * awaiting caller resumes, and the feedback watcher runs in between, so a
 * claim made after the fact would lose that race.
 */
import { nextTick, onScopeDispose, ref, shallowRef, watch } from "vue";

import type { EditorError } from "../editorTypes";
import { useEditorProjectStore } from "../stores/editorProject";

const claims = ref(0);

/** Whether a surface's own request is in flight and renders `lastError`. */
export function lastErrorShownInline(): boolean {
  return claims.value > 0;
}

export function useInlineLastError(active?: () => boolean) {
  const project = useEditorProjectStore();
  const error = shallowRef<EditorError | null>(null);
  /** This surface's requests still holding a claim. */
  let held = 0;

  function release(): void {
    if (held === 0) return;
    held -= 1;
    claims.value -= 1;
  }

  if (active) {
    watch(active, (on) => {
      if (on) error.value = null;
    });
  }
  // Sync, so the error is on this surface in the same tick the store set
  // it — before the feedback watcher could ever look for a surface.
  watch(
    () => project.lastError,
    (next) => {
      if (held > 0 && next) error.value = next;
    },
    { flush: "sync" },
  );
  onScopeDispose(() => {
    while (held > 0) release();
  });

  /** Run this surface's own request, claiming its refusal. */
  async function track<T>(request: () => Promise<T>): Promise<T> {
    error.value = null;
    held += 1;
    claims.value += 1;
    try {
      return await request();
    } finally {
      // One tick more: the feedback watcher's pre-flush for an error set
      // at the very end of the request must still see the claim.
      await nextTick();
      release();
    }
  }

  return { error, track };
}
