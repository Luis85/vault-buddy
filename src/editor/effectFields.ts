/**
 * Which inspector fields each teaching-cue kind has (Task 35; F-27–F-33;
 * SCREENS-AND-INTERACTIONS.md: "Arrows expose endpoints; text and numbered
 * steps expose copy/style; highlights and spotlights expose bounds; zoom
 * exposes focal point/factor/ramp; privacy cover is clearly stationary").
 * `EffectSection.vue` renders whatever this table lists, in this order.
 *
 * Every per-kind field is exactly what `cues.rs`'s `patch_props` accepts
 * for that kind (`payloads.rs`'s seven `*EffectProps`) — a spotlight and a
 * zoom carry no colour on the wire, so they get no colour field here — and
 * every bound is `validate.rs`'s `check_effect` bound, read from the Rust
 * source (the `useInspectorDraft.ts` rule): positions `[0, 1]`, a box's
 * `w`/`h` `[0.01, 1]`, `stroke` `[1, 20]`, `fontSize` `[12, 100]`, `dim`
 * `[0, 1]`, `factor` `[1, 4]`, `easing` `[0, 10000]`, `number` `[1, 99]`,
 * and text at most `MAX_EFFECT_TEXT_CHARS` (1000). Fractions are shown and
 * typed as percentages.
 *
 * A cue's times are SOURCE time; the inspector shows them in OUTPUT time
 * (where the user sees them on the timeline) and `timeMap.sourceAtClamped`
 * converts back on commit, at the clip's current speed.
 */
import type { EffectKind } from "../editorTypes";

/** `core::editor::limits::MAX_EFFECT_TEXT_CHARS`. */
export const MAX_EFFECT_TEXT_CHARS = 1_000;

/** F-33's permanent limitation notice for a privacy cover. */
export const MASK_WARNING =
  "Covers pixels only while visible. It does not track motion and the original recording is unchanged.";

export type EffectFieldKey =
  | "x" | "y" | "w" | "h" | "x2" | "y2" | "dim"
  | "stroke" | "fontSize" | "factor" | "easing" | "number"
  | "text" | "color" | "background";

export type EffectFieldSpec =
  | { key: EffectFieldKey; label: string; kind: "percent"; min: number; max: number }
  | { key: EffectFieldKey; label: string; kind: "number"; min: number; max: number; integer: boolean }
  | { key: EffectFieldKey; label: string; kind: "text" | "color" | "toggle" };

const X: EffectFieldSpec = { key: "x", label: "X", kind: "percent", min: 0, max: 1 };
const Y: EffectFieldSpec = { key: "y", label: "Y", kind: "percent", min: 0, max: 1 };
const W: EffectFieldSpec = { key: "w", label: "Width", kind: "percent", min: 0.01, max: 1 };
const H: EffectFieldSpec = { key: "h", label: "Height", kind: "percent", min: 0.01, max: 1 };
const COLOR: EffectFieldSpec = { key: "color", label: "Colour", kind: "color" };
const STROKE: EffectFieldSpec = { key: "stroke", label: "Stroke", kind: "number", min: 1, max: 20, integer: true };
const TEXT: EffectFieldSpec = { key: "text", label: "Text", kind: "text" };

const FIELDS: Record<EffectKind, EffectFieldSpec[]> = {
  text: [
    X, Y, W, H, TEXT,
    { key: "fontSize", label: "Size", kind: "number", min: 12, max: 100, integer: true },
    COLOR,
    { key: "background", label: "Background", kind: "toggle" },
  ],
  arrow: [
    X, Y,
    { key: "x2", label: "End X", kind: "percent", min: 0, max: 1 },
    { key: "y2", label: "End Y", kind: "percent", min: 0, max: 1 },
    STROKE, COLOR,
  ],
  highlight: [X, Y, W, H, STROKE, COLOR],
  spotlight: [X, Y, W, H, { key: "dim", label: "Dim", kind: "percent", min: 0, max: 1 }],
  zoom: [
    { ...X, label: "Focus X" },
    { ...Y, label: "Focus Y" },
    { key: "factor", label: "Zoom", kind: "number", min: 1, max: 4, integer: false },
    { key: "easing", label: "Ease (ms)", kind: "number", min: 0, max: 10_000, integer: true },
  ],
  step: [X, Y, { key: "number", label: "Number", kind: "number", min: 1, max: 99, integer: true }, TEXT, COLOR],
  mask: [X, Y, W, H, COLOR],
};

export function fieldsFor(kind: EffectKind): EffectFieldSpec[] {
  return FIELDS[kind];
}

export const EFFECT_NAMES: Record<EffectKind, string> = {
  text: "Text",
  arrow: "Arrow",
  highlight: "Highlight",
  spotlight: "Spotlight",
  zoom: "Zoom",
  step: "Numbered step",
  mask: "Privacy cover",
};
