<script setup lang="ts">
/**
 * The tutorial editor's context menu (Task 17; F-15, F-49;
 * SCREENS-AND-INTERACTIONS.md §03: "Right-click operates on its actual
 * target and pointer time … The same actions are reachable from Edit
 * actions/More and Shift+F10. Menus support arrow navigation, Home/End,
 * Escape and focus return; disabled actions explain prerequisites.").
 *
 * A fully controlled popup: the caller owns `open`/position and supplies
 * the already-built `context` (its `pointerTarget` IS the right-clicked/
 * keyboard-focused thing — this component never guesses one) plus the
 * ordered `items` to offer for that target (which actions make sense for a
 * clip vs. a track vs. a gap is a domain decision the future timeline/
 * canvas surfaces make when they open this menu, not something a generic
 * menu shell should hard-code). Every item's enabled state, reason and
 * label come from the ONE registry (`actions.ts`'s `resolveActions`) — the
 * same source the preview toolbar reads, so the two surfaces can never
 * disagree about whether an action is available right now.
 *
 * Disabled items stay reachable by arrow key (ARIA-DISABLED, never the
 * native `disabled` attribute) so a keyboard user can still discover WHY
 * something is unavailable (the `reason` as the item's `title`) instead of
 * the item silently vanishing from the roving-tabindex order.
 */
import type { ComponentPublicInstance } from "vue";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import type { ActionContext, ActionId } from "../../../editor/actions";
import { resolveActions } from "../../../editor/actions";

const props = defineProps<{
  open: boolean;
  items: ActionId[];
  context: ActionContext;
  /** Viewport position (px) — the pointer's own location for a right-click
   * open, or a focused clip's rect for a Shift+F10/Menu-key open. */
  x: number;
  y: number;
}>();
const emit = defineEmits<{
  (e: "close"): void;
  (e: "activate", actionId: ActionId): void;
}>();

const resolved = computed(() => resolveActions(props.context));

const root = ref<HTMLElement | null>(null);
const menu = ref<HTMLElement | null>(null);
const itemEls = ref<(HTMLElement | null)[]>([]);
function setItemRef(i: number, el: Element | ComponentPublicInstance | null) {
  // Every item is a native <button>, so `el` is always an `Element` in
  // practice — the `ComponentPublicInstance` half of Vue's template-ref
  // callback type only applies to component refs, never a plain DOM node.
  itemEls.value[i] = el as HTMLElement | null;
}

/** Clamps a position so a `size`-wide/tall box starting there stays fully
 * inside `[0, viewport]` — never off the left/top edge, and pulled back
 * from the right/bottom edge once `pos + size` would exceed it. A `size`
 * that alone exceeds `viewport` (a menu taller than a very short window)
 * still lands at 0 rather than a negative coordinate. */
function clamp(pos: number, size: number, viewport: number): number {
  return Math.max(0, Math.min(pos, viewport - size));
}

/** The rendered position — `props.x`/`y` until the menu has actually
 * painted, then pulled back into the viewport by its own measured size
 * (the `TaskScheduleMenu` precedent: `getBoundingClientRect` only answers
 * once the box exists, so clamping has to happen AFTER a render, not
 * before it — see the `reposition` watcher below). A right-click near the
 * bottom-right corner used to render the menu partly or fully off-screen,
 * stranding its own lower/right items unreachable by mouse OR keyboard. */
const clampedX = ref(props.x);
const clampedY = ref(props.y);

async function reposition() {
  clampedX.value = props.x;
  clampedY.value = props.y;
  await nextTick();
  const rect = root.value?.getBoundingClientRect();
  if (!rect) return;
  clampedX.value = clamp(props.x, rect.width, window.innerWidth);
  clampedY.value = clamp(props.y, rect.height, window.innerHeight);
}

watch(
  [() => props.open, () => props.x, () => props.y],
  ([isOpen]) => {
    if (!isOpen) return;
    void reposition();
  },
  { immediate: true },
);

const activeIndex = ref(0);
/** Who to return focus to on close — whatever had focus when the menu
 * opened (the invoking clip/toolbar button), per SCREENS-AND-INTERACTIONS.md
 * §03's "focus return". Captured automatically rather than taken as a prop:
 * every caller opens this menu FROM a click/keypress on some focused
 * element, so `document.activeElement` at that instant already IS the
 * invoker — a prop would just be a second way to say the same thing and a
 * chance for it to drift from what was actually focused. */
let invoker: HTMLElement | null = null;

watch(
  () => props.open,
  (isOpen) => {
    if (!isOpen) return;
    invoker = document.activeElement as HTMLElement | null;
    activeIndex.value = 0;
    void nextTick(() => {
      (itemEls.value[0] ?? menu.value)?.focus();
    });
  },
);

function close(returnFocus: boolean) {
  emit("close");
  if (returnFocus) void nextTick(() => invoker?.focus());
}

function activate(actionId: ActionId) {
  if (!resolved.value[actionId]?.enabled) return;
  emit("activate", actionId);
  close(true);
}

async function onItemsKeydown(event: KeyboardEvent) {
  const n = props.items.length;
  if (n === 0) return;
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close(true);
    return;
  }
  let target: number;
  if (event.key === "ArrowDown") target = (activeIndex.value + 1) % n;
  else if (event.key === "ArrowUp") target = (activeIndex.value - 1 + n) % n;
  else if (event.key === "Home") target = 0;
  else if (event.key === "End") target = n - 1;
  else if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    activate(props.items[activeIndex.value]);
    return;
  } else return;
  event.preventDefault();
  activeIndex.value = target;
  await nextTick();
  itemEls.value[target]?.focus();
}

function onWindowPointerDown(event: PointerEvent) {
  if (!props.open) return;
  if (root.value && !root.value.contains(event.target as Node)) close(false);
}
onMounted(() => window.addEventListener("pointerdown", onWindowPointerDown));
onBeforeUnmount(() => window.removeEventListener("pointerdown", onWindowPointerDown));

const menuLabel = computed(() => {
  const kind = props.context.pointerTarget?.kind;
  return kind ? `Actions for ${kind}` : "Actions";
});
</script>

<template>
  <div
    v-if="open"
    ref="root"
    data-testid="editor-context-menu-root"
    class="fixed z-50"
    :style="{ left: `${clampedX}px`, top: `${clampedY}px` }"
  >
    <div
      ref="menu"
      role="menu"
      :aria-label="menuLabel"
      tabindex="-1"
      data-testid="editor-context-menu"
      class="flex min-w-44 flex-col gap-0.5 rounded-control border border-line bg-panel p-1 text-micro text-fg shadow-lg focus:outline-none"
      @click.stop
      @keydown="onItemsKeydown"
    >
      <button
        v-for="(id, i) in items"
        :key="id"
        :ref="(el) => setItemRef(i, el)"
        type="button"
        role="menuitem"
        :data-testid="`editor-context-menu-item-${id}`"
        :tabindex="i === activeIndex ? 0 : -1"
        :aria-disabled="!resolved[id].enabled"
        :title="resolved[id].reason ?? undefined"
        class="flex cursor-pointer items-center justify-between gap-3 rounded px-1.5 py-0.5 text-left transition-colors hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="resolved[id].enabled ? 'text-fg-secondary' : 'cursor-default text-fg-subtle opacity-50'"
        @click="activate(id)"
      >
        <span>{{ resolved[id].label }}</span>
        <span
          v-if="resolved[id].shortcut"
          class="text-fg-subtle"
        >{{ resolved[id].shortcut }}</span>
      </button>
    </div>
  </div>
</template>
