<script setup lang="ts">
/**
 * The timeline's toolbar (Task 20; visual-parity Task 16, concept spec
 * §6.2): 44px, left to right — "Timeline", Undo and Redo, Split, Delete,
 * the Delete-mode select, Add chapter marker, **Edit actions**, Snap, and
 * the zoom group (`TimelineZoom`) at the right.
 *
 * Undo, Redo, Split, Delete and the marker read the ONE action registry
 * (`resolveActions`/`commandFor` over `baseActionContext`), so a disabled
 * reason here can never disagree with the preview header, a menu or a
 * shortcut. A disabled control keeps its place and its focus, carries its
 * reason as its title, and says it in a toast when pressed (D14; the
 * `Toolstrip` precedent).
 *
 * **Delete mode**: `editorWorkspace.deleteMode` decides which of the two
 * delete actions THIS toolbar's trash sends — `delete` (leave a gap) or
 * `deleteClose` (close the gap, on the deleted clip's own track only: Rust's
 * `delete_close_gap_ripples_only_its_track`). The Delete / Shift+Delete
 * shortcuts stay an explicit per-keypress choice.
 *
 * **Edit actions** opens the timeline's own action menu — the one a
 * right-click on a clip opens — for the SELECTION (onboarding lesson 9).
 * `TimelineView` owns that menu, so this only reports where the button is.
 * Shift+F10 or the Menu key anywhere in the toolbar opens it too.
 *
 * **Keyboard (D16)**: the buttons — its own and the zoom group's — are one
 * Tab stop with roving arrows (`useToolbarRoving`); the Delete-mode select
 * and the zoom range stay their own stops with their own arrow keys.
 * It, the row itself, Split and Undo are the guide's `timeline.more` /
 * `timeline.toolbar` / `timeline.split` / `timeline.undo`.
 */
import type { ComponentPublicInstance } from "vue";
import { computed, ref } from "vue";

import { useActionRegistry, useBaseActionContext } from "../../../composables/useActionRegistry";
import type { GuideRef } from "../../../composables/useGuideTarget";
import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useToolbarRoving } from "../../../composables/useToolbarRoving";
import type { ActionId } from "../../../editor/actionMeta";
import { isContextMenuShortcut } from "../../../editor/shortcuts";
import type { DeleteMode } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import TimelineZoom from "./TimelineZoom.vue";

/** `moreOpen`: the menu Edit actions opened is showing (`aria-expanded`;
 * a clip's right-click menu is not this button's). */
defineProps<{ moreOpen?: boolean }>();
const emit = defineEmits<{ (e: "fit"): void; (e: "more", at: { x: number; y: number }): void }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const context = useBaseActionContext();
const { resolved, enabledCommand } = useActionRegistry(() => context.value);

/** Which delete action the trash sends — follows the Delete-mode select. */
const deleteActionId = computed<ActionId>(() => (workspace.deleteMode === "close" ? "deleteClose" : "delete"));

function activate(id: ActionId): void {
  const command = enabledCommand(id);
  if (command) void editorProject.execute(command);
}

const HISTORY = ["undo", "redo"] as const;

/** A registry control's title: its reason while disabled, else what it
 * does and its shortcut. */
function titleFor(id: ActionId, hint: string): string {
  const verdict = resolved.value[id];
  if (!verdict.enabled) return verdict.reason ?? hint;
  return verdict.shortcut ? `${hint} (${verdict.shortcut})` : hint;
}
const deleteHint = computed(() =>
  workspace.deleteMode === "close" ? "Delete selection and close the gap on its track" : "Delete selection, leaving a gap",
);
function stateClass(id: ActionId): string {
  return resolved.value[id].enabled ? "" : "cursor-not-allowed opacity-40";
}

function onDeleteMode(event: Event): void {
  workspace.setDeleteMode((event.target as HTMLSelectElement).value as DeleteMode);
}

const toolbarTarget = useGuideTarget("timeline.toolbar");
const moreTarget = useGuideTarget("timeline.more");
const actionTargets: Partial<Record<ActionId, GuideRef>> = {
  split: useGuideTarget("timeline.split"),
  undo: useGuideTarget("timeline.undo"),
};

/** One element, two refs: the roving root and the guide's target. */
const root = ref<HTMLElement | null>(null);
function bindRoot(el: Element | ComponentPublicInstance | null): void {
  root.value = el as HTMLElement | null;
  toolbarTarget(el);
}
const moreButton = ref<HTMLElement | null>(null);
function bindMore(el: Element | ComponentPublicInstance | null): void {
  moreButton.value = el as HTMLElement | null;
  moreTarget(el);
}

/** Opens the action menu just below the Edit actions button. */
function openMore(): void {
  const rect = moreButton.value?.getBoundingClientRect();
  if (rect) emit("more", { x: rect.left, y: rect.bottom });
}

