/**
 * The timeline's snap guide (visual-parity Task 18, concept spec §6.5): a
 * dashed gold line from the ruler down, at the snap target a dragged clip
 * edge sits on. `TimelineView` draws it — it alone spans every lane — and
 * `provide`s this ref; the one `ClipItem` being dragged writes its
 * `useTimelineDrag.snapGuideMs` into it (`useSnapGuideReport`, an
 * `inject`, so a clip mounted with no timeline around it writes nowhere).
 *
 * `useTimelineReport` is that report, for any value a dragged thing shows
 * on the whole timeline — the footer's edit hint (`dragHint.ts`,
 * visual-parity Task 20) rides it too.
 */
import type { InjectionKey, Ref } from "vue";
import { inject, onBeforeUnmount, watch } from "vue";

export const SNAP_GUIDE_KEY: InjectionKey<Ref<number | null>> = Symbol("timeline snap guide");

/** Writes `value` into the ref `TimelineView` provides under `key` while
 * it changes, and clears it if the reporting component unmounts while it
 * still holds a value (a drag cut short by a re-render). Call from setup. */
export function useTimelineReport<T>(key: InjectionKey<Ref<T | null>>, value: Readonly<Ref<T | null>>): void {
  const target = inject(key, null);
  if (!target) return;
  watch(value, (v) => (target.value = v));
  onBeforeUnmount(() => {
    if (value.value !== null) target.value = null;
  });
}

/** Reports a dragged edge's snap target (`snapGuideMs`, a drag
 * composable's own) to the timeline's guide — a clip (`ClipItem`) and,
 * since visual-parity Task 19, a teaching cue (`CueChip`). */
export function useSnapGuideReport(snapGuideMs: Ref<number | null>): void {
  useTimelineReport(SNAP_GUIDE_KEY, snapGuideMs);
}
