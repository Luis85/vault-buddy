/**
 * `EditorCommand` (F31) — the ~50-variant wire command union, split out of
 * `editorTypes.ts` from the start so that file does not grow toward the
 * 500-line cap as later tasks add arms. `editorTypes.ts` re-exports
 * `EditorCommand` so every caller still imports from one place.
 *
 * Mirrors `core::editor::commands::{EditorCommand, payloads::*}` field for
 * field (Contract reference `Commands` paragraph, `global-constraints.md`).
 * `EditorCommand` is internally tagged on `kind` — every field of a
 * variant's own payload sits at the SAME JSON level as `kind`, exactly as
 * `#[serde(tag = "kind", rename_all = "camelCase")]` produces on the wire.
 *
 * Three commands (`addTrack`/`addTransition`/`addEffect`) carry their own
 * inner kind field — `trackKind`/`transitionKind`/`effectKind` — renamed on
 * the Rust side so it cannot collide with the enum's own `"kind"` tag at the
 * same JSON level (`payloads.rs`'s own doc on `AddTrackPayload`).
 *
 * `addEffect`'s `props` shape depends on the sibling `effectKind` (a
 * controller ruling, Task 6 fix round 1): modeled here as a DISCRIMINATED
 * union of seven variants, one per `effectKind`, each carrying only that
 * kind's own `props` fields — so sending `fontSize` (a `text`-only field)
 * inside an `arrow`'s `props` is a TypeScript error, not merely a runtime
 * one. `updateEffect.props` stays untyped JSON (Contract reference): an
 * update carries no sibling `effectKind`, so there is nothing to
 * discriminate on at this layer — `core::editor::commands::payloads::
 * UpdateEffectPayload`'s own doc says the same about the Rust side.
 *
 * Every entity/enum type these payloads reference (`TrackKind`,
 * `TransitionKind`, `FadeCurve`, `Fit`, `FrameShape`, `Rotation`,
 * `CardPreset`, `CaptionPosition`, `Adjustments`, `ClipboardFragment`) is
 * declared once in `editorTypes.ts` and imported here — never redeclared —
 * so the entity shapes stay single-sourced even though this file is a
 * physically separate module.
 */
import type {
  Adjustments,
  CaptionPosition,
  CardPreset,
  ClipboardFragment,
  FadeCurve,
  Fit,
  FrameShape,
  Rotation,
  TrackKind,
  TransitionKind,
} from "../editorTypes";

// ---- effect props (one shape per EffectKind) -----------------------------

/** `core::editor::commands::payloads::TextEffectProps`. */
export interface TextEffectProps {
  x?: number;
  y?: number;
  w?: number;
  h?: number;
  text?: string;
  fontSize?: number;
  color?: string;
  background?: boolean;
}

/** `core::editor::commands::payloads::ArrowEffectProps`. */
export interface ArrowEffectProps {
  x?: number;
  y?: number;
  x2?: number;
  y2?: number;
  color?: string;
  stroke?: number;
}

/** `core::editor::commands::payloads::HighlightEffectProps`. */
export interface HighlightEffectProps {
  x?: number;
  y?: number;
  w?: number;
  h?: number;
  color?: string;
  stroke?: number;
}

/** `core::editor::commands::payloads::SpotlightEffectProps`. */
export interface SpotlightEffectProps {
  x?: number;
  y?: number;
  w?: number;
  h?: number;
  dim?: number;
}

/** `core::editor::commands::payloads::ZoomEffectProps`. */
export interface ZoomEffectProps {
  x?: number;
  y?: number;
  factor?: number;
  easing?: number;
}

/** `core::editor::commands::payloads::StepEffectProps`. */
export interface StepEffectProps {
  x?: number;
  y?: number;
  number?: number;
  text?: string;
  color?: string;
}

/** `core::editor::commands::payloads::MaskEffectProps`. */
export interface MaskEffectProps {
  x?: number;
  y?: number;
  w?: number;
  h?: number;
  color?: string;
}

/**
 * `addEffect{clipId, effectKind, startMs, endMs, props}`, one variant per
 * `effectKind` so `props`'s shape is a compile-time consequence of the
 * sibling tag (controller ruling, Task 6 fix round 1). `startMs`/`endMs`
 * are SOURCE time, like every clip-linked cue command (Contract reference:
 * "Effect/caption startMs/endMs and marker sourceMs are SOURCE time").
 */
export type AddEffectCommand =
  | {
      kind: "addEffect";
      clipId: string;
      effectKind: "text";
      startMs: number;
      endMs: number;
      props: TextEffectProps;
    }
  | {
      kind: "addEffect";
      clipId: string;
      effectKind: "arrow";
      startMs: number;
      endMs: number;
      props: ArrowEffectProps;
    }
  | {
      kind: "addEffect";
      clipId: string;
      effectKind: "highlight";
      startMs: number;
      endMs: number;
      props: HighlightEffectProps;
    }
  | {
      kind: "addEffect";
      clipId: string;
      effectKind: "spotlight";
      startMs: number;
      endMs: number;
      props: SpotlightEffectProps;
    }
  | {
      kind: "addEffect";
      clipId: string;
      effectKind: "zoom";
      startMs: number;
      endMs: number;
      props: ZoomEffectProps;
    }
  | {
      kind: "addEffect";
      clipId: string;
      effectKind: "step";
      startMs: number;
      endMs: number;
      props: StepEffectProps;
    }
  | {
      kind: "addEffect";
      clipId: string;
      effectKind: "mask";
      startMs: number;
      endMs: number;
      props: MaskEffectProps;
    };

