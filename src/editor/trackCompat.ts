/**
 * Which track can take media of a given kind (Task 25) — the ONE copy of
 * the rule the timeline drag (`useTimelineDrag`'s `trackAccepts`, wired in
 * `ClipItem.vue`, Task 21) and the media library's "+" (`MediaLibrary.vue`)
 * both apply, so a clip can never be droppable where "+" would refuse it or
 * the reverse. It mirrors Rust's own two refusals rather than re-deriving
 * them: a locked track refuses every edit (`commands::ensure_unlocked`), and
 * a clip's asset kind must match its track's kind (`validate::check_clip`).
 * An IMAGE asset is `kind: "video"` (its `media_type` says it is a still),
 * so it lands on a video track by the same comparison.
 */
import type { AssetKind, Project, Track } from "../editorTypes";

/** Can `track` take a clip of `kind`? A missing track takes nothing. */
export function trackAccepts(track: Track | undefined, kind: AssetKind): boolean {
  return track !== undefined && !track.locked && track.kind === kind;
}

/** The first track, in the project's own track order, that accepts `kind`. */
export function firstAcceptingTrack(project: Project | null, kind: AssetKind): Track | undefined {
  return project?.tracks.find((t) => trackAccepts(t, kind));
}
