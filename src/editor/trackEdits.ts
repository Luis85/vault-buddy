/**
 * Edits that act on one track as a whole (visual-parity Task 5): closing
 * its empty stretches, and minting a new track — optionally with an asset
 * placed on it. The lane and track context menus, the asset menu and the
 * timeline's below-the-last-lane drop all send these same commands.
 *
 * **Gaps.** A gap is the time before a clip that no earlier clip on the
 * track covers (the concept's `gapsOn`). Closing one is a `moveClips` of
 * every clip that starts at or after its end, earlier by the gap's length;
 * Rust carries grouped partners along and refuses an overlap or a locked
 * track, so nothing here re-checks those.
 *
 * **New tracks** (Task 26's drop, moved here). Two `execute` calls,
 * deliberately: `addTrack` then `insertClip`, which Undo sees as two
 * labelled steps — Rust has no single "add a track and a clip" command. A
 * refused `addTrack` inserts nothing.
 */
import type { Asset, Project, Track, TrackKind } from "../editorTypes";
import { NO_PROJECT } from "./actionMeta";
import { clipSpanOf } from "./actionTargets";
import type { EditorCommand } from "./editorCommandTypes";
import { clipOutputEnd } from "./timeMap";

// ---- gaps ----------------------------------------------------------------------

export interface Gap {
  start: number;
  end: number;
}

/** Gaps shorter than this are rounding, not a hole a person can see. */
const MIN_GAP_MS = 1;

export function gapsOnTrack(project: Project, trackId: string): Gap[] {
  const clips = project.clips.filter((c) => c.track_id === trackId).sort((a, b) => a.start_ms - b.start_ms);
  const gaps: Gap[] = [];
  let cursor = 0;
  for (const clip of clips) {
    if (clip.start_ms > cursor + MIN_GAP_MS) gaps.push({ start: cursor, end: clip.start_ms });
    cursor = Math.max(cursor, clipOutputEnd(clipSpanOf(clip)));
  }
  return gaps;
}

export function closeGapCommand(project: Project, trackId: string, gap: Gap): EditorCommand {
  const clipIds = project.clips.filter((c) => c.track_id === trackId && c.start_ms >= gap.end).map((c) => c.id);
  return { kind: "moveClips", clipIds, deltaMs: gap.start - gap.end, trackId: null };
}

/** Last gap first: each move then only pulls clips into space already
 * empty, so no intermediate state overlaps. */
export function closeAllGapsCommands(project: Project, trackId: string): EditorCommand[] {
  return gapsOnTrack(project, trackId)
    .reverse()
    .map((gap) => closeGapCommand(project, trackId, gap));
}

// ---- new tracks ------------------------------------------------------------------

type Execute = (command: EditorCommand) => Promise<boolean>;

/** `core::editor::limits::MAX_TRACKS`: Rust refuses a 33rd track. */
const MAX_TRACKS = 32;

/** Why no track can be added, or `null` when one can. */
export function addTrackRefusal(project: Project | null): string | null {
  if (!project) return NO_PROJECT;
  return project.tracks.length >= MAX_TRACKS ? `This project already has the maximum of ${MAX_TRACKS} tracks` : null;
}

/** "Video N"/"Audio N", N one past how many tracks of that kind exist. */
function nextTrackName(project: Project | null, kind: TrackKind): string {
  const count = (project?.tracks ?? []).filter((t) => t.kind === kind).length + 1;
  return kind === "audio" ? `Audio ${count}` : `Video ${count}`;
}

/**
 * The ONE `addTrack` every surface sends (visual-parity Task 16, ruling
 * P3): the registry's Add video/audio track (the timeline's Add track menu
 * and the empty-lane menu), a drop below the last lane, a free-track
 * placement and the webcam presenter. Without `index` a video track goes
 * on top — index 0, the frontmost layer — and an audio track at the
 * bottom, the concept's `addTrack` (`unshift` / `push`).
 */
export function addTrackCommand(project: Project | null, kind: TrackKind, index?: number, name?: string): EditorCommand {
  const at = index ?? (kind === "video" ? 0 : (project?.tracks.length ?? 0));
  return { kind: "addTrack", trackKind: kind, name: name ?? nextTrackName(project, kind), index: at };
}

/** Adds a track of `kind` at `index` (below the others by default);
 * resolves to it, or `null` when Rust refused. `getProject` is read again
 * after the edit lands. */
export async function addTrackOfKind(
  execute: Execute,
  getProject: () => Project | null,
  kind: TrackKind,
  at?: number,
): Promise<Track | null> {
  const project = getProject();
  const before = new Set((project?.tracks ?? []).map((t) => t.id));
  const added = await execute(addTrackCommand(project, kind, at ?? project?.tracks.length ?? 0));
  if (!added) return null;
  return getProject()?.tracks.find((t) => !before.has(t.id)) ?? null;
}

/** The whole of `asset` at `startMs` on `trackId`. */
export function insertAssetCommand(asset: Asset, trackId: string, startMs: number): EditorCommand {
  return { kind: "insertClip", assetId: asset.id, trackId, startMs, inMs: 0, outMs: asset.duration_ms };
}

export async function addTrackThenInsert(
  execute: Execute,
  getProject: () => Project | null,
  asset: Asset,
  startMs: number,
): Promise<void> {
  const track = await addTrackOfKind(execute, getProject, asset.kind);
  if (track) await execute(insertAssetCommand(asset, track.id, startMs));
}
