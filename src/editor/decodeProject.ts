/**
 * `decodeProject` — the project graph's own decoder, split out of
 * `decode.ts` (F31: "split decoders by family if decode.ts grows past
 * [500 lines]", this task's own brief) since the full interchange graph
 * (assets/tracks/clips/effects/markers/transitions/captions/canvas/
 * destination) is the single biggest decode surface this task owns.
 *
 * Mirrors `core::editor::model`/`core::editor::model_cues` field for
 * field, document spelling throughout (R3) — every `*_ms` field is bounded
 * at `MAX_DURATION_MS` and every enum member set is closed, so an unknown
 * effect kind or a corrupted timestamp fails here rather than reaching a
 * component that assumes the graph is well-formed.
 */
import type {
  Adjustments,
  Asset,
  AssetKind,
  Builtin,
  Canvas,
  CaptionCue,
  CaptionSettings,
  Card,
  CardPreset,
  Clip,
  Destination,
  Effect,
  EffectKind,
  FadeCurve,
  Fit,
  FrameShape,
  Marker,
  MediaType,
  Project,
  Rotation,
  Track,
  TrackKind,
  Transition,
  TransitionKind,
} from "../editorTypes";
import {
  asArray,
  asBoolean,
  asEnum,
  asId,
  asMs,
  asNumber,
  asObject,
  asOptionalBoolean,
  asOptionalEnum,
  asOptionalId,
  asOptionalNumber,
  asOptionalString,
  asString,
  fail,
} from "./decodePrimitives";

const ASSET_KINDS: readonly AssetKind[] = ["video", "audio"];
const BUILTINS: readonly Builtin[] = ["presenter", "screen", "detail", "cues", "ambient", "card"];
const MEDIA_TYPES: readonly MediaType[] = ["image"];
const TRACK_KINDS: readonly TrackKind[] = ["audio", "video"];
const FADE_CURVES: readonly FadeCurve[] = ["linear", "smooth", "equal-power"];
const FRAME_SHAPES: readonly FrameShape[] = ["circle", "rounded", "rectangle"];
const FITS: readonly Fit[] = ["cover", "contain"];
const ROTATIONS: readonly Rotation[] = [0, 90, 180, 270];
const CARD_PRESETS: readonly CardPreset[] = ["intro", "chapter", "outro", "blank"];
const EFFECT_KINDS: readonly EffectKind[] = [
  "text",
  "arrow",
  "highlight",
  "spotlight",
  "zoom",
  "step",
  "mask",
];
const CAPTION_POSITIONS: readonly ("top" | "bottom")[] = ["top", "bottom"];
const TRANSITION_KINDS: readonly TransitionKind[] = ["dissolve", "equal-power"];

function asRotation(value: unknown, field: string): Rotation {
  if (typeof value !== "number" || !(ROTATIONS as readonly number[]).includes(value)) {
    fail(`${field} must be a quarter turn (0, 90, 180 or 270)`);
  }
  return value as Rotation;
}

function asOptionalRotation(value: unknown, field: string): Rotation | undefined {
  if (value === undefined) return undefined;
  return asRotation(value, field);
}

function asPixels(value: unknown, field: string): number {
  const n = asNumber(value, field);
  if (!Number.isInteger(n) || n < 0) fail(`${field} must be a non-negative integer`);
  return n;
}

function decodeCanvas(value: unknown, field: string): Canvas {
  const v = asObject(value, field);
  return {
    width: asPixels(v.width, `${field}.width`),
    height: asPixels(v.height, `${field}.height`),
    fps: asPixels(v.fps, `${field}.fps`),
  };
}

function decodeAsset(value: unknown, field: string): Asset {
  const v = asObject(value, field);
  return {
    id: asId(v.id, `${field}.id`),
    kind: asEnum(v.kind, `${field}.kind`, ASSET_KINDS),
    name: asString(v.name, `${field}.name`),
    duration_ms: asMs(v.duration_ms, `${field}.duration_ms`),
    width: asOptionalNumber(v.width, `${field}.width`),
    height: asOptionalNumber(v.height, `${field}.height`),
    size: v.size === undefined ? undefined : asPixels(v.size, `${field}.size`),
    builtin: asOptionalEnum(v.builtin, `${field}.builtin`, BUILTINS),
    media_type: asOptionalEnum(v.media_type, `${field}.media_type`, MEDIA_TYPES),
    linked_asset: asOptionalId(v.linked_asset, `${field}.linked_asset`),
    original_name: asOptionalString(v.original_name, `${field}.original_name`),
  };
}

