/**
 * What the preview shows at one output instant (Task 22; NATIVE-MEDIA.md §
 * Preview architecture) — a PURE function of the project graph, the time,
 * the stage size and the monitor settings. `previewController.ts` applies
 * the result to real media elements; nothing here touches the DOM.
 *
 * The rules, each one a named test in `tests/previewController.test.ts`:
 * - a clip is laid out only while it is ACTIVE (`timeMap.sourceAt` is
 *   half-open, exactly the Rust twin's rule) and only on a VISIBLE track;
 * - z-order is the REVERSE track index — `tracks[0]` is the upper lane of
 *   the timeline and paints on top;
 * - `muted` is the clip's mute ∨ the track not reaching the mix (its mute,
 *   or another track's solo — `mixRules.isTrackAudible`, the ONE copy of
 *   Rust's documented solo rule the mixer popover reads too) ∨ the
 *   workspace's `monitor_muted`. The last one is LOCAL preview state
 *   (`editorWorkspace`), never an edit: monitoring is not mixing.
 *
 * **Fades** (Task 29; F-17, F-18) ARE modeled, as of this task: `fadeFactor`
 * multiplies both `opacity` and `gain` by the clip's own fade envelope at
 * the requested OUTPUT time `t` (DATA-MODEL.md § Fade and transition rules:
 * "Edge fade envelopes multiply the clip's alpha/audio amplitude"), via
 * `fadeCurves.gainAt` — the same function `tests/editorFades.test.ts` and
 * Rust's `fades.rs` hold to one shared fixture table, so the preview and
 * that table can never silently disagree.
 *
 * **Layout** (Task 31; F-21, F-23) is modeled too: every visual layer
 * carries its `look` — frame shape, fit, crop zoom/anchor, rotation, mirror
 * and flip, plus the asset's recorded pixel size — which
 * `previewTransform.mediaPlacement` turns into the media element's place
 * inside the layer's clipping frame. The frame IS the clip's box, the same
 * `clipBox` `LayoutHandles.vue` draws its handles around, so the handles
 * and the picture cannot disagree.
 *
 * **Colour** (Task 32; F-39) is modeled as a CSS `filter:` string, one per
 * visual layer, via `colorPresets.adjustmentsFilter(clip.adjustments)` —
 * `previewController.ts` assigns it straight to the media element's own
 * `style.filter`, which is why teaching cues stay unaffected (they paint
 * ABOVE the source element, never inside it). This is an approximation,
 * not export parity: the render (a later task) applies ffmpeg's `eq`/
 * `hue`/`colorchannelmixer` filters to the same five values, and nothing
 * has compared the two yet (docs/Gaps.md GAP-173).
 *
 * What this deliberately still does NOT model (docs/Gaps.md GAP-173 — the
 * preview approximates the render): transitions, effects, captions, cards
 * and other SYNTHESIZED builtin assets (they have no file to show), and
 * frame-accurate sync. The fade CURVE SHAPE is itself only an
 * approximation for `smooth` — GAP-173 records that the preview's
 * smoothstep and the render's ffmpeg `hsin` are close but not bit-identical.
 */
import type { Asset, Builtin, Clip, Project, Track } from "../editorTypes";
import { adjustmentsFilter } from "./colorPresets";
import { gainAt } from "./fadeCurves";
import { isTrackAudible } from "./mixRules";
import type { Box, Size } from "./previewGeometry";
import { clipBox, containRect } from "./previewGeometry";
import type { LayerLook } from "./previewTransform";
import { layerLook } from "./previewTransform";
import { clipOutputEnd, sourceAt } from "./timeMap";

export type LayerKind = "video" | "image" | "audio";

/**
 * Which `Builtin` variants THIS codebase backs with a real file
 * (docs/Gaps.md GAP-175). `core::editor::migrate::from_staged` is the one
 * place that mints a `builtin: screen` asset (see its own one-line
 * cross-reference comment at the mint site), and it deliberately diverges
 * from the reference format: that asset has a real `sources.json` record,
 * resolvable through `editor_media_url` (`validate_media.rs`'s own module
 * doc — "a staged capture's asset is builtin: screen here AND has a real
 * file … which the reference's synthesized builtins never had"). Every
 * other builtin is procedurally supplied with no file, per the reference
 * format, and nothing in this codebase mints one with a registered source.
 *
 * Deliberately a `Record`, not a `Set` of the file-backed few: fix round 1
 * of GAP-175 replaced the original `Set<Builtin>(["screen"])` because a set
 * only needed vigilance to stay a superset of whatever `migrate.rs` mints
 * next — a future file-backed builtin could reproduce this gap silently. A
 * `Record<Builtin, boolean>` must enumerate every member of the `Builtin`
 * union (`editorTypes.ts`) or the object literal fails to type-check, so
 * the compiler itself refuses to build the moment a new `Builtin` variant
 * is added here without an explicit `true`/`false` decision.
 */
const BUILTIN_HAS_FILE: Record<Builtin, boolean> = {
  screen: true,
  presenter: false,
  detail: false,
  cues: false,
  ambient: false,
  card: false,
};

/** Has this asset a real file the preview could show — as opposed to a
 * synthesized builtin with none (GAP-175)? */
export function hasPreviewSource(asset: Asset): boolean {
  return asset.builtin === undefined || BUILTIN_HAS_FILE[asset.builtin];
}

/** Local preview monitoring — never part of the project. */
export interface MonitorState {
  muted: boolean;
  /** 0..1, multiplied into every layer's gain. */
  volume: number;
}

