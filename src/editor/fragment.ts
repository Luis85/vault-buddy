/**
 * `buildFragment` (F9, Task 8): the read-side twin of Rust's
 * `duplicateClips`/`pasteFragment` copy semantics.
 *
 * Copies the named clips plus their ATTACHED cues (effects, captions,
 * markers — every entity linked to a copied clip via its own `clip_id`)
 * and reports `originMs` as the earliest copied clip's own `start_ms`, so
 * a later `pasteFragment{trackId, atMs}` call can reconstruct every
 * clip's relative offset from one anchor point exactly as
 * `core::editor::commands::groups::paste_fragment` expects. A transition
 * pairs two SPECIFIC clips rather than belonging to one, so it is copied
 * only when BOTH of its clips are (GAP-178, the reference editor's rule):
 * Rust re-points it at the pasted clips, and the copies' overlap it
 * explains is then a crossfade rather than a refusal. One joining a copied
 * clip to one left behind stays behind.
 *
 * This is a READ helper only: it builds the payload a later
 * `editor_execute({kind: "pasteFragment", ...})` call sends, and never
 * mutates `project` or calls into Rust itself (R14: Rust is authoritative
 * for every committed edit — this is the clipboard's local, uncommitted
 * copy, a read-side helper, not a write).
 */
import type { CaptionCue, Clip, ClipboardFragment, Effect, Marker, Project, Transition } from "../editorTypes";

export function buildFragment(project: Project, clipIds: string[]): ClipboardFragment {
  const idSet = new Set(clipIds);
  const clips: Clip[] = project.clips.filter((clip) => idSet.has(clip.id));
  const effects: Effect[] = project.effects.filter((effect) => idSet.has(effect.clip_id));
  const markers: Marker[] = project.markers.filter((marker) => idSet.has(marker.clip_id));
  const captions: CaptionCue[] = (project.captions?.cues ?? []).filter((cue) =>
    idSet.has(cue.clip_id),
  );

  const transitions: Transition[] = project.transitions.filter(
    (transition) => idSet.has(transition.from) && idSet.has(transition.to),
  );

  const originMs = clips.length > 0 ? Math.min(...clips.map((clip) => clip.start_ms)) : 0;

  return { clips, effects, captions, markers, transitions, originMs };
}
