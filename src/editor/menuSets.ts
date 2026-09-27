/**
 * The editor's context menus (visual-parity Task 5; concept spec §8, design
 * D13/D14): which menu a target opens and its heading (`contextMenuFor`,
 * the concept's `menuTitle`), and the track-header, lane, teaching-cue and
 * media-asset item sets. The two clip sets are in `menuSetsClip.ts`.
 *
 * Every item maps to one command, a registry action or a view change
 * through the `MenuContext`, and every item that cannot act says why. Two
 * concept items are omitted because nothing backs them: "Rename library
 * label…" and "Remove from library" (no command renames or removes an
 * asset).
 */
import type { MenuAction, MenuItem } from "../components/editor/menus/menuModel";
import { formatMenuTime, SEPARATOR } from "../components/editor/menus/menuModel";
import type { Project, Track } from "../editorTypes";
import { primaryTargetClip, targetClipIds } from "./actionTargets";
import { duplicateCueCommand } from "./cueActions";
import type { MenuContext } from "./menuContext";
import {
  actionItem,
  clearSelectionItem,
  executeInOrder,
  lockReason,
  sendCommand,
  trackLockReason,
} from "./menuContext";
import { clipMenu, multiClipMenu } from "./menuSetsClip";
import { closeAllGapsCommands, closeGapCommand, gapsOnTrack } from "./trackEdits";

/** A new title card's and an inserted intro's length (`TitlesLibrary`'s). */
const CARD_MS = 3_000;
const NO_GAPS = "This track has no gaps.";
const RECONNECT_FIRST = "Reconnect the source first.";

// ---- which menu, under which heading ---------------------------------------------

export interface BuiltMenu {
  heading: string;
  items: MenuItem[];
}

/** A clip in a multi-selection opens the multi-clip menu ("3 selected
 * clips"); a clip on its own, its menu under its own name. */
function clipTargetMenu(ctx: MenuContext): BuiltMenu {
  const count = targetClipIds(ctx.action).length;
  if (count > 1) {
    return { heading: `${count} selected clips`, items: multiClipMenu(ctx) };
  }
  return { heading: primaryTargetClip(ctx.action)?.name || "Clip", items: clipMenu(ctx) };
}

/** An asset row opens the media-asset menu, headed by its own name (the
 * media library's right-click/Shift+F10, visual-parity Task 9) — the
 * `clipTargetMenu` precedent, one target kind earlier. */
function assetTargetMenu(ctx: MenuContext, assetId: string): BuiltMenu {
  const name = ctx.action.project?.assets.find((a) => a.id === assetId)?.name;
  return { heading: name || "Media asset", items: assetMenu(ctx, assetId) };
}

/** A lane opens the gap menu ("Timeline gap"); no target at all — the
 * toolbar's Edit actions with nothing selected — the editor actions, at the
 * playhead. */
export function contextMenuFor(ctx: MenuContext): BuiltMenu {
  const target = ctx.action.pointerTarget;
  if (target?.kind === "clip") return clipTargetMenu(ctx);
  if (target?.kind === "asset" && target.id) return assetTargetMenu(ctx, target.id);
  if (target?.kind === "gap") {
    return {
      heading: "Timeline gap",
      items: laneMenu(ctx, target.id, target.timeMs ?? ctx.action.playheadMs),
    };
  }
  return { heading: "Editor actions", items: laneMenu(ctx, null, ctx.action.playheadMs) };
}

// ---- track header ----------------------------------------------------------------

type TrackFlag = "visible" | "muted" | "solo" | "locked";

/** A checkbox item that flips one of the track's flags. A locked track
 * refuses every flag change but unlocking (core's `setTrackFlags`), so
 * every flag but Lock track carries the lock reason there. */
function flagItem(ctx: MenuContext, track: Track, flag: TrackFlag, label: string, icon: MenuAction["icon"]): MenuAction {
  const change: Partial<Record<TrackFlag, boolean>> = { [flag]: !track[flag] };
  return {
    id: `track-${flag}`,
    label,
    icon,
    checked: track[flag],
    disabledReason: flag === "locked" ? null : trackLockReason(track),
    run: sendCommand(ctx, { kind: "setTrackFlags", trackId: track.id, ...change }),
  };
}

