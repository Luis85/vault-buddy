/**
 * Put a finished webcam take on the timeline as the presenter (Task 50;
 * F-20, F-21; A08): a NEW video track at index 0 — the frontmost layer
 * (`core::editor::commands::tracks`: index 0 is the top) — the take as its
 * OWN clip there at the playhead, then the ADR's presenter placement
 * (`PRESENTER_CORNER`, passed explicitly, never `cornerPreset`'s margin).
 * The take stays an ordinary independent clip, never baked into the screen
 * capture; moving or resizing it afterwards is Task 31's `LayoutHandles`/
 * `LayoutSection`.
 *
 * Three `execute` calls — `addTrack`, `insertClip`, `setLayout` — each one
 * labelled undo step, the `TimelineView.addTrackThenInsert` precedent: Rust
 * has no compound command, and a refusal at any step stops the rest (a
 * refused `addTrack` inserts nothing).
 */
import type { EditorCommand, Project, TakeDto } from "../editorTypes";
import { PRESENTER_CORNER, presenterBox } from "./layoutGeometry";

/** The slice of the project store this needs. */
export interface PlacementTarget {
  readonly project: Project | null;
  execute(command: EditorCommand): Promise<boolean>;
}

/** "Presenter", or "Presenter 2" when that name is taken. */
function presenterTrackName(project: Project): string {
  const names = new Set(project.tracks.map((t) => t.name));
  let n = 1;
  while (names.has(n === 1 ? "Presenter" : `Presenter ${n}`)) n += 1;
  return n === 1 ? "Presenter" : `Presenter ${n}`;
}

/** Resolves `true` once all three steps landed. */
export async function placePresenterTake(target: PlacementTarget, take: TakeDto, atMs: number): Promise<boolean> {
  const before = target.project;
  if (!before) return false;
  const known = new Set(before.tracks.map((t) => t.id));
  const name = presenterTrackName(before);
  if (!(await target.execute({ kind: "addTrack", trackKind: "video", name, index: 0 }))) return false;
  const track = target.project?.tracks.find((t) => !known.has(t.id));
  if (!track) return false;
  const inserted = await target.execute({
    kind: "insertClip",
    assetId: take.assetId,
    trackId: track.id,
    startMs: atMs,
    inMs: 0,
    outMs: take.durationMs,
  });
  const clip = target.project?.clips.find((c) => c.track_id === track.id && c.asset_id === take.assetId);
  if (!inserted || !clip || !target.project) return false;
  const box = presenterBox(target.project.canvas);
  return target.execute({
    kind: "setLayout",
    clipIds: [clip.id],
    ...box,
    frameShape: PRESENTER_CORNER.frameShape,
    fit: PRESENTER_CORNER.fit,
  });
}
