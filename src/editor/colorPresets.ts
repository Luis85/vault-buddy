/**
 * Colour treatment presets and the CSS filter they map to (Task 32; F-39;
 * DATA-MODEL.md § Layout and compositing properties: "brightness/contrast/
 * saturation/sepia/grayscale adjustments"). `ColorSection.vue`'s preset
 * buttons read `COLOR_PRESETS`; `previewLayers.ts` reads `adjustmentsFilter`
 * to compute each visual layer's CSS `filter:` string, which
 * `previewController.ts` then assigns to the media element's style — one
 * formula, so a preset and a hand-tuned slider render through the exact
 * same mapping.
 *
 * Every preset besides `none` sets EVERY `Adjustments` field explicitly —
 * `core::editor::model::Adjustments`'s own doc: the object requires all
 * five once it is present at all — so applying a preset can never leave a
 * stale value from whatever was set before. `none` is the one preset that
 * is not itself an `Adjustments` object: it CLEARS a clip's adjustments
 * (`adjustments: null` on the wire), matching `core::editor::commands::
 * layout::set_adjustments`'s own "the whole object is nullable, never
 * merged field by field" rule.
 */
import type { Adjustments } from "../editorTypes";

export type ColorPresetId = "none" | "vivid" | "warm" | "cool" | "mono" | "sepia";

/** The five fields every non-`none` preset starts from and overrides only
 * what it names — never a partial object `core::editor::commands::layout::
 * validate_adjustments` (Rust) would reject as missing a field. */
const DEFAULT: Adjustments = { brightness: 1, contrast: 1, saturation: 1, sepia: 0, grayscale: 0 };

export interface ColorPreset {
  id: ColorPresetId;
  label: string;
  /** `null` only for `none` — see the module doc. */
  adjustments: Adjustments | null;
}

/**
 * The brief's own values, verbatim. `cool` is deliberately SATURATION-ONLY
 * in v1: a fuller "cool" grade would also lift brightness slightly and
 * shift hue toward blue, but `Adjustments` carries no hue field and CSS
 * `filter:` has no directional "cool cast" primitive short of a full
 * `hue-rotate` (which rotates every colour around the wheel, not a
 * blue-leaning tint) — so v1 cools by desaturating alone, and brightness
 * stays at the default rather than the `1.02` a fuller treatment might use.
 */
export const COLOR_PRESETS: readonly ColorPreset[] = [
  { id: "none", label: "None", adjustments: null },
  { id: "vivid", label: "Vivid", adjustments: { ...DEFAULT, saturation: 1.3, contrast: 1.1 } },
  { id: "warm", label: "Warm", adjustments: { ...DEFAULT, sepia: 0.2, saturation: 1.1 } },
  { id: "cool", label: "Cool", adjustments: { ...DEFAULT, saturation: 0.9 } },
  { id: "mono", label: "Black & white", adjustments: { ...DEFAULT, grayscale: 1 } },
  { id: "sepia", label: "Sepia", adjustments: { ...DEFAULT, sepia: 0.8 } },
];

/** The preset with `id`, or throws — every caller passes a `ColorPresetId`
 * the type system already limits to the six declared above, so a miss here
 * can only mean `COLOR_PRESETS` itself drifted from `ColorPresetId`. */
export function findColorPreset(id: ColorPresetId): ColorPreset {
  const preset = COLOR_PRESETS.find((p) => p.id === id);
  if (!preset) throw new Error(`unknown color preset: ${id}`);
  return preset;
}

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
 * `FILTERS`), offered by the clip context menu's "Color treatment". Kept
 * beside `COLOR_PRESETS` so the inspector's Color tab can adopt the same
 * five when it takes the concept's layout. */
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
  return (Object.keys(DEFAULT) as (keyof Adjustments)[]).every((k) => current?.[k] === treatment?.[k]);
}
