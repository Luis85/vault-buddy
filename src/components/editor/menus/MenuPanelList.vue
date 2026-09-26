<script setup lang="ts">
/**
 * One level of a `MenuPanel` (visual-parity Task 5; concept spec §8): the
 * root panel with its heading, or an open submenu. It owns what happens
 * inside one list — which item has focus, the arrow keys, the live hint —
 * and reports everything that crosses levels (open a submenu, go back,
 * choose, close) to `MenuPanel`, which owns the submenu and the close.
 *
 * Every item stays focusable while disabled (`aria-disabled`, never the
 * native `disabled`), so a keyboard user can reach it and hear WHY in the
 * hint line; the same reason is the item's `title` for the pointer.
 */
import type { ComponentPublicInstance } from "vue";
import { computed, nextTick, onMounted, ref, watch } from "vue";

import type { MenuAction, MenuItem } from "./menuModel";
import { clampInto, DEFAULT_HINT, hintFor, isDisabled, isSeparator } from "./menuModel";
import MenuPanelItem from "./MenuPanelItem.vue";

const props = defineProps<{
  items: MenuItem[];
  /** 0 is the root panel, 1 a submenu. */
  level: number;
  heading?: string;
  subtitle?: string;
  label: string;
  testid: string;
  /** Items are `<itemTestid>-<id>`, the same across levels. */
  itemTestid: string;
  point: { x: number; y: number };
  /** The item whose submenu is open, for its `aria-expanded`. */
  expandedIndex: number | null;
}>();
const emit = defineEmits<{
  (e: "choose", index: number): void;
  (e: "open", index: number, el: HTMLElement, focus: boolean): void;
  (e: "hover", index: number, el: HTMLElement): void;
  (e: "back"): void;
  (e: "escape"): void;
}>();

const panel = ref<HTMLElement | null>(null);
const itemEls: (HTMLElement | null)[] = [];
function setItemRef(i: number, el: Element | ComponentPublicInstance | null) {
  itemEls[i] = ((el as ComponentPublicInstance | null)?.$el as HTMLElement | undefined) ?? null;
}

/** The live line under the items: the default key guide until an item has
 * focus, then what that item will do — or why it cannot. */
const hint = ref(DEFAULT_HINT);
/** The focused item's index in `items` (separators included, never one). */
const active = ref(-1);
const actionable = computed(() => props.items.flatMap((item, i) => (isSeparator(item) ? [] : [i])));

function actionAt(i: number): MenuAction {
  return props.items[i] as MenuAction;
}

// ---- position: the anchor point, pulled 8px inside the viewport ------------

const left = ref(props.point.x);
const top = ref(props.point.y);
/** The panel's size is only known once it has rendered, so the clamp
 * happens after a tick; until then it sits at the anchor. */
async function place() {
  left.value = props.point.x;
  top.value = props.point.y;
  await nextTick();
  const rect = panel.value?.getBoundingClientRect();
  if (!rect) return;
  left.value = clampInto(props.point.x, rect.width, window.innerWidth);
  top.value = clampInto(props.point.y, rect.height, window.innerHeight);
}
onMounted(place);
watch(() => props.point, place);

// ---- focus -----------------------------------------------------------------------

function focusIndex(i: number) {
  active.value = i;
  itemEls[i]?.focus();
}
/** Opening lands on the first item but keeps the key guide in the hint
 * line (the concept's screen 03): the guide is what a person needs on
 * arrival; the first item's own hint follows the first move. */
let opening = false;
function focusFirst() {
  const first = actionable.value[0];
  if (first === undefined) return;
  opening = true;
  focusIndex(first);
  opening = false;
}
defineExpose({ focusIndex, focusFirst });

function onItemFocus(i: number) {
  active.value = i;
  if (!opening) hint.value = hintFor(actionAt(i));
}

