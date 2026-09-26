<script setup lang="ts">
/**
 * The timeline's context menu (Task 17; visual-parity Task 5, concept spec
 * §8): right-click on a clip or an empty lane, Shift+F10/the Menu key on a
 * focused clip, and the toolbar's Edit actions all open it. The caller owns
 * `open` and the position and supplies the registry context — its
 * `pointerTarget` IS the thing right-clicked (or `null` for Edit actions
 * with nothing selected); `contextMenuFor` picks the clip, multi-clip or
 * lane item set and its heading, and `MenuPanel` renders it.
 *
 * Items act themselves (a registry action, one command, a tab or a view
 * change through `useEditorMenuContext`); the timeline passes the two view
 * changes only it can make, fitting a range and the whole edit.
 */
import { computed } from "vue";

import type { TimelineViewOps } from "../../../composables/useEditorMenuContext";
import { useEditorMenuContext } from "../../../composables/useEditorMenuContext";
import type { ActionContext } from "../../../editor/actions";
import { contextMenuFor } from "../../../editor/menuSets";
import MenuPanel from "./MenuPanel.vue";

const props = defineProps<{
  open: boolean;
  context: ActionContext;
  /** Viewport position (px): the pointer, or a focused clip's corner. */
  x: number;
  y: number;
  view: TimelineViewOps;
}>();
const emit = defineEmits<(e: "close") => void>();

const menuContextFor = useEditorMenuContext(props.view);
const menu = computed(() => contextMenuFor(menuContextFor(props.context)));
/** A new target or position is a new menu: remount, so focus starts on its
 * first item and Escape returns to what had focus when THIS one opened. */
const openKey = computed(() => `${props.x},${props.y},${JSON.stringify(props.context.pointerTarget)}`);
</script>

<template>
  <MenuPanel
    v-if="open"
    :key="openKey"
    testid="editor-context-menu"
    :heading="menu.heading"
    :items="menu.items"
    :anchor="{ x, y }"
    @close="emit('close')"
  />
</template>
