/**
 * What a context-menu item set can see and do (visual-parity Task 5; design
 * D13/D14). The item sets (`menuSetsClip.ts`, `menuSets.ts`) are pure
 * functions of this context: every effect they cause goes through one of the
 * functions below, so a test hands them a context of spies and asserts which
 * command or view change each item makes. `useEditorMenuContext` builds the
 * real one from the editor stores.
 */
import type { MenuAction } from "../components/editor/menus/menuModel";
import type { Asset, Effect, Project, Track, TrackKind } from "../editorTypes";
import type { ActionId } from "./actionMeta";
import { lockedReason } from "./actionMeta";
import type { ActionContext } from "./actions";
import { resolveActions } from "./actions";
import { lockedTrackName } from "./actionTargets";
import type { EditorCommand } from "./editorCommandTypes";

/** The inspector tabs a menu item can open (`InspectorPanel.vue`'s ids). */
export type InspectorTab = "clip" | "layout" | "fades" | "audio" | "speed" | "color";

export interface MenuContext {
  /** The registry context; its `pointerTarget` is what was right-clicked. */
  action: ActionContext;
  snap: boolean;
  /** A clip or a cue is selected (what "Clear selection" would clear). */
  hasSelection: boolean;
  missingAssetIds: ReadonlySet<string>;
  execute(command: EditorCommand): Promise<boolean>;
  /** A registry action, run the way every other surface runs it. */
  activate(actionId: ActionId, actionCtx: ActionContext): void;
  seek(ms: number): void;
  selectClips(ids: string[]): void;
  selectEffect(effect: Effect): void;
  clearSelection(): void;
  openProperty(tab: InspectorTab): void;
  focusClipName(clipId: string): void;
  openLibrary(tab: string): void;
  fitRange(startMs: number, endMs: number): void;
  fitTimeline(): void;
  toggleSnap(): void;
  addTrack(kind: TrackKind): void;
  addAssetOnNewTrack(asset: Asset, startMs: number): void;
  renameTrack(trackId: string): void;
  removeTrack(trackId: string): void;
  reconnect(assetId: string): void;
}

/** How a registry action looks in a menu: the concept's own label and
 * icon ("Copy clip", not the registry's "Copy"). */
type ItemLook = Pick<MenuAction, "id" | "label" | "icon" | "danger">;

/** A registry action as a menu item: the registry's own verdict, reason and
 * shortcut, run through `activate` against `actionCtx`. */
export function actionItem(
  ctx: MenuContext,
  actionId: ActionId,
  look: Omit<ItemLook, "id"> & { id?: string },
  actionCtx: ActionContext = ctx.action,
): MenuAction {
  const verdict = resolveActions(actionCtx)[actionId];
  return {
    id: look.id ?? actionId,
    label: look.label,
    icon: look.icon,
    danger: look.danger,
    kbd: verdict.shortcut ?? undefined,
    disabledReason: verdict.enabled ? null : verdict.reason,
    run: () => ctx.activate(actionId, actionCtx),
  };
}

/** "Track X is locked" when any of `clipIds` sits on a locked track. */
export function lockReason(project: Project | null, clipIds: string[]): string | null {
  const name = lockedTrackName(project, clipIds);
  return name ? lockedReason(name) : null;
}

/** "Track X is locked" for the track itself. */
export function trackLockReason(track: Track): string | null {
  return track.locked ? lockedReason(track.name) : null;
}

/** An item's `run` that sends one command. A refusal is not handled here:
 * the store records it for the shell to show. */
export function sendCommand(ctx: MenuContext, command: EditorCommand): () => void {
  return () => void ctx.execute(command);
}

/** Sends each command in order, stopping at the first Rust refuses: a
 * batch the concept does as one change (fading every selected clip,
 * closing every gap) is several commands here, and a refused step must not
 * be followed by steps that assumed it landed. */
export async function executeInOrder(ctx: MenuContext, commands: EditorCommand[]): Promise<void> {
  for (const command of commands) {
    if (!(await ctx.execute(command))) return;
  }
}

/** "Clear selection", refused when nothing is selected — an enabled item
 * that changes nothing is a no-op (design D14). */
export function clearSelectionItem(ctx: MenuContext): MenuAction {
  return {
    id: "clearSelection",
    label: "Clear selection",
    icon: "cursor",
    disabledReason: ctx.hasSelection ? null : "Nothing is selected.",
    run: () => ctx.clearSelection(),
  };
}
