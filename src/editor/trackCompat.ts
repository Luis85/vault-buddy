/**
 * Which track can take media of a given kind (Task 25) — the ONE copy of
 * the rule the timeline drag (`useTimelineDrag`'s `trackAccepts`, wired in
 * `ClipItem.vue`, Task 21), the media library's "+" (`MediaLibrary.vue`),
 * and the media library's native drag-onto-a-lane drop (`TrackLane.vue`,
 * Task 26) all apply, so a clip can never be droppable where "+" would
 * refuse it or the reverse. It mirrors Rust's own two refusals rather than
 * re-deriving them: a locked track refuses every edit (`commands::
 * ensure_unlocked`), and a clip's asset kind must match its track's kind
 * (`validate::check_clip`). An IMAGE asset is `kind: "video"` (its
 * `media_type` says it is a still), so it lands on a video track by the
 * same comparison.
 *
 * Task 26 also adds this file's native-drag-and-drop payload helpers: a
 * `LibraryAssetCard` drag has to cross from the media-library sidebar into
 * a completely different component subtree (`TimelineView`'s lanes and its
 * below-the-last-lane strip), which is exactly what the browser's own
 * `DataTransfer`-based drag-and-drop exists for — the alternative (a
 * hand-rolled `pointermove`/`elementFromPoint` tracker spanning two
 * sibling component trees) would reinvent what the platform already does.
 */
import type { AssetKind, Project, Track } from "../editorTypes";
import { lockedReason } from "./actionMeta";

/** Can `track` take a clip of `kind`? A missing track takes nothing. */
export function trackAccepts(track: Track | undefined, kind: AssetKind): boolean {
  return track !== undefined && !track.locked && track.kind === kind;
}

/** The first track, in the project's own track order, that accepts `kind`. */
export function firstAcceptingTrack(project: Project | null, kind: AssetKind): Track | undefined {
  return project?.tracks.find((t) => trackAccepts(t, kind));
}

// ---- native drag-and-drop payload (Task 26) --------------------------------

/**
 * The MIME type a `LibraryAssetCard`'s native drag carries — `kind` is
 * baked into the type STRING itself, not just its value, because
 * `DataTransfer.getData` returns `""` for every type during `dragover` (a
 * browser security restriction: the actual VALUE is readable only once the
 * drop happens), while `DataTransfer.types` — the list of type strings — is
 * readable throughout the whole drag. Encoding `kind` in the type name is
 * what lets a lane answer `dropRefusalReason` below while the pointer is
 * still moving, not only after the user lets go. Not exported: every
 * caller outside this file goes through `setAssetDragData`/
 * `draggedAssetKind`/`draggedAssetId` below, never the raw type string.
 */
function assetDragMimeType(kind: AssetKind): string {
  return `application/x-vault-buddy-asset-${kind}`;
}

/** Sets the payload a drag from the library carries — `LibraryAssetCard`'s
 * own `dragstart`. */
export function setAssetDragData(dataTransfer: DataTransfer, assetId: string, kind: AssetKind): void {
  dataTransfer.effectAllowed = "copy";
  dataTransfer.setData(assetDragMimeType(kind), assetId);
}

/** The asset kind an in-progress drag carries, read from `types` alone (see
 * `assetDragMimeType`'s own doc for why) — `null` for a drag that is not
 * one of ours (a file drop, a browser tab, plain text). */
export function draggedAssetKind(dataTransfer: DataTransfer): AssetKind | null {
  if (dataTransfer.types.includes(assetDragMimeType("video"))) return "video";
  if (dataTransfer.types.includes(assetDragMimeType("audio"))) return "audio";
  return null;
}

/** The dragged asset's id — only readable once the drop actually happens
 * (see `assetDragMimeType`'s own doc); `null` for a payload carrying no
 * value under `kind`'s own type (should not happen for a genuine
 * `LibraryAssetCard` drag, but a caller must not crash on it). */
export function draggedAssetId(dataTransfer: DataTransfer, kind: AssetKind): string | null {
  const id = dataTransfer.getData(assetDragMimeType(kind));
  return id === "" ? null : id;
}

/**
 * Why `track` would refuse a drop of `kind` — the SAME two checks
 * `trackAccepts` applies, but as the human reason a lane surfaces on its
 * own `title` while the drag is over it (the `LibraryAssetCard`/
 * `TrackHeader` "aria-disabled + title" precedent). `null` when `track`
 * would accept it — mirrors `trackAccepts` rather than growing a second
 * rule, and reuses `actionMeta.lockedReason` for the locked case so a
 * locked lane never explains itself two different ways depending on
 * whether the refusal came from a clip drag or a fresh asset drop.
 */
export function dropRefusalReason(track: Track | undefined, kind: AssetKind): string | null {
  if (track === undefined || trackAccepts(track, kind)) return null;
  return track.locked ? lockedReason(track.name) : `Track ${track.name} only accepts ${track.kind} media`;
}
