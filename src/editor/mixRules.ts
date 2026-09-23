/**
 * The mixer's read-side rules (Task 27; F-05, F-24, F-25) — PURE, shared by
 * the preview's monitoring (`previewLayers.ts`), the mixer popover
 * (`MixerPopover.vue`), the Audio inspector (`AudioSection.vue`) and the
 * `detachAudio` action (`actions.ts`), so none of them grows a second copy.
 *
 * **Solo** is the rule Rust documents in `core::editor::commands::tracks`
 * (Task 23): when ANY track is soloed, only soloed tracks are audible, and
 * a soloed track's own mute still wins. `isTrackAudible` is that rule; the
 * preview's `computeLayers` and the mixer's "silenced by solo" label both
 * call it rather than restating it.
 *
 * **Detach** mirrors the refusals Rust's `commands::mix::detach_audio` can
 * decide from the graph alone (a video, not a still; not already
 * detached; the locked-track refusal is `actions.ts`'s shared one). Whether the source HAS sound lives in
 * `sources.json`, which the frontend never sees — Rust answers that one,
 * and its refusal surfaces as the store's `lastError`.
 */
import type { Clip, Project, Track } from "../editorTypes";
import { clipOutputEnd } from "./timeMap";

/** Is any track soloed? */
function soloActive(tracks: readonly Track[]): boolean {
  return tracks.some((t) => t.solo);
}

/** Does `track` reach the mix at all — its own mute, then the solo rule. */
export function isTrackAudible(track: Track, tracks: readonly Track[]): boolean {
  return !track.muted && (!soloActive(tracks) || track.solo);
}

/** A linear gain as decibels, one decimal, with a real minus sign; silence
 * is `−∞ dB`. The inspector STORES linear (`Clip.volume`, `[0,2]`) and
 * only READS OUT in dB. */
export function formatDb(linear: number): string {
  if (!(linear > 0)) return "−∞ dB";
  const db = 20 * Math.log10(linear);
  const rounded = Math.round(db * 10) / 10;
  if (rounded === 0) return "0.0 dB";
  return `${rounded > 0 ? "+" : "−"}${Math.abs(rounded).toFixed(1)} dB`;
}

function outputEnd(clip: Clip): number {
  return clipOutputEnd({ start_ms: clip.start_ms, in_ms: clip.in_ms, out_ms: clip.out_ms, speed: clip.speed ?? 1 });
}

/** Does any clip on `trackId` play during `[start, end)`? Half-open spans,
 * exactly Rust's `overlaps`. */
function trackBusy(project: Project, trackId: string, start: number, end: number): boolean {
  return project.clips.some((c) => c.track_id === trackId && start < outputEnd(c) && c.start_ms < end);
}

/** The first unlocked audio track (project order) free over `clip`'s whole
 * output span — where a detach should land — or `null`, which asks Rust
 * for a NEW audio track. */
export function freeAudioTrackFor(project: Project, clip: Clip): string | null {
  const end = outputEnd(clip);
  const free = project.tracks.find(
    (t) => t.kind === "audio" && !t.locked && !trackBusy(project, t.id, clip.start_ms, end),
  );
  return free?.id ?? null;
}

/** Is `candidate` a detached copy of `clip`'s audio — Rust's `<id>-audio`
 * asset over the identical source range? */
function isDetachedCopy(candidate: Clip, clip: Clip): boolean {
  return (
    candidate.asset_id === `${clip.asset_id}-audio` &&
    candidate.start_ms === clip.start_ms &&
    candidate.in_ms === clip.in_ms &&
    candidate.out_ms === clip.out_ms
  );
}

/** Why `clip`'s audio cannot be detached, or `null` when Rust should be
 * asked (it still refuses a source with no audio stream). The caller has
 * already refused a locked track (`actions.ts`'s shared clip guard). */
export function detachRefusal(project: Project, clip: Clip): string | null {
  const asset = project.assets.find((a) => a.id === clip.asset_id);
  if (asset?.kind !== "video" || asset.media_type === "image") {
    return "Only a video clip's audio can be detached";
  }
  const already = project.clips.some((c) => isDetachedCopy(c, clip));
  return already ? "This clip's audio is already detached" : null;
}
