/**
 * The Clip tab's Track select (visual-parity Task 14; concept spec §5
 * "Clip": "Track select (same-kind tracks, '· locked' suffix)") — which
 * tracks a clip may move to with `moveClips{trackId}`, and why it may not
 * move at all, in Rust's words and order (`commands::clips::move_clips`):
 * a locked track refuses, a locked destination refuses, a grouped clip
 * refuses (a track move names ONE clip, and a group moves together), and a
 * clip only fits a track of its own kind.
 */
import type { Clip, Project } from "../editorTypes";
import { lockedReason } from "./actionMeta";

const GROUPED_NOTE = "Ungroup this clip to move it to another track.";

export interface TrackOption {
  id: string;
  label: string;
  /** A locked track cannot receive the clip. The clip's own track stays
   * choosable, so the select can show where the clip is. */
  disabled: boolean;
}

/** The tracks of the clip's own kind, top to bottom. */
export function trackOptions(project: Project | null, clip: Clip): TrackOption[] {
  const kind = project?.tracks.find((t) => t.id === clip.track_id)?.kind;
  return (project?.tracks ?? [])
    .filter((t) => t.kind === kind)
    .map((t) => ({
      id: t.id,
      label: t.locked ? `${t.name} · locked` : t.name,
      disabled: t.locked && t.id !== clip.track_id,
    }));
}

/** Why the clip cannot change track, or `null` when it can. */
export function trackMoveRefusal(project: Project | null, clip: Clip): string | null {
  const own = project?.tracks.find((t) => t.id === clip.track_id);
  if (own?.locked) return lockedReason(own.name);
  const grouped = clip.group_id && project?.clips.some((c) => c.id !== clip.id && c.group_id === clip.group_id);
  return grouped ? GROUPED_NOTE : null;
}