/** The item `step` places from the focused one, wrapping (Home/End: ±∞). */
function stepFrom(step: number): number {
  const list = actionable.value;
  if (step === -Infinity) return list[0];
  if (step === Infinity) return list[list.length - 1];
  const at = list.indexOf(active.value);
  return list[(at + step + list.length) % list.length];
}

// ---- activation --------------------------------------------------------------------

/** Enter, Space or a click: a disabled item only explains itself. */
function activate(i: number) {
  const item = actionAt(i);
  if (isDisabled(item)) {
    hint.value = item.disabledReason as string;
    return;
  }
  const el = itemEls[i];
  if (item.submenu && el) emit("open", i, el, true);
  else if (!item.submenu) emit("choose", i);
}

function openSubmenu() {
  const item = actionAt(active.value);
  const el = itemEls[active.value];
  if (item?.submenu && !isDisabled(item) && el) emit("open", active.value, el, true);
}

/** Escape: a submenu steps back to its item; the root closes the menu. */
function leave() {
  if (props.level > 0) emit("back");
  else emit("escape");
}

const STEPS: Record<string, number> = { ArrowDown: 1, ArrowUp: -1, Home: -Infinity, End: Infinity };

function onKeydown(event: KeyboardEvent) {
  const step = STEPS[event.key];
  if (step !== undefined) focusIndex(stepFrom(step));
  else if (event.key === "Enter" || event.key === " ") activate(active.value);
  else if (event.key === "ArrowRight") openSubmenu();
  else if (event.key === "ArrowLeft" && props.level > 0) emit("back");
  else if (event.key === "Escape") leave();
  else return;
  event.preventDefault();
  event.stopPropagation();
}

/** The pointer moves focus with it (the concept's rule), so the hint line
 * and a following key press act on the item under the pointer. */
function onPointerMove(i: number, event: PointerEvent) {
  const el = event.currentTarget as HTMLElement;
  if (document.activeElement !== el) focusIndex(i);
  emit("hover", i, el);
}

</script>

<template>
  <div
    ref="panel"
    role="menu"
    :aria-label="label"
    tabindex="-1"
    :data-testid="testid"
    class="fixed z-50 flex max-h-[calc(100vh-20px)] max-w-[calc(100vw-16px)] flex-col overflow-auto rounded-[10px] border border-menu-edge bg-panel p-1.5 text-fg shadow-[var(--editor-menu-shadow)] focus:outline-none"
    :class="level === 0 ? 'w-[282px] max-[1300px]:w-[260px]' : 'w-[245px]'"
    :style="{ left: `${left}px`, top: `${top}px` }"
    @click.stop
    @keydown="onKeydown"
  >
    <div
      v-if="heading !== undefined"
      class="sticky top-0 z-10 mb-1 flex shrink-0 flex-col gap-1 border-b border-line bg-panel px-2.5 pt-2 pb-[11px]"
    >
      <b
        :data-testid="`${testid}-heading`"
        class="truncate text-[11px] font-semibold"
      >{{ heading }}</b>
      <small
        :data-testid="`${testid}-subtitle`"
        class="vb-mono text-[9px] text-fg-muted"
      >{{ subtitle }}</small>
    </div>
    <template
      v-for="(item, i) in items"
      :key="i"
    >
      <div
        v-if="isSeparator(item)"
        role="separator"
        class="mx-2 my-[5px] h-px shrink-0 bg-line"
      />
      <MenuPanelItem
        v-else
        :ref="(el) => setItemRef(i, el)"
        :item="item"
        :testid="`${itemTestid}-${item.id}`"
        :expanded="expandedIndex === i"
        @click="activate(i)"
        @focus="onItemFocus(i)"
        @pointermove="onPointerMove(i, $event)"
      />
    </template>
    <div
      :data-testid="`${testid}-hint`"
      aria-live="polite"
      class="mt-[5px] flex min-h-[31px] shrink-0 items-center border-t border-line px-[9px] pt-[9px] pb-1 text-[9px] text-fg-muted"
    >
      {{ hint }}
    </div>
  </div>
</template>