function decodeTrack(value: unknown, field: string): Track {
  const v = asObject(value, field);
  return {
    id: asId(v.id, `${field}.id`),
    kind: asEnum(v.kind, `${field}.kind`, TRACK_KINDS),
    name: asString(v.name, `${field}.name`),
    visible: asBoolean(v.visible, `${field}.visible`),
    locked: asBoolean(v.locked, `${field}.locked`),
    muted: asBoolean(v.muted, `${field}.muted`),
    solo: asBoolean(v.solo, `${field}.solo`),
    volume: asNumber(v.volume, `${field}.volume`),
  };
}

function decodeAdjustments(value: unknown, field: string): Adjustments {
  const v = asObject(value, field);
  return {
    brightness: asNumber(v.brightness, `${field}.brightness`),
    contrast: asNumber(v.contrast, `${field}.contrast`),
    saturation: asNumber(v.saturation, `${field}.saturation`),
    sepia: asNumber(v.sepia, `${field}.sepia`),
    grayscale: asNumber(v.grayscale, `${field}.grayscale`),
  };
}

function decodeCard(value: unknown, field: string): Card {
  const v = asObject(value, field);
  return {
    preset: asEnum(v.preset, `${field}.preset`, CARD_PRESETS),
    title: asString(v.title, `${field}.title`),
    subtitle: asString(v.subtitle, `${field}.subtitle`),
    background: asString(v.background, `${field}.background`),
    foreground: asString(v.foreground, `${field}.foreground`),
    accent: asString(v.accent, `${field}.accent`),
  };
}

function decodeClip(value: unknown, field: string): Clip {
  const v = asObject(value, field);
  return {
    id: asId(v.id, `${field}.id`),
    asset_id: asId(v.asset_id, `${field}.asset_id`),
    track_id: asId(v.track_id, `${field}.track_id`),
    name: asString(v.name, `${field}.name`),
    start_ms: asMs(v.start_ms, `${field}.start_ms`),
    in_ms: asMs(v.in_ms, `${field}.in_ms`),
    out_ms: asMs(v.out_ms, `${field}.out_ms`),
    fade_in_ms: asMs(v.fade_in_ms, `${field}.fade_in_ms`),
    fade_out_ms: asMs(v.fade_out_ms, `${field}.fade_out_ms`),
    fade_curve: asEnum(v.fade_curve, `${field}.fade_curve`, FADE_CURVES),
    opacity: asNumber(v.opacity, `${field}.opacity`),
    volume: asNumber(v.volume, `${field}.volume`),
    muted: asBoolean(v.muted, `${field}.muted`),
    x: asNumber(v.x, `${field}.x`),
    y: asNumber(v.y, `${field}.y`),
    w: asNumber(v.w, `${field}.w`),
    h: asNumber(v.h, `${field}.h`),
    speed: asOptionalNumber(v.speed, `${field}.speed`),
    rotation: asOptionalRotation(v.rotation, `${field}.rotation`),
    frame_shape: asOptionalEnum(v.frame_shape, `${field}.frame_shape`, FRAME_SHAPES),
    fit: asOptionalEnum(v.fit, `${field}.fit`, FITS),
    mirror: asOptionalBoolean(v.mirror, `${field}.mirror`),
    flip_y: asOptionalBoolean(v.flip_y, `${field}.flip_y`),
    preserve_pitch: asOptionalBoolean(v.preserve_pitch, `${field}.preserve_pitch`),
    group_id: asOptionalId(v.group_id, `${field}.group_id`),
    crop_zoom: asOptionalNumber(v.crop_zoom, `${field}.crop_zoom`),
    crop_x: asOptionalNumber(v.crop_x, `${field}.crop_x`),
    crop_y: asOptionalNumber(v.crop_y, `${field}.crop_y`),
    adjustments: v.adjustments === undefined ? undefined : decodeAdjustments(v.adjustments, `${field}.adjustments`),
    card: v.card === undefined ? undefined : decodeCard(v.card, `${field}.card`),
  };
}

