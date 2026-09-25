/**
 * What the preview shows for a TITLE CARD clip at one output instant (Task
 * 33; F-37) — a PURE function of the project graph and the time, split out
 * of `previewLayers.ts` (which deliberately still returns NOTHING for a
 * card clip, `BUILTIN_HAS_FILE.card === false`: a card has text/colour, not
 * a file, so it is never a `<video>`/`<img>`/`<audio>` element). This
 * module answers the question `previewLayers.ts` leaves open: cards are
 * rendered by `previewCardDom.ts` as a styled `<div>` instead, driven by
 * THIS module's output.
 *
 * A clip is a card by its ASSET's `builtin` kind, never by the clip's own
 * `card` content alone (`core::editor::commands::layout`'s own rule for the
 * exact same reason) — `Clip.card` is read here only once that asset check
 * already passed, so a stray inline `card` object on an ordinary clip can
 * never spawn a phantom card layer.
 *
 * The active-span/visibility rules are the SAME ones `previewLayers.ts`
 * applies to a media layer (`sourceAt`'s half-open span, an invisible
 * track excludes its clips) — restated here rather than imported, since a
 * card layer carries none of `computeLayers`' media-specific fields (mix,
 * fades, look, filter) and importing just the two shared checks would cost
 * more than it saves.
 */
import type { Asset, Clip, Project, Track } from "../editorTypes";
import type { Box, Size } from "./previewGeometry";
import { clipBox } from "./previewGeometry";
import { layerContext } from "./previewLayers";
import { sourceAt } from "./timeMap";

export interface CardLayer {
  clipId: string;
  box: Box;
  /** Higher paints on top — the same track-index-derived value
   * `previewLayers.ts`'s media layers use, so a card and a media layer on
   * adjacent tracks stack exactly as the timeline predicts. */
  z: number;
  opacity: number;
  title: string;
  subtitle: string;
  background: string;
  foreground: string;
  accent: string;
}

function isCardAsset(asset: Asset | undefined): boolean {
  return asset?.builtin === "card";
}

/** Every title-card layer active at output time `t`, sorted top-most
 * first — `previewLayers.computeLayers`'s own contract, for a disjoint set
 * of clips (a card clip never has a media layer, since
 * `previewLayers.hasPreviewSource` already excludes it). */
export function computeCardLayers(project: Project, t: number, stage: Size): CardLayer[] {
  const { trackIndex, assets, canvasBox } = layerContext(project, stage);
  const layers: CardLayer[] = [];
  for (const clip of project.clips) {
    const layer = cardLayerFor(clip, project.tracks, trackIndex, assets, canvasBox, t);
    if (layer) layers.push(layer);
  }
  return layers.sort((a, b) => b.z - a.z);
}

function cardLayerFor(
  clip: Clip,
  tracks: readonly Track[],
  trackIndex: Map<string, number>,
  assets: Map<string, Asset>,
  canvasBox: Box,
  t: number,
): CardLayer | null {
  if (!clip.card) return null;
  if (!isCardAsset(assets.get(clip.asset_id))) return null;
  const index = trackIndex.get(clip.track_id);
  const track = index === undefined ? undefined : tracks[index];
  if (index === undefined || !track || !track.visible) return null;
  const speed = clip.speed ?? 1;
  const sourceMs = sourceAt({ start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed }, t);
  if (sourceMs === null) return null;
  const card = clip.card;
  return {
    clipId: clip.id,
    box: clipBox(canvasBox, clip),
    z: tracks.length - index,
    opacity: clip.opacity,
    title: card.title,
    subtitle: card.subtitle,
    background: card.background,
    foreground: card.foreground,
    accent: card.accent,
  };
}
