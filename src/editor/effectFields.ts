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
 * and text at most `MAX_EFFECT_TEXT_CHARS` (1000). Positions are typed as
 * percentages, the easing in seconds; size, line weight, dim and
 * magnification are sliders (visual-parity Task 15, concept spec §5
 * "Teaching cue"), each field placed in its concept section.
 *
 * A cue's times are SOURCE time; the inspector shows them in seconds of
 * OUTPUT time from the clip's start ("Timing within this clip") and
 * `timeMap.sourceAtClamped` converts back on commit, at the clip's current
 * speed.
 */
import type { EditorIconName } from "../components/editor/icons/conceptIcons";
import type { EffectKind } from "../editorTypes";

/** `core::editor::limits::MAX_EFFECT_TEXT_CHARS`. */
export const MAX_EFFECT_TEXT_CHARS = 1_000;

/** F-33's permanent limitation notice for a privacy cover. */
export const MASK_WARNING =
  "Covers pixels only while visible. It does not track motion and the original recording is unchanged.";

type EffectFieldKey =
  | "x" | "y" | "w" | "h" | "x2" | "y2" | "dim"
  | "stroke" | "fontSize" | "factor" | "easing" | "number"
  | "text" | "color" | "background";

/** Where a field sits in the cue inspector (concept spec §5 "Teaching
 * cue"), in `CUE_SECTIONS` order. */
export type CueSection = "instruction" | "focus" | "appearance" | "color" | "position";

interface FieldBase {
  key: EffectFieldKey;
  label: string;
  section: CueSection;
}

/** `percent`: typed as a percentage of a stored fraction; `number`: typed
 * as itself; `seconds`: typed in seconds, stored as whole ms; `range`: a
 * slider showing the value times `scale` with its `suffix`, moving `step`
 * shown units at a time. */
export type EffectFieldSpec =
  | (FieldBase & { kind: "percent"; min: number; max: number })
  | (FieldBase & { kind: "number"; min: number; max: number; integer: boolean })
  | (FieldBase & { kind: "seconds"; maxMs: number })
  | (FieldBase & { kind: "range"; min: number; max: number; step: number; scale: number; suffix: string })
  | (FieldBase & { kind: "text" | "color" | "toggle" });

/** The cue inspector's sections, in order, with the concept's titles. The
 * timing section (`CueTiming`) sits between Color and Position. */
export const CUE_SECTIONS: readonly { id: CueSection; title: string }[] = [
  { id: "instruction", title: "Instruction" },
  { id: "focus", title: "Focus" },
  { id: "appearance", title: "Appearance" },
  { id: "color", title: "Color" },
  { id: "position", title: "Position · % of this video" },
];

const X: EffectFieldSpec = { key: "x", label: "Horizontal", section: "position", kind: "percent", min: 0, max: 1 };
const Y: EffectFieldSpec = { key: "y", label: "Vertical", section: "position", kind: "percent", min: 0, max: 1 };
const W: EffectFieldSpec = { key: "w", label: "Width", section: "position", kind: "percent", min: 0.01, max: 1 };
const H: EffectFieldSpec = { key: "h", label: "Height", section: "position", kind: "percent", min: 0.01, max: 1 };
const COLOR: EffectFieldSpec = { key: "color", label: "Color", section: "color", kind: "color" };
const STROKE: EffectFieldSpec = {
  key: "stroke", label: "Line weight", section: "appearance", kind: "range", min: 1, max: 20, step: 1, scale: 1, suffix: " px",
};
const TEXT: EffectFieldSpec = { key: "text", label: "Text", section: "instruction", kind: "text" };

const FIELDS: Record<EffectKind, EffectFieldSpec[]> = {
  text: [
    X, Y, W, H, TEXT,
    { key: "fontSize", label: "Text size", section: "instruction", kind: "range", min: 12, max: 100, step: 1, scale: 1, suffix: " px" },
    COLOR,
    { key: "background", label: "Readable text background", section: "instruction", kind: "toggle" },
  ],
  arrow: [
    X, Y,
    { key: "x2", label: "Arrow tip X", section: "position", kind: "percent", min: 0, max: 1 },
    { key: "y2", label: "Arrow tip Y", section: "position", kind: "percent", min: 0, max: 1 },
    STROKE, COLOR,
  ],
  highlight: [X, Y, W, H, STROKE, COLOR],
  spotlight: [
    X, Y, W, H,
    { key: "dim", label: "Background dim", section: "appearance", kind: "range", min: 0, max: 1, step: 5, scale: 100, suffix: "%" },
  ],
  zoom: [
    X, Y,
    { key: "factor", label: "Magnification", section: "focus", kind: "range", min: 1, max: 4, step: 0.05, scale: 1, suffix: "×" },
    { key: "easing", label: "Ease in / out (s)", section: "focus", kind: "seconds", maxMs: 10_000 },
  ],
  step: [
    X, Y,
    { key: "number", label: "Step number", section: "instruction", kind: "number", min: 1, max: 99, integer: true },
    TEXT, COLOR,
  ],
  mask: [X, Y, W, H, COLOR],
};

export function fieldsFor(kind: EffectKind): EffectFieldSpec[] {
  return FIELDS[kind];
}

/** Each kind's glyph (the concept's `effectIcons`; a privacy cover is the
 * shield its tool shows). */
export const EFFECT_ICONS: Record<EffectKind, EditorIconName> = {
  text: "text",
  arrow: "arrowUpRight",
  highlight: "box",
  spotlight: "spotlight",
  zoom: "zoomIn",
  step: "step",
  mask: "shield",
};

export const EFFECT_NAMES: Record<EffectKind, string> = {
  text: "Text",
  arrow: "Arrow",
  highlight: "Highlight",
  spotlight: "Spotlight",
  zoom: "Zoom",
  step: "Numbered step",
  mask: "Privacy cover",
};