function decodeEffect(value: unknown, field: string): Effect {
  const v = asObject(value, field);
  return {
    id: asId(v.id, `${field}.id`),
    clip_id: asId(v.clip_id, `${field}.clip_id`),
    kind: asEnum(v.kind, `${field}.kind`, EFFECT_KINDS),
    start_ms: asMs(v.start_ms, `${field}.start_ms`),
    end_ms: asMs(v.end_ms, `${field}.end_ms`),
    x: asNumber(v.x, `${field}.x`),
    y: asNumber(v.y, `${field}.y`),
    color: asString(v.color, `${field}.color`),
    text: asOptionalString(v.text, `${field}.text`),
    w: asOptionalNumber(v.w, `${field}.w`),
    h: asOptionalNumber(v.h, `${field}.h`),
    x2: asOptionalNumber(v.x2, `${field}.x2`),
    y2: asOptionalNumber(v.y2, `${field}.y2`),
    factor: asOptionalNumber(v.factor, `${field}.factor`),
    // `fontSize`, not `font_size` — the one entity field that stays
    // camelCase even inside the document-spelled graph
    // (`core::editor::model_cues::Effect`'s own `#[serde(rename = "fontSize")]`).
    fontSize: asOptionalNumber(v.fontSize, `${field}.fontSize`),
    stroke: asOptionalNumber(v.stroke, `${field}.stroke`),
    dim: asOptionalNumber(v.dim, `${field}.dim`),
    easing: asOptionalNumber(v.easing, `${field}.easing`),
    number: asOptionalNumber(v.number, `${field}.number`),
    background: asOptionalBoolean(v.background, `${field}.background`),
  };
}

function decodeMarker(value: unknown, field: string): Marker {
  const v = asObject(value, field);
  return {
    id: asId(v.id, `${field}.id`),
    clip_id: asId(v.clip_id, `${field}.clip_id`),
    source_ms: asMs(v.source_ms, `${field}.source_ms`),
    title: asString(v.title, `${field}.title`),
  };
}

function decodeCaptionCue(value: unknown, field: string): CaptionCue {
  const v = asObject(value, field);
  return {
    id: asId(v.id, `${field}.id`),
    clip_id: asId(v.clip_id, `${field}.clip_id`),
    start_ms: asMs(v.start_ms, `${field}.start_ms`),
    end_ms: asMs(v.end_ms, `${field}.end_ms`),
    text: asString(v.text, `${field}.text`),
  };
}

function decodeCaptionSettings(value: unknown, field: string): CaptionSettings {
  const v = asObject(value, field);
  return {
    enabled: asBoolean(v.enabled, `${field}.enabled`),
    burn_in: asBoolean(v.burn_in, `${field}.burn_in`),
    font_size: asNumber(v.font_size, `${field}.font_size`),
    position: asEnum(v.position, `${field}.position`, CAPTION_POSITIONS),
    background: asBoolean(v.background, `${field}.background`),
    cues: asArray(v.cues, `${field}.cues`).map((c, i) => decodeCaptionCue(c, `${field}.cues[${i}]`)),
  };
}

function decodeTransition(value: unknown, field: string): Transition {
  const v = asObject(value, field);
  return {
    id: asId(v.id, `${field}.id`),
    from: asId(v.from, `${field}.from`),
    to: asId(v.to, `${field}.to`),
    duration_ms: asMs(v.duration_ms, `${field}.duration_ms`),
    kind: asEnum(v.kind, `${field}.kind`, TRANSITION_KINDS),
  };
}

function decodeDestination(value: unknown, field: string): Destination {
  const v = asObject(value, field);
  return {
    vault: asString(v.vault, `${field}.vault`),
    folder: asString(v.folder, `${field}.folder`),
    dated: asBoolean(v.dated, `${field}.dated`),
  };
}

/** The full interchange project graph (`core::editor::model::Project`).
 * `captions` normalizes an omitted key to `null` (Rust's own
 * `skip_serializing_if` on that field), so callers rely on one falsy
 * shape rather than checking both `undefined` and `null`. */
export function decodeProject(value: unknown): Project {
  const v = asObject(value, "project");
  const captionsRaw = v.captions;
  return {
    schema: asString(v.schema, "project.schema"),
    id: asId(v.id, "project.id"),
    title: asString(v.title, "project.title"),
    canvas: decodeCanvas(v.canvas, "project.canvas"),
    master_gain: asNumber(v.master_gain, "project.master_gain"),
    assets: asArray(v.assets, "project.assets").map((a, i) => decodeAsset(a, `project.assets[${i}]`)),
    tracks: asArray(v.tracks, "project.tracks").map((t, i) => decodeTrack(t, `project.tracks[${i}]`)),
    clips: asArray(v.clips, "project.clips").map((c, i) => decodeClip(c, `project.clips[${i}]`)),
    effects: asArray(v.effects, "project.effects").map((e, i) => decodeEffect(e, `project.effects[${i}]`)),
    markers: asArray(v.markers, "project.markers").map((m, i) => decodeMarker(m, `project.markers[${i}]`)),
    transitions: asArray(v.transitions, "project.transitions").map((t, i) =>
      decodeTransition(t, `project.transitions[${i}]`),
    ),
    captions:
      captionsRaw === undefined || captionsRaw === null
        ? null
        : decodeCaptionSettings(captionsRaw, "project.captions"),
    destination: decodeDestination(v.destination, "project.destination"),
  };
}
