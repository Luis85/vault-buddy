/**
 * The tutorial editor's TS declaration site (F10; the `screenTypes.ts`
 * precedent for a domain-scoped split of `types.ts`).
 *
 * Created here with only the entity type `core::editor::time` /
 * `src/editor/timeMap.ts` need (`ClipSpan`); Task 8 extends it with the
 * entity/clipboard shapes `src/editor/fragment.ts` needs (`Clip`/
 * `Effect`/`CaptionCue`/`Marker`/`ClipboardFragment`, plus the minimal
 * `Project` slice `buildFragment` reads). Later tasks keep growing this
 * file rather than forking a second declaration site or growing
 * `types.ts`.
 *
 * Field spelling: the IPC ENVELOPE wraps everything in camelCase
 * (`sessionId`, `projectId`, …), but the `project` graph it carries keeps
 * the interchange document's OWN spelling, which is snake_case (R3,
 * `core::editor::model`'s module doc: "Field names are the interchange
 * document's own snake_case spelling"). `ClipSpan` below describes a
 * fragment of that graph, so it is snake_case too — not a TypeScript
 * convention violation, but the one place it would be wrong to camelCase.
 * The entity interfaces below (`Clip`/`Effect`/`CaptionCue`/`Marker`) are
 * document spelling for the same reason, mirroring
 * `core::editor::model`/`core::editor::model_cues` field-for-field —
 * `Effect.fontSize` is the ONE field that stays camelCase even there,
 * because the Rust struct itself renames it
 * (`#[serde(rename = "fontSize")]`, pinned by
 * `font_size_keeps_its_camel_case_spelling`).
 */

/** A clip's time-mapping span: output start, half-open source range
 * `[in_ms, out_ms)`, and speed. Mirrors `core::editor::time::ClipSpan`
 * field-for-field so a `Project`'s clip entity, read straight off the
 * wire, can be passed to `timeMap.ts` without any renaming. */
export interface ClipSpan {
  start_ms: number;
  in_ms: number;
  out_ms: number;
  speed: number;
}

/** `core::editor::model::FadeCurve` (kebab-case on the wire). */
export type FadeCurve = "linear" | "smooth" | "equal-power";

/** A clip's quarter-turn rotation in plain degrees (`core::editor::model::Rotation`). */
export type Rotation = 0 | 90 | 180 | 270;

export type FrameShape = "circle" | "rounded" | "rectangle";

export type Fit = "cover" | "contain";

/** Brightness/contrast/saturation/sepia/grayscale
 * (`core::editor::model::Adjustments`) — every field is required once the
 * object is present at all. */
export interface Adjustments {
  brightness: number;
  contrast: number;
  saturation: number;
  sepia: number;
  grayscale: number;
}

export type CardPreset = "intro" | "chapter" | "outro" | "blank";

/** A titles/chapter card's editable content (`core::editor::model::Card`). */
export interface Card {
  preset: CardPreset;
  title: string;
  subtitle: string;
  background: string;
  foreground: string;
  accent: string;
}

/** An instance of a source range at an output time
 * (`core::editor::model::Clip`). Document spelling (snake_case) — see the
 * module doc above. */
export interface Clip {
  id: string;
  asset_id: string;
  track_id: string;
  name: string;
  start_ms: number;
  in_ms: number;
  out_ms: number;
  fade_in_ms: number;
  fade_out_ms: number;
  fade_curve: FadeCurve;
  opacity: number;
  volume: number;
  muted: boolean;
  x: number;
  y: number;
  w: number;
  h: number;
  speed?: number;
  rotation?: Rotation;
  frame_shape?: FrameShape;
  fit?: Fit;
  mirror?: boolean;
  flip_y?: boolean;
  preserve_pitch?: boolean;
  group_id?: string;
  crop_zoom?: number;
  crop_x?: number;
  crop_y?: number;
  adjustments?: Adjustments;
  card?: Card;
}

export type EffectKind = "text" | "arrow" | "highlight" | "spotlight" | "zoom" | "step" | "mask";

/** A clip-linked teaching cue (`core::editor::model_cues::Effect`).
 * `start_ms`/`end_ms` are SOURCE time, like every clip-linked cue. */
export interface Effect {
  id: string;
  clip_id: string;
  kind: EffectKind;
  start_ms: number;
  end_ms: number;
  x: number;
  y: number;
  color: string;
  text?: string;
  w?: number;
  h?: number;
  x2?: number;
  y2?: number;
  factor?: number;
  fontSize?: number;
  stroke?: number;
  dim?: number;
  easing?: number;
  number?: number;
  background?: boolean;
}

export type CaptionPosition = "top" | "bottom";

/** A caption cue (`core::editor::model_cues::CaptionCue`) — clip-linked
 * SOURCE time, like `Effect`. */
export interface CaptionCue {
  id: string;
  clip_id: string;
  start_ms: number;
  end_ms: number;
  text: string;
}

/** Project-level caption presentation plus the source-linked cue records
 * (`core::editor::model_cues::CaptionSettings`). */
export interface CaptionSettings {
  enabled: boolean;
  burn_in: boolean;
  font_size: number;
  position: CaptionPosition;
  background: boolean;
  cues: CaptionCue[];
}

/** A chapter marker (`core::editor::model_cues::Marker`) — clip-linked
 * SOURCE time, like `Effect`/`CaptionCue`. */
export interface Marker {
  id: string;
  clip_id: string;
  source_ms: number;
  title: string;
}

/**
 * The slice of `core::editor::model::Project` this task's TS code reads —
 * grows as later tasks need more of the project graph (the single-TS-
 * declaration-site rule: extend this interface rather than forking a
 * second, narrower one).
 */
export interface Project {
  clips: Clip[];
  effects: Effect[];
  markers: Marker[];
  captions: CaptionSettings | null;
}

/**
 * `pasteFragment`'s clipboard payload (Task 8, F9): the IPC ENVELOPE's own
 * fields are camelCase (`origin_ms` → `originMs`, per
 * `commands::payloads::ClipboardFragment`'s own
 * `#[serde(rename_all = "camelCase")]`), but the entities it carries
 * (`Clip`/`Effect`/`CaptionCue`/`Marker`) stay in document spelling, same
 * as everywhere else in the project graph.
 */
export interface ClipboardFragment {
  clips: Clip[];
  effects: Effect[];
  captions: CaptionCue[];
  markers: Marker[];
  originMs: number;
}