const { roving, onToolbarKeydown } = useToolbarRoving(root, "undo");

/** Shift+F10 / the Menu key anywhere in the toolbar is Edit actions (its
 * tooltip names it); every other key is the roving set's. */
function onKeydown(event: KeyboardEvent): void {
  if (isContextMenuShortcut(event)) {
    event.preventDefault();
    event.stopPropagation();
    openMore();
    return;
  }
  onToolbarKeydown(event);
}

const ICON_BUTTON = "inline-flex h-8 w-8 shrink-0 items-center justify-center p-1.5 text-fg-muted";
const LABELLED = "inline-flex min-h-[30px] shrink-0 items-center gap-[7px] px-2.5 text-[11px]";
</script>

<template>
  <div
    :ref="bindRoot"
    data-testid="timeline-toolbar"
    role="toolbar"
    aria-label="Timeline tools"
    class="flex h-11 shrink-0 items-center gap-1.5 border-b border-line bg-panel px-3 max-[1200px]:gap-0.5"
    @keydown="onKeydown"
  >
    <span class="mr-[5px] shrink-0 text-[10px] text-fg-muted">Timeline</span>
    <button
      v-for="id in HISTORY"
      :key="id"
      :ref="actionTargets[id]"
      type="button"
      :data-testid="`timeline-toolbar-${id}`"
      v-bind="roving.bind(id)"
      :aria-label="resolved[id].label"
      :aria-disabled="!resolved[id].enabled"
      :title="titleFor(id, resolved[id].label)"
      :class="[ICON_BUTTON, stateClass(id)]"
      @click="activate(id)"
    >
      <EditorIcon :name="id" />
    </button>

    <span class="mx-[3px] h-5 w-px shrink-0 bg-line" />

    <button
      :ref="actionTargets.split"
      type="button"
      data-testid="timeline-toolbar-split"
      v-bind="roving.bind('split')"
      :aria-disabled="!resolved.split.enabled"
      :title="titleFor('split', 'Split selected clip at playhead')"
      :class="[LABELLED, 'text-fg-secondary', stateClass('split')]"
      @click="activate('split')"
    >
      <EditorIcon name="scissors" />
      Split
    </button>
    <button
      type="button"
      data-testid="timeline-toolbar-delete"
      v-bind="roving.bind('delete')"
      aria-label="Delete selection"
      :aria-disabled="!resolved[deleteActionId].enabled"
      :title="titleFor(deleteActionId, deleteHint)"
      :class="[ICON_BUTTON, stateClass(deleteActionId)]"
      @click="activate(deleteActionId)"
    >
      <EditorIcon name="trash" />
    </button>
    <select
      data-testid="timeline-toolbar-delete-mode"
      aria-label="Delete behavior"
      title="Choose what Delete does: leave a gap, or close the gap on the clip's own track"
      class="min-h-[30px] w-[136px] shrink-0 bg-panel p-1 text-[10px]"
      :value="workspace.deleteMode"
      @change="onDeleteMode"
    >
      <option value="gap">
        Delete: leave gap
      </option>
      <option value="close">
        Delete: close gap
      </option>
    </select>
    <button
      type="button"
      data-testid="timeline-toolbar-marker"
      v-bind="roving.bind('marker')"
      aria-label="Add chapter marker"
      :aria-disabled="!resolved.addMarker.enabled"
      :title="titleFor('addMarker', 'Add chapter marker at the playhead')"
      :class="[ICON_BUTTON, stateClass('addMarker')]"
      @click="activate('addMarker')"
    >
      <EditorIcon name="bookmark" />
    </button>
    <button
      :ref="bindMore"
      type="button"
      data-testid="timeline-toolbar-more"
      v-bind="roving.bind('more')"
      aria-haspopup="menu"
      :aria-expanded="moreOpen ? 'true' : 'false'"
      title="Edit actions for the current selection (Shift+F10)"
      class="ml-[3px] inline-flex min-h-8 shrink-0 items-center gap-[7px] rounded-l-none rounded-r-[6px] border-y-0 border-r-0 border-l border-line px-2.5 text-[11px] text-fg-secondary"
      @click="openMore"
    >
      <EditorIcon name="more" />
      Edit actions
    </button>

    <span class="mx-[3px] h-5 w-px shrink-0 bg-line" />

    <button
      type="button"
      data-testid="timeline-toolbar-snap"
      v-bind="roving.bind('snap')"
      :aria-pressed="workspace.snap"
      title="Snap to clip edges, markers and the playhead"
      :class="[LABELLED, 'border-0', workspace.snap ? 'bg-accent-bg text-accent' : 'bg-transparent text-fg-secondary']"
      @click="workspace.toggleSnap()"
    >
      <EditorIcon name="magnet" />
      Snap
    </button>

    <TimelineZoom @fit="emit('fit')" />
  </div>
</template>
