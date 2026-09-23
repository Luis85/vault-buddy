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
 * What this deliberately does NOT model (docs/Gaps.md GAP-173 — the preview
 * approximates the render): fades, transitions, effects, captions, cards
 * and other builtin assets (they have no file to show), rotation/mirror/
 * crop/adjustments, and frame-accurate sync.
 */
import type { Asset, Clip, Fit, Project, Track } from "../editorTypes";
import { isTrackAudible } from "./mixRules";
import type { Box, Size } from "./previewGeometry";
import { clipBox, containRect } from "./previewGeometry";
import { sourceAt } from "./timeMap";

export type LayerKind = "video" | "image" | "audio";

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
  fit: Fit;
}

function layerKind(asset: Asset, track: Track): LayerKind {
  if (asset.media_type === "image") return "image";
  if (asset.kind === "audio" || track.kind === "audio") return "audio";
  return "video";
}

function isMuted(clip: Clip, track: Track, tracks: readonly Track[], monitor: MonitorState): boolean {
  return monitor.muted || clip.muted || !isTrackAudible(track, tracks);
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
    const index = trackIndex.get(clip.track_id);
    const track = index === undefined ? undefined : project.tracks[index];
    const asset = assets.get(clip.asset_id);
    if (index === undefined || !track || !track.visible || !asset || asset.builtin) continue;
    const speed = clip.speed ?? 1;
    const sourceMs = sourceAt({ start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed }, t);
    if (sourceMs === null) continue;
    const kind = layerKind(asset, track);
    const muted = kind === "image" || isMuted(clip, track, project.tracks, monitor);
    layers.push({
      clipId: clip.id,
      assetId: asset.id,
      kind,
      box: kind === "audio" ? null : clipBox(canvasBox, clip),
      opacity: clip.opacity,
      z: project.tracks.length - index,
      muted,
      gain: muted ? 0 : clip.volume * track.volume * project.master_gain * monitor.volume,
      sourceMs,
      speed,
      fit: clip.fit ?? "contain",
    });
  }
  return layers.sort((a, b) => b.z - a.z);
}
