/**
 * Roving-tabindex keyboard behavior for a `role="tablist"` (Task 19's
 * `InspectorPanel.vue`; Task 33 fix round 1's `LibraryPanel.vue`):
 * ArrowLeft/ArrowRight move focus AND selection by one tab, Home/End jump
 * to the first/last. Extracted here once `check:quality`'s clone-group
 * gate caught the two components carrying byte-identical copies of this
 * same handler.
 *
 * Parameterized over the CALLER's own notion of "how many tabs" and "which
 * one is active" rather than owning a tab list itself — `InspectorPanel`
 * keys its active tab off a persisted `editorWorkspace` field,
 * `LibraryPanel` off a local, unpersisted `ref`, and this composable does
 * not need to know which.
 */
import { nextTick, ref } from "vue";

export interface RovingTablist {
  /** Bind via `:ref="(el) => setTabRef(i, el)"` on the i-th tab button, so
   * a keyboard move can focus the element it just selected. */
  setTabRef: (i: number, el: Element | null) => void;
  /** Bind via `@keydown` on the `role="tablist"` container. */
  onKeydown: (event: KeyboardEvent) => Promise<void>;
}

/**
 * `count()` is the number of tabs right now, `currentIndex()` the active
 * one's index, `select(i)` activates the i-th. All three are functions
 * (not plain values) so this composable always reads the caller's CURRENT
 * state rather than a snapshot taken at setup time.
 */
export function useRovingTablist(
  count: () => number,
  currentIndex: () => number,
  select: (i: number) => void,
): RovingTablist {
  const tabEls = ref<(HTMLElement | null)[]>([]);

  function setTabRef(i: number, el: Element | null): void {
    tabEls.value[i] = el as HTMLElement | null;
  }

  async function onKeydown(event: KeyboardEvent): Promise<void> {
    const n = count();
    const current = currentIndex();
    let target: number;
    if (event.key === "ArrowRight") target = (current + 1) % n;
    else if (event.key === "ArrowLeft") target = (current - 1 + n) % n;
    else if (event.key === "Home") target = 0;
    else if (event.key === "End") target = n - 1;
    else return;
    event.preventDefault();
    select(target);
    await nextTick();
    tabEls.value[target]?.focus();
  }

  return { setTabRef, onKeydown };
}
