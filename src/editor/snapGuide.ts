/**
 * The timeline's snap guide (visual-parity Task 18, concept spec §6.5): a
 * dashed gold line from the ruler down, at the snap target a dragged clip
 * edge sits on. `TimelineView` draws it — it alone spans every lane — and
 * `provide`s this ref; the one `ClipItem` being dragged writes its
 * `useTimelineDrag.snapGuideMs` into it (`useSnapGuideReport`, an
 * `inject`, so a clip mounted with no timeline around it writes nowhere).
 */
import type { InjectionKey, Ref } from "vue";
import { inject, onBeforeUnmount, watch } from "vue";

export const SNAP_GUIDE_KEY: InjectionKey<Ref<number | null>> = Symbol("timeline snap guide");

/** Reports a dragged edge's snap target (`snapGuideMs`, a drag
 * composable's own) to the timeline's guide, and clears it if the dragged
 * thing unmounts mid-drag — a clip (`ClipItem`) and, since visual-parity
 * Task 19, a teaching cue (`CueChip`). Call from setup. */
export function useSnapGuideReport(snapGuideMs: Ref<number | null>): void {
  const guide = inject(SNAP_GUIDE_KEY, null);
  if (!guide) return;
  watch(snapGuideMs, (ms) => (guide.value = ms));
  onBeforeUnmount(() => {
    if (snapGuideMs.value !== null) guide.value = null;
  });
}