// ---- the command union ----------------------------------------------------

/**
 * Every `EditorCommand` wire kind (Contract reference `Commands`
 * paragraph). Field order mirrors `payloads.rs`'s own section comments
 * (rename/destination, clips, tracks, mix, transitions, speed/layout/
 * adjustments/canvas, cards, effects, captions, markers).
 */
export type EditorCommand =
  | { kind: "rename"; title: string }
  | { kind: "undo" }
  | { kind: "redo" }
  | { kind: "insertClip"; assetId: string; trackId: string; startMs: number; inMs: number; outMs: number }
  | { kind: "updateClip"; clipId: string; name: string }
  | { kind: "splitClip"; clipId: string; atMs: number }
  | { kind: "trimClip"; clipId: string; startMs: number; inMs: number; outMs: number }
  | { kind: "deleteClips"; clipIds: string[]; closeGap: boolean }
  | { kind: "moveClips"; clipIds: string[]; deltaMs: number; trackId: string | null }
  | { kind: "reorderClip"; clipId: string; direction: "earlier" | "later" }
  | { kind: "groupClips"; clipIds: string[] }
  | { kind: "ungroupClips"; groupId: string }
  | { kind: "duplicateClips"; clipIds: string[]; offsetMs: number }
  | { kind: "pasteFragment"; fragment: ClipboardFragment; trackId: string; atMs: number }
  | { kind: "cutClips"; clipIds: string[]; closeGap: boolean }
  | { kind: "addTrack"; trackKind: TrackKind; name: string; index: number }
  | { kind: "renameTrack"; trackId: string; name: string }
  | { kind: "moveTrack"; trackId: string; toIndex: number }
  | {
      kind: "setTrackFlags";
      trackId: string;
      visible?: boolean;
      locked?: boolean;
      muted?: boolean;
      solo?: boolean;
      volume?: number;
    }
  | { kind: "deleteTrack"; trackId: string }
  | { kind: "setClipMix"; clipIds: string[]; volume?: number; muted?: boolean }
  | { kind: "setMasterGain"; gain: number }
  | { kind: "detachAudio"; clipId: string; audioTrackId: string | null }
  | {
      kind: "setFades";
      clipId: string;
      fadeInMs?: number;
      fadeOutMs?: number;
      fadeCurve?: FadeCurve;
    }
  | {
      kind: "addTransition";
      fromClipId: string;
      toClipId: string;
      durationMs: number;
      transitionKind: TransitionKind;
    }
  | { kind: "setTransitionDuration"; transitionId: string; durationMs: number }
  | { kind: "removeTransition"; transitionId: string }
  | { kind: "setSpeed"; clipId: string; speed: number; preservePitch: boolean }
  | {
      kind: "setLayout";
      clipIds: string[];
      x?: number;
      y?: number;
      w?: number;
      h?: number;
      opacity?: number;
      fit?: Fit;
      frameShape?: FrameShape;
      rotation?: Rotation;
      mirror?: boolean;
      flipY?: boolean;
      cropZoom?: number;
      cropX?: number;
      cropY?: number;
    }
  | { kind: "setAdjustments"; clipIds: string[]; adjustments: Adjustments | null }
  | { kind: "setCanvas"; width: number; height: number }
  | {
      kind: "addCard";
      preset: CardPreset;
      trackId: string | null;
      startMs: number;
      durationMs: number;
      title: string;
      subtitle: string;
    }
  | {
      kind: "updateCard";
      clipId: string;
      title?: string;
      subtitle?: string;
      background?: string;
      foreground?: string;
      accent?: string;
    }
  | { kind: "insertIntro"; durationMs: number; title: string; subtitle: string }
  | AddEffectCommand
  | {
      kind: "updateEffect";
      effectId: string;
      startMs?: number;
      endMs?: number;
      props?: Record<string, unknown>;
    }
  | { kind: "removeEffect"; effectId: string }
  | {
      kind: "setCaptionSettings";
      enabled?: boolean;
      burnIn?: boolean;
      fontSize?: number;
      position?: CaptionPosition;
      background?: boolean;
    }
  | { kind: "addCaption"; clipId: string; startMs: number; endMs: number; text: string }
  | {
      kind: "updateCaption";
      captionId: string;
      startMs?: number;
      endMs?: number;
      text?: string;
    }
  | { kind: "splitCaption"; captionId: string; atMs: number }
  | { kind: "removeCaptions"; captionIds: string[] }
  | { kind: "addMarker"; clipId: string; sourceMs: number; title: string }
  | { kind: "updateMarker"; markerId: string; sourceMs?: number; title?: string }
  | { kind: "removeMarker"; markerId: string }
  | { kind: "setDestination"; vaultId: string; folder: string; dated: boolean };