/** Move up/down, each saying why at the edge (audit finding 6). */
function moveItems(ctx: MenuContext, track: Track, index: number, count: number): MenuAction[] {
  const locked = trackLockReason(track);
  const move = (toIndex: number) => sendCommand(ctx, { kind: "moveTrack", trackId: track.id, toIndex });
  return [
    {
      id: "track-up",
      label: "Move track up",
      icon: "up",
      disabledReason: locked ?? (index === 0 ? "Already the top track" : null),
      run: move(index - 1),
    },
    {
      id: "track-down",
      label: "Move track down",
      icon: "down",
      disabledReason: locked ?? (index === count - 1 ? "Already the bottom track" : null),
      run: move(index + 1),
    },
  ];
}

/** The four flags; "Visible video" only means something on a video track. */
function flagItems(ctx: MenuContext, track: Track): MenuAction[] {
  const visible = track.kind === "video" ? [flagItem(ctx, track, "visible", "Visible video", "eye")] : [];
  return [
    ...visible,
    flagItem(ctx, track, "muted", "Mute audio", "muted"),
    flagItem(ctx, track, "solo", "Solo audio", "volume"),
    flagItem(ctx, track, "locked", "Lock track", "lock"),
  ];
}

/** "Rename track…" and "Remove track…" hand over to the header that owns
 * the name field and the confirmation (`MenuContext.renameTrack` /
 * `removeTrack`). */
export function trackMenu(ctx: MenuContext, trackId: string): MenuItem[] {
  const project = ctx.action.project;
  const index = project?.tracks.findIndex((t) => t.id === trackId) ?? -1;
  if (!project || index < 0) return [];
  const track = project.tracks[index];
  const locked = trackLockReason(track);
  const clipIds = project.clips.filter((c) => c.track_id === trackId).map((c) => c.id);
  const gaps = closeAllGapsCommands(project, trackId);
  return [
    {
      id: "track-select",
      label: "Select clips on this track",
      icon: "group",
      disabledReason: clipIds.length > 0 ? null : "This track has no clips.",
      run: () => ctx.selectClips(clipIds),
    },
    {
      id: "track-rename",
      label: "Rename track…",
      icon: "edit",
      disabledReason: locked,
      run: () => ctx.renameTrack(trackId),
    },
    SEPARATOR,
    ...flagItems(ctx, track),
    SEPARATOR,
    ...moveItems(ctx, track, index, project.tracks.length),
    {
      id: "track-close-gaps",
      label: "Close gaps on this track",
      icon: "gap",
      disabledReason: locked ?? (gaps.length > 0 ? null : NO_GAPS),
      run: () => void executeInOrder(ctx, gaps),
    },
    clearSelectionItem(ctx),
    {
      id: "track-remove",
      label: "Remove track…",
      icon: "trash",
      danger: true,
      disabledReason: locked,
      run: () => ctx.removeTrack(trackId),
    },
  ];
}

// ---- empty lane / timeline gap ------------------------------------------------------

/** The gap under the pointer, and every gap on the track (`trackEdits.ts`). */
function gapItems(ctx: MenuContext, project: Project, track: Track, atMs: number): MenuAction[] {
  const locked = trackLockReason(track);
  const gaps = gapsOnTrack(project, track.id);
  const here = gaps.find((g) => atMs >= g.start && atMs < g.end);
  const closeHere = here ? [closeGapCommand(project, track.id, here)] : [];
  return [
    {
      id: "lane-close-gap",
      label: "Close this gap · this track",
      icon: "gap",
      disabledReason: locked ?? (here ? null : "There is no gap here."),
      run: () => void executeInOrder(ctx, closeHere),
    },
    {
      id: "lane-close-gaps",
      label: "Close all gaps · this track",
      icon: "gap",
      disabledReason: locked ?? (gaps.length > 0 ? null : NO_GAPS),
      run: () => void executeInOrder(ctx, closeAllGapsCommands(project, track.id)),
    },
  ];
}

/** Titles and tracks: a card on this lane when it can take one (an
 * unlocked video track), else on a new top track (`addCard`'s `null`). */
