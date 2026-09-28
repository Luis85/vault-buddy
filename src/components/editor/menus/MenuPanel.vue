<script setup lang="ts">
/**
 * The tutorial editor's one menu panel (visual-parity Task 5; concept spec
 * §8 `openMenuPanel`, design D13): a heading with the mono subtitle, 31px
 * icon items with a shortcut or a chevron, separators, one level of
 * submenus and a live hint line. The context menus render through it now;
 * the header's Project/View/Help menus and More tools will too.
 *
 * Mounted while open (the caller's `v-if`). It remembers what had focus
 * when it opened: Escape returns focus there; choosing an item returns it
 * there BEFORE the item runs, so a dialog the item opens gives focus back
 * to it on Escape, and an item that moves focus itself (Rename focuses a
 * name field) still wins; a click outside closes without touching focus, since the person
 * aimed somewhere else on purpose.
 *
 * A submenu opens on Enter or →, or after 180 ms of hover, 4px right of its
 * item (flipped to the left when it would leave the window); ← or Escape
 * closes only the submenu and puts focus back on its item.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";

import type { MenuAction, MenuAnchor, MenuItem } from "./menuModel";
import { anchorPoint, isDisabled, MENU_SUBTITLE, SUBMENU_DELAY_MS, submenuPoint } from "./menuModel";
import MenuPanelList from "./MenuPanelList.vue";

const props = withDefaults(
  defineProps<{
    heading: string;
    subtitle?: string;
    items: MenuItem[];
    anchor: MenuAnchor;
    /** `data-testid` of the panel; its parts and items extend it. */
    testid?: string;
  }>(),
  { subtitle: MENU_SUBTITLE, testid: "menu-panel" },
);
const emit = defineEmits<(e: "close") => void>();

const root = ref<HTMLElement | null>(null);
const point = computed(() => anchorPoint(props.anchor));
const rootList = ref<InstanceType<typeof MenuPanelList> | null>(null);
const subList = ref<InstanceType<typeof MenuPanelList> | null>(null);
const sub = ref<{ parent: number; items: MenuItem[]; point: { x: number; y: number } } | null>(null);

/** Captured during setup, before the first item takes focus: whatever the
 * person was on when the menu opened (a clip, a toolbar button). */
const invoker = document.activeElement as HTMLElement | null;
let hoverTimer: ReturnType<typeof setTimeout> | undefined;

/** The submenu is placed from its item as rendered now, so a panel that
 * scrolled still opens it beside the item. A keyboard open moves focus
 * into it; a hover open leaves focus with the pointer's item. */
function openSub(index: number, el: HTMLElement, focus: boolean) {
  clearTimeout(hoverTimer);
  const item = props.items[index] as MenuAction;
  if (!item.submenu || isDisabled(item)) return;
  sub.value = { parent: index, items: item.submenu, point: submenuPoint(el.getBoundingClientRect(), window.innerWidth) };
  if (focus) void nextTick(() => subList.value?.focusFirst());
}

function closeSub() {
  const parent = sub.value?.parent;
  sub.value = null;
  if (parent !== undefined) rootList.value?.focusIndex(parent);
}

/** A hovered submenu item opens after the delay; any other closes one. */
function onHover(index: number, el: HTMLElement) {
  clearTimeout(hoverTimer);
  const item = props.items[index] as MenuAction;
  if (item.submenu && !isDisabled(item)) hoverTimer = setTimeout(() => openSub(index, el, false), SUBMENU_DELAY_MS);
  else sub.value = null;
}

/**
 * `always` for Escape (the person backed out: go back to where they were),
 * `ifLost` after choosing an item (only if the item did not move focus
 * somewhere on purpose), `never` for a click outside or Tab (the person is
 * going somewhere else).
 */
function close(returnFocus: "always" | "ifLost" | "never") {
  clearTimeout(hoverTimer);
  emit("close");
  if (returnFocus === "never") return;
  void nextTick(() => {
    const lost = !document.activeElement || document.activeElement === document.body;
    if (returnFocus === "always" || lost) invoker?.focus();
  });
}

/** Focus back on the invoker, close, then run. The item runs with focus
 * already on what opened the menu (D16, final review): a dialog it opens
 * captures THAT as the control to return to on Escape — not the menu item
 * about to be detached, nor `body`. An item that moves focus itself (Rename
 * focuses a name field) still wins, because it runs last. */
function choose(items: MenuItem[], index: number) {
  invoker?.focus();
  close("ifLost");
  (items[index] as MenuAction).run?.();
}

/** Right-clicking another clip closes this menu here, on pointerdown, and
 * the timeline opens the next one on the same gesture's contextmenu. */
function onWindowPointerDown(event: PointerEvent) {
  if (root.value && !root.value.contains(event.target as Node)) close("never");
}

onMounted(() => {
  window.addEventListener("pointerdown", onWindowPointerDown);
  void nextTick(() => rootList.value?.focusFirst());
});
onBeforeUnmount(() => {
  clearTimeout(hoverTimer);
  window.removeEventListener("pointerdown", onWindowPointerDown);
});
</script>

<template>
  <div
    ref="root"
    :data-testid="`${testid}-root`"
  >
    <MenuPanelList
      ref="rootList"
      :items="items"
      :level="0"
      :heading="heading"
      :subtitle="subtitle"
      :label="heading"
      :testid="testid"
      :item-testid="`${testid}-item`"
      :point="point"
      :expanded-index="sub?.parent ?? null"
      @choose="(i) => choose(items, i)"
      @open="openSub"
      @hover="onHover"
      @escape="close('always')"
      @tab="close('never')"
    />
    <MenuPanelList
      v-if="sub"
      ref="subList"
      :items="sub.items"
      :level="1"
      :label="(items[sub.parent] as MenuAction).label"
      :testid="`${testid}-submenu`"
      :item-testid="`${testid}-item`"
      :point="sub.point"
      :expanded-index="null"
      @choose="(i) => sub && choose(sub.items, i)"
      @back="closeSub"
      @tab="close('never')"
    />
  </div>
</template>
