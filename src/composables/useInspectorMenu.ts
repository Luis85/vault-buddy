/**
 * The menu item sets as the inspector's controls (visual-parity Task 13):
 * the Track and multi-clip states show the SAME items the track and
 * multi-clip context menus build (`menuSets.ts`, `menuSetsClip.ts`), so
 * each button's command, registry action and refusal reason exist once.
 * The inspector changes no timeline view, so the view operations only the
 * timeline can make are inert here (the media library's precedent).
 */
import { computed } from "vue";

import type { MenuAction, MenuItem } from "../components/editor/menus/menuModel";
import { isSeparator } from "../components/editor/menus/menuModel";
import { baseActionContext } from "../editor/actionContext";
import type { MenuContext } from "../editor/menuContext";
import { useEditorProjectStore } from "../stores/editorProject";
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";
import type { TimelineViewOps } from "./useEditorMenuContext";
import { useEditorMenuContext } from "./useEditorMenuContext";

const NOOP_VIEW: TimelineViewOps = { fitRange: () => {}, fitTimeline: () => {} };

/** The current selection's menu context, rebuilt with every projection. */
export function useInspectorMenuContext() {
  const project = useEditorProjectStore();
  const workspace = useEditorWorkspaceStore();
  const contextFor = useEditorMenuContext(NOOP_VIEW);
  return computed<MenuContext>(() =>
    contextFor(baseActionContext(project.project, project.snapshot, workspace.playheadMs, workspace.selectionClipIds)),
  );
}

/** The item `id` in `items`, submenus included. */
export function findMenuAction(items: readonly MenuItem[], id: string): MenuAction | null {
  for (const item of items) {
    if (isSeparator(item)) continue;
    if (item.id === id) return item;
    const inner = item.submenu ? findMenuAction(item.submenu, id) : null;
    if (inner) return inner;
  }
  return null;
}

/** Why `item` cannot act: its own reason, else its parent's (a submenu
 * item inherits the lock of the menu it sits in). */
export function reasonOf(item: MenuAction | null, parent: MenuAction | null = null): string | null {
  if (!item) return "Not available.";
  return parent?.disabledReason ?? item.disabledReason ?? null;
}
