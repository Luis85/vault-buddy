/**
 * Where an "insert at the playhead" lands (visual-parity Task 7; no-op
 * audit finding 1a) — the ONE rule the media library's "+", the Titles
 * cards and the asset menu's "Add at playhead" share.
 *
 * They used to take the first unlocked track of the right kind and let
 * Rust refuse an overlap; in the parity sample an Intro card at 0 ms aimed
 * at v3, which the presenter overlay already covers, and nothing happened.
 * Now the insert goes on the first track, top-down, that is free for the
 * whole span; with none free, a new track is added — above the topmost
 * video track (a card or an overlay reads on top), or below the last audio
 * track — and the insert lands on it. Two `execute` calls in that case,
 * the `trackEdits.addTrackThenInsert` precedent: Undo sees two labelled
 * steps, and a refused `addTrack` inserts nothing.
 *
 * Rust stays the authority; this only chooses a track it will accept.
 */
import type { Asset, AssetKind, Project } from "../editorTypes";
import { clipSpanOf } from "./actionTargets";
import type { EditorCommand } from "./editorCommandTypes";
import { clipOutputEnd } from "./timeMap";
import { trackAccepts } from "./trackCompat";
import { addTrackOfKind, insertAssetCommand } from "./trackEdits";

export type Placement = { trackId: string } | { newTrackIndex: number };

/** Does any clip on `trackId` overlap `[startMs, endMs)`? */
function trackBusy(project: Project, trackId: string, startMs: number, endMs: number): boolean {
  return project.clips.some(
    (c) => c.track_id === trackId && c.start_ms < endMs && clipOutputEnd(clipSpanOf(c)) > startMs,
  );
}

/** Where a new track of `kind` goes: above the topmost video track, or
 * after the last audio track (the top / the bottom when there is none). */
function newTrackIndex(project: Project, kind: AssetKind): number {
  const indexes = project.tracks.flatMap((t, i) => (t.kind === kind ? [i] : []));
  if (kind === "video") return indexes[0] ?? 0;
  return indexes.length > 0 ? indexes[indexes.length - 1] + 1 : project.tracks.length;
}

export function placeOnFreeTrack(project: Project, kind: AssetKind, atMs: number, lengthMs: number): Placement {
  const endMs = atMs + Math.max(lengthMs, 1);
  const free = project.tracks.find((t) => trackAccepts(t, kind) && !trackBusy(project, t.id, atMs, endMs));
  return free ? { trackId: free.id } : { newTrackIndex: newTrackIndex(project, kind) };
}

/** The track a placement names, for a control's title. */
export function placementLabel(project: Project, placement: Placement, kind: AssetKind): string {
  if ("trackId" in placement) return project.tracks.find((t) => t.id === placement.trackId)?.name ?? "";
  return `a new ${kind} track`;
}

type Execute = (command: EditorCommand) => Promise<boolean>;

/**
 * Sends `build(trackId)` on the free track, or adds a track first and
 * sends it there. `getProject` is read again after the track lands.
 */
export async function insertOnFreeTrack(
  execute: Execute,
  getProject: () => Project | null,
  kind: AssetKind,
  span: { atMs: number; lengthMs: number },
  build: (trackId: string) => EditorCommand,
): Promise<void> {
  const project = getProject();
  if (!project) return;
  const placement = placeOnFreeTrack(project, kind, span.atMs, span.lengthMs);
  if ("trackId" in placement) {
    await execute(build(placement.trackId));
    return;
  }
  const track = await addTrackOfKind(execute, getProject, kind, placement.newTrackIndex);
  if (track) await execute(build(track.id));
}

/** The whole of `asset` at `startMs`, on a free track (the "+" and "Add at
 * playhead" insert). */
export function insertAssetOnFreeTrack(
  execute: Execute,
  getProject: () => Project | null,
  asset: Asset,
  startMs: number,
): Promise<void> {
  return insertOnFreeTrack(execute, getProject, asset.kind, { atMs: startMs, lengthMs: asset.duration_ms }, (trackId) =>
    insertAssetCommand(asset, trackId, startMs),
  );
}
