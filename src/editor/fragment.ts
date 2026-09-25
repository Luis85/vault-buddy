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
 * is deliberately never copied — it pairs two SPECIFIC clips rather than
 * belonging to one, the same reason `core::editor::commands::payloads::
 * ClipboardFragment` carries no `transitions` field at all.
 *
 * This is a READ helper only: it builds the payload a later
 * `editor_execute({kind: "pasteFragment", ...})` call sends, and never
 * mutates `project` or calls into Rust itself (R14: Rust is authoritative
 * for every committed edit — this is the clipboard's local, uncommitted
 * copy, a read-side helper, not a write).
 */
import type { CaptionCue, Clip, ClipboardFragment, Effect, Marker, Project } from "../editorTypes";

export function buildFragment(project: Project, clipIds: string[]): ClipboardFragment {
  const idSet = new Set(clipIds);
  const clips: Clip[] = project.clips.filter((clip) => idSet.has(clip.id));
  const effects: Effect[] = project.effects.filter((effect) => idSet.has(effect.clip_id));
  const markers: Marker[] = project.markers.filter((marker) => idSet.has(marker.clip_id));
  const captions: CaptionCue[] = (project.captions?.cues ?? []).filter((cue) =>
    idSet.has(cue.clip_id),
  );

  const originMs = clips.length > 0 ? Math.min(...clips.map((clip) => clip.start_ms)) : 0;

  return { clips, effects, captions, markers, originMs };
}