export interface PreviewLayer {
  clipId: string;
  assetId: string;
  kind: LayerKind;
  /** Stage pixels; `null` for an audio layer, which has no picture. */
  box: Box | null;
  opacity: number;
  /** Higher paints on top. */
  z: number;
  muted: boolean;
  /** The linear gain a monitor would apply: 0 whenever `muted`. */
  gain: number;
  /** The SOURCE instant this layer shows at the requested output time. */
  sourceMs: number;
  speed: number;
  /** The clip's `preserve_pitch` (unset reads as on), for `preservesPitch`. */
  preservePitch: boolean;
  /** Everything about how the picture sits in its box (Task 31). */
  look: LayerLook;
  /** The CSS `filter:` string for this layer's colour (Task 32) — `"none"`
   * for an audio layer (no picture) or a clip carrying no adjustments. */
  filter: string;
}

function layerKind(asset: Asset, track: Track): LayerKind {
  if (asset.media_type === "image") return "image";
  if (asset.kind === "audio" || track.kind === "audio") return "audio";
  return "video";
}

function isMuted(clip: Clip, track: Track, tracks: readonly Track[], monitor: MonitorState): boolean {
  return monitor.muted || clip.muted || !isTrackAudible(track, tracks);
}

/** The fade envelope's gain (0..1) at OUTPUT time `t` — 1 outside both
 * fade windows, `gainAt(clip.fade_curve, …)` inside either, and the
 * PRODUCT of both when a very short clip's fades overlap (each edge's
 * envelope attenuates independently, DATA-MODEL.md's "multiply"). */
function fadeFactor(clip: Clip, t: number): number {
  const end = clipOutputEnd({ start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed: clip.speed ?? 1 });
  let factor = 1;
  if (clip.fade_in_ms > 0 && t < clip.start_ms + clip.fade_in_ms) {
    factor *= gainAt(clip.fade_curve, (t - clip.start_ms) / clip.fade_in_ms);
  }
  if (clip.fade_out_ms > 0 && t > end - clip.fade_out_ms) {
    factor *= gainAt(clip.fade_curve, (end - t) / clip.fade_out_ms);
  }
  return factor;
}

/** Everything `buildLayer` needs, resolved once per clip. */
interface ResolvedLayerInputs {
  track: Track;
  index: number;
  asset: Asset;
  sourceMs: number;
  speed: number;
  kind: LayerKind;
}

/** Resolves whether `clip` has anything to show at output time `t`, and if
 * so, everything that decides WHETHER it shows -- an unknown/invisible
 * track, an asset with no preview source, or `t` outside the clip's active
 * span (`sourceAt`'s half-open rule) all read as `null`. Split out of
 * `computeLayers` (Task 32 fix) so each half of the per-clip work — whether
 * to show it, and how — stays under the fallow complexity ceiling on its
 * own; `buildLayer` below is the "how" half. */
function resolveLayerInputs(
  clip: Clip,
  project: Project,
  t: number,
  trackIndex: Map<string, number>,
  assets: Map<string, Asset>,
): ResolvedLayerInputs | null {
  const index = trackIndex.get(clip.track_id);
  const track = index === undefined ? undefined : project.tracks[index];
  const asset = assets.get(clip.asset_id);
  if (index === undefined || !track || !track.visible || !asset || !hasPreviewSource(asset)) return null;
  const speed = clip.speed ?? 1;
  const sourceMs = sourceAt({ start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed }, t);
  if (sourceMs === null) return null;
  return { track, index, asset, sourceMs, speed, kind: layerKind(asset, track) };
}

/** Builds the layer from `resolveLayerInputs`' resolved facts -- mix, box,
 * colour and everything else that decides HOW a clip shows, never whether. */
function buildLayer(
  clip: Clip,
  project: Project,
  t: number,
  canvasBox: Box,
  monitor: MonitorState,
  resolved: ResolvedLayerInputs,
): PreviewLayer {
  const { track, index, asset, sourceMs, speed, kind } = resolved;
  const muted = kind === "image" || isMuted(clip, track, project.tracks, monitor);
  const fade = fadeFactor(clip, t);
  return {
    clipId: clip.id,
    assetId: asset.id,
    kind,
    box: kind === "audio" ? null : clipBox(canvasBox, clip),
    opacity: clip.opacity * fade,
    z: project.tracks.length - index,
    muted,
    gain: muted ? 0 : clip.volume * track.volume * project.master_gain * monitor.volume * fade,
    sourceMs,
    speed,
    preservePitch: clip.preserve_pitch ?? true,
    look: layerLook(clip, asset),
    filter: kind === "audio" ? "none" : adjustmentsFilter(clip.adjustments),
  };
}

/** Every layer active at output time `t`, sorted top-most first. */
export function computeLayers(
  project: Project,
  t: number,
  stage: Size,
  monitor: MonitorState,
): PreviewLayer[] {
  const trackIndex = new Map(project.tracks.map((track, i) => [track.id, i]));
  const assets = new Map(project.assets.map((a) => [a.id, a]));
  const canvasBox = containRect(project.canvas, stage);
  const layers: PreviewLayer[] = [];
  for (const clip of project.clips) {
    const resolved = resolveLayerInputs(clip, project, t, trackIndex, assets);
    if (resolved) layers.push(buildLayer(clip, project, t, canvasBox, monitor, resolved));
  }
  return layers.sort((a, b) => b.z - a.z);
}
