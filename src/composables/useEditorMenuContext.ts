/**
 * The real `MenuContext` the context-menu item sets act through
 * (visual-parity Task 5): edits go to `editorProject.execute`, registry
 * actions through `activateEditorAction` (the path the toolbar and the
 * shortcuts share), view changes to `editorWorkspace` and the reveal bus.
 * The timeline supplies the two view changes only it can make — it owns the
 * scroller — so any surface that mounts a menu passes its own.
 *
 * Nothing here decides whether an item is available: the item sets do,
 * from the context's snapshot. These are only the doors an enabled item
 * walks through, so each one is the same door another surface already uses
 * (the media library's tab, the inspector's reveal, the header's rename).
 */
import { activateEditorAction } from "../editor/clipboard";
import type { MenuContext } from "../editor/menuContext";
import { insertAssetOnFreeTrack } from "../editor/placeOnFreeTrack";
import { clipNameFocus, requestReveal, requestTimelineReveal, trackRenameRequest } from "../editor/revealBus";
import { addTrackThenInsert } from "../editor/trackEdits";
import { requestTrackRemoval } from "../editor/trackRemoval";
import { useEditorProjectStore } from "../stores/editorProject";
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";

export interface TimelineViewOps {
  /** Zoom and scroll so `[startMs, endMs)` fills the lanes. */
  fitRange(startMs: number, endMs: number): void;
  /** Zoom and scroll so the whole edit fits. */
  fitTimeline(): void;
}

type Workspace = ReturnType<typeof useEditorWorkspaceStore>;

/** A cue lives on its clip: select both, and bring the inspector in (a
 * drawer at narrow widths) where the cue's fields are — the cue menu's
 * "Edit annotation" and a click on a timeline cue (visual-parity Task 19). */
export function selectCue(workspace: Workspace, effect: { id: string; clip_id: string }): void {
  workspace.select([effect.clip_id]);
  workspace.setSelected({ type: "effect", id: effect.id });
  requestReveal("inspector");
}

/** Selecting from a menu: a clip selection clears a selected cue, the way a
 * click on a clip does, so the inspector shows the clip and not the cue. */
function selectionOps(workspace: Workspace) {
  return {
    selectClips(ids: string[]): void {
      workspace.select(ids);
      workspace.setSelected(null);
    },
    selectEffect(effect: { id: string; clip_id: string }): void {
      selectCue(workspace, effect);
    },
    clearSelection(): void {
      workspace.select([]);
      workspace.setSelected(null);
    },
  };
}

/** Where a menu sends the person next: the playhead (scrolled into view, so
 * the move is visible), an inspector tab or a library tab. */
function navigationOps(workspace: Workspace) {
  function openLibrary(tab: string): void {
    workspace.setLibraryTab(tab);
    requestReveal("library");
  }
  return {
    seek(ms: number): void {
      workspace.setPlayhead(ms);
      requestTimelineReveal(ms);
    },
    openProperty(tab: Parameters<MenuContext["openProperty"]>[0]): void {
      workspace.setPropertyTab(tab);
      requestReveal("inspector");
    },
    /** The Clip section showing that clip focuses its name field. */
    focusClipName(clipId: string): void {
      clipNameFocus.value = clipId;
    },
    openLibrary,
    /** The media library's own Reconnect dialog, which lists every missing
     * source; the one asked about is among them. */
    reconnect(): void {
      openLibrary("media");
      requestReveal("reconnect");
    },
    toggleSnap(): void {
      workspace.toggleSnap();
    },
  };
}

export function useEditorMenuContext(view: TimelineViewOps): (action: MenuContext["action"]) => MenuContext {
  const project = useEditorProjectStore();
  const workspace = useEditorWorkspaceStore();
  const execute: MenuContext["execute"] = (command) => project.execute(command);
  const currentProject = () => project.project;
  const selection = selectionOps(workspace);
  const navigation = navigationOps(workspace);

  return (action) => ({
    action,
    snap: workspace.snap,
    hasSelection: workspace.selectionClipIds.length > 0 || workspace.selected !== null,
    missingAssetIds: new Set(project.missing.map((m) => m.assetId)),
    execute,
    activate: (actionId, actionCtx) => void activateEditorAction(actionId, actionCtx, execute),
    ...selection,
    ...navigation,
    fitRange: view.fitRange,
    fitTimeline: view.fitTimeline,
    addAssetOnFreeTrack: (asset, startMs) => void insertAssetOnFreeTrack(execute, currentProject, asset, startMs),
    addAssetOnNewTrack: (asset, startMs) => void addTrackThenInsert(execute, currentProject, asset, startMs),
    // The header that owns the name field starts its inline rename.
    renameTrack: (trackId) => {
      trackRenameRequest.value = trackId;
    },
    // A track with clips asks first (`RemoveTrackDialog`), the inspector's
    // own Remove track… rule (visual-parity Task 13).
    removeTrack: (trackId) => requestTrackRemoval(project, trackId),
  });
}
