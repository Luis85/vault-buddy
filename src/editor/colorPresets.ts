/**
 * Colour treatments and the CSS filter they map to (Task 32; F-39;
 * DATA-MODEL.md § Layout and compositing properties: "brightness/contrast/
 * saturation/sepia/grayscale adjustments"). The concept's five treatments
 * (`COLOR_TREATMENTS`) are the one preset set — the clip context menus'
 * "Color treatment", the multi inspector's Shared color and the Color tab
 * (ruling T5-3); `previewLayers.ts` reads `adjustmentsFilter` to compute
 * each visual layer's CSS `filter:` string, which `previewController.ts`
 * then assigns to the media element's style — one formula, so a treatment
 * and a hand-tuned slider render through the exact same mapping.
 *
 * Every treatment besides Original sets EVERY `Adjustments` field
 * explicitly — `core::editor::model::Adjustments`'s own doc: the object
 * requires all five once it is present at all — so applying one can never
 * leave a stale value from whatever was set before. Original is not itself
 * an `Adjustments` object: it CLEARS a clip's adjustments (`adjustments:
 * null` on the wire), matching `core::editor::commands::layout::
 * set_adjustments`'s own "the whole object is nullable, never merged field
 * by field" rule.
 */
import type { Adjustments } from "../editorTypes";

/** What a clip with no adjustments renders as — the Color tab's sliders
 * start here. */
export const ORIGINAL_ADJUSTMENTS: Adjustments = { brightness: 1, contrast: 1, saturation: 1, sepia: 0, grayscale: 0 };

/**
 * The CSS `filter:` string for `adjustments` (brief: "Preview maps to CSS
 * filter: (brightness/contrast/saturate/sepia/grayscale)" — that exact
 * function order). `null`/`undefined` (no colour set, or a layer colour
 * cannot apply to at all — a card, an audio layer) renders as `"none"`,
 * CSS's own identity filter, so every layer can assign this string
 * unconditionally with no separate branch for "nothing to apply".
 */
export function adjustmentsFilter(adjustments: Adjustments | null | undefined): string {
  if (!adjustments) return "none";
  const { brightness, contrast, saturation, sepia, grayscale } = adjustments;
  return (
    `brightness(${brightness}) contrast(${contrast}) saturate(${saturation}) ` +
    `sepia(${sepia}) grayscale(${grayscale})`
  );
}

/** The concept's colour treatments (`reference/editing-features.js`'
 * `FILTERS`): Original, Clear, Warm, Soft and Mono. */
export const COLOR_TREATMENTS: readonly { id: string; label: string; adjustments: Adjustments | null }[] = [
  { id: "original", label: "Original", adjustments: null },
  { id: "clear", label: "Clear", adjustments: { brightness: 1.07, contrast: 1.08, saturation: 0.94, sepia: 0, grayscale: 0 } },
  { id: "warm", label: "Warm", adjustments: { brightness: 1.02, contrast: 1.04, saturation: 1.08, sepia: 0.18, grayscale: 0 } },
  { id: "soft", label: "Soft", adjustments: { brightness: 1.06, contrast: 0.9, saturation: 0.8, sepia: 0, grayscale: 0 } },
  { id: "mono", label: "Mono", adjustments: { brightness: 1, contrast: 1.12, saturation: 1, sepia: 0, grayscale: 1 } },
];

/** Whether a clip's adjustments are exactly `treatment`. A clip with none
 * reads as Original (`null`): every field is absent on both sides. */
export function isTreatment(current: Adjustments | null | undefined, treatment: Adjustments | null): boolean {
  return (Object.keys(ORIGINAL_ADJUSTMENTS) as (keyof Adjustments)[]).every((k) => current?.[k] === treatment?.[k]);
}
