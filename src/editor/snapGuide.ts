/**
 * The timeline's snap guide (visual-parity Task 18, concept spec §6.5): a
 * dashed gold line from the ruler down, at the snap target a dragged clip
 * edge sits on. `TimelineView` draws it — it alone spans every lane — and
 * `provide`s this ref; the one `ClipItem` being dragged writes its
 * `useTimelineDrag.snapGuideMs` into it (`inject`, so a clip mounted with
 * no timeline around it writes nowhere).
 */
import type { InjectionKey, Ref } from "vue";

export const SNAP_GUIDE_KEY: InjectionKey<Ref<number | null>> = Symbol("timeline snap guide");
