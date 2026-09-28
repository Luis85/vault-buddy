<script setup lang="ts">
/**
 * The ruler row's **Add track** (visual-parity Task 16; concept spec §6.3,
 * design D13/D14, no-op audit finding 3): a plus button opening the one
 * `MenuPanel` with Add video track and Add audio track. Both are the
 * registry's own actions (`addTrackVideo`/`addTrackAudio`, ruling P3) —
 * the same resolver, builder and path the empty-lane menu uses — so a
 * disabled reason (no project, Rust's 32-track limit) shows the same way
 * in both, as the item's hint line.
 *
 * It is the guide's `track.menu` target (the concept's
 * `[data-action="trackMenu"]`, "Add a video or audio track"): visual-parity
 * Task 17 removed the track header's ⋮ menu the lesson used to point at.
 */
import { computed, ref } from "vue";

import { useBaseActionContext } from "../../../composables/useActionRegistry";
import type { GuideRef } from "../../../composables/useGuideTarget";
import { useGuideTarget } from "../../../composables/useGuideTarget";
import type { ActionId } from "../../../editor/actions";
import { resolveActions } from "../../../editor/actions";
import { activateEditorAction } from "../../../editor/clipboard";
import { useEditorProjectStore } from "../../../stores/editorProject";
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";
import type { MenuItem } from "../menus/menuModel";
import MenuPanel from "../menus/MenuPanel.vue";

const editorProject = useEditorProjectStore();
const trigger = ref<HTMLButtonElement | null>(null);
const guideTarget: GuideRef = useGuideTarget("track.menu");
/** One element, two refs: the menu's anchor and the guide's target. */
function bindTrigger(el: Element | null): void {
  trigger.value = el instanceof HTMLButtonElement ? el : null;
  guideTarget(el);
}
/** Whether the menu is showing — the ruler row lifts itself above the
 * playhead while it is, so the line never crosses the open menu. */
const open = defineModel<boolean>("open", { default: false });

const TRACK_ACTIONS: { id: ActionId; icon: EditorIconName }[] = [
  { id: "addTrackVideo", icon: "video" },
  { id: "addTrackAudio", icon: "music" },
];

const context = useBaseActionContext();

const items = computed<MenuItem[]>(() => {
  const resolved = resolveActions(context.value);
  return TRACK_ACTIONS.map(({ id, icon }) => ({
    id,
    label: resolved[id].label,
    icon,
    disabledReason: resolved[id].enabled ? null : resolved[id].reason,
    run: () => void activateEditorAction(id, context.value, (command) => editorProject.execute(command)),
  }));
});

/** The open menu closes itself on a press outside it; the trigger is
 * outside it, so without this a press there would close and reopen it. */
function onPointerDown(event: PointerEvent): void {
  if (open.value) event.stopPropagation();
}
</script>

<template>
  <button
    :ref="(el) => bindTrigger(el as Element | null)"
    type="button"
    data-testid="timeline-add-track"
    aria-haspopup="menu"
    :aria-expanded="open"
    title="Add a video or audio track"
    class="inline-flex min-h-[26px] shrink-0 items-center gap-[7px] px-2.5 text-[10px] text-fg"
    @pointerdown="onPointerDown"
    @click="open = !open"
  >
    <EditorIcon
      name="plus"
      :size="14"
    />
    Add track
  </button>
  <MenuPanel
    v-if="open && trigger"
    testid="timeline-add-track-panel"
    heading="Add track"
    :items="items"
    :anchor="trigger"
    @close="open = false"
  />
</template>