function laneAddItems(ctx: MenuContext, track: Track | undefined, atMs: number): MenuAction[] {
  const titleTrack = track?.kind === "video" && !track.locked ? track.id : null;
  const card = { preset: "chapter" as const, trackId: titleTrack, startMs: atMs, durationMs: CARD_MS };
  return [
    {
      id: "lane-title",
      label: "Add title here",
      icon: "text",
      run: sendCommand(ctx, { kind: "addCard", ...card, title: "Chapter", subtitle: "" }),
    },
    {
      id: "lane-intro",
      label: "Insert intro · shift all tracks",
      icon: "layers",
      run: sendCommand(ctx, { kind: "insertIntro", durationMs: CARD_MS, title: "Intro", subtitle: "" }),
    },
    {
      id: "lane-add-video",
      label: "Add video track",
      icon: "video",
      run: () => ctx.addTrack("video"),
    },
    {
      id: "lane-add-audio",
      label: "Add audio track",
      icon: "music",
      run: () => ctx.addTrack("audio"),
    },
  ];
}

/** `trackId` is the lane right-clicked, or `null` for the toolbar's Edit
 * actions with nothing selected ("Editor actions"): then there is no gap
 * to close and Paste has no track to land on, and says so. */
export function laneMenu(ctx: MenuContext, trackId: string | null, atMs: number): MenuItem[] {
  const project = ctx.action.project;
  const track = project?.tracks.find((t) => t.id === trackId);
  const pasteTarget = track ? { kind: "gap" as const, id: track.id, timeMs: atMs } : null;
  return [
    {
      id: "lane-seek",
      label: `Move playhead to ${formatMenuTime(atMs)}`,
      icon: "cursor",
      run: () => ctx.seek(atMs),
    },
    actionItem(ctx, "paste", { label: "Paste clips here", icon: "paste" }, { ...ctx.action, pointerTarget: pasteTarget }),
    SEPARATOR,
    ...(project && track ? gapItems(ctx, project, track, atMs) : []),
    ...laneAddItems(ctx, track, atMs),
    {
      id: "lane-fit",
      label: "Fit timeline",
      icon: "zoomOut",
      run: () => ctx.fitTimeline(),
    },
    {
      id: "lane-snap",
      label: "Snapping",
      icon: "magnet",
      checked: ctx.snap,
      run: () => ctx.toggleSnap(),
    },
  ];
}

// ---- teaching cue --------------------------------------------------------------------

/** A cue is locked with the clip it is attached to. */
export function cueMenu(ctx: MenuContext, effectId: string): MenuItem[] {
  const project = ctx.action.project;
  const effect = project?.effects.find((e) => e.id === effectId);
  const clip = effect && project?.clips.find((c) => c.id === effect.clip_id);
  if (!project || !effect || !clip) return [];
  const locked = lockReason(project, [clip.id]);
  return [
    {
      id: "cue-edit",
      label: "Edit annotation",
      icon: "edit",
      disabledReason: locked,
      run: () => ctx.selectEffect(effect),
    },
    {
      id: "cue-duplicate",
      label: "Duplicate annotation",
      icon: "copy",
      disabledReason: locked,
      run: sendCommand(ctx, duplicateCueCommand(effect, clip)),
    },
    {
      id: "cue-select-clip",
      label: "Select attached clip",
      icon: "link",
      run: () => ctx.selectClips([clip.id]),
    },
    clearSelectionItem(ctx),
    {
      id: "cue-delete",
      label: "Delete annotation",
      icon: "trash",
      danger: true,
      disabledReason: locked,
      run: sendCommand(ctx, { kind: "removeEffect", effectId }),
    },
  ];
}

// ---- media asset (library row) ------------------------------------------------------------

/** "Add at playhead" lands where the media library's "+" does
 * (`placeOnFreeTrack`: a free track of the asset's kind, else a new one);
 * a missing source is refused until it is reconnected, and only then is
 * "Reconnect original…" offered. */
export function assetMenu(ctx: MenuContext, assetId: string): MenuItem[] {
  const project = ctx.action.project;
  const asset = project?.assets.find((a) => a.id === assetId);
  if (!asset) return [];
  const missing = ctx.missingAssetIds.has(asset.id) ? RECONNECT_FIRST : null;
  const playhead = ctx.action.playheadMs;
  const reconnect: MenuAction[] = missing
    ? [{ id: "asset-reconnect", label: "Reconnect original…", icon: "link", run: () => ctx.reconnect(assetId) }]
    : [];
  return [
    {
      id: "asset-add",
      label: "Add at playhead",
      icon: "plus",
      disabledReason: missing,
      run: () => ctx.addAssetOnFreeTrack(asset, playhead),
    },
    {
      id: "asset-new-track",
      label: "Add on a new track",
      icon: "layers",
      disabledReason: missing,
      run: () => ctx.addAssetOnNewTrack(asset, playhead),
    },
    ...reconnect,
  ];
}
