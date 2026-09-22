<script setup lang="ts">
/**
 * The timeline's own control row (Task 20; F-14; brief's own Behavior
 * section: "split/delete/undo/redo/snap/zoom/fit buttons from the action
 * registry"). Split/delete/undo/redo read `resolveActions`/`commandFor` from
 * the SAME `actions.ts` registry `PreviewToolbar.vue`/`ContextMenu.vue`
 * already read (`baseActionContext`, Task 20's own shared builder) — so a
 * disabled reason here can never disagree with the other two surfaces.
 *
 * Snap/zoom/fit have no `ActionId` at all (`actions.ts`'s own module doc:
 * "Actions with no wire command" — zoom/snap/fit aren't even in that list,
 * they are pure `editorWorkspace` view state) and are wired directly against
 * that store — EXCEPT fit, which `editorWorkspace.fit()`'s own doc admits is
 * an "honest, minimal" placeholder because the store layer knows no pixel
 * viewport width. This toolbar doesn't know one either (it isn't the
 * scrolling element) — `TimelineView.vue` is, so `fit` is emitted upward for
 * `TimelineView` to compute a REAL fit and apply it directly, rather than
 * calling `editorWorkspace.fit()`.
 *
 * **Delete mode (Task 21)**: `editorWorkspace.deleteMode` ("gap"|"close")
 * finally has a control — the two-button toggle below the zoom group,
 * `TimelineToolbar`'s own Snap toggle precedent. It decides which of the
 * two delete-shaped `ActionId`s (`delete` = leave a gap, `deleteClose` =
 * ripple the gap shut) THIS toolbar's own Delete button sends; it does NOT
 * touch `SHORTCUTS`' existing Delete/Shift+Delete split (`shortcuts.ts`),
 * which stays an explicit per-keypress choice independent of the persisted
 * default. The close-gap button's visible label says "on this track" because that
 * is exactly the scope `deleteClips{closeGap:true}` ripples (Rust's own
 * `delete_close_gap_ripples_only_its_track`, `clips.rs`) — a user picking
 * this mode must not read it as "closes every track's gap".
 */
import { computed } from "vue";

import { baseActionContext } from "../../../editor/actionContext";
import type { ActionId } from "../../../editor/actionMeta";
import { commandFor, resolveActions } from "../../../editor/actions";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

const emit = defineEmits<{ (e: "fit"): void }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const TOOLBAR_ACTIONS = ["split", "undo", "redo"] as const;

/** Which `ActionId` the toolbar's own Delete button sends this render —
 * follows `editorWorkspace.deleteMode`, unlike the keyboard shortcuts. */
const deleteActionId = computed<ActionId>(() => (workspace.deleteMode === "close" ? "deleteClose" : "delete"));

const context = computed(() =>
  baseActionContext(editorProject.project, editorProject.snapshot, workspace.playheadMs, workspace.selectionClipIds),
);
const resolved = computed(() => resolveActions(context.value));

function onAction(id: ActionId) {
  if (!resolved.value[id].enabled) return;
  const command = commandFor(id, context.value);
  if (command) void editorProject.execute(command);
}
function onDelete() {
  onAction(deleteActionId.value);
}

/** Multiplicative per-click step, applied through `editorWorkspace.setZoom`
 * (which already clamps to its own `[0.1, 20]` range) -- deliberately
 * multiplicative rather than additive so a click feels proportional at both
 * ends of that range. */
const ZOOM_STEP = 1.25;
function zoomIn() {
  workspace.setZoom(workspace.timelineZoom * ZOOM_STEP);
}
function zoomOut() {
  workspace.setZoom(workspace.timelineZoom / ZOOM_STEP);
}

function itemTitle(id: ActionId): string {
  return resolved.value[id].reason ?? resolved.value[id].label;
}
function itemClass(id: ActionId): string {
  return resolved.value[id].enabled ? "text-fg-secondary" : "cursor-default opacity-50";
}
</script>

<template>
  <div
    data-testid="timeline-toolbar"
    role="toolbar"
    aria-label="Timeline tools"
    class="flex h-7 shrink-0 items-center gap-1 rounded-control border border-line bg-raised px-2 text-micro text-fg-subtle"
  >
    <button
      v-for="id in TOOLBAR_ACTIONS"
      :key="id"
      type="button"
      :data-testid="`timeline-toolbar-${id}`"
      :aria-disabled="!resolved[id].enabled"
      :title="itemTitle(id)"
      class="cursor-pointer rounded px-1.5 py-0.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="itemClass(id)"
      @click="onAction(id)"
    >
      {{ resolved[id].label }}
    </button>

    <button
      type="button"
      data-testid="timeline-toolbar-delete"
      :aria-disabled="!resolved[deleteActionId].enabled"
      :title="itemTitle(deleteActionId)"
      class="cursor-pointer rounded px-1.5 py-0.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="itemClass(deleteActionId)"
      @click="onDelete"
    >
      {{ resolved[deleteActionId].label }}
    </button>

    <span class="mx-1 h-4 w-px bg-line" />

    <button
      type="button"
      data-testid="timeline-toolbar-delete-mode-gap"
      :aria-pressed="workspace.deleteMode === 'gap'"
      title="Leave a gap when deleting"
      class="cursor-pointer rounded px-1.5 py-0.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="workspace.deleteMode === 'gap' ? 'bg-accent/20 text-accent-fg' : 'text-fg-secondary'"
      @click="workspace.setDeleteMode('gap')"
    >
      Leave gap
    </button>
    <button
      type="button"
      data-testid="timeline-toolbar-delete-mode-close"
      :aria-pressed="workspace.deleteMode === 'close'"
      title="Close the gap on this track when deleting"
      class="cursor-pointer rounded px-1.5 py-0.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="workspace.deleteMode === 'close' ? 'bg-accent/20 text-accent-fg' : 'text-fg-secondary'"
      @click="workspace.setDeleteMode('close')"
    >
      Close gap on this track
    </button>

    <span class="mx-1 h-4 w-px bg-line" />

    <button
      type="button"
      data-testid="timeline-toolbar-snap"
      :aria-pressed="workspace.snap"
      title="Snap to clip edges, markers and the playhead"
      class="cursor-pointer rounded px-1.5 py-0.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="workspace.snap ? 'bg-accent/20 text-accent-fg' : 'text-fg-secondary'"
      @click="workspace.toggleSnap()"
    >
      Snap
    </button>

    <span class="mx-1 h-4 w-px bg-line" />

    <button
      type="button"
      data-testid="timeline-toolbar-zoom-out"
      title="Zoom out"
      class="cursor-pointer rounded px-1.5 py-0.5 text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      @click="zoomOut"
    >
      &minus;
    </button>
    <button
      type="button"
      data-testid="timeline-toolbar-zoom-in"
      title="Zoom in"
      class="cursor-pointer rounded px-1.5 py-0.5 text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      @click="zoomIn"
    >
      +
    </button>
    <button
      type="button"
      data-testid="timeline-toolbar-fit"
      title="Fit the whole edit"
      class="cursor-pointer rounded px-1.5 py-0.5 text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      @click="emit('fit')"
    >
      Fit
    </button>
  </div>
</template>
