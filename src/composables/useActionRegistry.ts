/**
 * The action registry as a toolbar sees it (visual-parity Task 16): the
 * context of a surface with no pointer target — the open project, the
 * playhead and the selection (`baseActionContext`) — and the one way such
 * a surface activates an action: an enabled action yields its command, a
 * disabled one says its reason in a toast and yields nothing (design D14:
 * the reason is reachable by keyboard, not only as a title) — through
 * `announceDisabled`, so a held key says it once per 1.5 s.
 *
 * The preview header, its tool strip and the timeline toolbar share it, so
 * none of them grows its own copy of either rule.
 */
import type { ComputedRef } from "vue";
import { computed } from "vue";

import { baseActionContext } from "../editor/actionContext";
import type { ActionContext, ActionId, ResolvedAction } from "../editor/actions";
import { commandFor, resolveActions } from "../editor/actions";
import { announceDisabled } from "../editor/disabledAnnouncer";
import type { EditorCommand } from "../editor/editorCommandTypes";
import { useEditorProjectStore } from "../stores/editorProject";
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";

export function useBaseActionContext(): ComputedRef<ActionContext> {
  const project = useEditorProjectStore();
  const workspace = useEditorWorkspaceStore();
  return computed(() => baseActionContext(project.project, project.snapshot, workspace.playheadMs, workspace.selectionClipIds));
}

export function useActionRegistry(context: () => ActionContext): {
  resolved: ComputedRef<Record<ActionId, ResolvedAction>>;
  enabledCommand: (id: ActionId) => EditorCommand | null;
} {
  const resolved = computed(() => resolveActions(context()));

  function enabledCommand(id: ActionId): EditorCommand | null {
    const verdict = resolved.value[id];
    if (verdict.enabled) return commandFor(id, context());
    announceDisabled(verdict.reason);
    return null;
  }

  return { resolved, enabledCommand };
}
